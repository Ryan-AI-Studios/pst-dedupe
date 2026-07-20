//! Errors for people_graph.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, PeopleError>;

/// People graph / job errors.
#[derive(Debug, Error)]
pub enum PeopleError {
    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("{0}")]
    Other(String),
}

impl PeopleError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}
