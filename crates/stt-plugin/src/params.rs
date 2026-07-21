//! Job params for `transcribe`.

use serde::{Deserialize, Serialize};

use crate::limits::{DEFAULT_MAX_DURATION_SECS, DEFAULT_MAX_NATIVE_BYTES};

/// JSON params for kind `"transcribe"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SttParams {
    /// Desk enable gate — job fails closed when false.
    #[serde(default)]
    pub enabled: bool,
    /// Engine selector: production accepts `"whisper_cli"` / `"auto"`.
    /// `"mock"` is rejected on the production path; tests inject via
    /// `run_transcribe_with_engine`.
    #[serde(default = "default_engine")]
    pub engine: String,
    /// Path to Whisper model weights (operator-installed; never downloaded).
    #[serde(default)]
    pub model_path: Option<String>,
    /// Optional path to `whisper-cli` / `main` (whisper.cpp).
    #[serde(default)]
    pub whisper_cli_path: Option<String>,
    /// Optional path to `ffmpeg` for video / complex audio conversion.
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    /// Language hint (default `en`).
    #[serde(default = "default_language")]
    pub language: String,
    /// Max media duration in seconds (default 3600).
    #[serde(default = "default_max_duration_secs")]
    pub max_duration_secs: u64,
    /// Max native CAS bytes (default 500_000_000).
    #[serde(default = "default_max_native_bytes")]
    pub max_native_bytes: u64,
    /// Re-transcribe even when already done for the same native (default false).
    /// Also accepted as `force` for OCR-style callers.
    #[serde(default, alias = "force")]
    pub reset: bool,
    /// Items between cancel checks / checkpoint writes (default 5).
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Scope placeholder (`all` only for P0).
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_engine() -> String {
    "auto".into()
}

fn default_language() -> String {
    "en".into()
}

fn default_max_duration_secs() -> u64 {
    DEFAULT_MAX_DURATION_SECS
}

fn default_max_native_bytes() -> u64 {
    DEFAULT_MAX_NATIVE_BYTES
}

fn default_batch_size() -> usize {
    5
}

fn default_scope() -> String {
    "all".into()
}

impl Default for SttParams {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: default_engine(),
            model_path: None,
            whisper_cli_path: None,
            ffmpeg_path: None,
            language: default_language(),
            max_duration_secs: default_max_duration_secs(),
            max_native_bytes: default_max_native_bytes(),
            reset: false,
            batch_size: default_batch_size(),
            scope: default_scope(),
        }
    }
}

impl SttParams {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.batch_size == 0 {
            return Err("batch_size must be >= 1".into());
        }
        if self.max_duration_secs == 0 {
            return Err("max_duration_secs must be >= 1".into());
        }
        if self.max_native_bytes == 0 {
            return Err("max_native_bytes must be >= 1".into());
        }
        if self.language.trim().is_empty() {
            return Err("language must be non-empty".into());
        }
        // P0 only supports scope "all". Reject placeholders that would be silently ignored.
        let scope = self.scope.trim().to_ascii_lowercase();
        if scope != "all" {
            return Err(format!(
                "unsupported scope '{scope}' (P0 supports only \"all\")"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_fail_closed_disabled() {
        let p = SttParams::from_json("{}").unwrap();
        assert!(!p.enabled);
        assert!(!p.reset);
        assert_eq!(p.batch_size, 5);
        assert_eq!(p.language, "en");
        assert_eq!(p.engine, "auto");
        assert_eq!(p.max_duration_secs, DEFAULT_MAX_DURATION_SECS);
        assert_eq!(p.max_native_bytes, DEFAULT_MAX_NATIVE_BYTES);
        p.validate().unwrap();
    }

    #[test]
    fn force_alias_maps_to_reset() {
        let p = SttParams::from_json(r#"{"enabled":true,"force":true}"#).unwrap();
        assert!(p.enabled);
        assert!(p.reset);
    }

    #[test]
    fn enabled_roundtrip() {
        let j = r#"{
            "enabled": true,
            "engine": "mock",
            "language": "en",
            "reset": false,
            "batch_size": 5
        }"#;
        let p = SttParams::from_json(j).unwrap();
        assert!(p.enabled);
        assert_eq!(p.engine, "mock");
    }

    #[test]
    fn default_scope_is_all() {
        let p = SttParams::default();
        assert_eq!(p.scope, "all");
        p.validate().unwrap();
    }

    #[test]
    fn invalid_scope_rejected() {
        let p = SttParams {
            scope: "review_corpus".into(),
            ..SttParams::default()
        };
        let err = p.validate().expect_err("scope must be all");
        assert!(
            err.to_ascii_lowercase().contains("scope"),
            "expected scope error, got {err}"
        );
    }
}
