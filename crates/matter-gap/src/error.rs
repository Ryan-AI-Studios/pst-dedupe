//! Typed errors for matter-gap.

use thiserror::Error;

/// Result alias for matter-gap operations.
pub type Result<T> = std::result::Result<T, GapError>;

/// Errors from the gap analysis engine.
#[derive(Debug, Error)]
pub enum GapError {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("invalid DAT header: missing {missing}")]
    InvalidDatHeader { missing: String },

    #[error("invalid column map: {0}")]
    InvalidColumnMap(String),

    #[error("DAT file too large: {size} bytes exceeds cap {cap}")]
    DatTooLarge { size: u64, cap: u64 },

    #[error("DAT row count {count} exceeds cap {cap}")]
    DatTooManyRows { count: u64, cap: u64 },

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    /// Cooperative cancel during evaluation.
    #[error("cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}
