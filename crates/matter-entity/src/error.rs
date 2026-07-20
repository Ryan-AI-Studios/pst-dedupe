//! Errors for entity scan.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, EntityError>;

/// Entity pack / job errors.
#[derive(Debug, Error)]
pub enum EntityError {
    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("{0}")]
    Other(String),
}

impl EntityError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}
