//! Errors for concept clustering.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, ClusterError>;

/// Concept cluster job / engine errors.
#[derive(Debug, Error)]
pub enum ClusterError {
    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("max_docs exceeded: {candidate_count} candidates > max_docs {max_docs} (fail closed)")]
    MaxDocsExceeded { candidate_count: u64, max_docs: u64 },

    #[error(
        "no usable text features for clustering (candidates={candidate_count}, skipped_empty={skipped_empty}) — fail closed"
    )]
    NoUsableFeatures {
        candidate_count: u64,
        skipped_empty: u64,
    },

    #[error(
        "no usable vocabulary after DF filters (docs={doc_count}, min_df / max_df_ratio / max_vocab) — fail closed"
    )]
    EmptyVocabulary { doc_count: u64 },

    #[error("CAS text read failed for item {item_id}: {message}")]
    CasReadFailed { item_id: String, message: String },

    #[error("{0}")]
    Other(String),
}

impl ClusterError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}
