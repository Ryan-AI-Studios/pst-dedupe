//! Typed errors for blob storage backends.

use thiserror::Error;

/// Result alias for matter-storage operations.
pub type Result<T> = std::result::Result<T, StorageError>;

/// Errors returned by [`crate::BlobStore`] implementations and config helpers.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("blob not found: {0}")]
    NotFound(String),

    #[error("digest mismatch: expected {expected}, computed {computed}")]
    DigestMismatch { expected: String, computed: String },

    #[error("invalid SHA-256 hex digest: {0}")]
    InvalidDigest(String),

    #[error("storage config error: {0}")]
    Config(String),

    #[error("cloud storage error: {0}")]
    Cloud(String),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}
