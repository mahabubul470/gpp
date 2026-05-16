//! Error type for the Graphex layer.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("object store error: {0}")]
    Core(#[from] gpp_core::Error),

    #[error("index database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("serialization error: {0}")]
    Serde(String),

    #[error("encryption error: {0}")]
    Crypto(String),

    #[error("key store not initialized — run `gpp keys generate`")]
    NoKeys,

    #[error("unknown access tier {0:?}")]
    UnknownTier(String),

    #[error("unknown node type {0:?}")]
    UnknownNodeType(String),

    #[error("unknown edge relation {0:?}")]
    UnknownRelation(String),

    #[error("node {0:?} not found")]
    NodeNotFound(String),

    #[error("malformed query: {0}")]
    BadQuery(String),

    #[error("access denied: tier {tier} exceeds accessor max tier {max}")]
    AccessDenied { tier: String, max: String },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
