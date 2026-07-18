//! CullRules JSON v1 model and validation.

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CullError, Result};

/// Stable reason codes written into `cull_reasons_json`.
pub mod reason {
    pub const EXACT_DUPLICATE: &str = "exact_duplicate";
    pub const DATE_OUT_OF_RANGE: &str = "date_out_of_range";
    pub const DATE_MISSING: &str = "date_missing";
    pub const CUSTODIAN: &str = "custodian";
    pub const PATH: &str = "path";
    pub const FILE_CATEGORY: &str = "file_category";
    pub const MIME: &str = "mime";
    pub const SIZE: &str = "size";
    pub const EMPTY: &str = "empty";
    pub const STATUS: &str = "status";
    pub const NEAR_DUP_MEMBER: &str = "near_dup_member";
    pub const DENIST: &str = "denist";
    pub const FAMILY_WITH_CULLED_PARENT: &str = "family_with_culled_parent";
    pub const OTHER: &str = "other";
}

/// Family integrity policy after the per-item rule pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FamilyPolicy {
    /// Included parent forces **all** direct children included (absolute).
    #[default]
    KeepChildrenWithIncludedParent,
    /// Each item evaluated alone.
    Independent,
    /// Culled parent → children culled with `family_with_culled_parent`.
    CullChildrenWithParent,
}

/// Date field used for window evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DateField {
    SentAt,
    ReceivedAt,
    CreatedAt,
    #[default]
    BestEffort,
}

/// When item has no usable date and date filter is enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MissingDatePolicy {
    /// Conservative default: keep undated items.
    #[default]
    Include,
    /// Aggressive: cull undated items with reason `date_missing`.
    Cull,
}

/// Include vs exclude list mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ListMode {
    #[default]
    Include,
    Exclude,
}

/// Date window rule fragment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub field: DateField,
    /// Inclusive start bound (RFC3339 with offset or Z).
    #[serde(default)]
    pub start: Option<String>,
    /// Exclusive end bound (RFC3339 with offset or Z).
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub missing_policy: MissingDatePolicy,
}

impl Default for DateRule {
    fn default() -> Self {
        Self {
            enabled: false,
            field: DateField::BestEffort,
            start: None,
            end: None,
            missing_policy: MissingDatePolicy::Include,
        }
    }
}

/// String-list rule (custodian / file_category).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringListRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: ListMode,
    #[serde(default)]
    pub values: Vec<String>,
}

impl Default for StringListRule {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ListMode::Include,
            values: Vec::new(),
        }
    }
}

/// Path substring patterns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathContainsRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_exclude")]
    pub mode: ListMode,
    #[serde(default)]
    pub patterns: Vec<String>,
}

fn default_exclude() -> ListMode {
    ListMode::Exclude
}

impl Default for PathContainsRule {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ListMode::Exclude,
            patterns: Vec::new(),
        }
    }
}

/// MIME prefix list (typically exclude executables).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MimePrefixesRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_exclude")]
    pub mode: ListMode,
    #[serde(default)]
    pub values: Vec<String>,
}

impl Default for MimePrefixesRule {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ListMode::Exclude,
            values: Vec::new(),
        }
    }
}

/// Size bounds in bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SizeBytesRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
}

/// Empty / noise item rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub zero_size: bool,
    #[serde(default)]
    pub no_text_and_no_native: bool,
}

impl Default for EmptyRule {
    fn default() -> Self {
        Self {
            enabled: false,
            zero_size: true,
            no_text_and_no_native: false,
        }
    }
}

/// Status include gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusesRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_statuses")]
    pub include: Vec<String>,
}

fn default_statuses() -> Vec<String> {
    vec!["extracted".into(), "partial".into(), "normalized".into()]
}

impl Default for StatusesRule {
    fn default() -> Self {
        Self {
            enabled: true,
            include: default_statuses(),
        }
    }
}

