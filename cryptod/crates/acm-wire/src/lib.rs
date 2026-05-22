//! ACM-UZ encrypted-frame wire format.
//!
//! Designed to be algorithm-agnostic: the frame header carries an
//! [`AlgoId`] field so peers can mix algorithms during the AES → O'z DSt
//! 1105 migration. Each side encrypts with whatever its current provider
//! is and tags the frame; the receiver dispatches to the matching
//! provider based on the byte in the header.
//!
//! ## Layout
//!
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  Magic ('A' 'C') |  Ver  | Flags |          KeyId (32-bit BE) |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |               KeyId continued | Algo  |NLen   |  Reserved 2B  |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                    Nonce (NonceLen bytes)                     |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |           Ciphertext + Tag (rest of frame; tag is the          |
//! |           last AlgoId.tag_len() bytes)                         |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! The fixed header is **12 bytes**. The whole header (12 bytes) is fed
//! into the AEAD as `aad` — peers can route by KeyId/AlgoId without
//! decrypting, and an attacker cannot rewrite header fields without
//! breaking the tag.

use acm_crypto::{AlgoId, CryptoError, CryptoProvider, KeyHandle};
use thiserror::Error;

pub const MAGIC: [u8; 2] = [b'A', b'C'];
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 12;

#[derive(Debug, Error)]
pub enum WireError {
    #[error("frame too short ({0} bytes, need at least {HEADER_LEN} for header)")]
    Truncated(usize),
    #[error("bad magic bytes")]
    BadMagic,
    #[error("unsupported version: {0}")]
    BadVersion(u8),
    #[error("unknown algorithm id: 0x{0:02x}")]
    UnknownAlgo(u8),
    #[error("nonce length in header ({header}) does not match algo expected ({algo})")]
    NonceLenMismatch { header: u8, algo: usize },
    #[error("frame is missing nonce or ciphertext bytes")]
    BodyTooShort,
    #[error("output buffer too small")]
    OutputTooSmall,
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
}

/// Parsed fixed header. Authenticated as AAD by `seal`, not encrypted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub version: u8,
    pub flags: u8,
    pub key_id: u32,
    pub algo: AlgoId,
    pub nonce_len: u8,
}

impl FrameHeader {
    /// Encode into a 12-byte buffer.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), WireError> {
        if out.len() < HEADER_LEN {
            return Err(WireError::Truncated(out.len()));
        }
        out[0..2].copy_from_slice(&MAGIC);
        out[2] = self.version;
        out[3] = self.flags;
        out[4..8].copy_from_slice(&self.key_id.to_be_bytes());
        out[8] = self.algo as u8;
        out[9] = self.nonce_len;
        out[10] = 0;
        out[11] = 0;
        Ok(())
    }

    pub fn decode(src: &[u8]) -> Result<Self, WireError> {
        if src.len() < HEADER_LEN {
            return Err(WireError::Truncated(src.len()));
        }
        if src[0..2] != MAGIC {
            return Err(WireError::BadMagic);
        }
        if src[2] != VERSION {
            return Err(WireError::BadVersion(src[2]));
        }
        let algo = AlgoId::from_u8(src[8]).ok_or(WireError::UnknownAlgo(src[8]))?;
        Ok(FrameHeader {
            version: src[2],
            flags: src[3],
            key_id: u32::from_be_bytes([src[4], src[5], src[6], src[7]]),
            algo,
            nonce_len: src[9],
        })
    }
}

/// Encode a plaintext into a full ACM frame:
///
///   `[ HEADER (12B) | NONCE (NLen) | CIPHERTEXT | TAG (16B) ]`
///
/// The header is authenticated (used as AAD) and the nonce is included so
/// the receiver can replay it into `provider.open`.
///
/// `out` must be at least `frame_len(plaintext.len(), nonce.len(),
/// provider.algorithm().tag_len())` bytes.
pub fn seal(
    provider: &dyn CryptoProvider,
    key: &KeyHandle,
    key_id: u32,
    nonce: &[u8],
    flags: u8,
    plaintext: &[u8],
    out: &mut [u8],
) -> Result<usize, WireError> {
    let algo = provider.algorithm();
    if nonce.len() != algo.nonce_len() {
        return Err(WireError::NonceLenMismatch {
            header: nonce.len() as u8,
            algo: algo.nonce_len(),
        });
    }
    let need = frame_len(plaintext.len(), nonce.len(), algo.tag_len());
    if out.len() < need {
        return Err(WireError::OutputTooSmall);
    }

    let header = FrameHeader {
        version: VERSION,
        flags,
        key_id,
        algo,
        nonce_len: nonce.len() as u8,
    };
    header.encode(&mut out[..HEADER_LEN])?;
    out[HEADER_LEN..HEADER_LEN + nonce.len()].copy_from_slice(nonce);

    let body_off = HEADER_LEN + nonce.len();
    // AAD = the 12-byte header (so any tampering of KeyId / Algo / etc
    // breaks the tag).
    let mut hdr_buf = [0u8; HEADER_LEN];
    hdr_buf.copy_from_slice(&out[..HEADER_LEN]);
    let written = provider.seal(
        key,
        nonce,
        &hdr_buf,
        plaintext,
        &mut out[body_off..body_off + plaintext.len() + algo.tag_len()],
    )?;
    Ok(body_off + written)
}

