//! Typed errors for matter-produce.

use thiserror::Error;

/// Result alias for matter-produce operations.
pub type Result<T> = std::result::Result<T, ProduceError>;

/// Errors from the production-export engine.
#[derive(Debug, Error)]
pub enum ProduceError {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("{0}")]
    Other(String),
}
