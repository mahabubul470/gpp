use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("noise error: {0}")]
    Noise(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("object store error: {0}")]
    Core(#[from] gpp_core::Error),
    #[error("repo id mismatch: local {local} != peer {remote}")]
    RepoMismatch { local: String, remote: String },
    #[error(
        "peer key changed for {0:?} — refusing (TOFU). Remove it from .gpp/sync/known_peers to re-trust."
    )]
    PeerKeyChanged(String),
    #[error("unauthorized peer static key {0} — not in the authorized-keys allowlist")]
    Unauthorized(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
