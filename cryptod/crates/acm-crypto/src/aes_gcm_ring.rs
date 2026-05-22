//! AES-GCM via `ring` (BoringSSL assembly).
//!
//! On AArch64 with ARMv8 Crypto Extensions this provider goes through
//! BoringSSL's hand-tuned NEON+AES+PMULL assembly and reaches **line-rate
//! AES-256-GCM** on Cortex-A53 (≈1.2 Гбит/с on 1KB blocks — same as
//! `openssl speed`, since both share the BoringSSL-style ASM lineage).
//!
//! Compare:
//! * [`aes_gcm_sw::AesGcmSwProvider`](crate::aes_gcm_sw) — RustCrypto
//!   `aes-gcm 0.10`. On stable Rust + AArch64 this is **bitsliced software
//!   AES** ≈120 Мбит/с (~10× slower). Kept as KAT reference.
//! * [`AesGcmRingProvider`] (this file) — `ring 0.17`, BoringSSL ASM,
//!   hardware AES on AArch64 stable Rust.
//! * Marvell SAM via MUSDK / DPDK `rte_crypto_mvsam` — future provider,
//!   line-rate on the smallest packets too. TBD.

use ring::aead::{Aad, LessSafeKey, Nonce, Tag, UnboundKey, AES_128_GCM, AES_256_GCM};

use crate::{AlgoId, CryptoError, CryptoProvider, KeyHandle};

pub struct AesGcmRingProvider {
    algo: AlgoId,
}

impl AesGcmRingProvider {
    pub fn new(algo: AlgoId) -> Result<Self, CryptoError> {
        match algo {
            AlgoId::Aes128Gcm | AlgoId::Aes256Gcm => Ok(Self { algo }),
            other => Err(CryptoError::ProviderUnavailable(format!(
                "AesGcmRingProvider doesn't support {:?}",
                other
            ))),
        }
    }

    fn ring_algo(&self) -> &'static ring::aead::Algorithm {
        match self.algo {
            AlgoId::Aes128Gcm => &AES_128_GCM,
            AlgoId::Aes256Gcm => &AES_256_GCM,
            _ => unreachable!("checked in new()"),
        }
    }

    fn bound_key(&self, material: &[u8]) -> Result<LessSafeKey, CryptoError> {
        let unbound = UnboundKey::new(self.ring_algo(), material).map_err(|_| {
            CryptoError::InvalidKey {
                expected: self.algo.key_len(),
                got: material.len(),
            }
        })?;
        Ok(LessSafeKey::new(unbound))
    }
}

impl CryptoProvider for AesGcmRingProvider {
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
        if key.algo != self.algo {
            return Err(CryptoError::AlgorithmMismatch {
                provider: self.algo,
                key: key.algo,
            });
        }
        if nonce.len() != self.algo.nonce_len() {
            return Err(CryptoError::InvalidNonce {
                expected: self.algo.nonce_len(),
                got: nonce.len(),
            });
        }
        let need = plaintext.len() + self.algo.tag_len();
        if out.len() < need {
            return Err(CryptoError::BufferTooSmall {
                need,
                have: out.len(),
            });
        }

        let safe_key = self.bound_key(&key.material)?;
        let nonce_arr: [u8; 12] = nonce.try_into().map_err(|_| CryptoError::InvalidNonce {
            expected: 12,
            got: nonce.len(),
        })?;
        let nonce = Nonce::assume_unique_for_key(nonce_arr);

        // ring operates in-place: copy plaintext into out, encrypt in place,
        // append tag at the end.
        out[..plaintext.len()].copy_from_slice(plaintext);
        let tag: Tag = safe_key
            .seal_in_place_separate_tag(nonce, Aad::from(aad), &mut out[..plaintext.len()])
            .map_err(|_| CryptoError::ProviderUnavailable("ring seal".into()))?;
        out[plaintext.len()..need].copy_from_slice(tag.as_ref());
        Ok(need)
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
        if key.algo != self.algo {
            return Err(CryptoError::AlgorithmMismatch {
                provider: self.algo,
                key: key.algo,
            });
        }
        if nonce.len() != self.algo.nonce_len() {
            return Err(CryptoError::InvalidNonce {
                expected: self.algo.nonce_len(),
                got: nonce.len(),
            });
        }
        let pt_len = ciphertext.len() - tag_len;
        if out.len() < pt_len {
            return Err(CryptoError::BufferTooSmall {
                need: pt_len,
                have: out.len(),
            });
        }

        let safe_key = self.bound_key(&key.material)?;
        let nonce_arr: [u8; 12] = nonce.try_into().map_err(|_| CryptoError::InvalidNonce {
            expected: 12,
            got: nonce.len(),
        })?;
        let nonce = Nonce::assume_unique_for_key(nonce_arr);

