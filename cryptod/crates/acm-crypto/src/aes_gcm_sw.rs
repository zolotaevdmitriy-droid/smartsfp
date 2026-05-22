//! Software AES-GCM provider via the RustCrypto `aes-gcm` crate.
//!
//! On aarch64 with ARMv8 Crypto Extensions (which our ISM4120I has — the
//! Cortex-A53 reports `aes` and `pmull` in `/proc/cpuinfo`) the crate
//! routes through hardware AES instructions, giving ~800 Мбит/с–1.2 Гбит/с
//! single-core. Fallback on systems without those extensions is pure-Rust.
//!
//! This is the **Phase 1 PoC provider**. The eventual production
//! production-grade AES path on the module goes through Marvell SAM
//! (EIP-97) via MUSDK / DPDK `rte_crypto_mvsam` — that provider lives in
//! a separate module (TBD).

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Aes256Gcm};

use crate::{AlgoId, CryptoError, CryptoProvider, KeyHandle};

/// Software AES-GCM provider. Holds no per-call state; safe to share.
///
/// The `algo` is chosen at construction time; the key passed in `seal/open`
/// must match (we verify and return [`CryptoError::AlgorithmMismatch`]).
pub struct AesGcmSwProvider {
    algo: AlgoId,
}

impl AesGcmSwProvider {
    pub fn new(algo: AlgoId) -> Result<Self, CryptoError> {
        match algo {
            AlgoId::Aes128Gcm | AlgoId::Aes256Gcm => Ok(Self { algo }),
            other => Err(CryptoError::ProviderUnavailable(format!(
                "AesGcmSwProvider doesn't support {:?}",
                other
            ))),
        }
    }

    fn check_inputs(
        &self,
        key: &KeyHandle,
        nonce: &[u8],
        out: &mut [u8],
        body_len: usize,
        is_seal: bool,
    ) -> Result<(), CryptoError> {
        if key.algo != self.algo {
            return Err(CryptoError::AlgorithmMismatch {
                provider: self.algo,
                key: key.algo,
            });
        }
        if key.material.len() != self.algo.key_len() {
            return Err(CryptoError::InvalidKey {
                expected: self.algo.key_len(),
                got: key.material.len(),
            });
        }
        if nonce.len() != self.algo.nonce_len() {
            return Err(CryptoError::InvalidNonce {
                expected: self.algo.nonce_len(),
                got: nonce.len(),
            });
        }
        let need = if is_seal {
            body_len + self.algo.tag_len()
        } else {
            body_len.saturating_sub(self.algo.tag_len())
        };
        if out.len() < need {
            return Err(CryptoError::BufferTooSmall {
                need,
                have: out.len(),
            });
        }
        Ok(())
    }
}

impl CryptoProvider for AesGcmSwProvider {
    fn algorithm(&self) -> AlgoId {
        self.algo
    }

