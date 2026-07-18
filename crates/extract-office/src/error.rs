//! Structured office extract errors.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Office extract error with stable codes for item bookkeeping / audit.
#[derive(Debug, Error)]
pub enum Error {
    #[error("unsupported legacy Office format (OLE): {0}")]
    UnsupportedLegacy(String),

    #[error("encrypted Office package: {0}")]
    Encrypted(String),

    #[error("office parse error: {0}")]
    Parse(String),

    #[error("office limit exceeded ({code}): {message}")]
    LimitExceeded { code: String, message: String },

    #[error("office empty text: {0}")]
    EmptyText(String),

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
    /// Short stable code for `office_extract_error` / item_errors.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedLegacy(_) => codes::UNSUPPORTED_LEGACY_OFFICE,
            Self::Encrypted(_) => codes::ENCRYPTED_OFFICE,
            Self::Parse(_) => codes::OFFICE_PARSE_ERROR,
            Self::LimitExceeded { .. } => codes::OFFICE_LIMIT_EXCEEDED,
            Self::EmptyText(_) => codes::OFFICE_EMPTY_TEXT,
            Self::Matter(_) => "matter_error",
            Self::Io(_) => "io_error",
            Self::InvalidParams(_) => "invalid_params",
            Self::Other(_) => "other",
        }
    }

    /// Human-readable message (without the code prefix duplication).
    pub fn short_message(&self) -> String {
        self.to_string()
    }

    pub fn limit(message: impl Into<String>) -> Self {
        Self::LimitExceeded {
            code: codes::OFFICE_LIMIT_EXCEEDED.into(),
            message: message.into(),
        }
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse(message.into())
    }
}

/// Stable error code strings (spec §3.3 / §3.4).
pub mod codes {
    pub const UNSUPPORTED_LEGACY_OFFICE: &str = "unsupported_legacy_office";
    pub const ENCRYPTED_OFFICE: &str = "encrypted_office";
    pub const OFFICE_PARSE_ERROR: &str = "office_parse_error";
    pub const OFFICE_LIMIT_EXCEEDED: &str = "office_limit_exceeded";
    pub const OFFICE_EMPTY_TEXT: &str = "office_empty_text";
}