/// Decode and decrypt an ACM frame, writing plaintext into `out`.
///
/// Returns plaintext length on success. `out` must be at least
/// `frame.len() - HEADER_LEN - nonce_len - tag_len` bytes.
pub fn open(
    provider: &dyn CryptoProvider,
    key: &KeyHandle,
    frame: &[u8],
    out: &mut [u8],
) -> Result<usize, WireError> {
    let header = FrameHeader::decode(frame)?;
    let algo = header.algo;

    if header.algo != provider.algorithm() {
        return Err(WireError::Crypto(CryptoError::AlgorithmMismatch {
            provider: provider.algorithm(),
            key: header.algo,
        }));
    }
    if header.nonce_len as usize != algo.nonce_len() {
        return Err(WireError::NonceLenMismatch {
            header: header.nonce_len,
            algo: algo.nonce_len(),
        });
    }

    let body_off = HEADER_LEN + header.nonce_len as usize;
    if frame.len() < body_off + algo.tag_len() {
        return Err(WireError::BodyTooShort);
    }
    let nonce = &frame[HEADER_LEN..body_off];
    let body = &frame[body_off..];

    let pt_len = body.len() - algo.tag_len();
    if out.len() < pt_len {
        return Err(WireError::OutputTooSmall);
    }

    // AAD = the original header bytes from the frame.
    let mut hdr_buf = [0u8; HEADER_LEN];
    hdr_buf.copy_from_slice(&frame[..HEADER_LEN]);

    let n = provider.open(key, nonce, &hdr_buf, body, out)?;
    Ok(n)
}

