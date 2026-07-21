//! Job params for `teams_extract`.

use serde::{Deserialize, Serialize};

use crate::limits::{DEFAULT_MAX_HTML_BYTES, DEFAULT_MAX_MESSAGES_PER_FILE};

/// JSON params for kind `"teams_extract"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamsExtractParams {
    /// Optional source filter; null = entire matter.
    #[serde(default)]
    pub source_id: Option<String>,
    /// Formats to process: `pst`, `html`, `json`.
    #[serde(default = "default_formats")]
    pub formats: Vec<String>,
    /// Max CAS bytes for HTML/JSON export leaves.
    #[serde(default = "default_max_html_bytes")]
    pub max_html_bytes: u64,
    /// Max messages emitted per export file.
    #[serde(default = "default_max_messages")]
    pub max_messages_per_file: usize,
    /// When true, re-process leaves already marked ok/skipped.
    #[serde(default)]
    pub reset: bool,
    /// Items between cancel checks / checkpoint writes.
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Alias for re-extract (same effect as `reset` when true).
    #[serde(default)]
    pub force: bool,
}

fn default_formats() -> Vec<String> {
    vec!["pst".into(), "html".into(), "json".into()]
}

fn default_max_html_bytes() -> u64 {
    DEFAULT_MAX_HTML_BYTES
}

fn default_max_messages() -> usize {
    DEFAULT_MAX_MESSAGES_PER_FILE
}

fn default_batch_size() -> usize {
    50
}

impl Default for TeamsExtractParams {
    fn default() -> Self {
        Self {
            source_id: None,
            formats: default_formats(),
            max_html_bytes: default_max_html_bytes(),
            max_messages_per_file: default_max_messages(),
            reset: false,
            batch_size: default_batch_size(),
            force: false,
        }
    }
}

impl TeamsExtractParams {
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
        if self.max_html_bytes == 0 {
            return Err("max_html_bytes must be >= 1".into());
        }
        if self.max_messages_per_file == 0 {
            return Err("max_messages_per_file must be >= 1".into());
        }
        if self.formats.is_empty() {
            return Err("formats must not be empty".into());
        }
        for f in &self.formats {
            match f.as_str() {
                "pst" | "html" | "json" => {}
                other => return Err(format!("unknown format: {other}")),
            }
        }
        Ok(())
    }

    /// True when force or reset requests re-processing.
    pub fn reprocess(&self) -> bool {
        self.force || self.reset
    }

    pub fn allows_format(&self, format: &str) -> bool {
        self.formats.iter().any(|f| f == format)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let p = TeamsExtractParams::from_json("{}").unwrap();
        assert!(!p.force);
        assert!(!p.reset);
        assert_eq!(p.batch_size, 50);
        assert_eq!(p.max_html_bytes, DEFAULT_MAX_HTML_BYTES);
        assert!(p.allows_format("html"));
        p.validate().unwrap();
    }
}
