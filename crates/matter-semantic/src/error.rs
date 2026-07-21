//! Errors for semantic index / query.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, SemanticError>;

/// Semantic engine / job / query errors.
#[derive(Debug, Error)]
pub enum SemanticError {
    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("model mismatch: embedder={embedder}, active index={active}")]
    ModelMismatch { embedder: String, active: String },

    #[error("semantic index not built — run job kind semantic_index first")]
    IndexNotBuilt,

    #[error("path rejected: {0}")]
    PathRejected(String),

    #[error("embedder error: {0}")]
    Embedder(String),

    #[error("{0}")]
    Other(String),
}

impl SemanticError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }

    pub fn embedder(msg: impl Into<String>) -> Self {
        Self::Embedder(msg.into())
    }
}