/// Near-dup cull (off by default — near-dups are not exact).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct NearDupRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cull_members: bool,
    #[serde(default)]
    pub keep_pivots_only: bool,
}

/// Thread-related cull (deferred / optional).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ThreadRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub cull_singleton_only: bool,
}

/// Optional DeNIST / known-file filter (SHA-256 list only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DenistRule {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub hash_list_path: Option<String>,
}

/// Role processing options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RolesRule {
    #[serde(default = "default_true")]
    pub process_attachments: bool,
}

fn default_true() -> bool {
    true
}

impl Default for RolesRule {
    fn default() -> Self {
        Self {
            process_attachments: true,
        }
    }
}

/// Composable cull rules (JSON v1).
///
/// Item is **culled** if **any** enabled condition matches; all matching
/// reason codes are collected into `cull_reasons_json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CullRules {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_true")]
    pub exclude_exact_duplicates: bool,
    /// When non-empty and `exclude_exact_duplicates` is false, only these
    /// dedup roles are treated as included candidates for the unique-style
    /// gate. Kept for forward compatibility; P0 uses `exclude_exact_duplicates`.
    #[serde(default)]
    pub include_dedup_roles: Vec<String>,
    #[serde(default)]
    pub date: DateRule,
    #[serde(default)]
    pub custodians: StringListRule,
    #[serde(default)]
    pub path_contains: PathContainsRule,
    #[serde(default)]
    pub file_categories: StringListRule,
    #[serde(default)]
    pub mime_prefixes: MimePrefixesRule,
    #[serde(default)]
    pub size_bytes: SizeBytesRule,
    #[serde(default)]
    pub empty: EmptyRule,
    #[serde(default)]
    pub statuses: StatusesRule,
    #[serde(default)]
    pub near_dup: NearDupRule,
    #[serde(default)]
    pub thread: ThreadRule,
    #[serde(default)]
    pub denist: DenistRule,
    #[serde(default)]
    pub family_policy: FamilyPolicy,
    #[serde(default)]
    pub roles: RolesRule,
}

fn default_version() -> u32 {
    1
}

impl Default for CullRules {
    fn default() -> Self {
        Self {
            version: 1,
            exclude_exact_duplicates: true,
            include_dedup_roles: vec!["unique".into(), "skipped".into(), "none".into()],
            date: DateRule::default(),
            custodians: StringListRule::default(),
            path_contains: PathContainsRule::default(),
            file_categories: StringListRule::default(),
            mime_prefixes: MimePrefixesRule::default(),
            size_bytes: SizeBytesRule::default(),
            empty: EmptyRule::default(),
            statuses: StatusesRule::default(),
            near_dup: NearDupRule::default(),
            thread: ThreadRule::default(),
            denist: DenistRule::default(),
            family_policy: FamilyPolicy::KeepChildrenWithIncludedParent,
            roles: RolesRule::default(),
        }
    }
}

