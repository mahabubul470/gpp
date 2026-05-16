//! BLAKE3 content hash and its base32 display encoding.
//!
//! Per `docs/DATA_MODEL.md`: hashes are BLAKE3 (256-bit), displayed as
//! lowercase base32 (RFC 4648 alphabet, unpadded) which is exactly 52
//! characters for 32 bytes. Short form is the first 8 characters.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::Error;

/// Length of a BLAKE3 digest in bytes.
pub const HASH_LEN: usize = 32;

/// Length of the base32 textual form of a [`Hash`].
pub const HASH_STR_LEN: usize = 52;

/// Number of leading characters used for the short display form.
pub const SHORT_LEN: usize = 8;

/// RFC 4648 base32 alphabet, lowercased (case-insensitive on decode).
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// A 256-bit BLAKE3 content address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectHash([u8; HASH_LEN]);

/// Public alias — most call sites just want `Hash`.
pub type Hash = ObjectHash;

impl ObjectHash {
    /// Hash an arbitrary byte slice.
    pub fn of(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).into())
    }

    /// Wrap a raw 32-byte digest.
    pub fn from_raw(bytes: [u8; HASH_LEN]) -> Self {
        Self(bytes)
    }

    /// The raw 32-byte digest.
    pub fn as_bytes(&self) -> &[u8; HASH_LEN] {
        &self.0
    }

    /// Full lowercase base32 form (52 chars).
    pub fn to_base32(self) -> String {
        base32_encode(&self.0)
    }

    /// First [`SHORT_LEN`] characters of the base32 form.
    pub fn short(self) -> String {
        let mut s = self.to_base32();
        s.truncate(SHORT_LEN);
        s
    }

    /// Parse a (case-insensitive) base32 hash string.
    pub fn from_base32(s: &str) -> Result<Self, Error> {
        let decoded = base32_decode(s)?;
        let bytes: [u8; HASH_LEN] = decoded.as_slice().try_into().map_err(|_| {
            Error::InvalidHash(format!("expected {HASH_LEN} bytes, got {}", decoded.len()))
        })?;
        Ok(Self(bytes))
    }
}

impl fmt::Display for ObjectHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_base32())
    }
}

impl fmt::Debug for ObjectHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", self.short())
    }
}

impl FromStr for ObjectHash {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_base32(s)
    }
}

impl Serialize for ObjectHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for ObjectHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct HashVisitor;
        impl<'de> Visitor<'de> for HashVisitor {
            type Value = ObjectHash;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a 32-byte BLAKE3 digest")
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<ObjectHash, E> {
                let b: [u8; HASH_LEN] = v
                    .try_into()
                    .map_err(|_| E::invalid_length(v.len(), &self))?;
                Ok(ObjectHash(b))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<ObjectHash, A::Error> {
                let mut b = [0u8; HASH_LEN];
                for (i, slot) in b.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }
                Ok(ObjectHash(b))
            }
        }
        d.deserialize_bytes(HashVisitor)
    }
}

/// Encode bytes as lowercase, unpadded base32 (RFC 4648 alphabet).
fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in data {
        buf = (buf << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// Decode a (case-insensitive) base32 string. Rejects non-alphabet chars.
fn base32_decode(s: &str) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for ch in s.chars() {
        let lc = ch.to_ascii_lowercase();
        let val = match lc {
            'a'..='z' => lc as u32 - 'a' as u32,
            '2'..='7' => lc as u32 - '2' as u32 + 26,
            _ => {
                return Err(Error::InvalidHash(format!(
                    "illegal base32 character {ch:?}"
                )));
            }
        };
        buf = (buf << 5) | val;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_string_is_52_chars_and_roundtrips() {
        let h = Hash::of(b"hello gpp");
        let s = h.to_base32();
        assert_eq!(s.len(), HASH_STR_LEN);
        assert_eq!(Hash::from_base32(&s).unwrap(), h);
    }

    #[test]
    fn decode_is_case_insensitive() {
        let h = Hash::of(b"case test");
        let lower = h.to_base32();
        let upper = lower.to_uppercase();
        assert_eq!(Hash::from_base32(&upper).unwrap(), h);
    }

    #[test]
    fn short_form_is_prefix() {
        let h = Hash::of(b"short");
        assert_eq!(h.short().len(), SHORT_LEN);
        assert!(h.to_base32().starts_with(&h.short()));
    }

    #[test]
    fn rejects_non_alphabet() {
        assert!(Hash::from_base32("not valid!!").is_err());
        assert!(Hash::from_base32("0189").is_err()); // 0,1,8,9 are not in the alphabet
    }

    #[test]
    fn distinct_inputs_distinct_hashes() {
        assert_ne!(Hash::of(b"a"), Hash::of(b"b"));
    }
}
