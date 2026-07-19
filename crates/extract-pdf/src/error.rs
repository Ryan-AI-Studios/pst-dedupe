//! Structured PDF extract errors.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// PDF extract error with stable codes for item bookkeeping / audit.
#[derive(Debug, Error)]
pub enum Error {
    #[error("not a PDF: {0}")]
    NotPdf(String),

    #[error("encrypted PDF: {0}")]
    Encrypted(String),

    #[error("pdf parse error: {0}")]
    Parse(String),

    #[error("pdf limit exceeded ({code}): {message}")]
    LimitExceeded { code: String, message: String },

    #[error("pdf empty text: {0}")]
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
    /// Short stable code for `pdf_extract_error` / item_errors.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotPdf(_) => codes::PDF_NOT_PDF,
            Self::Encrypted(_) => codes::PDF_ENCRYPTED,
            Self::Parse(_) => codes::PDF_PARSE_ERROR,
            Self::LimitExceeded { .. } => codes::PDF_LIMIT_EXCEEDED,
            Self::EmptyText(_) => codes::PDF_EMPTY_TEXT,
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
            code: codes::PDF_LIMIT_EXCEEDED.into(),
            message: message.into(),
        }
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse(message.into())
    }
}

/// Stable error code strings (spec §3.3).
pub mod codes {
    pub const PDF_NOT_PDF: &str = "pdf_not_pdf";
    pub const PDF_ENCRYPTED: &str = "pdf_encrypted";
    pub const PDF_PARSE_ERROR: &str = "pdf_parse_error";
    pub const PDF_LIMIT_EXCEEDED: &str = "pdf_limit_exceeded";
    pub const PDF_EMPTY_TEXT: &str = "pdf_empty_text";
}
