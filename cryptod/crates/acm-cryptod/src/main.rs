//! ACM-UZ crypto datapath daemon (`acm-cryptod`).
//!
//! Runs on a Smart SFP ISM4120I, encrypts/decrypts transit traffic between
//! `gbe0` (line) and `gbe1` (host) in bridge-or-routing mode. Talks to the
//! local `acm-agent` over Unix Domain Socket for control plane and
//! shared-memory ring for high-rate stats.
//!
//! This file is the bootstrap skeleton. Real pipeline lives in worker
//! threads pinned to a dedicated CPU core; see `acm-dpdk`.

use anyhow::Result;
use clap::Parser;
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "acm-cryptod", version, about = "ACM-UZ crypto datapath", long_about = None)]
struct Cli {
    /// Path to TOML configuration file.
    #[arg(short, long, default_value = "/etc/acm/cryptod.toml")]
    config: String,

    /// Run a single self-test (KAT against the active provider) and exit.
    #[arg(long)]
    selftest: bool,
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
    info!(config = %cli.config, selftest = cli.selftest, "configuration");

    info!("Hello, ACM-UZ. Pipeline not yet implemented.");

    // TODO Phase 1: load config, initialize DPDK EAL, set up cryptodev_mvsam,
    // bind gbe0/gbe1, start worker thread, start IPC server, signal handlers.

    Ok(())
}
