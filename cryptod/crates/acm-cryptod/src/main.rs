//! ACM-UZ crypto datapath daemon (`acm-cryptod`).
//!
//! Runs on a Smart SFP ISM4120I, encrypts/decrypts transit traffic between
//! `gbe0` (line) and `gbe1` (host) in bridge-or-routing mode. Talks to the
//! local `acm-agent` over Unix Domain Socket for control plane and
//! shared-memory ring for high-rate stats.
//!
//! This file is the bootstrap skeleton. Real pipeline lives in worker
//! threads pinned to a dedicated CPU core; see `acm-dpdk`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use tokio::sync::Mutex;
use tracing::info;

use acm_crypto::aes_gcm_ring::AesGcmRingProvider;
use acm_crypto::aes_gcm_sw::AesGcmSwProvider;
use acm_crypto::{AlgoId, CryptoProvider, KeyHandle};
use acm_ipc::server::Handler;
use acm_ipc::{
    Bytes, DecryptParams, EncryptParams, Request, Response, StatusReport, DEFAULT_SOCKET_PATH,
};
use acm_wire::{self, frame_len};

#[derive(Debug, Parser)]
#[command(name = "acm-cryptod", version, about = "ACM-UZ crypto datapath", long_about = None)]
struct Cli {
    /// Path to TOML configuration file.
    #[arg(short, long, default_value = "/etc/acm/cryptod.toml")]
    config: String,

    /// Run a single self-test (KAT against the active provider) and exit.
    #[arg(long)]
    selftest: bool,

    /// Run the built-in AES-GCM micro-benchmark and exit. Output is
    /// formatted similarly to `openssl speed` for direct comparison.
    #[arg(long)]
    bench: bool,

    /// How many seconds to bench each block size in --bench mode.
    #[arg(long, default_value_t = 3)]
    bench_seconds: u64,

    /// Unix domain socket path for control-plane IPC. Empty disables.
    #[arg(long, default_value_t = DEFAULT_SOCKET_PATH.to_string())]
    ipc_socket: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,acm_cryptod=debug")),
        )
        .with_target(false)
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        arch = std::env::consts::ARCH,
        os = std::env::consts::OS,
        "ACM-UZ cryptod starting"
    );

    if cli.bench {
        return run_bench(cli.bench_seconds);
    }

    // For Phase 1 (no DPDK datapath yet), the daemon's job is to bring up
    // the IPC server with a real crypto provider behind it. The agent can
    // already exercise key rotation and status reads end-to-end.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    rt.block_on(async {
        let state = Arc::new(CryptodState::new()?);
        let socket = cli.ipc_socket.clone();
        info!(socket = %socket, "starting IPC server");
        acm_ipc::server::serve(socket, state).await?;
        anyhow::Ok(())
    })?;

    Ok(())
}

// ============================================================================
// Cryptod state — what's held in memory and exposed via IPC.
// ============================================================================

struct CryptodState {
    started_at: Instant,
    // Active provider (ring AES-256-GCM by default, swap on RotateKey).
    provider: Mutex<Box<dyn CryptoProvider>>,
    active_algo: Mutex<AlgoId>,
    active_key: Mutex<Option<KeyHandle>>,
    // Counters incremented by the datapath; for Phase 1 only RotateKey /
    // GetStatus paths touch them but the type is real.
    packets_sealed: AtomicU64,
    packets_opened: AtomicU64,
    crypto_errors: AtomicU64,
}

impl CryptodState {
    fn new() -> Result<Self> {
        let default_algo = AlgoId::Aes256Gcm;
        let provider = Box::new(AesGcmRingProvider::new(default_algo)?) as Box<dyn CryptoProvider>;
        Ok(Self {
            started_at: Instant::now(),
            provider: Mutex::new(provider),
            active_algo: Mutex::new(default_algo),
            active_key: Mutex::new(None),
            packets_sealed: AtomicU64::new(0),
            packets_opened: AtomicU64::new(0),
            crypto_errors: AtomicU64::new(0),
        })
    }

    fn provider_name(&self, algo: AlgoId) -> String {
        format!(
            "ring/{}",
            match algo {
                AlgoId::Aes128Gcm => "aes-128-gcm",
                AlgoId::Aes256Gcm => "aes-256-gcm",
                AlgoId::OzDst1105 => "ozdst1105",
            }
        )
    }
}

