//! Job params for matter-level dedupe.

use serde::{Deserialize, Serialize};

use crate::policy::FamilyPolicy;

/// JSON params for kind `"dedupe"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupeParams {
    /// Use normalized Message-ID as tier 1 (default true).
    #[serde(default = "default_true")]
    pub use_message_id: bool,
    /// Use `logical_hash` as tier 2 (default true).
    #[serde(default = "default_true")]
    pub use_logical_hash: bool,
    /// Family attachment policy (default suppress children with parent).
    #[serde(default)]
    pub family_policy: FamilyPolicy,
    /// Clear prior dedupe fields then full recompute (default true).
    #[serde(default = "default_true")]
    pub reset: bool,
    /// Commit batch size for role updates + checkpoint (default 500).
    #[serde(default = "default_batch_size")]
    pub batch_size: u64,
}

fn default_true() -> bool {
    true
}

fn default_batch_size() -> u64 {
    500
}

impl Default for DedupeParams {
    fn default() -> Self {
        Self {
            use_message_id: true,
            use_logical_hash: true,
            family_policy: FamilyPolicy::default(),
            reset: true,
            batch_size: 500,
        }
    }
}

impl DedupeParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty_object() {
        let p = DedupeParams::from_json("{}").unwrap();
        assert!(p.use_message_id);
        assert!(p.use_logical_hash);
        assert!(p.reset);
        assert_eq!(p.batch_size, 500);
        assert_eq!(p.family_policy, FamilyPolicy::SuppressChildrenWithParent);
    }

    #[test]
    fn parse_overrides() {
        let p = DedupeParams::from_json(
            r#"{"use_message_id":false,"batch_size":10,"family_policy":"parents_only"}"#,
        )
        .unwrap();
        assert!(!p.use_message_id);
        assert_eq!(p.batch_size, 10);
        assert_eq!(p.family_policy, FamilyPolicy::ParentsOnly);
    }
}
