//! Timeline error type.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("object store error: {0}")]
    Store(#[from] gpp_core::Error),

    #[error("invalid ignore pattern {pattern:?}: {source}")]
    Ignore {
        pattern: String,
        #[source]
        source: globset::Error,
    },

    #[error("file watch error: {0}")]
    Watch(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
