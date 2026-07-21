//! Structured STT errors.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// STT error with stable codes for item bookkeeping / audit.
#[derive(Debug, Error)]
pub enum Error {
    #[error("STT disabled: {0}")]
    Disabled(String),

    #[error("STT engine not found: {0}")]
    EngineNotFound(String),

    #[error("STT model not found: {0}")]
    ModelNotFound(String),

    #[error("ffmpeg not found: {0}")]
    FfmpegNotFound(String),

    #[error("STT engine error: {0}")]
    Engine(String),

    #[error("STT limit exceeded ({code}): {message}")]
    LimitExceeded { code: String, message: String },

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    /// Cooperative cancel mid ffmpeg/whisper wait (Job Object terminated).
    #[error("STT cancelled")]
    Cancelled,

    #[error("other: {0}")]
    Other(String),
}

impl Error {
    /// Short stable code for `transcript_error` / item_errors.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled(_) => codes::STT_DISABLED,
            Self::EngineNotFound(_) => codes::STT_ENGINE_NOT_FOUND,
            Self::ModelNotFound(_) => codes::STT_MODEL_NOT_FOUND,
            Self::FfmpegNotFound(_) => codes::STT_FFMPEG_NOT_FOUND,
            Self::Engine(_) => codes::STT_ENGINE_ERROR,
            Self::LimitExceeded { .. } => codes::STT_LIMIT_EXCEEDED,
            Self::Matter(_) => "matter_error",
            Self::Io(_) => "io_error",
            Self::InvalidParams(_) => "invalid_params",
            Self::Cancelled => codes::STT_CANCELLED,
            Self::Other(_) => "other",
        }
    }

    /// Human-readable message.
    pub fn short_message(&self) -> String {
        self.to_string()
    }

    pub fn limit(message: impl Into<String>) -> Self {
        Self::LimitExceeded {
            code: codes::STT_LIMIT_EXCEEDED.into(),
            message: message.into(),
        }
    }

    /// True when this is cooperative mid-item cancel (do not mark item failed).
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// Stable error code strings.
pub mod codes {
    pub const STT_DISABLED: &str = "stt_disabled";
    pub const STT_ENGINE_NOT_FOUND: &str = "stt_engine_not_found";
    pub const STT_MODEL_NOT_FOUND: &str = "stt_model_not_found";
    pub const STT_FFMPEG_NOT_FOUND: &str = "stt_ffmpeg_not_found";
    pub const STT_ENGINE_ERROR: &str = "stt_engine_error";
    pub const STT_LIMIT_EXCEEDED: &str = "stt_limit_exceeded";
    pub const STT_EMPTY_TEXT: &str = "stt_empty_text";
    pub const STT_VIDEO_NEEDS_FFMPEG: &str = "stt_video_needs_ffmpeg";
    pub const STT_CANCELLED: &str = "stt_cancelled";
}
