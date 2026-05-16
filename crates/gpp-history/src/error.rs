//! History error type.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("object store error: {0}")]
    Store(#[from] gpp_core::Error),

    #[error("timeline error: {0}")]
    Timeline(#[from] gpp_timeline::Error),

    #[error("serialization failed: {0}")]
    Serialize(String),

    #[error("deserialization failed: {0}")]
    Deserialize(String),

    #[error("nothing to promote: no unpromoted timeline entries in range")]
    NothingToPromote,

    #[error("branch {0:?} does not exist")]
    NoSuchBranch(String),

    #[error("branch {0:?} already exists")]
    BranchExists(String),

    #[error("invalid reference name {0:?}")]
    InvalidRefName(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