#[async_trait::async_trait]
impl Handler for CryptodState {
    async fn handle(&self, req: Request) -> Response {
        match req {
            Request::GetStatus => {
                let algo = *self.active_algo.lock().await;
                let key = self.active_key.lock().await;
                Response::Status(StatusReport {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    running: true,
                    uptime_s: self.started_at.elapsed().as_secs(),
                    active_provider: self.provider_name(algo),
                    active_key_id: key.as_ref().map(|k| k.id),
                    packets_sealed: self.packets_sealed.load(Ordering::Relaxed),
                    packets_opened: self.packets_opened.load(Ordering::Relaxed),
                    crypto_errors: self.crypto_errors.load(Ordering::Relaxed),
                })
            }
            Request::RotateKey(p) => {
                let new_algo = match AlgoId::from_u8(p.algo) {
                    Some(a) => a,
                    None => {
                        return Response::Error {
                            code: 422,
                            message: format!("unknown algo: 0x{:02x}", p.algo),
                        }
                    }
                };
                let new_provider: Box<dyn CryptoProvider> = match new_algo {
                    AlgoId::Aes128Gcm | AlgoId::Aes256Gcm => {
                        match AesGcmRingProvider::new(new_algo) {
                            Ok(p) => Box::new(p),
                            Err(e) => {
                                return Response::Error {
                                    code: 500,
                                    message: e.to_string(),
                                }
                            }
                        }
                    }
                    AlgoId::OzDst1105 => {
                        // Eventually: certified provider. For now,
                        // explicitly unsupported so admin sees a clear
                        // error rather than silent acceptance.
                        return Response::Error {
                            code: 501,
                            message: "O'z DSt 1105 provider not yet implemented".into(),
                        };
                    }
                };
                let new_key = match KeyHandle::new(p.key_id, new_algo, p.material) {
                    Ok(k) => k,
                    Err(e) => {
                        return Response::Error {
                            code: 422,
                            message: e.to_string(),
                        }
                    }
                };

                // Self-test the new provider+key with a roundtrip on
                // a small known plaintext before committing.
                if let Err(e) = self_test(new_provider.as_ref(), &new_key) {
                    return Response::Error {
                        code: 500,
                        message: format!("new key/provider failed self-test: {}", e),
                    };
                }

                *self.provider.lock().await = new_provider;
                *self.active_algo.lock().await = new_algo;
                *self.active_key.lock().await = Some(new_key);
                Response::Ok
            }
            Request::SetPolicy(_) => {
                // Policy parsing TBD; we accept and ack for now.
                Response::Ok
            }
            Request::Encrypt(p) => self.handle_encrypt(p).await,
            Request::Decrypt(p) => self.handle_decrypt(p).await,
        }
    }
}

impl CryptodState {
    async fn handle_encrypt(&self, p: EncryptParams) -> Response {
        // Need both an active key and the corresponding provider.
        let key_guard = self.active_key.lock().await;
        let key = match key_guard.as_ref() {
            Some(k) => k.clone(),
            None => {
                return Response::Error {
                    code: 409,
                    message: "no active key — call rotate_key first".into(),
                }
            }
        };
        drop(key_guard);
        let provider = self.provider.lock().await;
        let algo = provider.algorithm();

        // Validate nonce length up-front so we get a clean error.
        if p.nonce.len() != algo.nonce_len() {
            return Response::Error {
                code: 422,
                message: format!(
                    "nonce length mismatch: got {}, want {} for {:?}",
                    p.nonce.len(),
                    algo.nonce_len(),
                    algo,
                ),
            };
        }

        let need = frame_len(p.plaintext.len(), p.nonce.len(), algo.tag_len());
        let mut out = vec![0u8; need];
        match acm_wire::seal(
            provider.as_ref(),
            &key,
            key.id,
            &p.nonce,
            /*flags=*/ 0,
            &p.plaintext,
            &mut out,
        ) {
            Ok(n) => {
                debug_assert_eq!(n, need);
                self.packets_sealed.fetch_add(1, Ordering::Relaxed);
                Response::Ciphertext(Bytes::from(out))
            }
            Err(e) => {
                self.crypto_errors.fetch_add(1, Ordering::Relaxed);
                Response::Error {
                    code: 500,
                    message: format!("seal failed: {}", e),
                }
            }
        }
    }

    async fn handle_decrypt(&self, p: DecryptParams) -> Response {
        let key_guard = self.active_key.lock().await;
        let key = match key_guard.as_ref() {
            Some(k) => k.clone(),
            None => {
                return Response::Error {
                    code: 409,
                    message: "no active key".into(),
                }
            }
        };
        drop(key_guard);
        let provider = self.provider.lock().await;

        // Worst-case plaintext is frame.len() (no nonce/header/tag) — that's
        // a safe upper bound for the out buffer.
        let mut out = vec![0u8; p.frame.len()];
        match acm_wire::open(provider.as_ref(), &key, &p.frame, &mut out) {
            Ok(n) => {
                out.truncate(n);
                self.packets_opened.fetch_add(1, Ordering::Relaxed);
                Response::Plaintext(Bytes::from(out))
            }
            Err(e) => {
                self.crypto_errors.fetch_add(1, Ordering::Relaxed);
                // 401-class for authentication failures, 400-class for
                // malformed input.
                let code = match e {
                    acm_wire::WireError::Crypto(acm_crypto::CryptoError::AuthFailed) => 401,
                    _ => 422,
                };
                Response::Error {
                    code,
                    message: format!("open failed: {}", e),
                }
            }
        }
    }
}

