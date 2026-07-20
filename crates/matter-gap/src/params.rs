//! Job params for gap analysis.

use serde::{Deserialize, Serialize};

use crate::error::{GapError, Result};

/// Job kind / run mode: collection roster gaps.
pub const KIND_COLLECTION: &str = "collection";
/// Opposing DAT set-diff.
pub const KIND_OPPOSING: &str = "opposing";
/// Both collection and opposing in one job.
pub const KIND_BOTH: &str = "both";

/// Date hole bucket: calendar week (default).
pub const BUCKET_WEEK: &str = "week";
/// Date hole bucket: calendar month.
pub const BUCKET_MONTH: &str = "month";

/// Matter side scope: all items.
pub const SCOPE_INVENTORY: &str = "inventory";
/// Matter side scope: review corpus only.
pub const SCOPE_IN_REVIEW: &str = "in_review";
/// Matter side scope: a production set.
pub const SCOPE_PRODUCTION_SET: &str = "production_set";

/// Max opposing DAT file size (bytes).
pub const DEFAULT_MAX_DAT_BYTES: u64 = 256 * 1024 * 1024;
/// Max opposing DAT data rows.
pub const DEFAULT_MAX_DAT_ROWS: u64 = 2_000_000;

/// Collection-only params (also embedded in unified [`GapParams`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionGapParams {
    /// Optional relevance window start (RFC3339 UTC).
    #[serde(default)]
    pub window_start: Option<String>,
    /// Optional relevance window end (RFC3339 UTC).
    #[serde(default)]
    pub window_end: Option<String>,
    /// Date hole bucket: `week` (default) or `month`. Day is forbidden.
    #[serde(default = "default_bucket")]
    pub bucket: String,
    /// Flag custodians present in matter but not on roster (default true).
    #[serde(default = "default_true")]
    pub flag_unexpected_custodian: bool,
    /// Optional report directory (must not already exist when set).
    #[serde(default)]
    pub report_dir: Option<String>,
}

fn default_bucket() -> String {
    BUCKET_WEEK.into()
}

fn default_true() -> bool {
    true
}

impl Default for CollectionGapParams {
    fn default() -> Self {
        Self {
            window_start: None,
            window_end: None,
            bucket: default_bucket(),
            flag_unexpected_custodian: true,
            report_dir: None,
        }
    }
}

impl CollectionGapParams {
    pub fn validate_shape(&self) -> Result<()> {
        match self.bucket.as_str() {
            BUCKET_WEEK | BUCKET_MONTH => {}
            "day" => {
                return Err(GapError::InvalidParams(
                    "date bucket 'day' is forbidden in P0 (use week or month)".into(),
                ));
            }
            other => {
                return Err(GapError::InvalidParams(format!(
                    "unknown date bucket '{other}' (expected week or month)"
                )));
            }
        }
        Ok(())
    }
}

/// Opposing set-diff params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpposingGapParams {
    /// Import id from prior opposing DAT import.
    pub import_id: String,
    /// Matter side scope: inventory | in_review | production_set.
    #[serde(default = "default_matter_scope")]
    pub matter_scope: String,
    /// Required when matter_scope is production_set.
    #[serde(default)]
    pub production_set_id: Option<String>,
    /// Report matter items not in expected set (default false).
    #[serde(default)]
    pub flag_matter_not_in_expected: bool,
    #[serde(default)]
    pub report_dir: Option<String>,
}

fn default_matter_scope() -> String {
    SCOPE_INVENTORY.into()
}

impl Default for OpposingGapParams {
    fn default() -> Self {
        Self {
            import_id: String::new(),
            matter_scope: default_matter_scope(),
            production_set_id: None,
            flag_matter_not_in_expected: false,
            report_dir: None,
        }
    }
}

impl OpposingGapParams {
    pub fn validate_shape(&self) -> Result<()> {
        if self.import_id.trim().is_empty() {
            return Err(GapError::InvalidParams(
                "opposing gap requires non-empty import_id".into(),
            ));
        }
        match self.matter_scope.as_str() {
            SCOPE_INVENTORY | SCOPE_IN_REVIEW => {}
            SCOPE_PRODUCTION_SET | "production_set_id" => {
                if self
                    .production_set_id
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
                {
                    return Err(GapError::InvalidParams(
                        "matter_scope=production_set requires production_set_id".into(),
                    ));
                }
            }
            other => {
                return Err(GapError::InvalidParams(format!(
                    "unknown matter_scope '{other}'"
                )));
            }
        }
        Ok(())
    }
}

