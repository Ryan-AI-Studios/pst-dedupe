//! Typed errors for matter-qc.

use thiserror::Error;

/// Result alias for matter-qc operations.
pub type Result<T> = std::result::Result<T, QcError>;

/// Errors from the production QC engine.
#[derive(Debug, Error)]
pub enum QcError {
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

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("{0}")]
    Other(String),
}
