//! Error type for the storage layer.

use crate::hash::Hash;

/// Errors produced by the `gpp-core` storage layer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization failed: {0}")]
    Serialize(String),

    #[error("deserialization failed: {0}")]
    Deserialize(String),

    #[error("compression failed: {0}")]
    Compression(String),

    #[error("not a valid gpp object: bad magic bytes")]
    BadMagic,

    #[error("unsupported object wire-format version: {0}")]
    UnsupportedVersion(u8),

    #[error("unknown object type code: {0}")]
    UnknownObjectType(u8),

    #[error("object type mismatch: expected {expected}, found {found}")]
    TypeMismatch { expected: u8, found: u8 },

    #[error("frame is truncated or malformed")]
    TruncatedFrame,

    #[error("payload checksum mismatch (object is corrupt)")]
    ChecksumMismatch,

    #[error("hash verification failed: expected {expected}, computed {computed}")]
    HashMismatch { expected: Hash, computed: Hash },

    #[error("invalid hash string: {0}")]
    InvalidHash(String),

    #[error("object not found: {0}")]
    NotFound(Hash),
}

/// Convenience alias used throughout `gpp-core`.
pub type Result<T> = std::result::Result<T, Error>;
