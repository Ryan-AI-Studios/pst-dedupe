//! Job params for matter-level production export.

use serde::{Deserialize, Serialize};

use crate::error::{ProduceError, Result};

/// Scope: default review corpus (`in_review = 1`).
pub const SCOPE_REVIEW_CORPUS: &str = "review_corpus";
/// Scope: explicit item id list.
pub const SCOPE_ITEM_IDS: &str = "item_ids";

/// Default Bates / control prefix.
pub const DEFAULT_BATES_PREFIX: &str = "PROD";

/// JSON params for kind `"produce"` (alias `"production_export"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProduceParams {
    /// Selection mode: `review_corpus` (default) or `item_ids`.
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Explicit item ids when `scope = item_ids`.
    #[serde(default)]
    pub item_ids: Vec<String>,
    /// Production display name (folder stamp when set).
    #[serde(default)]
    pub name: Option<String>,
    /// Bates / control number prefix (default `PROD`).
    #[serde(default = "default_bates_prefix")]
    pub bates_prefix: String,
    /// Zero-pad width for sequence (default 6 → `PROD000001`).
    #[serde(default = "default_seq_width")]
    pub seq_width: u32,
    /// Abort entire job if any selected item is withheld.
    #[serde(default)]
    pub fail_if_withheld: bool,
    /// Generate export-only EML for email items missing native.
    #[serde(default = "default_true")]
    pub export_eml_if_missing_native: bool,
    /// Write `DATA/load.csv` twin alongside DAT.
    #[serde(default = "default_true")]
    pub include_csv_twin: bool,
    /// Family expand residual (default false — produce exact selection).
    #[serde(default)]
    pub expand_family: bool,
    /// Output root; when null, `exports/productions/<name_or_stamp>/` under matter.
    #[serde(default)]
    pub output_dir: Option<String>,
    /// Require a fresh passed production QC run for the same selection (track 0041).
    /// Default **true** (fail closed).
    #[serde(default = "default_true")]
    pub require_qc_pass: bool,
}

fn default_scope() -> String {
    SCOPE_REVIEW_CORPUS.into()
}

fn default_bates_prefix() -> String {
    DEFAULT_BATES_PREFIX.into()
}

fn default_seq_width() -> u32 {
    6
}

fn default_true() -> bool {
    true
}

impl Default for ProduceParams {
    fn default() -> Self {
        Self {
            scope: default_scope(),
            item_ids: Vec::new(),
            name: None,
            bates_prefix: default_bates_prefix(),
            seq_width: default_seq_width(),
            fail_if_withheld: false,
            export_eml_if_missing_native: true,
            include_csv_twin: true,
            expand_family: false,
            output_dir: None,
            require_qc_pass: true,
        }
    }
}

impl ProduceParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| ProduceError::InvalidParams(e.to_string()))?;
        p.validate_shape()?;
        Ok(p)
    }

    /// Validate scope, prefix, and item list consistency.
    pub fn validate_shape(&self) -> Result<()> {
        match self.scope.as_str() {
            SCOPE_REVIEW_CORPUS => {}
            SCOPE_ITEM_IDS => {
                if self.item_ids.is_empty() {
                    return Err(ProduceError::InvalidParams(
                        "scope=item_ids requires non-empty item_ids".into(),
                    ));
                }
            }
            other => {
                return Err(ProduceError::InvalidParams(format!(
                    "unknown produce scope '{other}' (expected review_corpus or item_ids)"
                )));
            }
        }
        let prefix = self.bates_prefix.trim();
        if prefix.is_empty() {
            return Err(ProduceError::InvalidParams(
                "bates_prefix must be non-empty".into(),
            ));
        }
        if prefix
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        {
            return Err(ProduceError::InvalidParams(
                "bates_prefix may only contain ASCII alphanumeric, '_' or '-'".into(),
            ));
        }
        if self.seq_width == 0 || self.seq_width > 12 {
            return Err(ProduceError::InvalidParams(
                "seq_width must be 1..=12".into(),
            ));
        }
        Ok(())
    }

    /// Sanitized Bates prefix.
    pub fn bates_prefix_clean(&self) -> &str {
        self.bates_prefix.trim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty() {
        let p = ProduceParams::from_json("{}").unwrap();
        assert_eq!(p.scope, "review_corpus");
        assert_eq!(p.bates_prefix, "PROD");
        assert!(!p.fail_if_withheld);
        assert!(p.export_eml_if_missing_native);
        assert!(p.include_csv_twin);
        assert!(!p.expand_family);
        assert!(p.output_dir.is_none());
        assert!(p.require_qc_pass);
    }

    #[test]
    fn rejects_unknown_scope() {
        let err = ProduceParams::from_json(r#"{"scope":"nope"}"#).unwrap_err();
        assert!(err.to_string().contains("unknown produce scope"));
    }

    #[test]
    fn item_ids_requires_list() {
        let err = ProduceParams::from_json(r#"{"scope":"item_ids"}"#).unwrap_err();
        assert!(err.to_string().contains("item_ids"));
    }
}
