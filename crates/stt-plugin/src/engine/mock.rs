//! Mock STT engine for CI (no Whisper weights).

use std::io::Read;

use camino::Utf8Path;
use sha2::{Digest, Sha256};

use super::trait_::{SttEngine, TranscriptResult};
use crate::error::Result;
use crate::limits::engines;

/// Deterministic mock engine used by default tests.
///
/// Returns text derived from file content hash (or a fixed override) so
/// integration tests can assert CAS digests without installing Whisper.
#[derive(Debug)]
pub struct MockSttEngine {
    /// When set, always return this text (ignores file content).
    pub fixed_text: Option<String>,
    /// Model label for bookkeeping.
    pub model: String,
}

impl Default for MockSttEngine {
    fn default() -> Self {
        Self {
            fixed_text: Some("MOCK_STT_TRANSCRIPT hello world".into()),
            model: "mock-1.0".into(),
        }
    }
}

impl MockSttEngine {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            fixed_text: Some(text.into()),
            model: "mock-1.0".into(),
        }
    }

    pub fn from_content_hash() -> Self {
        Self {
            fixed_text: None,
            model: "mock-hash-1.0".into(),
        }
    }
}

impl SttEngine for MockSttEngine {
    fn engine_id(&self) -> &str {
        engines::MOCK
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    fn transcribe_wav_path(
        &self,
        path: &Utf8Path,
        language: Option<&str>,
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<TranscriptResult> {
        if cancel.map(|c| c()).unwrap_or(false) {
            return Err(crate::error::Error::Cancelled);
        }
        if !path.as_std_path().exists() {
            return Err(crate::error::Error::Engine(format!(
                "mock: wav not found: {path}"
            )));
        }
        let text = if let Some(t) = &self.fixed_text {
            t.clone()
        } else {
            let mut f = std::fs::File::open(path.as_std_path())?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            let mut hasher = Sha256::new();
            hasher.update(&buf);
            let d = hasher.finalize();
            let short = format!("{:02x}{:02x}{:02x}{:02x}", d[0], d[1], d[2], d[3]);
            format!("MOCK_STT_HASH_{short}")
        };
        Ok(TranscriptResult {
            text,
            language: language
                .map(|s| s.to_string())
                .or_else(|| Some("en".into())),
        })
    }
}
