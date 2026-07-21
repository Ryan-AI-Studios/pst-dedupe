//! Structured Teams extract errors.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Teams extract error with stable codes for item bookkeeping / audit.
#[derive(Debug, Error)]
pub enum Error {
    #[error("teams parse error: {0}")]
    Parse(String),

    #[error("teams limit exceeded ({code}): {message}")]
    LimitExceeded { code: String, message: String },

    #[error("teams cas error: {0}")]
    Cas(String),

    #[error("teams utf8 error: {0}")]
    Utf8(String),

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("other: {0}")]
    Other(String),
}

impl Error {
    /// Short stable code for `teams_extract_error` / item_errors.
    pub fn code(&self) -> &str {
        match self {
            Self::Parse(_) => codes::TEAMS_PARSE_ERROR,
            Self::LimitExceeded { code, .. } => code.as_str(),
            Self::Cas(_) => codes::TEAMS_CAS_ERROR,
            Self::Utf8(_) => codes::TEAMS_UTF8_ERROR,
            Self::Matter(_) => "matter_error",
            Self::Io(_) => "io_error",
            Self::InvalidParams(_) => "invalid_params",
            Self::UnsupportedFormat(_) => codes::TEAMS_UNSUPPORTED,
            Self::Other(_) => "other",
        }
    }

    /// Human-readable message.
    pub fn short_message(&self) -> String {
        self.to_string()
    }

    pub fn limit(message: impl Into<String>) -> Self {
        Self::LimitExceeded {
            code: codes::TEAMS_LIMIT_EXCEEDED.into(),
            message: message.into(),
        }
    }

    /// File has more messages than `max_messages_per_file` — fail closed (no silent drop).
    pub fn max_messages_exceeded(max: usize) -> Self {
        Self::LimitExceeded {
            code: codes::MAX_MESSAGES_EXCEEDED.into(),
            message: format!("message count exceeds max_messages_per_file ({max})"),
        }
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse(message.into())
    }

    pub fn cas(message: impl Into<String>) -> Self {
        Self::Cas(message.into())
    }

    pub fn utf8(message: impl Into<String>) -> Self {
        Self::Utf8(message.into())
    }
}

/// Stable error code strings.
pub mod codes {
    pub const TEAMS_PARSE_ERROR: &str = "teams_parse_error";
    pub const TEAMS_LIMIT_EXCEEDED: &str = "teams_limit_exceeded";
    /// Message count exceeds `max_messages_per_file` (no silent truncation).
    pub const MAX_MESSAGES_EXCEEDED: &str = "max_messages_exceeded";
    pub const TEAMS_UNSUPPORTED: &str = "teams_unsupported";
    pub const TEAMS_NOT_TEAMS: &str = "teams_not_teams";
    /// CAS open/read failed for a declared `text_sha256`.
    pub const TEAMS_CAS_ERROR: &str = "teams_cas_error";
    /// Declared text CAS is not valid UTF-8.
    pub const TEAMS_UTF8_ERROR: &str = "teams_utf8_error";
}
