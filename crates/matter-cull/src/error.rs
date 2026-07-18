//! Typed errors for matter-cull.

use thiserror::Error;

/// Result alias for matter-cull operations.
pub type Result<T> = std::result::Result<T, CullError>;

/// Errors from the matter-level cull engine.
#[derive(Debug, Error)]
pub enum CullError {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("invalid rules: {0}")]
    InvalidRules(String),

    #[error("denist: {0}")]
    Denist(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}
