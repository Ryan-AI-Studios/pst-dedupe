//! STT engine trait.

use camino::Utf8Path;

use crate::error::Result;

/// Result of a single transcription.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptResult {
    pub text: String,
    pub language: Option<String>,
}

/// Pluggable STT backend (whisper.cpp CLI, mock, future engines).
pub trait SttEngine: Send + Sync {
    /// Stable engine id (e.g. `whisper_cli`, `mock`).
    fn engine_id(&self) -> &str;

    /// Model id / label for audit (e.g. model path basename or `mock-1.0`).
    fn model_id(&self) -> &str;

    /// Transcribe a 16 kHz mono s16le WAV file path.
    ///
    /// `cancel`, when provided, is polled during long-running sidecar work so
    /// cooperative job cancel can terminate the active child (Job Object).
    fn transcribe_wav_path(
        &self,
        path: &Utf8Path,
        language: Option<&str>,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<TranscriptResult>;
}