/// Compute the total wire size of an ACM frame for the given parameters.
#[inline]
pub fn frame_len(plaintext_len: usize, nonce_len: usize, tag_len: usize) -> usize {
    HEADER_LEN + nonce_len + plaintext_len + tag_len
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use acm_crypto::aes_gcm_sw::AesGcmSwProvider;

    fn hex(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    #[test]
    fn header_roundtrip() {
        let h = FrameHeader {
            version: VERSION,
            flags: 0x80,
            key_id: 0xDEADBEEF,
            algo: AlgoId::Aes256Gcm,
            nonce_len: 12,
        };
        let mut buf = [0u8; HEADER_LEN];
        h.encode(&mut buf).unwrap();
        let d = FrameHeader::decode(&buf).unwrap();
        assert_eq!(d, h);
    }

    #[test]
    fn header_bad_magic() {
        let mut buf = [0u8; HEADER_LEN];
        buf[0] = b'X';
        buf[1] = b'X';
        assert!(matches!(FrameHeader::decode(&buf), Err(WireError::BadMagic)));
    }

    #[test]
    fn header_bad_version() {
        let mut buf = [0u8; HEADER_LEN];
        buf[..2].copy_from_slice(&MAGIC);
        buf[2] = 99;
        assert!(matches!(FrameHeader::decode(&buf), Err(WireError::BadVersion(99))));
    }

    #[test]
    fn header_unknown_algo() {
        let mut buf = [0u8; HEADER_LEN];
        buf[..2].copy_from_slice(&MAGIC);
        buf[2] = VERSION;
        buf[8] = 0xFE; // unknown
        assert!(matches!(FrameHeader::decode(&buf), Err(WireError::UnknownAlgo(0xFE))));
    }

    #[test]
    fn seal_open_aes128() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(11, AlgoId::Aes128Gcm, vec![0x33u8; 16]).unwrap();
        let nonce = vec![0xAAu8; 12];
        let pt = b"hello, ACM-UZ wire format roundtrip!";

        let mut frame = vec![0u8; frame_len(pt.len(), nonce.len(), AlgoId::Aes128Gcm.tag_len())];
        let n = seal(&prov, &kh, kh.id, &nonce, 0, pt, &mut frame).unwrap();
        assert_eq!(n, frame.len());

        // Magic and AlgoId in plaintext header (router can read them):
        assert_eq!(&frame[..2], &MAGIC);
        assert_eq!(frame[8], AlgoId::Aes128Gcm as u8);
        // KeyId BE = 11:
        assert_eq!(u32::from_be_bytes([frame[4], frame[5], frame[6], frame[7]]), 11);

        let mut pt_back = vec![0u8; pt.len()];
        let m = open(&prov, &kh, &frame[..n], &mut pt_back).unwrap();
        assert_eq!(m, pt.len());
        assert_eq!(&pt_back[..], pt);
    }

    #[test]
    fn seal_open_aes256_various_sizes() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes256Gcm).unwrap();
        let kh = KeyHandle::new(7, AlgoId::Aes256Gcm, vec![0x55u8; 32]).unwrap();
        let nonce = vec![0x11u8; 12];

        for size in [0usize, 1, 64, 1500, 9000] {
            let pt: Vec<u8> = (0..size).map(|i| (i * 31) as u8).collect();
            let total = frame_len(size, 12, 16);
            let mut frame = vec![0u8; total];
            let n = seal(&prov, &kh, kh.id, &nonce, 0, &pt, &mut frame).unwrap();
            assert_eq!(n, total);

            let mut pt_back = vec![0u8; size];
            let m = open(&prov, &kh, &frame[..n], &mut pt_back).unwrap();
            assert_eq!(m, size);
            assert_eq!(pt_back, pt, "size {} failed", size);
        }
    }

    #[test]
    fn open_fails_when_header_tampered() {
        // Header is AAD — flipping a header bit must break the tag.
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]).unwrap();
        let nonce = [0u8; 12];
        let pt = b"top secret";

        let mut frame = vec![0u8; frame_len(pt.len(), 12, 16)];
        seal(&prov, &kh, kh.id, &nonce, 0, pt, &mut frame).unwrap();

        // Tamper key_id (byte 4..8) — semantically: pretend it's a different
        // session. Tag MUST fail because the original key_id is in AAD.
        frame[4] ^= 0xFF;
        let mut out = vec![0u8; pt.len()];
        let r = open(&prov, &kh, &frame, &mut out);
        assert!(matches!(r, Err(WireError::Crypto(CryptoError::AuthFailed))));
    }

    #[test]
    fn open_fails_when_ciphertext_tampered() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]).unwrap();
        let nonce = [0u8; 12];
        let pt = b"do not trust the wire";

        let mut frame = vec![0u8; frame_len(pt.len(), 12, 16)];
        let n = seal(&prov, &kh, kh.id, &nonce, 0, pt, &mut frame).unwrap();

        // Tamper one ciphertext byte (skip header and nonce).
        frame[HEADER_LEN + 12 + 0] ^= 0x01;
        let mut out = vec![0u8; pt.len()];
        let r = open(&prov, &kh, &frame[..n], &mut out);
        assert!(matches!(r, Err(WireError::Crypto(CryptoError::AuthFailed))));
    }

    #[test]
    fn open_rejects_truncated_frame() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let kh = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]).unwrap();
        let mut out = [0u8; 16];

        // Too short for even a header.
        let r = open(&prov, &kh, &[0u8; 5], &mut out);
        assert!(matches!(r, Err(WireError::Truncated(5))));
    }

    #[test]
    fn open_rejects_mismatched_provider() {
        // Sealed with AES-128, try to open with AES-256 provider.
        let p_seal = AesGcmSwProvider::new(AlgoId::Aes128Gcm).unwrap();
        let p_open = AesGcmSwProvider::new(AlgoId::Aes256Gcm).unwrap();
        let kh_seal = KeyHandle::new(1, AlgoId::Aes128Gcm, vec![0u8; 16]).unwrap();
        let kh_open = KeyHandle::new(1, AlgoId::Aes256Gcm, vec![0u8; 32]).unwrap();
        let nonce = [0u8; 12];

        let mut frame = vec![0u8; frame_len(8, 12, 16)];
        seal(&p_seal, &kh_seal, kh_seal.id, &nonce, 0, b"hi there", &mut frame).unwrap();

        let mut out = [0u8; 8];
        let r = open(&p_open, &kh_open, &frame, &mut out);
        assert!(matches!(r, Err(WireError::Crypto(CryptoError::AlgorithmMismatch { .. }))));
    }

    /// End-to-end demonstration that a sniffed wire frame is readable
    /// at the header level (for routing) but plaintext is hidden.
    #[test]
    fn header_inspectable_plaintext_hidden() {
        let prov = AesGcmSwProvider::new(AlgoId::Aes256Gcm).unwrap();
        let kh = KeyHandle::new(0xCAFEBABE, AlgoId::Aes256Gcm, hex(
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
        )).unwrap();
        let nonce = hex("000102030405060708090a0b");
        let secret = b"ATTACK AT DAWN";

        let mut frame = vec![0u8; frame_len(secret.len(), 12, 16)];
        seal(&prov, &kh, kh.id, &nonce, 0, secret, &mut frame).unwrap();

        // Router on a busy switch can dispatch without crypto:
        let h = FrameHeader::decode(&frame).unwrap();
        assert_eq!(h.key_id, 0xCAFEBABE);
        assert_eq!(h.algo, AlgoId::Aes256Gcm);

        // The plaintext as a contiguous sequence MUST NOT appear in the
        // ciphertext (a property the AEAD trivially gives us, but worth
        // asserting end-to-end through our wire layer).
        let cipher_blob = &frame[HEADER_LEN + 12..];
        assert!(
            !contains_subslice(cipher_blob, secret),
            "plaintext appeared in ciphertext"
        );
    }

    fn contains_subslice(hay: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() || needle.len() > hay.len() {
            return false;
        }
        hay.windows(needle.len()).any(|w| w == needle)
    }
}
