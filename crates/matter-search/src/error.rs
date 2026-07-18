//! Typed errors for matter-search.

use thiserror::Error;

/// Result alias for matter-search operations.
pub type Result<T> = std::result::Result<T, SearchError>;

/// Errors from the matter-level Tantivy FTS engine.
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("invalid query: {0}")]
    InvalidQuery(String),

    #[error("index missing or empty — run Build / Update search index")]
    IndexMissing,

    #[error("index error: {0}")]
    Index(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl From<tantivy::TantivyError> for SearchError {
    fn from(e: tantivy::TantivyError) -> Self {
        SearchError::Index(e.to_string())
    }
}
