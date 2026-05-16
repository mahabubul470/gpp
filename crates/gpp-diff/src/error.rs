//! Error type for the semantic diff path.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no semantic parser registered for this file")]
    UnsupportedLanguage,

    #[error("file is not valid UTF-8 (semantic diff needs source text)")]
    NotUtf8,

    #[error("tree-sitter could not parse the source")]
    ParseFailed,

    #[error("invalid tree-sitter query for {language}: {message}")]
    BadQuery { language: String, message: String },

    #[error("tree-sitter language load failed: {0}")]
    LanguageLoad(String),
}

pub type Result<T> = std::result::Result<T, Error>;
