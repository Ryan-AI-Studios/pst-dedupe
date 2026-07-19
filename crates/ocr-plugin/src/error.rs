//! Structured OCR errors.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// OCR error with stable codes for item bookkeeping / audit.
#[derive(Debug, Error)]
pub enum Error {
    #[error("OCR disabled: {0}")]
    Disabled(String),

    #[error("OCR engine not found: {0}")]
    EngineNotFound(String),

    #[error("OCR PDF renderer missing: {0}")]
    PdfRendererMissing(String),

    #[error("OCR OSD traineddata missing: {0}")]
    OsdMissing(String),

    #[error("OCR engine error: {0}")]
    Engine(String),

    #[error("OCR limit exceeded ({code}): {message}")]
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
    /// Short stable code for `ocr_error` / item_errors.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled(_) => codes::OCR_DISABLED,
            Self::EngineNotFound(_) => codes::OCR_ENGINE_NOT_FOUND,
            Self::PdfRendererMissing(_) => codes::OCR_PDF_RENDERER_MISSING,
            Self::OsdMissing(_) => codes::OCR_OSD_MISSING,
            Self::Engine(_) => codes::OCR_ENGINE_ERROR,
            Self::LimitExceeded { .. } => codes::OCR_LIMIT_EXCEEDED,
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
            code: codes::OCR_LIMIT_EXCEEDED.into(),
            message: message.into(),
        }
    }
}

/// Stable error code strings.
pub mod codes {
    pub const OCR_DISABLED: &str = "ocr_disabled";
    pub const OCR_ENGINE_NOT_FOUND: &str = "ocr_engine_not_found";
    pub const OCR_PDF_RENDERER_MISSING: &str = "ocr_pdf_renderer_missing";
    pub const OCR_OSD_MISSING: &str = "ocr_osd_missing";
    pub const OCR_ENGINE_ERROR: &str = "ocr_engine_error";
    pub const OCR_LIMIT_EXCEEDED: &str = "ocr_limit_exceeded";
    pub const OCR_REDACTIONS: &str = "ocr_redactions_present";
    pub const OCR_EMPTY_TEXT: &str = "ocr_empty_text";
}
