//! ACM-UZ encrypted-frame wire format.
//!
//! Designed to be algorithm-agnostic: the frame header carries an
//! [`AlgoId`] field so peers can mix algorithms during the AES → O'z DSt
//! 1105 migration. Each side encrypts with whatever its current provider
//! is and tags the frame; the receiver dispatches to the matching
//! provider based on the byte in the header.
//!
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  Magic ('A''C') |  Version (1B) |  Flags (1B)   | KeyId (4B...)|
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  ...KeyId       | Algo (1B)     | NonceLen (1B) | Reserved (2B)|
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                    Nonce (NonceLen bytes)                     |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |           Ciphertext + Tag (rest of frame; tag is the          |
//! |           last AlgoId.tag_len() bytes)                         |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! Wire is big-endian where applicable. The fixed header is 12 bytes.

use acm_crypto::AlgoId;
use thiserror::Error;

pub const MAGIC: [u8; 2] = [b'A', b'C'];
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 12;

#[derive(Debug, Error)]
pub enum WireError {
    #[error("frame too short")]
    Truncated,
    #[error("bad magic bytes")]
    BadMagic,
    #[error("unsupported version: {0}")]
    BadVersion(u8),
    #[error("unknown algorithm id: 0x{0:02x}")]
    UnknownAlgo(u8),
    #[error("nonce length mismatch")]
    NonceLenMismatch,
}

/// Parsed fixed header. The header is **authenticated as AAD** but **not
/// encrypted** — peers can dispatch by KeyId/AlgoId without decrypting.
#[derive(Debug, Clone, Copy)]
pub struct FrameHeader {
    pub version: u8,
    pub flags: u8,
    pub key_id: u32,
    pub algo: AlgoId,
    pub nonce_len: u8,
}

impl FrameHeader {
    pub fn encode(&self, out: &mut [u8]) -> Result<(), WireError> {
        if out.len() < HEADER_LEN {
            return Err(WireError::Truncated);
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
            return Err(WireError::Truncated);
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

// TODO Phase 1: encode_frame() / decode_frame() that calls CryptoProvider
// TODO Phase 1: replay protection (sliding window keyed by KeyId)
// TODO Phase 1: anti-replay counter / sequence integration

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = FrameHeader {
            version: VERSION,
            flags: 0,
            key_id: 0xDEADBEEF,
            algo: AlgoId::Aes256Gcm,
            nonce_len: 12,
        };
        let mut buf = [0u8; HEADER_LEN];
        h.encode(&mut buf).unwrap();
        let d = FrameHeader::decode(&buf).unwrap();
        assert_eq!(d.key_id, 0xDEADBEEF);
        assert_eq!(d.algo, AlgoId::Aes256Gcm);
        assert_eq!(d.nonce_len, 12);
    }

    #[test]
    fn header_bad_magic() {
        let mut buf = [0u8; HEADER_LEN];
        buf[0] = b'X';
        buf[1] = b'X';
        assert!(matches!(FrameHeader::decode(&buf), Err(WireError::BadMagic)));
    }
}
