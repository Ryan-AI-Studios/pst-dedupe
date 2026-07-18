//! Job params for matter-level promote-to-review.

use serde::{Deserialize, Serialize};

use crate::error::{PromoteError, Result};
use crate::policy::{policy_id_valid, POLICY_AUTO};

/// Default review set display name.
pub const DEFAULT_REVIEW_SET_NAME: &str = "Review Corpus";

/// JSON params for kind `"promote"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromoteParams {
    /// Policy id or `"auto"` (default).
    #[serde(default = "default_policy")]
    pub policy: String,
    /// Target review set name (default set when matching default name).
    #[serde(default = "default_review_set_name")]
    pub review_set_name: String,
    /// Bidirectional family expand (default true).
    #[serde(default = "default_true")]
    pub expand_families: bool,
    /// Clear prior membership for the set then recompute (default true).
    #[serde(default = "default_true")]
    pub reset: bool,
    /// Checkpoint / write batch size (default 500).
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
    /// If true and no item has `dedup_role`, fail the job.
    #[serde(default)]
    pub require_dedupe: bool,
    /// Reserved: fail when final membership is empty (default false).
    #[serde(default)]
    pub fail_if_empty: bool,
    /// Reserved for 0056 — thread expand is never applied in P0.
    #[serde(default)]
    pub expand_threads: bool,
}

fn default_policy() -> String {
    POLICY_AUTO.into()
}

fn default_review_set_name() -> String {
    DEFAULT_REVIEW_SET_NAME.into()
}

fn default_true() -> bool {
    true
}

fn default_batch_size() -> u64 {
    500
}

impl Default for PromoteParams {
    fn default() -> Self {
        Self {
            policy: default_policy(),
            review_set_name: default_review_set_name(),
            expand_families: true,
            reset: true,
            batch_size: default_batch_size(),
            require_dedupe: false,
            fail_if_empty: false,
            expand_threads: false,
        }
    }
}

impl PromoteParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| PromoteError::InvalidParams(e.to_string()))?;
        p.validate_shape()?;
        Ok(p)
    }

    /// Validate batch size and known policy ids.
    pub fn validate_shape(&self) -> Result<()> {
        if self.batch_size == 0 {
            return Err(PromoteError::InvalidParams(
                "batch_size must be >= 1".into(),
            ));
        }
        if !policy_id_valid(&self.policy) {
            return Err(PromoteError::InvalidParams(format!(
                "unknown promote policy '{}'",
                self.policy
            )));
        }
        if self.expand_threads {
            return Err(PromoteError::InvalidParams(
                "expand_threads is reserved (track 0056); set expand_threads=false".into(),
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
        let p = PromoteParams::from_json("{}").unwrap();
        assert_eq!(p.policy, "auto");
        assert!(p.expand_families);
        assert!(p.reset);
        assert_eq!(p.batch_size, 500);
        assert!(!p.require_dedupe);
        assert_eq!(p.review_set_name, "Review Corpus");
    }

    #[test]
    fn rejects_unknown_policy() {
        let err = PromoteParams::from_json(r#"{"policy":"nope"}"#).unwrap_err();
        assert!(err.to_string().contains("unknown promote policy"));
    }
}
