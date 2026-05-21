//! IPC layer between the Rust `acm-cryptod` and the Go `acm-agent`.
//!
//! Two channels live on the module:
//!
//! 1. **Control plane** — JSON-RPC over Unix Domain Socket
//!    (`/run/acm/cryptod.sock`). Low rate: configure policies, rotate keys,
//!    get status, stream events. Easy to debug with `nc -U`.
//!
//! 2. **Stats fast-path** — lock-free shared-memory ring at
//!    `/run/acm/cryptod-stats.shm`. High rate: per-flow / per-port packet
//!    and byte counters updated by cryptod's worker threads, read
//!    periodically by the Go agent to publish to Prometheus.
//!
//! This crate only defines the message types and a Tokio-based UDS server.
//! Shared-memory ring lives in a separate crate (not yet implemented).

use serde::{Deserialize, Serialize};

/// Default socket path on the module. Created by the systemd unit.
pub const DEFAULT_SOCKET_PATH: &str = "/run/acm/cryptod.sock";

/// All control-plane requests the agent can send to cryptod.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    GetStatus,
    SetPolicy(PolicyBlob),
    RotateKey(RotateKeyParams),
    StreamEvents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", content = "data", rename_all = "snake_case")]
pub enum Response {
    Status(StatusReport),
    Ok,
    Error { code: u32, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub version: String,
    pub running: bool,
    pub packets_in: u64,
    pub packets_out: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub crypto_errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyBlob {
    /// Opaque policy bytes (protobuf or CBOR) — interpreted by cryptod.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateKeyParams {
    pub key_id: u32,
    pub algo: u8,
    pub material: Vec<u8>,
}

// TODO Phase 1: tokio UDS server skeleton + a single GetStatus handler.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_get_status() {
        let req = Request::GetStatus;
        let j = serde_json::to_string(&req).unwrap();
        assert!(j.contains("get_status"));
    }
}