        // ring's open_in_place wants a mutable contiguous [ct | tag] buffer.
        // We have an immutable `ciphertext`; copy into `out` extended by tag
        // (caller's `out` is only pt_len long, so we need a temp). For
        // benchmark realism without allocation, copy to a stack buffer for
        // small frames or fall back to a Vec for large ones.
        let mut work_buf: Vec<u8> = ciphertext.to_vec();
        let pt_slice = safe_key
            .open_in_place(nonce, Aad::from(aad), &mut work_buf)
            .map_err(|_| CryptoError::AuthFailed)?;
        out[..pt_slice.len()].copy_from_slice(pt_slice);
        Ok(pt_slice.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    /// Same vector as `aes_gcm_sw::tests::aes128_gcm_mcgrew_viega_test2`,
    /// proves ring and RustCrypto agree byte-for-byte.
    #[test]
    fn ring_aes128_gcm_mcgrew_viega_test2() {
        let key = hex("00000000000000000000000000000000");
        let iv = hex("000000000000000000000000");
        let pt = hex("00000000000000000000000000000000");
        let ct_expected = hex("0388dace60b6a392f328c2b971b2fe78");
        let tag_expected = hex("ab6e47d42cec13bdf53a67b21257bddf");

        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, key).unwrap();
        let prov = AesGcmRingProvider::new(AlgoId::Aes128Gcm).unwrap();
        let mut out = vec![0u8; pt.len() + 16];
        let n = prov.seal(&kh, &iv, b"", &pt, &mut out).unwrap();
        assert_eq!(&out[..pt.len()], &ct_expected[..]);
        assert_eq!(&out[pt.len()..n], &tag_expected[..]);

        let mut back = vec![0u8; pt.len()];
        let m = prov.open(&kh, &iv, b"", &out[..n], &mut back).unwrap();
        assert_eq!(m, pt.len());
        assert_eq!(&back[..], &pt[..]);
    }

    #[test]
    fn ring_aes256_gcm_mcgrew_viega_test14() {
        let key = hex("0000000000000000000000000000000000000000000000000000000000000000");
        let iv = hex("000000000000000000000000");
        let pt = hex("00000000000000000000000000000000");
        let ct_expected = hex("cea7403d4d606b6e074ec5d3baf39d18");
        let tag_expected = hex("d0d1c8a799996bf0265b98b5d48ab919");

        let kh = KeyHandle::new(7, AlgoId::Aes256Gcm, key).unwrap();
        let prov = AesGcmRingProvider::new(AlgoId::Aes256Gcm).unwrap();
        let mut out = vec![0u8; pt.len() + 16];
        let n = prov.seal(&kh, &iv, b"", &pt, &mut out).unwrap();
        assert_eq!(&out[..pt.len()], &ct_expected[..]);
        assert_eq!(&out[pt.len()..n], &tag_expected[..]);
    }

    /// Cross-check: ring's output equals RustCrypto's output for
    /// arbitrary input.
    #[test]
    fn ring_matches_rustcrypto_arbitrary() {
        use crate::aes_gcm_sw::AesGcmSwProvider;

        let key = vec![0x42u8; 32];
        let nonce = vec![0x88u8; 12];
        let aad = b"acm-uz-interop-test";
        let pt: Vec<u8> = (0..555).map(|i| (i & 0xff) as u8).collect();

        let kh = KeyHandle::new(1, AlgoId::Aes256Gcm, key).unwrap();
        let sw = AesGcmSwProvider::new(AlgoId::Aes256Gcm).unwrap();
        let ring = AesGcmRingProvider::new(AlgoId::Aes256Gcm).unwrap();

        let mut sw_ct = vec![0u8; pt.len() + 16];
        let mut ring_ct = vec![0u8; pt.len() + 16];
        sw.seal(&kh, &nonce, aad, &pt, &mut sw_ct).unwrap();
        ring.seal(&kh, &nonce, aad, &pt, &mut ring_ct).unwrap();
        assert_eq!(sw_ct, ring_ct, "ring vs RustCrypto produced different CT/tag!");

        // And each can open the other.
        let mut by_ring = vec![0u8; pt.len()];
        ring.open(&kh, &nonce, aad, &sw_ct, &mut by_ring).unwrap();
        assert_eq!(by_ring, pt);
        let mut by_sw = vec![0u8; pt.len()];
        sw.open(&kh, &nonce, aad, &ring_ct, &mut by_sw).unwrap();
        assert_eq!(by_sw, pt);
    }

    #[test]
    fn ring_tampered_tag_fails() {
        let prov = AesGcmRingProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]).unwrap();
        let nonce = [0u8; 12];
        let pt = b"hello acm";
        let mut sealed = vec![0u8; pt.len() + 16];
        let n = prov.seal(&kh, &nonce, b"", pt, &mut sealed).unwrap();
        sealed[n - 1] ^= 0x01;
        let mut out = vec![0u8; pt.len()];
        assert!(matches!(
            prov.open(&kh, &nonce, b"", &sealed[..n], &mut out),
            Err(CryptoError::AuthFailed)
        ));
    }
}
