//! ACM-UZ crypto datapath daemon (`acm-cryptod`).
//!
//! Runs on a Smart SFP ISM4120I, encrypts/decrypts transit traffic between
//! `gbe0` (line) and `gbe1` (host) in bridge-or-routing mode. Talks to the
//! local `acm-agent` over Unix Domain Socket for control plane and
//! shared-memory ring for high-rate stats.
//!
//! This file is the bootstrap skeleton. Real pipeline lives in worker
//! threads pinned to a dedicated CPU core; see `acm-dpdk`.

use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use tracing::info;

use acm_crypto::aes_gcm_ring::AesGcmRingProvider;
use acm_crypto::aes_gcm_sw::AesGcmSwProvider;
use acm_crypto::{AlgoId, CryptoProvider, KeyHandle};

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

    info!(config = %cli.config, selftest = cli.selftest, "configuration");
    info!("Hello, ACM-UZ. Pipeline not yet implemented.");

    // TODO Phase 1: load config, initialize DPDK EAL, set up cryptodev_mvsam,
    // bind gbe0/gbe1, start worker thread, start IPC server, signal handlers.

    Ok(())
}

/// Built-in micro-benchmark of the software AES-GCM provider.
///
/// Pinned to single thread (we don't spawn). Repeats `seal()` on a
/// pre-allocated buffer for `duration_s` seconds per block size, then
/// `open()` the same number of times. Output format mirrors
/// `openssl speed -elapsed -evp aes-...-gcm` so two tables can be
/// compared cell-by-cell.
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
        // RustCrypto provider — KAT-reference
        bench_one(
            "rustcrypto",
            algo,
            duration,
            &sizes,
            &AesGcmSwProvider::new(algo)?,
        )?;
        // ring provider — HW AES on AArch64 stable
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

    {
        println!("=== {:?}  [{}] ===", algo, label.trim());
        println!(
            "{:>7}  {:>14}  {:>14}  {:>14}  {:>14}",
            "block", "seal ops/s", "seal Mbps", "open ops/s", "open Mbps",
        );

        for &size in sizes {
            let plaintext = vec![0u8; size];
            let mut sealed = vec![0u8; size + algo.tag_len()];
            let mut opened = vec![0u8; size];

            // ---- Seal benchmark ----
            // Warmup ~50 ms to settle CPU caches / frequency.
            let warm_end = Instant::now() + Duration::from_millis(50);
            while Instant::now() < warm_end {
                provider.seal(&key_handle, &nonce, aad, &plaintext, &mut sealed)?;
            }

            let mut seal_ops: u64 = 0;
            let start = Instant::now();
            while start.elapsed() < duration {
                // Inner batch so we don't query the clock every iteration.
                for _ in 0..256 {
                    let n = provider.seal(&key_handle, &nonce, aad, &plaintext, &mut sealed)?;
                    // black_box prevents the optimizer from eliding the
                    // result usage.
                    std::hint::black_box(n);
                }
                seal_ops += 256;
            }
            let seal_elapsed = start.elapsed();
            let seal_ops_per_sec = seal_ops as f64 / seal_elapsed.as_secs_f64();
            let seal_mbps = (seal_ops_per_sec * size as f64) * 8.0 / 1_000_000.0;

            // ---- Open benchmark ----
            // First produce one good sealed buffer to open repeatedly.
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
    }

    Ok(())
}
