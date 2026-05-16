//! On-disk / on-wire framing for stored objects.
//!
//! Layout (see `docs/DATA_MODEL.md` § Wire Format):
//!
//! ```text
//! Magic:    b"GPP\0"        (4 bytes)
//! Version:  u8              (1 byte)
//! Type:     u8              (1 byte)   ObjectType::code()
//! Flags:    u16 LE          (2 bytes)  Compressed | Encrypted | Signed
//! Length:   u32 LE          (4 bytes)  payload length
//! Payload:  [u8; Length]               zstd(encode_body())
//! Checksum: [u8; 4]                    BLAKE3(payload) truncated to 4 bytes
//! ```

use crate::error::{Error, Result};
use crate::object::ObjectType;

pub const MAGIC: [u8; 4] = *b"GPP\0";
pub const VERSION: u8 = 1;

/// zstd compression level used for all stored objects.
const ZSTD_LEVEL: i32 = 3;

const HEADER_LEN: usize = 4 + 1 + 1 + 2 + 4;
const CHECKSUM_LEN: usize = 4;

/// Frame flag bits.
pub mod flags {
    pub const COMPRESSED: u16 = 1 << 0;
    pub const ENCRYPTED: u16 = 1 << 1;
    pub const SIGNED: u16 = 1 << 2;
}

/// A decoded frame: the object type plus its uncompressed canonical body.
pub struct Decoded {
    pub object_type: ObjectType,
    pub body: Vec<u8>,
}

/// Frame `body` (the object's canonical bytes) into the stored representation.
///
/// Phase 0 always sets the `COMPRESSED` flag; encryption/signing are later phases.
pub fn encode(object_type: ObjectType, body: &[u8]) -> Result<Vec<u8>> {
    let payload =
        zstd::encode_all(body, ZSTD_LEVEL).map_err(|e| Error::Compression(e.to_string()))?;
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| Error::Serialize("object payload exceeds 4 GiB".into()))?;

    let mut out = Vec::with_capacity(HEADER_LEN + payload.len() + CHECKSUM_LEN);
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    out.push(object_type.code());
    out.extend_from_slice(&flags::COMPRESSED.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&payload);

    let checksum = blake3::hash(&payload);
    out.extend_from_slice(&checksum.as_bytes()[..CHECKSUM_LEN]);
    Ok(out)
}

/// Parse and validate a stored frame, returning the uncompressed body.
pub fn decode(frame: &[u8]) -> Result<Decoded> {
    if frame.len() < HEADER_LEN + CHECKSUM_LEN {
        return Err(Error::TruncatedFrame);
    }
    if frame[0..4] != MAGIC {
        return Err(Error::BadMagic);
    }
    let version = frame[4];
    if version != VERSION {
        return Err(Error::UnsupportedVersion(version));
    }
    let object_type = ObjectType::from_code(frame[5])?;
    let flag_bits = u16::from_le_bytes([frame[6], frame[7]]);
    let length = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;

    let payload_start = HEADER_LEN;
    let payload_end = payload_start
        .checked_add(length)
        .ok_or(Error::TruncatedFrame)?;
    if frame.len() != payload_end + CHECKSUM_LEN {
        return Err(Error::TruncatedFrame);
    }
    let payload = &frame[payload_start..payload_end];
    let stored_checksum = &frame[payload_end..payload_end + CHECKSUM_LEN];
    if &blake3::hash(payload).as_bytes()[..CHECKSUM_LEN] != stored_checksum {
        return Err(Error::ChecksumMismatch);
    }

    let body = if flag_bits & flags::COMPRESSED != 0 {
        zstd::decode_all(payload).map_err(|e| Error::Compression(e.to_string()))?
    } else {
        payload.to_vec()
    };

    Ok(Decoded { object_type, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let body = b"the quick brown fox".repeat(10);
        let frame = encode(ObjectType::Blob, &body).unwrap();
        let decoded = decode(&frame).unwrap();
        assert_eq!(decoded.object_type, ObjectType::Blob);
        assert_eq!(decoded.body, body);
    }

    #[test]
    fn detects_bad_magic() {
        let mut frame = encode(ObjectType::Blob, b"x").unwrap();
        frame[0] = b'X';
        assert!(matches!(decode(&frame), Err(Error::BadMagic)));
    }

    #[test]
    fn detects_corruption() {
        let mut frame = encode(ObjectType::Tree, b"payload bytes here").unwrap();
        let n = frame.len();
        frame[n - CHECKSUM_LEN - 1] ^= 0xff; // flip a payload bit
        assert!(matches!(decode(&frame), Err(Error::ChecksumMismatch)));
    }

    #[test]
    fn rejects_truncated() {
        let frame = encode(ObjectType::Blob, b"data").unwrap();
        assert!(matches!(decode(&frame[..5]), Err(Error::TruncatedFrame)));
    }

    #[test]
    fn rejects_unknown_version() {
        let mut frame = encode(ObjectType::Blob, b"v").unwrap();
        frame[4] = 99;
        assert!(matches!(decode(&frame), Err(Error::UnsupportedVersion(99))));
    }
}
