//! IPC layer between the Rust `acm-cryptod` and the Go `acm-agent`.
//!
//! Two channels live on the module:
//!
//! 1. **Control plane** — line-delimited JSON-RPC over Unix Domain Socket
//!    (`/run/acm/cryptod.sock`). Low rate: configure policies, rotate
//!    keys, get status. Easy to debug with `nc -U /run/acm/cryptod.sock`.
//!
//! 2. **Stats fast-path** — lock-free shared-memory ring at
//!    `/run/acm/cryptod-stats.shm`. High rate: per-flow / per-port packet
//!    and byte counters updated by cryptod's worker threads, read
//!    periodically by the Go agent to publish to Prometheus.
//!    Not implemented in this skeleton.
//!
//! Wire framing for control plane: one JSON object per line, terminated
//! by `\n`. This is the simplest, most debuggable framing. Future binary
//! framing (length-prefix) can be added if perf demands.

use serde::{Deserialize, Serialize};

/// Default socket path on the module. Created by the systemd unit
/// (directory `/run/acm/` must exist and be writable for the cryptod user).
pub const DEFAULT_SOCKET_PATH: &str = "/run/acm/cryptod.sock";

// ===========================================================================
// Wire types — shared by Rust server and any client (Go agent, debug CLI).
// ===========================================================================

/// All control-plane requests the agent can send to cryptod.
///
/// JSON shape (serde enum with `tag = "method"` and `content = "params"`):
///
/// ```json
///   {"method": "get_status"}
///   {"method": "rotate_key", "params": {"key_id": 42, "algo": 2, "material": "base64..."}}
/// ```
///
/// Material is hex-encoded as a Vec<u8> by serde_json by default — for
/// real wire we may want explicit hex/base64; this PoC uses serde's
/// default array-of-numbers, which is fine for human debugging via `nc`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    GetStatus,
    SetPolicy(PolicyBlob),
    RotateKey(RotateKeyParams),
    /// Seal a one-shot buffer through the active provider/key. The full
    /// frame is built by `acm-wire::seal` so the on-wire output includes
    /// the ACM header + nonce + ciphertext + tag.
    Encrypt(EncryptParams),
    /// Open a previously-sealed ACM frame. Returns plaintext or AuthFailed.
    Decrypt(DecryptParams),
}

/// Top-level response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", content = "data", rename_all = "snake_case")]
pub enum Response {
    Status(StatusReport),
    Ok,
    /// Full ACM frame produced by `Encrypt`.
    Ciphertext(Bytes),
    /// Plaintext produced by `Decrypt`.
    Plaintext(Bytes),
    Error {
        code: u32,
        message: String,
    },
}

