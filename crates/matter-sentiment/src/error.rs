//! Errors for sentiment scoring job.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, SentimentError>;

/// Sentiment engine / job errors.
#[derive(Debug, Error)]
pub enum SentimentError {
    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("{0}")]
    Other(String),
}

impl SentimentError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}
