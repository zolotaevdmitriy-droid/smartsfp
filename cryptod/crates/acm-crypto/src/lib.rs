//! Crypto provider abstraction for ACM-UZ.
//!
//! The whole point of this crate is to make the algorithm and the
//! implementation **swappable** without touching the transport / pipeline
//! code. We will go through several implementations:
//!
//! * Phase 1: software AES-GCM (for unit tests + bringup).
//! * Phase 1: hardware AES-GCM via Marvell SAM (EIP-97) through MUSDK /
//!   DPDK `rte_crypto_mvsam`. Line-rate 1 Gbps on ISM4120I.
//! * Phase 2: software O'z DSt 1105:2009 with NEON SIMD on ARMv8.
//! * Phase 3: certified Uzbek O'z DSt 1105 library (vendor SDK), loaded
//!   through the same trait.
//!
//! Frame wire format carries the [`AlgoId`] in plaintext so multiple
//! algorithms can coexist during migration.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid key length")]
    InvalidKey,
    #[error("invalid nonce length")]
    InvalidNonce,
    #[error("output buffer too small")]
    BufferTooSmall,
    #[error("authentication failed")]
    AuthFailed,
    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Algorithm identifier — single byte embedded in every wire frame.
///
/// New algorithms get new IDs; old IDs are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AlgoId {
    Aes128Gcm = 0x01,
    Aes256Gcm = 0x02,
    /// O'z DSt 1105:2009 — Uzbek national block cipher. Mode (CBC / CTR /
    /// AEAD construction) TBD per certified library specification.
    OzDst1105 = 0x10,
}

impl AlgoId {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(AlgoId::Aes128Gcm),
            0x02 => Some(AlgoId::Aes256Gcm),
            0x10 => Some(AlgoId::OzDst1105),
            _ => None,
        }
    }

    pub fn key_len(&self) -> usize {
        match self {
            AlgoId::Aes128Gcm => 16,
            AlgoId::Aes256Gcm => 32,
            AlgoId::OzDst1105 => 32, // placeholder; confirm with standard
        }
    }

    pub fn nonce_len(&self) -> usize {
        match self {
            AlgoId::Aes128Gcm | AlgoId::Aes256Gcm => 12,
            AlgoId::OzDst1105 => 12, // placeholder; depends on AEAD mode
        }
    }

    pub fn tag_len(&self) -> usize {
        match self {
            AlgoId::Aes128Gcm | AlgoId::Aes256Gcm => 16,
            AlgoId::OzDst1105 => 16, // placeholder
        }
    }
}

/// Opaque key handle. Concrete providers decide what's behind it: raw bytes
/// in memory, a slot inside MUSDK SAM, a PKCS#11 object handle, a TPM
/// sealed blob, etc. Pipeline code never deals with raw key material.
#[derive(Debug, Clone)]
pub struct KeyHandle {
    pub id: u32,
    pub algo: AlgoId,
    // For Phase 1 — keep material inline. Phase 2+ — replace with opaque
    // backend-specific reference and a `Drop` impl that zeroizes.
    pub material: Vec<u8>,
}

/// AEAD provider. All ACM-UZ transport encryption goes through this trait.
pub trait CryptoProvider: Send + Sync {
    /// The algorithm this provider implements.
    fn algorithm(&self) -> AlgoId;

    /// Encrypt `plaintext` with `aad`, writing ciphertext-then-tag into `out`.
    /// Returns total bytes written (`plaintext.len() + algo.tag_len()`).
    fn seal(
        &self,
        key: &KeyHandle,
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, CryptoError>;

    /// Decrypt `ciphertext` (which has the tag appended) with `aad`,
    /// writing plaintext into `out`. Returns plaintext length.
    fn open(
        &self,
        key: &KeyHandle,
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, CryptoError>;
}

// TODO Phase 1: pub mod aes_gcm_sw;     // pure-Rust AES-GCM for tests
// TODO Phase 1: pub mod aes_gcm_sam;    // hardware AES-GCM via MUSDK SAM
// TODO Phase 2: pub mod ozdst1105_sw;   // own Rust impl, NEON-optimized
// TODO Phase 3: pub mod ozdst1105_certified;  // FFI to certified library

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algo_id_roundtrip() {
        for algo in [AlgoId::Aes128Gcm, AlgoId::Aes256Gcm, AlgoId::OzDst1105] {
            assert_eq!(AlgoId::from_u8(algo as u8), Some(algo));
        }
        assert_eq!(AlgoId::from_u8(0xFF), None);
    }

    #[test]
    fn algo_id_key_lengths() {
        assert_eq!(AlgoId::Aes128Gcm.key_len(), 16);
        assert_eq!(AlgoId::Aes256Gcm.key_len(), 32);
    }
}
