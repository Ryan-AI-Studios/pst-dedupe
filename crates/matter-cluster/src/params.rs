//! Job params for `concept_cluster`.

use serde::{Deserialize, Serialize};

use crate::error::{ClusterError, Result};

/// Scope: all items with non-null `text_sha256` (P0 only).
pub const SCOPE_ALL: &str = "all";

/// Default set name.
pub const DEFAULT_SET_NAME: &str = "default";

/// JSON params for kind `"concept_cluster"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptClusterParams {
    /// Named set (P0 default: `"default"`).
    #[serde(default = "default_set_name")]
    pub set_name: String,
    /// Requested k (target cluster count).
    #[serde(default = "default_k")]
    pub k: u32,
    /// Deterministic RNG seed.
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// Hard cap on candidate documents — **fail closed** if exceeded.
    #[serde(default = "default_max_docs")]
    pub max_docs: u64,
    /// Cap on CAS text bytes loaded per item.
    #[serde(default = "default_max_text_bytes")]
    pub max_text_bytes: u64,
    /// Drop terms with document frequency below this.
    #[serde(default = "default_min_df")]
    pub min_df: u32,
    /// Drop terms with DF ratio above this (in (0, 1]).
    #[serde(default = "default_max_df_ratio")]
    pub max_df_ratio: f64,
    /// Cap vocabulary size by DF rank.
    #[serde(default = "default_max_vocab")]
    pub max_vocab: u32,
    /// Top c-TF-IDF terms for labels.
    #[serde(default = "default_label_terms")]
    pub label_terms: u32,
    /// Candidate scope (P0: `"all"` only).
    #[serde(default = "default_scope")]
    pub scope: String,
    /// When true, rebuild set (default).
    #[serde(default = "default_reset")]
    pub reset: bool,
    /// Phase A page size for candidate listing.
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    /// Max k-means iterations.
    #[serde(default = "default_max_iters")]
    pub max_iters: u32,
    /// Drop pure-digit tokens after tokenize (default true).
    #[serde(default = "default_drop_digits")]
    pub drop_digits: bool,
}

fn default_set_name() -> String {
    DEFAULT_SET_NAME.into()
}
fn default_k() -> u32 {
    20
}
fn default_seed() -> u64 {
    42
}
fn default_max_docs() -> u64 {
    50_000
}
fn default_max_text_bytes() -> u64 {
    200_000
}
fn default_min_df() -> u32 {
    2
}
fn default_max_df_ratio() -> f64 {
    0.9
}
fn default_max_vocab() -> u32 {
    20_000
}
fn default_label_terms() -> u32 {
    8
}
fn default_scope() -> String {
    SCOPE_ALL.into()
}
fn default_reset() -> bool {
    true
}
fn default_batch_size() -> u32 {
    100
}
fn default_max_iters() -> u32 {
    50
}
fn default_drop_digits() -> bool {
    true
}

impl Default for ConceptClusterParams {
    fn default() -> Self {
        Self {
            set_name: default_set_name(),
            k: default_k(),
            seed: default_seed(),
            max_docs: default_max_docs(),
            max_text_bytes: default_max_text_bytes(),
            min_df: default_min_df(),
            max_df_ratio: default_max_df_ratio(),
            max_vocab: default_max_vocab(),
            label_terms: default_label_terms(),
            scope: default_scope(),
            reset: default_reset(),
            batch_size: default_batch_size(),
            max_iters: default_max_iters(),
            drop_digits: default_drop_digits(),
        }
    }
}

impl ConceptClusterParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| ClusterError::InvalidParams(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Validate caps and scope.
    pub fn validate(&self) -> Result<()> {
        if self.scope != SCOPE_ALL {
            return Err(ClusterError::InvalidParams(format!(
                "unknown concept_cluster scope '{}' (expected {SCOPE_ALL})",
                self.scope
            )));
        }
        if self.k < 1 {
            return Err(ClusterError::InvalidParams("k must be >= 1".into()));
        }
        if self.max_docs < 1 {
            return Err(ClusterError::InvalidParams("max_docs must be >= 1".into()));
        }
        if self.max_text_bytes == 0 {
            return Err(ClusterError::InvalidParams(
                "max_text_bytes must be > 0".into(),
            ));
        }
        if self.min_df < 1 {
            return Err(ClusterError::InvalidParams("min_df must be >= 1".into()));
        }
        if !(self.max_df_ratio > 0.0 && self.max_df_ratio <= 1.0) {
            return Err(ClusterError::InvalidParams(
                "max_df_ratio must be in (0, 1]".into(),
            ));
        }
        if self.max_vocab < 1 {
            return Err(ClusterError::InvalidParams("max_vocab must be >= 1".into()));
        }
        if self.label_terms < 1 {
            return Err(ClusterError::InvalidParams(
                "label_terms must be >= 1".into(),
            ));
        }
        if self.batch_size == 0 {
            return Err(ClusterError::InvalidParams("batch_size must be > 0".into()));
        }
        if self.max_iters < 1 {
            return Err(ClusterError::InvalidParams("max_iters must be >= 1".into()));
        }
        if self.set_name.trim().is_empty() {
            return Err(ClusterError::InvalidParams(
                "set_name must be non-empty".into(),
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
        let p = ConceptClusterParams::from_json("{}").unwrap();
        assert_eq!(p.set_name, "default");
        assert_eq!(p.k, 20);
        assert_eq!(p.seed, 42);
        assert_eq!(p.max_docs, 50_000);
        assert_eq!(p.max_text_bytes, 200_000);
        assert_eq!(p.min_df, 2);
        assert!((p.max_df_ratio - 0.9).abs() < 1e-9);
        assert_eq!(p.max_vocab, 20_000);
        assert_eq!(p.label_terms, 8);
        assert_eq!(p.scope, "all");
        assert!(p.reset);
        assert_eq!(p.batch_size, 100);
    }

    #[test]
    fn rejects_k_zero() {
        let err = ConceptClusterParams::from_json(r#"{"k":0}"#).unwrap_err();
        assert!(err.to_string().contains("k must"));
    }

    #[test]
    fn rejects_bad_scope() {
        let err = ConceptClusterParams::from_json(r#"{"scope":"in_review"}"#).unwrap_err();
        assert!(err.to_string().contains("scope"));
    }

    #[test]
    fn rejects_max_df_ratio() {
        let err = ConceptClusterParams::from_json(r#"{"max_df_ratio":0}"#).unwrap_err();
        assert!(err.to_string().contains("max_df_ratio"));
        let err2 = ConceptClusterParams::from_json(r#"{"max_df_ratio":1.5}"#).unwrap_err();
        assert!(err2.to_string().contains("max_df_ratio"));
    }
}