fn self_test(provider: &dyn CryptoProvider, key: &KeyHandle) -> Result<()> {
    let algo = provider.algorithm();
    let nonce = vec![0u8; algo.nonce_len()];
    let pt = b"acm-uz-self-test";
    let mut sealed = vec![0u8; pt.len() + algo.tag_len()];
    let n = provider.seal(key, &nonce, b"", pt, &mut sealed)?;
    let mut back = vec![0u8; pt.len()];
    provider.open(key, &nonce, b"", &sealed[..n], &mut back)?;
    if back != pt {
        anyhow::bail!("self-test roundtrip mismatch");
    }
    Ok(())
}

// ============================================================================
// Bench mode — unchanged from prior iteration.
// ============================================================================

/// Built-in micro-benchmark of the software AES-GCM provider.
fn run_bench(duration_s: u64) -> Result<()> {
    let sizes: [usize; 6] = [16, 64, 256, 1024, 8192, 16384];
    let duration = Duration::from_secs(duration_s);

    println!(
        "acm-cryptod bench  version={}  target={}  duration_per_size={}s",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH,
        duration_s
    );
    println!("Compare two software AES-GCM providers on the same hardware:");
    println!("  rustcrypto = `aes-gcm 0.10` (KAT reference; AArch64 stable Rust");
    println!("               falls back to bitsliced software AES - expect ~120 Mbps).");
    println!("  ring       = `ring 0.17` (BoringSSL ASM; uses ARMv8 Crypto Extensions");
    println!("               on AArch64 stable - expect line-rate AES-GCM).");
    println!();

    for algo in [AlgoId::Aes128Gcm, AlgoId::Aes256Gcm] {
        bench_one(
            "rustcrypto",
            algo,
            duration,
            &sizes,
            &AesGcmSwProvider::new(algo)?,
        )?;
        bench_one(
            "ring      ",
            algo,
            duration,
            &sizes,
            &AesGcmRingProvider::new(algo)?,
        )?;
    }
    Ok(())
}

fn bench_one(
    label: &str,
    algo: AlgoId,
    duration: Duration,
    sizes: &[usize],
    provider: &dyn CryptoProvider,
) -> Result<()> {
    let key = vec![0u8; algo.key_len()];
    let nonce = vec![0u8; algo.nonce_len()];
    let aad: &[u8] = b"";
    let key_handle = KeyHandle::new(0, algo, key)?;

    println!("=== {:?}  [{}] ===", algo, label.trim());
    println!(
        "{:>7}  {:>14}  {:>14}  {:>14}  {:>14}",
        "block", "seal ops/s", "seal Mbps", "open ops/s", "open Mbps",
    );

    for &size in sizes {
        let plaintext = vec![0u8; size];
        let mut sealed = vec![0u8; size + algo.tag_len()];
        let mut opened = vec![0u8; size];

        // Seal
        let warm_end = Instant::now() + Duration::from_millis(50);
        while Instant::now() < warm_end {
            provider.seal(&key_handle, &nonce, aad, &plaintext, &mut sealed)?;
        }
        let mut seal_ops: u64 = 0;
        let start = Instant::now();
        while start.elapsed() < duration {
            for _ in 0..256 {
                let n = provider.seal(&key_handle, &nonce, aad, &plaintext, &mut sealed)?;
                std::hint::black_box(n);
            }
            seal_ops += 256;
        }
        let seal_elapsed = start.elapsed();
        let seal_ops_per_sec = seal_ops as f64 / seal_elapsed.as_secs_f64();
        let seal_mbps = (seal_ops_per_sec * size as f64) * 8.0 / 1_000_000.0;

        // Open
        let n_ct = provider.seal(&key_handle, &nonce, aad, &plaintext, &mut sealed)?;
        let warm_end = Instant::now() + Duration::from_millis(50);
        while Instant::now() < warm_end {
            provider.open(&key_handle, &nonce, aad, &sealed[..n_ct], &mut opened)?;
        }
        let mut open_ops: u64 = 0;
        let start = Instant::now();
        while start.elapsed() < duration {
            for _ in 0..256 {
                let n = provider.open(&key_handle, &nonce, aad, &sealed[..n_ct], &mut opened)?;
                std::hint::black_box(n);
            }
            open_ops += 256;
        }
        let open_elapsed = start.elapsed();
        let open_ops_per_sec = open_ops as f64 / open_elapsed.as_secs_f64();
        let open_mbps = (open_ops_per_sec * size as f64) * 8.0 / 1_000_000.0;

        println!(
            "{:>5} B  {:>14.0}  {:>11.1} Mb/s  {:>14.0}  {:>11.1} Mb/s",
            size, seal_ops_per_sec, seal_mbps, open_ops_per_sec, open_mbps,
        );
    }
    println!();
    Ok(())
}
