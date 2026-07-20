//! Job params for `sentiment`.

use serde::{Deserialize, Serialize};

use crate::error::{Result, SentimentError};

/// Scope: all candidates with non-null `text_sha256`.
pub const SCOPE_ALL: &str = "all";

/// JSON params for kind `"sentiment"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentimentParams {
    /// Polarity: positive if compound ≥ this (default 0.05).
    #[serde(default = "default_pos_threshold")]
    pub pos_threshold: f64,
    /// Polarity: negative if compound ≤ this (default -0.05).
    #[serde(default = "default_neg_threshold")]
    pub neg_threshold: f64,
    /// Cap on CAS text bytes loaded per item (default 200_000).
    #[serde(default = "default_max_text_bytes")]
    pub max_text_bytes: u64,
    /// Max units (sentences/lines) scored after strip (default 200).
    #[serde(default = "default_max_units")]
    pub max_units: u32,
    /// When true, clear all scores then rescore every candidate.
    #[serde(default)]
    pub reset: bool,
    /// Page size for keyset candidate listing.
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    /// Candidate scope (P0: `"all"` only).
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_pos_threshold() -> f64 {
    0.05
}
fn default_neg_threshold() -> f64 {
    -0.05
}
fn default_max_text_bytes() -> u64 {
    200_000
}
fn default_max_units() -> u32 {
    200
}
fn default_batch_size() -> u32 {
    100
}
fn default_scope() -> String {
    SCOPE_ALL.into()
}

impl Default for SentimentParams {
    fn default() -> Self {
        Self {
            pos_threshold: default_pos_threshold(),
            neg_threshold: default_neg_threshold(),
            max_text_bytes: default_max_text_bytes(),
            max_units: default_max_units(),
            reset: false,
            batch_size: default_batch_size(),
            scope: default_scope(),
        }
    }
}

impl SentimentParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| SentimentError::InvalidParams(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Validate thresholds, caps, and scope.
    pub fn validate(&self) -> Result<()> {
        if self.scope != SCOPE_ALL {
            return Err(SentimentError::InvalidParams(format!(
                "unknown sentiment scope '{}' (expected {SCOPE_ALL})",
                self.scope
            )));
        }
        if self.pos_threshold < self.neg_threshold {
            return Err(SentimentError::InvalidParams(format!(
                "pos_threshold ({}) must be >= neg_threshold ({})",
                self.pos_threshold, self.neg_threshold
            )));
        }
        if self.max_text_bytes == 0 {
            return Err(SentimentError::InvalidParams(
                "max_text_bytes must be > 0".into(),
            ));
        }
        if self.max_units == 0 {
            return Err(SentimentError::InvalidParams(
                "max_units must be > 0".into(),
            ));
        }
        if self.batch_size == 0 {
            return Err(SentimentError::InvalidParams(
                "batch_size must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty() {
        let p = SentimentParams::from_json("{}").unwrap();
        assert!((p.pos_threshold - 0.05).abs() < f64::EPSILON);
        assert!((p.neg_threshold - (-0.05)).abs() < f64::EPSILON);
        assert_eq!(p.max_text_bytes, 200_000);
        assert_eq!(p.max_units, 200);
        assert!(!p.reset);
        assert_eq!(p.batch_size, 100);
        assert_eq!(p.scope, "all");
    }

    #[test]
    fn rejects_inverted_thresholds() {
        let err = SentimentParams::from_json(r#"{"pos_threshold":-0.1,"neg_threshold":0.1}"#)
            .unwrap_err();
        assert!(err.to_string().contains("pos_threshold"));
    }
}
