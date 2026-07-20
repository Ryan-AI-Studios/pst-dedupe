//! Job params for `entity_scan`.

use serde::{Deserialize, Serialize};

use crate::error::{EntityError, Result};
use crate::packs::{default_pack_ids, is_known_pack};

/// Scope: all candidates with text and/or subject.
pub const SCOPE_ALL: &str = "all";

/// JSON params for kind `"entity_scan"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityScanParams {
    /// Enabled pack ids. Empty / omitted → all five built-ins.
    #[serde(default = "default_packs")]
    pub packs: Vec<String>,
    /// Cap on CAS text bytes loaded per item (default 2_000_000).
    #[serde(default = "default_max_text_bytes")]
    pub max_text_bytes: u64,
    /// When true, wipe all matter entity hits then rescan every candidate.
    #[serde(default)]
    pub reset: bool,
    /// Page size for keyset candidate listing.
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    /// Candidate scope (P0: `"all"` only).
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_packs() -> Vec<String> {
    default_pack_ids()
}

fn default_max_text_bytes() -> u64 {
    2_000_000
}

fn default_batch_size() -> u32 {
    100
}

fn default_scope() -> String {
    SCOPE_ALL.into()
}

impl Default for EntityScanParams {
    fn default() -> Self {
        Self {
            packs: default_packs(),
            max_text_bytes: default_max_text_bytes(),
            reset: false,
            batch_size: default_batch_size(),
            scope: default_scope(),
        }
    }
}

impl EntityScanParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let mut p: Self =
            serde_json::from_str(json).map_err(|e| EntityError::InvalidParams(e.to_string()))?;
        if p.packs.is_empty() {
            p.packs = default_packs();
        }
        p.validate()?;
        Ok(p)
    }

    /// Validate packs, caps, and scope.
    pub fn validate(&self) -> Result<()> {
        if self.scope != SCOPE_ALL {
            return Err(EntityError::InvalidParams(format!(
                "unknown entity_scan scope '{}' (expected {SCOPE_ALL})",
                self.scope
            )));
        }
        if self.max_text_bytes == 0 {
            return Err(EntityError::InvalidParams(
                "max_text_bytes must be > 0".into(),
            ));
        }
        if self.batch_size == 0 {
            return Err(EntityError::InvalidParams("batch_size must be > 0".into()));
        }
        for p in &self.packs {
            if !is_known_pack(p) {
                return Err(EntityError::InvalidParams(format!(
                    "unknown entity pack '{p}'"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty() {
        let p = EntityScanParams::from_json("{}").unwrap();
        assert_eq!(p.packs.len(), 5);
        assert_eq!(p.max_text_bytes, 2_000_000);
        assert!(!p.reset);
        assert_eq!(p.batch_size, 100);
        assert_eq!(p.scope, "all");
    }

    #[test]
    fn rejects_unknown_pack() {
        let err = EntityScanParams::from_json(r#"{"packs":["ner"]}"#).unwrap_err();
        assert!(err.to_string().contains("unknown entity pack"));
    }
}
