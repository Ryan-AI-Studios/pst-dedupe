//! Typed errors for matter-promote.

use thiserror::Error;

/// Result alias for matter-promote operations.
pub type Result<T> = std::result::Result<T, PromoteError>;

/// Errors from the matter-level promote-to-review engine.
#[derive(Debug, Error)]
pub enum PromoteError {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
