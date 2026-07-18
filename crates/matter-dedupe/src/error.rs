//! Typed errors for matter-dedupe.

use thiserror::Error;

/// Result alias for matter-dedupe operations.
pub type Result<T> = std::result::Result<T, DedupeError>;

/// Errors from the matter-level dedupe engine.
#[derive(Debug, Error)]
pub enum DedupeError {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
