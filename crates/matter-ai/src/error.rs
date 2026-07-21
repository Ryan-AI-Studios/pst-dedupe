//! Errors for AI provider + suggest job.

use thiserror::Error;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, AiError>;

/// AI engine / job / provider errors.
#[derive(Debug, Error)]
pub enum AiError {
    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("AI disabled — enable AI in matter settings before running AI jobs")]
    AiDisabled,

    #[error("remote AI blocked — set allow_remote to use non-loopback base URL")]
    RemoteBlocked,

    #[error("API key missing: set PST_DEDUPE_AI_API_KEY or store key in OS keyring (service=dedupe-desk, user=ai_api_key)")]
    ApiKeyMissing,

    #[error("API key resolution failed: {0}")]
    ApiKeyError(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("JSON parse failed (ai_json_parse): {0}")]
    JsonParse(String),

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("{0}")]
    Other(String),
}

impl AiError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }

    pub fn provider(msg: impl Into<String>) -> Self {
        Self::Provider(msg.into())
    }

    pub fn json_parse(msg: impl Into<String>) -> Self {
        Self::JsonParse(msg.into())
    }
}
