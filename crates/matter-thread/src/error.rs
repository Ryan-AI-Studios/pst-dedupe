//! Typed errors for matter-thread.

use thiserror::Error;

/// Result alias for matter-thread operations.
pub type Result<T> = std::result::Result<T, ThreadError>;

/// Errors from the matter-level threading engine.
#[derive(Debug, Error)]
pub enum ThreadError {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
