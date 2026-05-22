//! Crypto provider abstraction for ACM-UZ.
//!
//! The whole point of this crate is to make the algorithm and the
//! implementation **swappable** without touching the transport / pipeline
//! code. We will go through several implementations:
//!
//! * Phase 1: software AES-GCM (this file, [`aes_gcm_sw`]) — uses the
//!   RustCrypto `aes-gcm` crate which on aarch64 auto-detects ARMv8
//!   Crypto Extensions at runtime (AESE/AESD/PMULL).
//! * Phase 1: hardware AES-GCM via Marvell SAM (EIP-97) through MUSDK /
//!   DPDK `rte_crypto_mvsam`. Line-rate 1 Gbps on ISM4120I. TBD.
//! * Phase 2: software O'z DSt 1105:2009 with NEON SIMD on ARMv8. TBD.
//! * Phase 3: certified Uzbek O'z DSt 1105 library (vendor SDK), loaded
//!   through the same trait. TBD.
//!
//! The wire format ([`acm-wire`]) carries the [`AlgoId`] in plaintext so
//! multiple algorithms can coexist during the migration period.

use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub mod aes_gcm_sw;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid key length (expected {expected}, got {got})")]
    InvalidKey { expected: usize, got: usize },
    #[error("invalid nonce length (expected {expected}, got {got})")]
    InvalidNonce { expected: usize, got: usize },
    #[error("output buffer too small (need at least {need} bytes, have {have})")]
    BufferTooSmall { need: usize, have: usize },
    #[error("authentication failed")]
    AuthFailed,
    #[error("algorithm mismatch (provider is {provider:?}, key is {key:?})")]
    AlgorithmMismatch { provider: AlgoId, key: AlgoId },
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

/// Opaque key handle.
///
/// Holds raw key material plus the algorithm it was bound to. `Drop`
/// zeroizes the buffer so master keys / session keys don't leak through
/// freed memory. Pipeline code never deals with bare `Vec<u8>` keys.
///
/// **Phase 2+:** replace `material` with a backend-specific opaque ref
/// (MUSDK SAM session slot, PKCS#11 object handle, TPM sealed blob, …)
/// without touching the trait or downstream code.
#[derive(Debug, Clone, ZeroizeOnDrop)]
pub struct KeyHandle {
    pub id: u32,
    #[zeroize(skip)]
    pub algo: AlgoId,
    pub material: Vec<u8>,
}

impl KeyHandle {
    pub fn new(id: u32, algo: AlgoId, material: Vec<u8>) -> Result<Self, CryptoError> {
        if material.len() != algo.key_len() {
            return Err(CryptoError::InvalidKey {
                expected: algo.key_len(),
                got: material.len(),
            });
        }
        Ok(Self { id, algo, material })
    }

    /// Erase key material immediately, before drop.
    pub fn wipe(&mut self) {
        self.material.zeroize();
    }
}

/// AEAD provider. All ACM-UZ transport encryption goes through this trait.
///
/// Implementations are expected to be **thread-safe** (the datapath holds
/// a single `Arc<dyn CryptoProvider>` shared across worker threads).
pub trait CryptoProvider: Send + Sync {
    /// The algorithm this provider implements.
    fn algorithm(&self) -> AlgoId;

    /// Encrypt `plaintext` with `aad`, writing ciphertext-then-tag into
    /// `out`. Returns total bytes written (`plaintext.len() + tag_len()`).
    ///
    /// Caller must pass `out` of at least `plaintext.len() + tag_len()`.
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
    ///
    /// Returns [`CryptoError::AuthFailed`] on tag mismatch.
    fn open(
        &self,
        key: &KeyHandle,
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, CryptoError>;
}

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
    fn algo_id_lengths() {
        assert_eq!(AlgoId::Aes128Gcm.key_len(), 16);
        assert_eq!(AlgoId::Aes256Gcm.key_len(), 32);
        assert_eq!(AlgoId::Aes128Gcm.nonce_len(), 12);
        assert_eq!(AlgoId::Aes128Gcm.tag_len(), 16);
    }

    #[test]
    fn keyhandle_rejects_wrong_length() {
        let bad = KeyHandle::new(1, AlgoId::Aes256Gcm, vec![0u8; 16]);
        assert!(matches!(
            bad,
            Err(CryptoError::InvalidKey {
                expected: 32,
                got: 16
            })
        ));
    }

    #[test]
    fn keyhandle_accepts_correct_length() {
        let ok = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]);
        assert!(ok.is_ok());
    }

    #[test]
    fn keyhandle_wipe_zeroes_material() {
        let mut k = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0xABu8; 16]).unwrap();
        k.wipe();
        assert!(k.material.iter().all(|b| *b == 0));
    }
}