/// Wire wrapper for arbitrary bytes (base64-on-wire — matches Go default).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bytes {
    #[serde(with = "wire_b64")]
    pub bytes: Vec<u8>,
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Self { bytes: v }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptParams {
    /// Caller-supplied 12-byte nonce. Caller is responsible for uniqueness
    /// per (key_id, nonce). For tests / PoC we just pass distinct values.
    #[serde(with = "wire_b64")]
    pub nonce: Vec<u8>,
    /// Additional authenticated data — included in tag, not encrypted.
    #[serde(with = "wire_b64", default)]
    pub aad: Vec<u8>,
    /// Plaintext to seal.
    #[serde(with = "wire_b64")]
    pub plaintext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptParams {
    /// The full ACM frame (header + nonce + ciphertext + tag) produced
    /// by a previous `Encrypt` call (or by a peer cryptod).
    #[serde(with = "wire_b64")]
    pub frame: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub version: String,
    pub running: bool,
    pub uptime_s: u64,
    pub active_provider: String, // e.g. "ring/aes-256-gcm"
    pub active_key_id: Option<u32>,
    pub packets_sealed: u64,
    pub packets_opened: u64,
    pub crypto_errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyBlob {
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateKeyParams {
    pub key_id: u32,
    pub algo: u8,
    /// Key material on the wire as base64-standard (matches Go's default
    /// `json.Marshal([]byte)` behaviour). Custom serde below.
    #[serde(with = "wire_b64")]
    pub material: Vec<u8>,
}

pub mod wire_b64 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        use serde::de::Error;
        let s = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(s.as_bytes())
            .map_err(D::Error::custom)
    }
}

// ===========================================================================
// Tokio-based server skeleton
// ===========================================================================

#[cfg(unix)]
pub mod server {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{UnixListener, UnixStream};
    use tracing::{debug, error, info, warn};

    /// Trait implemented by whatever owns the actual crypto state in
    /// cryptod. Server stays decoupled from concrete provider / key store.
    #[async_trait::async_trait]
    pub trait Handler: Send + Sync + 'static {
        async fn handle(&self, req: Request) -> Response;
    }

    /// Start a UDS server. Accepts connections forever; spawns a task per
    /// connection. Each connection is line-framed JSON: one request →
    /// one response, then continue (long-lived control session).
    pub async fn serve<H: Handler, P: AsRef<Path>>(
        socket_path: P,
        handler: Arc<H>,
    ) -> std::io::Result<()> {
        let path = socket_path.as_ref();
        // Best-effort cleanup of stale socket from a previous run.
        let _ = std::fs::remove_file(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let listener = UnixListener::bind(path)?;
        info!(socket = %path.display(), "cryptod IPC listening");

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let h = Arc::clone(&handler);
                    tokio::spawn(async move {
                        if let Err(e) = serve_connection(stream, h).await {
                            warn!(error = %e, "connection ended with error");
                        }
                    });
                }
                Err(e) => {
                    error!(error = %e, "accept failed");
                    return Err(e);
                }
            }
        }
    }

    async fn serve_connection<H: Handler>(
        stream: UnixStream,
        handler: Arc<H>,
    ) -> std::io::Result<()> {
        let (rd, mut wr) = stream.into_split();
        let mut reader = BufReader::new(rd);
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                debug!("client closed");
                return Ok(());
            }
            let req: Request = match serde_json::from_str(line.trim()) {
                Ok(r) => r,
                Err(e) => {
                    let resp = Response::Error {
                        code: 400,
                        message: format!("bad request json: {}", e),
                    };
                    write_response(&mut wr, &resp).await?;
                    continue;
                }
            };
            debug!(?req, "incoming");
            let resp = handler.handle(req).await;
            write_response(&mut wr, &resp).await?;
        }
    }

    async fn write_response(
        wr: &mut tokio::net::unix::OwnedWriteHalf,
        resp: &Response,
    ) -> std::io::Result<()> {
        let mut s = serde_json::to_string(resp).expect("Response serializable");
        s.push('\n');
        wr.write_all(s.as_bytes()).await?;
        wr.flush().await
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_get_status() {
        let req = Request::GetStatus;
        let j = serde_json::to_string(&req).unwrap();
        assert_eq!(j, r#"{"method":"get_status"}"#);
    }

    #[test]
    fn serialize_rotate_key_uses_base64() {
        let req = Request::RotateKey(RotateKeyParams {
            key_id: 42,
            algo: 0x02,
            material: vec![0xDE, 0xAD, 0xBE, 0xEF],
        });
        let j = serde_json::to_string(&req).unwrap();
        assert!(j.contains(r#""method":"rotate_key""#));
        assert!(j.contains(r#""key_id":42"#));
        assert!(j.contains(r#""algo":2"#));
        // 0xDEADBEEF == base64 standard "3q2+7w=="
        assert!(j.contains(r#""material":"3q2+7w==""#));
    }

    #[test]
    fn deserialize_rotate_key_from_base64() {
        let j = r#"{"method":"rotate_key","params":{"key_id":1,"algo":1,"material":"AQID"}}"#;
        let req: Request = serde_json::from_str(j).unwrap();
        match req {
            Request::RotateKey(p) => {
                assert_eq!(p.key_id, 1);
                assert_eq!(p.algo, 1);
                assert_eq!(p.material, vec![0x01, 0x02, 0x03]);
            }
            _ => panic!("expected RotateKey"),
        }
    }

    #[test]
    fn parse_status_response() {
        let payload = r#"{"result":"status","data":{
            "version":"0.1.0","running":true,"uptime_s":42,
            "active_provider":"ring/aes-256-gcm","active_key_id":7,
            "packets_sealed":1000,"packets_opened":999,"crypto_errors":0
        }}"#;
        let r: Response = serde_json::from_str(payload).unwrap();
        match r {
            Response::Status(s) => {
                assert_eq!(s.version, "0.1.0");
                assert!(s.running);
                assert_eq!(s.active_key_id, Some(7));
            }
            other => panic!("expected Status, got {:?}", other),
        }
    }

    #[test]
    fn parse_ok_response() {
        let r: Response = serde_json::from_str(r#"{"result":"ok"}"#).unwrap();
        assert!(matches!(r, Response::Ok));
    }
}
