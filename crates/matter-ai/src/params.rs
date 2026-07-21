//! Job params for `ai_suggest_codes`.

use serde::{Deserialize, Serialize};

use crate::error::{AiError, Result};
use crate::truncate::DEFAULT_MAX_TEXT_BYTES;

/// Preferred scope: items with `in_review = 1`.
pub const SCOPE_IN_REVIEW: &str = "in_review";
/// All items with text (not only in_review).
pub const SCOPE_ALL: &str = "all";

/// JSON params for kind `"ai_suggest_codes"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSuggestCodesParams {
    /// `in_review` (default) or `all`.
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Max items to process this run (default 100).
    #[serde(default = "default_max_items")]
    pub max_items: u64,
    /// Middle-drop cap for item text bytes (default 8000).
    #[serde(default = "default_max_text_bytes")]
    pub max_text_bytes: u64,
    /// When true, supersede prior pending and re-suggest even if fingerprint matches.
    #[serde(default)]
    pub reset: bool,
    /// Sampling temperature (default 0.0 for coding).
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    /// Page size for candidate listing.
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    /// Optional max tokens for completion.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_scope() -> String {
    SCOPE_IN_REVIEW.into()
}
fn default_max_items() -> u64 {
    100
}
fn default_max_text_bytes() -> u64 {
    DEFAULT_MAX_TEXT_BYTES as u64
}
fn default_temperature() -> f32 {
    0.0
}
fn default_batch_size() -> u32 {
    25
}
fn default_max_tokens() -> u32 {
    1024
}

impl Default for AiSuggestCodesParams {
    fn default() -> Self {
        Self {
            scope: default_scope(),
            max_items: default_max_items(),
            max_text_bytes: default_max_text_bytes(),
            reset: false,
            temperature: default_temperature(),
            batch_size: default_batch_size(),
            max_tokens: default_max_tokens(),
        }
    }
}

impl AiSuggestCodesParams {
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| AiError::InvalidParams(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<()> {
        match self.scope.as_str() {
            SCOPE_IN_REVIEW | SCOPE_ALL => {}
            other => {
                return Err(AiError::InvalidParams(format!(
                    "unknown ai_suggest_codes scope '{other}' (expected {SCOPE_IN_REVIEW}|{SCOPE_ALL})"
                )));
            }
        }
        if self.max_items == 0 {
            return Err(AiError::InvalidParams("max_items must be > 0".into()));
        }
        if self.max_text_bytes == 0 {
            return Err(AiError::InvalidParams("max_text_bytes must be > 0".into()));
        }
        if self.batch_size == 0 {
            return Err(AiError::InvalidParams("batch_size must be > 0".into()));
        }
        if self.max_tokens == 0 {
            return Err(AiError::InvalidParams("max_tokens must be > 0".into()));
        }
        if !(0.0..=2.0).contains(&self.temperature) {
            return Err(AiError::InvalidParams(
                "temperature must be between 0.0 and 2.0".into(),
            ));
        }
        Ok(())
    }

    pub fn in_review_only(&self) -> bool {
        self.scope == SCOPE_IN_REVIEW
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty() {
        let p = AiSuggestCodesParams::from_json("{}").unwrap();
        assert_eq!(p.scope, "in_review");
        assert_eq!(p.max_items, 100);
        assert_eq!(p.max_text_bytes, 8000);
        assert!(!p.reset);
        assert_eq!(p.temperature, 0.0);
    }
}
