//! Typed errors for matter-neardup.

use thiserror::Error;

/// Result alias for matter-neardup operations.
pub type Result<T> = std::result::Result<T, NearDupError>;

/// Errors from the matter-level near-duplicate engine.
#[derive(Debug, Error)]
pub enum NearDupError {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}
