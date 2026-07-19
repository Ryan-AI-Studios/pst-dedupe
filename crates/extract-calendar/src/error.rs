//! Structured ICS extract errors.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// ICS extract error with stable codes for item bookkeeping / audit.
#[derive(Debug, Error)]
pub enum Error {
    #[error("not an ICS calendar: {0}")]
    NotIcs(String),

    #[error("ics parse error: {0}")]
    Parse(String),

    #[error("ics limit exceeded ({code}): {message}")]
    LimitExceeded { code: String, message: String },

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("other: {0}")]
    Other(String),
}

impl Error {
    /// Short stable code for `ics_extract_error` / item_errors.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotIcs(_) => codes::ICS_NOT_ICS,
            Self::Parse(_) => codes::ICS_PARSE_ERROR,
            Self::LimitExceeded { .. } => codes::ICS_LIMIT_EXCEEDED,
            Self::Matter(_) => "matter_error",
            Self::Io(_) => "io_error",
            Self::InvalidParams(_) => "invalid_params",
            Self::Other(_) => "other",
        }
    }

    /// Human-readable message.
    pub fn short_message(&self) -> String {
        self.to_string()
    }

    pub fn limit(message: impl Into<String>) -> Self {
        Self::LimitExceeded {
            code: codes::ICS_LIMIT_EXCEEDED.into(),
            message: message.into(),
        }
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse(message.into())
    }
}

/// Stable error code strings.
pub mod codes {
    pub const ICS_NOT_ICS: &str = "ics_not_ics";
    pub const ICS_PARSE_ERROR: &str = "ics_parse_error";
    pub const ICS_LIMIT_EXCEEDED: &str = "ics_limit_exceeded";
}
