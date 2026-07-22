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
    /// Bates / control number prefix.
    ///
    /// `None` = not set by job → production profile `bates.prefix` wins.
    /// When set (including explicit `"PROD"`), overrides the profile.
    #[serde(default)]
    pub bates_prefix: Option<String>,
    /// Zero-pad width for sequence.
    ///
    /// `None` = not set by job → production profile `bates.pad_width` wins.
    #[serde(default)]
    pub seq_width: Option<u32>,
    /// Job-time Bates start sequence (1-based). **Never** stored in a profile.
    ///
    /// **Required** at produce time (`Some(n)` with `n >= 1`). Multi-volume
    /// productions must set this explicitly (e.g. volume 2 starts at 5001).
    /// Omitted / null → invalid params (no silent default of 1).
    #[serde(default)]
    pub bates_start: Option<u64>,
    /// Production profile slug (built-in or matter-local). When null, uses
    /// `us_concordance_native_text_v1`.
    #[serde(default)]
    pub production_profile: Option<String>,
    /// Optional QC pack id override (otherwise taken from the profile).
    #[serde(default)]
    pub qc_pack_id: Option<String>,
    /// Abort entire job if any selected item is withheld.
    #[serde(default)]
    pub fail_if_withheld: bool,
    /// Generate export-only EML for email items missing native.
    ///
    /// `None` = not set by job → production profile value wins.
    #[serde(default)]
    pub export_eml_if_missing_native: Option<bool>,
    /// Write `DATA/load.csv` twin alongside DAT.
    ///
    /// `None` = not set by job → production profile value wins.
    #[serde(default)]
    pub include_csv_twin: Option<bool>,
    /// Family expand residual (default false — produce exact selection).
    ///
    /// `None` = not set by job → production profile value wins.
    #[serde(default)]
    pub expand_family: Option<bool>,
    /// Output root; when null, `exports/productions/<name_or_stamp>/` under matter.
    #[serde(default)]
    pub output_dir: Option<String>,
    /// Require a fresh passed production QC run for the same selection (track 0041).
    /// Fingerprint includes the bound QC pack id.
    ///
    /// `None` = not set by job → production profile value wins (built-in default true).
    #[serde(default)]
    pub require_qc_pass: Option<bool>,
}

fn default_scope() -> String {
    SCOPE_REVIEW_CORPUS.into()
}

impl Default for ProduceParams {
    fn default() -> Self {
        Self {
            scope: default_scope(),
            item_ids: Vec::new(),
            name: None,
            // Tests / Desk: set explicit values. Empty JSON must include bates_start.
            bates_prefix: None,
            seq_width: None,
            // Default helper for unit/integration tests that use `..Default`.
            // Engine `from_json` without bates_start still fails validation.
            bates_start: Some(1),
            production_profile: None,
            qc_pack_id: None,
            fail_if_withheld: false,
            // None → profile/engine defaults (include_csv true, export_eml true,
            // expand false, require_qc true on built-in US profile).
            export_eml_if_missing_native: None,
            include_csv_twin: None,
            expand_family: None,
            output_dir: None,
            require_qc_pass: None,
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
        if let Some(ref prefix_raw) = self.bates_prefix {
            let prefix = prefix_raw.trim();
            if prefix.is_empty() {
                return Err(ProduceError::InvalidParams(
                    "bates_prefix must be non-empty when set".into(),
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
        }
        if let Some(w) = self.seq_width {
            if w == 0 || w > 12 {
                return Err(ProduceError::InvalidParams(
                    "seq_width must be 1..=12 when set".into(),
                ));
            }
        }
        match self.bates_start {
            None => {
                return Err(ProduceError::InvalidParams(
                    "bates_start is required (job-time Bates start >= 1; never stored in profile)"
                        .into(),
                ));
            }
            Some(0) => {
                return Err(ProduceError::InvalidParams(
                    "bates_start must be >= 1 (job-time Bates start; never stored in profile)"
                        .into(),
                ));
            }
            Some(_) => {}
        }
        Ok(())
    }

    /// Sanitized Bates prefix when the job set one; else `None` (use profile).
    pub fn bates_prefix_clean(&self) -> Option<&str> {
        self.bates_prefix
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Job-time Bates start (validated present).
    pub fn bates_start_value(&self) -> u64 {
        self.bates_start.unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty_requires_bates_start() {
        let err = ProduceParams::from_json("{}").unwrap_err();
        assert!(
            err.to_string().contains("bates_start"),
            "empty JSON must require bates_start: {err}"
        );
    }

    #[test]
    fn defaults_with_bates_start() {
        let p = ProduceParams::from_json(r#"{"bates_start":1}"#).unwrap();
        assert_eq!(p.scope, "review_corpus");
        assert!(p.bates_prefix.is_none());
        assert_eq!(p.bates_start, Some(1));
        assert!(p.production_profile.is_none());
        assert!(!p.fail_if_withheld);
        // Packaging knobs omitted → profile wins after resolve.
        assert!(p.export_eml_if_missing_native.is_none());
        assert!(p.include_csv_twin.is_none());
        assert!(p.expand_family.is_none());
        assert!(p.output_dir.is_none());
        assert!(p.require_qc_pass.is_none());
    }

    #[test]
    fn default_struct_has_bates_start_for_tests() {
        let p = ProduceParams::default();
        assert_eq!(p.bates_start, Some(1));
        p.validate_shape().unwrap();
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
