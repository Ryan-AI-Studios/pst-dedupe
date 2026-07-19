//! Job params for production QC.

use serde::{Deserialize, Serialize};

use crate::error::{QcError, Result};

/// Scope: default review corpus (`in_review = 1`).
pub const SCOPE_REVIEW_CORPUS: &str = "review_corpus";
/// Scope: explicit item id list.
pub const SCOPE_ITEM_IDS: &str = "item_ids";

/// Builtin profile id.
pub const PROFILE_DEFAULT_PRODUCTION_QC_V1: &str = "default_production_qc_v1";

/// Per-rule severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QcSeverity {
    Off,
    Warn,
    Error,
}

impl QcSeverity {
    /// Parse severity string (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Stable CSV / JSON label.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    /// Rank for comparisons (Off < Warn < Error).
    pub fn rank(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Warn => 1,
            Self::Error => 2,
        }
    }
}

impl std::fmt::Display for QcSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One rule severity override (or pack entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QcRuleConfig {
    pub id: String,
    pub severity: QcSeverity,
}

/// JSON params for kind `"qc"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QcParams {
    /// Selection mode: `review_corpus` (default) or `item_ids`.
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Explicit item ids when `scope = item_ids`.
    #[serde(default)]
    pub item_ids: Vec<String>,
    /// Expand family for the *scan set* only (default false).
    #[serde(default)]
    pub expand_family_for_scan: bool,
    /// Rule overrides; empty → default pack.
    #[serde(default)]
    pub rules: Vec<QcRuleConfig>,
    /// Optional report directory (must not already exist when set).
    #[serde(default)]
    pub report_dir: Option<String>,
    /// Profile name stored on `qc_runs` (default `default_production_qc_v1`).
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_scope() -> String {
    SCOPE_REVIEW_CORPUS.into()
}

fn default_profile() -> String {
    PROFILE_DEFAULT_PRODUCTION_QC_V1.into()
}

impl Default for QcParams {
    fn default() -> Self {
        Self {
            scope: default_scope(),
            item_ids: Vec::new(),
            expand_family_for_scan: false,
            rules: Vec::new(),
            report_dir: None,
            profile: default_profile(),
        }
    }
}

impl QcParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| QcError::InvalidParams(e.to_string()))?;
        p.validate_shape()?;
        Ok(p)
    }

    /// Validate scope and item list consistency.
    pub fn validate_shape(&self) -> Result<()> {
        match self.scope.as_str() {
            SCOPE_REVIEW_CORPUS => {}
            SCOPE_ITEM_IDS => {
                if self.item_ids.is_empty() {
                    return Err(QcError::InvalidParams(
                        "scope=item_ids requires non-empty item_ids".into(),
                    ));
                }
            }
            other => {
                return Err(QcError::InvalidParams(format!(
                    "unknown qc scope '{other}' (expected review_corpus or item_ids)"
                )));
            }
        }
        if self.profile.trim().is_empty() {
            return Err(QcError::InvalidParams("profile must be non-empty".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_from_empty() {
        let p = QcParams::from_json("{}").unwrap();
        assert_eq!(p.scope, "review_corpus");
        assert!(!p.expand_family_for_scan);
        assert!(p.rules.is_empty());
        assert_eq!(p.profile, PROFILE_DEFAULT_PRODUCTION_QC_V1);
    }

    #[test]
    fn rejects_unknown_scope() {
        let err = QcParams::from_json(r#"{"scope":"nope"}"#).unwrap_err();
        assert!(err.to_string().contains("unknown qc scope"));
    }

    #[test]
    fn severity_roundtrip() {
        assert_eq!(QcSeverity::parse("ERROR"), Some(QcSeverity::Error));
        assert_eq!(QcSeverity::Warn.as_str(), "warn");
    }
}