impl CullRules {
    /// Parse rules JSON (object required).
    pub fn from_json(json: &str) -> Result<Self> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        let rules: Self = serde_json::from_str(json)
            .map_err(|e| CullError::InvalidRules(format!("parse: {e}")))?;
        rules.validate()?;
        Ok(rules)
    }

    /// Validate version and date bounds (offset required when set).
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(CullError::InvalidRules(format!(
                "unsupported CullRules version {} (expected 1)",
                self.version
            )));
        }
        if self.date.enabled {
            if let Some(ref s) = self.date.start {
                parse_bound_instant(s)
                    .map_err(|e| CullError::InvalidRules(format!("date.start: {e}")))?;
            }
            if let Some(ref s) = self.date.end {
                parse_bound_instant(s)
                    .map_err(|e| CullError::InvalidRules(format!("date.end: {e}")))?;
            }
            if let (Some(start), Some(end)) = (&self.date.start, &self.date.end) {
                let a = parse_bound_instant(start)?;
                let b = parse_bound_instant(end)?;
                if a >= b {
                    return Err(CullError::InvalidRules(
                        "date.start must be strictly before date.end (start inclusive, end exclusive)"
                            .into(),
                    ));
                }
            }
        }
        if self.size_bytes.enabled {
            if let (Some(min), Some(max)) = (self.size_bytes.min, self.size_bytes.max) {
                if min > max {
                    return Err(CullError::InvalidRules(
                        "size_bytes.min must be <= size_bytes.max".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Parse an RFC3339 timestamp that **must** include an offset or `Z`.
///
/// Naive formats (`2023-01-01T00:00:00`, `2023-01-01`) are rejected.
pub fn parse_bound_instant(s: &str) -> Result<DateTime<Utc>> {
    let t = s.trim();
    if t.is_empty() {
        return Err(CullError::InvalidRules(
            "date bound is empty; require RFC3339 with offset or Z".into(),
        ));
    }
    // Reject common naive forms before chrono (chrono may accept some).
    if is_naive_datetime(t) {
        return Err(CullError::InvalidRules(format!(
            "date bound must include timezone offset or Z (got naive '{t}')"
        )));
    }
    // Prefer FixedOffset parse then convert to UTC.
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Also accept "+00:00" style that FixedOffset handles via rfc3339.
    if let Ok(dt) = t.parse::<DateTime<FixedOffset>>() {
        return Ok(dt.with_timezone(&Utc));
    }
    Err(CullError::InvalidRules(format!(
        "invalid RFC3339 date bound '{t}' (offset or Z required)"
    )))
}

/// True when the string looks like a datetime without offset/Z.
fn is_naive_datetime(s: &str) -> bool {
    // Ends with Z or has offset after time portion → not naive.
    if s.ends_with('Z') || s.ends_with('z') {
        return false;
    }
    // Offset patterns: +HH:MM / -HH:MM / +HHMM at end.
    let bytes = s.as_bytes();
    if let Some(pos) = s.rfind(['+', '-']) {
        // Must appear after a 'T' time separator to count as offset.
        if let Some(tpos) = s.find('T').or_else(|| s.find('t')) {
            if pos > tpos {
                // Likely offset.
                let rest = &bytes[pos + 1..];
                if rest.len() >= 4 {
                    return false;
                }
            }
        }
    }
    // Date-only or naive local datetime.
    true
}

/// Parse an item timestamp (stored as UTC RFC3339 from extract). Best-effort.
pub fn parse_item_instant(s: &str) -> Option<DateTime<Utc>> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Some(dt.with_timezone(&Utc));
    }
    // Legacy: treat trailing-naive as UTC for *item* fields only (extract writes Z).
    if let Ok(dt) = DateTime::parse_from_rfc3339(&format!("{t}Z")) {
        return Some(dt.with_timezone(&Utc));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_naive_date_bound() {
        let mut r = CullRules::default();
        r.date.enabled = true;
        r.date.start = Some("2023-01-01T00:00:00".into());
        assert!(r.validate().is_err());
        r.date.start = Some("2023-01-01".into());
        assert!(r.validate().is_err());
    }

    #[test]
    fn accepts_offset_date_bound() {
        let mut r = CullRules::default();
        r.date.enabled = true;
        r.date.start = Some("2023-01-01T00:00:00-05:00".into());
        r.date.end = Some("2023-01-02T00:00:00Z".into());
        r.validate().unwrap();
        let start = parse_bound_instant("2023-01-01T00:00:00-05:00").unwrap();
        assert_eq!(start.to_rfc3339(), "2023-01-01T05:00:00+00:00");
    }

    #[test]
    fn default_rules_json_roundtrip() {
        let r = CullRules::default();
        let j = serde_json::to_string(&r).unwrap();
        let back = CullRules::from_json(&j).unwrap();
        assert!(back.exclude_exact_duplicates);
        assert!(!back.near_dup.enabled);
        assert!(!back.denist.enabled);
    }
}
