//! Errors for file-category classify job.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Classifiable / job errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}