    fn seal(
        &self,
        key: &KeyHandle,
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, CryptoError> {
        self.check_inputs(key, nonce, out, plaintext.len(), true)?;

        let pt_len = plaintext.len();
        let tag_len = self.algo.tag_len();

        // The `aes-gcm` crate operates in-place: copy plaintext into out,
        // then call encrypt_in_place_detached(nonce, aad, &mut out[..pt_len])
        // which mutates the buffer and returns a Tag.
        out[..pt_len].copy_from_slice(plaintext);
        let tag = match self.algo {
            AlgoId::Aes128Gcm => {
                let cipher = Aes128Gcm::new_from_slice(&key.material).map_err(|_| {
                    CryptoError::InvalidKey {
                        expected: 16,
                        got: key.material.len(),
                    }
                })?;
                cipher
                    .encrypt_in_place_detached(nonce.into(), aad, &mut out[..pt_len])
                    .map_err(|_| CryptoError::ProviderUnavailable("AES-128-GCM seal".into()))?
            }
            AlgoId::Aes256Gcm => {
                let cipher = Aes256Gcm::new_from_slice(&key.material).map_err(|_| {
                    CryptoError::InvalidKey {
                        expected: 32,
                        got: key.material.len(),
                    }
                })?;
                cipher
                    .encrypt_in_place_detached(nonce.into(), aad, &mut out[..pt_len])
                    .map_err(|_| CryptoError::ProviderUnavailable("AES-256-GCM seal".into()))?
            }
            _ => unreachable!("checked in new()"),
        };

        out[pt_len..pt_len + tag_len].copy_from_slice(tag.as_slice());
        Ok(pt_len + tag_len)
    }

    fn open(
        &self,
        key: &KeyHandle,
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
        out: &mut [u8],
    ) -> Result<usize, CryptoError> {
        let tag_len = self.algo.tag_len();
        if ciphertext.len() < tag_len {
            return Err(CryptoError::AuthFailed);
        }
        let pt_len = ciphertext.len() - tag_len;
        self.check_inputs(key, nonce, out, ciphertext.len(), false)?;

        // Split ct||tag, copy ct into out, then decrypt_in_place_detached.
        out[..pt_len].copy_from_slice(&ciphertext[..pt_len]);
        let tag = aes_gcm::Tag::from_slice(&ciphertext[pt_len..]);

        match self.algo {
            AlgoId::Aes128Gcm => {
                let cipher = Aes128Gcm::new_from_slice(&key.material).map_err(|_| {
                    CryptoError::InvalidKey {
                        expected: 16,
                        got: key.material.len(),
                    }
                })?;
                cipher
                    .decrypt_in_place_detached(nonce.into(), aad, &mut out[..pt_len], tag)
                    .map_err(|_| CryptoError::AuthFailed)?;
            }
            AlgoId::Aes256Gcm => {
                let cipher = Aes256Gcm::new_from_slice(&key.material).map_err(|_| {
                    CryptoError::InvalidKey {
                        expected: 32,
                        got: key.material.len(),
                    }
                })?;
                cipher
                    .decrypt_in_place_detached(nonce.into(), aad, &mut out[..pt_len], tag)
                    .map_err(|_| CryptoError::AuthFailed)?;
            }
            _ => unreachable!(),
        }

        Ok(pt_len)
    }
}

// ============================================================================
// Tests — NIST CAVS vectors + roundtrip + tamper detection
// ============================================================================
//
// Vectors taken from the NIST GCM Validation Test Suite:
//   https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program/cavp-testing-block-cipher-modes
// Specifically gcmEncryptExtIV128.rsp / gcmEncryptExtIV256.rsp.
// We hard-code one representative vector per key size so the build doesn't
// pull in megabytes of .rsp files; the same `aes-gcm` crate is tested
// against the full corpus upstream.

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    /// McGrew & Viega "The Galois/Counter Mode of Operation (GCM)",
    /// Test Case 2 (AES-128, K=zeros, IV=zeros, P=16 bytes of zeros, A=empty).
    /// Widely-cited canonical GCM vector.
    #[test]
    fn aes128_gcm_mcgrew_viega_test2() {
        let key = hex("00000000000000000000000000000000");
        let iv = hex("000000000000000000000000");
        let aad: &[u8] = b"";
        let pt = hex("00000000000000000000000000000000");
        let ct_expected = hex("0388dace60b6a392f328c2b971b2fe78");
        let tag_expected = hex("ab6e47d42cec13bdf53a67b21257bddf");

        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, key).unwrap();
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();

        let mut out = vec![0u8; pt.len() + 16];
        let n = prov.seal(&kh, &iv, aad, &pt, &mut out).unwrap();
        assert_eq!(n, pt.len() + 16);
        assert_eq!(&out[..pt.len()], &ct_expected[..], "ciphertext mismatch");
        assert_eq!(&out[pt.len()..n], &tag_expected[..], "tag mismatch");

