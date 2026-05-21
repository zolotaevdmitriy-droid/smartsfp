//! FFI to DPDK 25.0 and MUSDK on the ISM4120I (Marvell Armada 3720).
//!
//! Plan:
//! * Use [`bindgen`] at build time to generate Rust bindings from DPDK and
//!   MUSDK headers. Headers must be present in the builder image (we'll add
//!   them either by copying from the module's filesystem, or by pulling
//!   the matching DPDK 25.0 source tarball — TBD with vendor).
//!
//! * Wrap EAL init, ethdev lifecycle, mempool, crypto-dev (`rte_crypto_mvsam`)
//!   in safe Rust abstractions.
//!
//! * Pipeline workers run on dedicated CPU core (pinned via systemd
//!   `CPUAffinity=` plus DPDK lcore options).
//!
//! For now this crate is a placeholder so the workspace builds.

/// Marker that the DPDK runtime is not yet wired up. Will be removed
/// once the EAL bindings are in place.
pub const PLACEHOLDER: &str = "acm-dpdk: placeholder, no FFI yet";

// TODO Phase 1: pub mod eal;          // EAL init / shutdown
// TODO Phase 1: pub mod ethdev;       // port lifecycle, queue setup
// TODO Phase 1: pub mod mempool;
// TODO Phase 1: pub mod cryptodev_mvsam; // hardware AES via Marvell SAM

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert!(!super::PLACEHOLDER.is_empty());
    }
}
