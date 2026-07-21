//! Typed errors for matter-search.

use thiserror::Error;

/// Result alias for matter-search operations.
pub type Result<T> = std::result::Result<T, SearchError>;

/// Stable error code: language pack / index fingerprint mismatch.
pub const CODE_FTS_LANG_PACK_STALE: &str = "fts_lang_pack_stale";

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

    /// Pack setting does not match the fingerprint of the last successful index.
    ///
    /// Never silently query a mismatched index. Stable code: [`CODE_FTS_LANG_PACK_STALE`].
    #[error(
        "Index is stale due to language pack change. Rebuild required. ({CODE_FTS_LANG_PACK_STALE}: {0})"
    )]
    LangPackStale(String),

    #[error("index error: {0}")]
    Index(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl SearchError {
    /// Stable machine-readable code when applicable.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            SearchError::LangPackStale(_) => Some(CODE_FTS_LANG_PACK_STALE),
            SearchError::IndexMissing => Some("fts_index_missing"),
            _ => None,
        }
    }

    /// True when search is blocked on a language pack / rebuild requirement.
    pub fn is_lang_pack_stale(&self) -> bool {
        matches!(self, SearchError::LangPackStale(_))
    }
}

impl From<tantivy::TantivyError> for SearchError {
    fn from(e: tantivy::TantivyError) -> Self {
        SearchError::Index(e.to_string())
    }
}