        let mut pt_back = vec![0u8; pt.len()];
        let m = prov.open(&kh, &iv, aad, &out[..n], &mut pt_back).unwrap();
        assert_eq!(m, pt.len());
        assert_eq!(&pt_back[..], &pt[..]);
    }

    /// McGrew & Viega Test Case 14 (AES-256, K=zeros, IV=zeros,
    /// P=16 bytes of zeros, A=empty).
    #[test]
    fn aes256_gcm_mcgrew_viega_test14() {
        let key = hex("0000000000000000000000000000000000000000000000000000000000000000");
        let iv = hex("000000000000000000000000");
        let aad: &[u8] = b"";
        let pt = hex("00000000000000000000000000000000");
        let ct_expected = hex("cea7403d4d606b6e074ec5d3baf39d18");
        let tag_expected = hex("d0d1c8a799996bf0265b98b5d48ab919");

        let kh = KeyHandle::new(7, AlgoId::Aes256Gcm, key).unwrap();
        let prov = AesGcmSwProvider::new(AlgoId::Aes256Gcm).unwrap();

        let mut out = vec![0u8; pt.len() + 16];
        let n = prov.seal(&kh, &iv, aad, &pt, &mut out).unwrap();
        assert_eq!(&out[..pt.len()], &ct_expected[..]);
        assert_eq!(&out[pt.len()..n], &tag_expected[..]);

        let mut pt_back = vec![0u8; pt.len()];
        prov.open(&kh, &iv, aad, &out[..n], &mut pt_back).unwrap();
        assert_eq!(&pt_back[..], &pt[..]);
    }

    /// McGrew & Viega Test Case 4 (AES-128, K and IV from test 3,
    /// P = 60 bytes of structured data, A = 20 bytes of structured AAD).
    /// Tests AAD path along with multi-block plaintext.
    #[test]
    fn aes128_gcm_mcgrew_viega_test4() {
        let key = hex("feffe9928665731c6d6a8f9467308308");
        let iv = hex("cafebabefacedbaddecaf888");
        let aad = hex("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let pt = hex(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72\
             1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let ct_expected = hex(
            "42831ec2217774244b7221b784d0d49ce3aa212f2c02a4e035c17e2329aca12e\
             21d514b25466931c7d8f6a5aac84aa051ba30b396a0aac973d58e091",
        );
        let tag_expected = hex("5bc94fbc3221a5db94fae95ae7121a47");

        let kh = KeyHandle::new(42, AlgoId::Aes128Gcm, key).unwrap();
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();

        let mut out = vec![0u8; pt.len() + 16];
        let n = prov.seal(&kh, &iv, &aad, &pt, &mut out).unwrap();
        assert_eq!(&out[..pt.len()], &ct_expected[..]);
        assert_eq!(&out[pt.len()..n], &tag_expected[..]);

        let mut pt_back = vec![0u8; pt.len()];
        prov.open(&kh, &iv, &aad, &out[..n], &mut pt_back).unwrap();
        assert_eq!(&pt_back[..], &pt[..]);
    }

    #[test]
    fn roundtrip_various_sizes() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes256Gcm).unwrap();
        let key = vec![0x42u8; 32];
        let kh = KeyHandle::new(99, AlgoId::Aes256Gcm, key).unwrap();
        let nonce = vec![0xCCu8; 12];
        let aad = b"acm-uz-frame-aad-v1";

        for &n in &[0usize, 1, 15, 16, 17, 64, 1500, 9000] {
            let pt: Vec<u8> = (0..n).map(|i| (i & 0xff) as u8).collect();
            let mut sealed = vec![0u8; pt.len() + 16];
            let total = prov.seal(&kh, &nonce, aad, &pt, &mut sealed).unwrap();
            assert_eq!(total, pt.len() + 16);

            let mut opened = vec![0u8; pt.len()];
            let m = prov.open(&kh, &nonce, aad, &sealed[..total], &mut opened).unwrap();
            assert_eq!(m, pt.len());
            assert_eq!(opened, pt, "size {} roundtrip failed", n);
        }
    }

    #[test]
    fn open_fails_on_tampered_ciphertext() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]).unwrap();
        let nonce = [0u8; 12];
        let pt = b"hello acm";
        let mut sealed = vec![0u8; pt.len() + 16];
        let n = prov.seal(&kh, &nonce, b"", pt, &mut sealed).unwrap();

        // Flip one bit in ciphertext.
        sealed[0] ^= 0x01;
        let mut out = vec![0u8; pt.len()];
        let r = prov.open(&kh, &nonce, b"", &sealed[..n], &mut out);
        assert!(matches!(r, Err(CryptoError::AuthFailed)));
    }

    #[test]
    fn open_fails_on_tampered_aad() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]).unwrap();
        let nonce = [0u8; 12];
        let pt = b"hello acm";
        let mut sealed = vec![0u8; pt.len() + 16];
        let n = prov.seal(&kh, &nonce, b"orig-aad", pt, &mut sealed).unwrap();

        let mut out = vec![0u8; pt.len()];
        let r = prov.open(&kh, &nonce, b"different-aad", &sealed[..n], &mut out);
        assert!(matches!(r, Err(CryptoError::AuthFailed)));
    }

    #[test]
    fn algorithm_mismatch_rejected() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(1, AlgoId::Aes256Gcm, vec![0u8; 32]).unwrap();
        let nonce = [0u8; 12];
        let mut sealed = vec![0u8; 16];
        let r = prov.seal(&kh, &nonce, b"", b"", &mut sealed);
        assert!(matches!(
            r,
            Err(CryptoError::AlgorithmMismatch {
                provider: AlgoId::Aes128Gcm,
                key: AlgoId::Aes256Gcm
            })
        ));
    }

    #[test]
    fn bad_nonce_length_rejected() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]).unwrap();
        let bad_nonce = [0u8; 8]; // GCM wants 12
        let mut sealed = vec![0u8; 16];
        let r = prov.seal(&kh, &bad_nonce, b"", b"", &mut sealed);
        assert!(matches!(
            r,
            Err(CryptoError::InvalidNonce {
                expected: 12,
                got: 8
            })
        ));
    }
}
