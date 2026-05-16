//! Error type for the Git bridge.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("gpp object store error: {0}")]
    Core(#[from] gpp_core::Error),

    #[error("gpp history error: {0}")]
    History(#[from] gpp_history::Error),

    #[error("mapping database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