/// Unified job params for kind `"gap"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapParams {
    /// `collection` | `opposing` | `both`.
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub window_start: Option<String>,
    #[serde(default)]
    pub window_end: Option<String>,
    #[serde(default = "default_bucket")]
    pub bucket: String,
    #[serde(default = "default_true")]
    pub flag_unexpected_custodian: bool,
    #[serde(default)]
    pub import_id: Option<String>,
    #[serde(default = "default_matter_scope")]
    pub matter_scope: String,
    #[serde(default)]
    pub production_set_id: Option<String>,
    #[serde(default)]
    pub flag_matter_not_in_expected: bool,
    #[serde(default)]
    pub report_dir: Option<String>,
    /// Optional DAT size/row cap overrides (tests).
    #[serde(default)]
    pub max_dat_bytes: Option<u64>,
    #[serde(default)]
    pub max_dat_rows: Option<u64>,
}

fn default_kind() -> String {
    KIND_COLLECTION.into()
}

impl Default for GapParams {
    fn default() -> Self {
        Self {
            kind: default_kind(),
            window_start: None,
            window_end: None,
            bucket: default_bucket(),
            flag_unexpected_custodian: true,
            import_id: None,
            matter_scope: default_matter_scope(),
            production_set_id: None,
            flag_matter_not_in_expected: false,
            report_dir: None,
            max_dat_bytes: None,
            max_dat_rows: None,
        }
    }
}

impl GapParams {
    /// Parse from JSON, applying defaults for missing keys.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let p: Self =
            serde_json::from_str(json).map_err(|e| GapError::InvalidParams(e.to_string()))?;
        p.validate_shape()?;
        Ok(p)
    }

    pub fn validate_shape(&self) -> Result<()> {
        match self.kind.as_str() {
            KIND_COLLECTION | KIND_OPPOSING | KIND_BOTH => {}
            other => {
                return Err(GapError::InvalidParams(format!(
                    "unknown gap kind '{other}' (expected collection, opposing, both)"
                )));
            }
        }
        match self.bucket.as_str() {
            BUCKET_WEEK | BUCKET_MONTH => {}
            "day" => {
                return Err(GapError::InvalidParams(
                    "date bucket 'day' is forbidden in P0 (use week or month)".into(),
                ));
            }
            other => {
                return Err(GapError::InvalidParams(format!(
                    "unknown date bucket '{other}' (expected week or month)"
                )));
            }
        }
        if matches!(self.kind.as_str(), KIND_OPPOSING | KIND_BOTH)
            && self
                .import_id
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        {
            return Err(GapError::InvalidParams(
                "opposing/both gap requires import_id".into(),
            ));
        }
        match self.matter_scope.as_str() {
            SCOPE_INVENTORY | SCOPE_IN_REVIEW => {}
            SCOPE_PRODUCTION_SET | "production_set_id" => {
                if self
                    .production_set_id
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
                {
                    return Err(GapError::InvalidParams(
                        "matter_scope=production_set requires production_set_id".into(),
                    ));
                }
            }
            other => {
                return Err(GapError::InvalidParams(format!(
                    "unknown matter_scope '{other}'"
                )));
            }
        }
        Ok(())
    }

    pub fn to_collection(&self) -> CollectionGapParams {
        CollectionGapParams {
            window_start: self.window_start.clone(),
            window_end: self.window_end.clone(),
            bucket: self.bucket.clone(),
            flag_unexpected_custodian: self.flag_unexpected_custodian,
            report_dir: self.report_dir.clone(),
        }
    }

    pub fn to_opposing(&self) -> Result<OpposingGapParams> {
        let import_id = self
            .import_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| GapError::InvalidParams("import_id required".into()))?;
        let p = OpposingGapParams {
            import_id,
            matter_scope: self.matter_scope.clone(),
            production_set_id: self.production_set_id.clone(),
            flag_matter_not_in_expected: self.flag_matter_not_in_expected,
            report_dir: self.report_dir.clone(),
        };
        p.validate_shape()?;
        Ok(p)
    }

    pub fn max_dat_bytes_or_default(&self) -> u64 {
        self.max_dat_bytes.unwrap_or(DEFAULT_MAX_DAT_BYTES)
    }

    pub fn max_dat_rows_or_default(&self) -> u64 {
        self.max_dat_rows.unwrap_or(DEFAULT_MAX_DAT_ROWS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_collection() {
        let p = GapParams::from_json("{}").unwrap();
        assert_eq!(p.kind, KIND_COLLECTION);
        assert_eq!(p.bucket, BUCKET_WEEK);
        assert!(p.flag_unexpected_custodian);
    }

    #[test]
    fn rejects_day_bucket() {
        let err = GapParams::from_json(r#"{"bucket":"day"}"#).unwrap_err();
        assert!(err.to_string().contains("day"));
    }

    #[test]
    fn opposing_requires_import() {
        let err = GapParams::from_json(r#"{"kind":"opposing"}"#).unwrap_err();
        assert!(err.to_string().contains("import_id"));
    }
}
