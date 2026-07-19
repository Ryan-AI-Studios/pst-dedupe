//! Metadata [`FilterSpec`] model + parameterized SQL compile (track 0028).
//!
//! Compiles structured conditions to bound-parameter SQL only — never
//! interpolates user strings into the query text. Body/keyword FTS is **0029**.
//!
//! ## Family expand (`include_family`)
//!
//! When true, base conditions apply **only** inside a `hits` CTE. The outer
//! SELECT is membership-by-family (parent + direct children / same
//! `family_id`). Outer rows still satisfy the same scope (e.g. `in_review = 1`).
//!
//! ## Sort order
//!
//! Filtered lists use:
//! ```text
//! ORDER BY (alias.review_order IS NULL), alias.review_order ASC,
//!          alias.imported_at ASC, alias.path ASC, alias.id ASC
//! ```
//! SQLite ASC places NULLs first by default; the `IS NULL` leading key
//! emulates NULLS LAST. Partial index `idx_items_review_list_order` covers
//! the common `in_review = 1` path for deep OFFSET pages.
//!
//! ## Date comparison
//!
//! Filter bounds are compiled to **UTC epoch milliseconds** (`i64`). Item
//! `sent_at` / `received_at` may be stored as offset-bearing RFC3339 (or
//! legacy naive-as-UTC). SQL comparisons wrap item expressions with the
//! connection UDF [`DESK_UTC_EPOCH_MS_FN`] (`desk_utc_epoch_ms`) so both
//! sides compare as integers (subsecond precision preserved). Extract-pst
//! writes Z form; the UDF is defense for offset-bearing and legacy values.

use chrono::{DateTime, FixedOffset, Utc};
use rusqlite::functions::FunctionFlags;
use rusqlite::types::Value;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::matter::item_status;

/// SQLite scalar UDF name: stored instant TEXT → Unix epoch milliseconds (UTC).
///
/// Registered on every matter connection via [`register_filter_functions`].
/// Returns SQL NULL for empty/unparseable values.
pub const DESK_UTC_EPOCH_MS_FN: &str = "desk_utc_epoch_ms";

/// Current `FilterSpec` document version.
pub const FILTER_SPEC_VERSION: u32 = 1;

/// Review-corpus scope (default): `in_review = 1` (+ optional review set).
pub const SCOPE_REVIEW_CORPUS: &str = "review_corpus";
/// Entire matter: extracted-like statuses only.
pub const SCOPE_ENTIRE_MATTER: &str = "entire_matter";

/// Structured metadata filter (JSON-serializable, versioned).
///
/// Conditions are combined with **AND**. Nested OR trees and body FTS are out of scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterSpec {
    /// Document version (currently `1`).
    #[serde(default = "default_filter_version")]
    pub version: u32,
    /// `review_corpus` (default) or `entire_matter`.
    #[serde(default = "default_scope")]
    pub scope: String,
    /// When true, expand each hit to its whole family unit (see module docs).
    #[serde(default)]
    pub include_family: bool,
    /// Flat AND conditions.
    #[serde(default)]
    pub conditions: Vec<FilterCondition>,
}

fn default_filter_version() -> u32 {
    FILTER_SPEC_VERSION
}

fn default_scope() -> String {
    SCOPE_REVIEW_CORPUS.to_string()
}

impl Default for FilterSpec {
    fn default() -> Self {
        Self {
            version: FILTER_SPEC_VERSION,
            scope: SCOPE_REVIEW_CORPUS.to_string(),
            include_family: false,
            conditions: Vec::new(),
        }
    }
}

impl FilterSpec {
    /// Empty filter over the review corpus (full corpus list).
    pub fn review_corpus() -> Self {
        Self::default()
    }

    /// Quick chip: items with no codes.
    pub fn preset_uncoded() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "code_missing".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// Quick chip: privilege code present.
    pub fn preset_privilege() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "code".into(),
                op: "any_of".into(),
                value: None,
                values: Some(vec!["privilege".into()]),
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// Quick chip: responsive code present.
    pub fn preset_responsive() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "code".into(),
                op: "any_of".into(),
                value: None,
                values: Some(vec!["responsive".into()]),
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// Quick chip: items with at least one note (track 0030).
    pub fn preset_has_notes() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "has_notes".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// Quick chip: items with at least one highlight (track 0030).
    pub fn preset_has_highlights() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "has_highlights".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// Quick chip: production withhold hold (track 0031).
    pub fn preset_withheld() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "privilege_withhold".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// Quick chip: asserted privilege with blank log description (track 0031).
    pub fn preset_privilege_log_incomplete() -> Self {
        Self {
            conditions: vec![
                FilterCondition {
                    field: "privilege_status".into(),
                    op: "any_of".into(),
                    value: None,
                    values: Some(vec!["asserted".into()]),
                    start: None,
                    end: None,
                },
                FilterCondition {
                    field: "privilege_log_ready".into(),
                    op: "eq".into(),
                    value: Some(serde_json::Value::Bool(false)),
                    values: None,
                    start: None,
                    end: None,
                },
            ],
            ..Self::default()
        }
    }

    /// Quick chip: items with at least one redaction region (track 0032).
    pub fn preset_has_redactions() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "has_redactions".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// Quick chip: PDF empty/low-text needing OCR (track 0034 / handoff 0036).
    pub fn preset_pdf_needs_ocr() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "pdf_needs_ocr".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// Quick chip: redactions present but redacted produce artifact missing/outdated (track 0032).
    pub fn preset_redacted_text_stale() -> Self {
        Self {
            conditions: vec![FilterCondition {
                field: "redacted_text_stale".into(),
                op: "eq".into(),
                value: Some(serde_json::Value::Bool(true)),
                values: None,
                start: None,
                end: None,
            }],
            ..Self::default()
        }
    }

    /// True when no conditions and default family flag (still may have scope).
    pub fn is_empty_conditions(&self) -> bool {
        self.conditions.is_empty() && !self.include_family
    }
}

/// One fielded condition in a [`FilterSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterCondition {
    pub field: String,
    pub op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

/// Compiled SQL fragments + bound parameters (never user-interpolated).
#[derive(Debug, Clone)]
pub struct CompiledFilter {
    /// Full SELECT of thin review columns, ending before LIMIT/OFFSET params.
    pub list_sql: String,
    /// `SELECT COUNT(*)` with the same FROM/WHERE (and family expand if any).
    pub count_sql: String,
    /// Bound values for the shared WHERE / CTE portion (not limit/offset).
    pub params: Vec<Value>,
}

/// Thin-column names shared with [`crate::Matter::list_review_thin`] (same order).
pub const THIN_COLUMN_NAMES: &[&str] = &[
    "id",
    "review_order",
    "role",
    "parent_item_id",
    "subject",
    "from_addr",
    "sent_at",
    "received_at",
    "path",
    "file_category",
    "mime_type",
    "size_bytes",
    "text_sha256",
    "html_sha256",
    "dedup_role",
    "cull_status",
    "attachment_count",
    "family_id",
];

/// Thin-column SELECT list (unqualified).
pub fn thin_columns_sql() -> String {
    THIN_COLUMN_NAMES.join(", ")
}

/// Thin-column SELECT list qualified with `alias.` (e.g. `out.id, out.review_order, …`).
pub fn thin_columns_sql_qualified(alias: &str) -> String {
    THIN_COLUMN_NAMES
        .iter()
        .map(|c| format!("{alias}.{c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// ORDER BY clause using table/alias `a` (NULLS LAST for review_order).
pub fn order_by_clause(alias: &str) -> String {
    format!(
        "({a}.review_order IS NULL), {a}.review_order ASC, \
         {a}.imported_at ASC, {a}.path ASC, {a}.id ASC",
        a = alias
    )
}

/// Compile a [`FilterSpec`] into parameterized SQL.
///
/// `matter_id` is always bound first. When `scope` is `review_corpus` and
/// `review_set_id` is `Some`, membership is restricted to that set (same
/// semantics as `list_review_thin(None)` resolving the default set).
pub fn compile_filter(
    spec: &FilterSpec,
    matter_id: &str,
    review_set_id: Option<&str>,
) -> Result<CompiledFilter> {
    validate_spec(spec)?;

    let alias = "i";
    let mut params: Vec<Value> = Vec::new();
    let mut where_parts: Vec<String> = Vec::new();

    // matter_id always required.
    where_parts.push(format!("{alias}.matter_id = ?"));
    params.push(Value::Text(matter_id.to_string()));

    push_scope(
        alias,
        &spec.scope,
        review_set_id,
        &mut where_parts,
        &mut params,
    )?;

    for cond in &spec.conditions {
        push_condition(alias, cond, matter_id, &mut where_parts, &mut params)?;
    }

    let where_sql = where_parts.join(" AND ");

    if !spec.include_family {
        let cols = thin_columns_sql();
        let order = order_by_clause(alias);
        let list_sql = format!(
            "SELECT {cols} FROM items {alias} \
             WHERE {where_sql} \
             ORDER BY {order} \
             LIMIT ? OFFSET ?"
        );
        let count_sql = format!("SELECT COUNT(*) FROM items {alias} WHERE {where_sql}");
        return Ok(CompiledFilter {
            list_sql,
            count_sql,
            params,
        });
    }

    // Family expand: conditions only on hits; outer is membership-by-family.
    // Outer re-binds matter_id + scope (no condition predicates).
    let mut outer_params: Vec<Value> = Vec::new();
    let mut outer_scope: Vec<String> = Vec::new();
    outer_scope.push("out.matter_id = ?".into());
    outer_params.push(Value::Text(matter_id.to_string()));
    push_scope(
        "out",
        &spec.scope,
        review_set_id,
        &mut outer_scope,
        &mut outer_params,
    )?;
    let outer_where = outer_scope.join(" AND ");
    let order = order_by_clause("out");
    let cols = thin_columns_sql_qualified("out");

    // Params order: hits CTE params first, then outer scope params.
    let mut all_params = params;
    all_params.extend(outer_params);

    let list_sql = format!(
        "WITH hits AS ( \
             SELECT {alias}.id, {alias}.family_id, \
                    COALESCE({alias}.parent_item_id, {alias}.id) AS family_root \
             FROM items {alias} \
             WHERE {where_sql} \
         ) \
         SELECT DISTINCT {cols} \
         FROM items out \
         WHERE {outer_where} \
           AND ( \
             (out.family_id IS NOT NULL AND out.family_id IN ( \
                 SELECT family_id FROM hits WHERE family_id IS NOT NULL \
             )) \
             OR out.id IN (SELECT family_root FROM hits) \
             OR out.parent_item_id IN (SELECT family_root FROM hits) \
           ) \
         ORDER BY {order} \
         LIMIT ? OFFSET ?"
    );

    let count_sql = format!(
        "WITH hits AS ( \
             SELECT {alias}.id, {alias}.family_id, \
                    COALESCE({alias}.parent_item_id, {alias}.id) AS family_root \
             FROM items {alias} \
             WHERE {where_sql} \
         ) \
         SELECT COUNT(*) FROM ( \
             SELECT DISTINCT out.id \
             FROM items out \
             WHERE {outer_where} \
               AND ( \
                 (out.family_id IS NOT NULL AND out.family_id IN ( \
                     SELECT family_id FROM hits WHERE family_id IS NOT NULL \
                 )) \
                 OR out.id IN (SELECT family_root FROM hits) \
                 OR out.parent_item_id IN (SELECT family_root FROM hits) \
               ) \
         )"
    );

    Ok(CompiledFilter {
        list_sql,
        count_sql,
        params: all_params,
    })
}

fn validate_spec(spec: &FilterSpec) -> Result<()> {
    if spec.version != FILTER_SPEC_VERSION {
        return Err(Error::Other(format!(
            "unsupported FilterSpec version {} (expected {FILTER_SPEC_VERSION})",
            spec.version
        )));
    }
    match spec.scope.as_str() {
        SCOPE_REVIEW_CORPUS | SCOPE_ENTIRE_MATTER => {}
        other => {
            return Err(Error::Other(format!(
                "invalid FilterSpec scope '{other}' (expected {SCOPE_REVIEW_CORPUS}|{SCOPE_ENTIRE_MATTER})"
            )));
        }
    }
    Ok(())
}

fn push_scope(
    alias: &str,
    scope: &str,
    review_set_id: Option<&str>,
    where_parts: &mut Vec<String>,
    params: &mut Vec<Value>,
) -> Result<()> {
    match scope {
        SCOPE_REVIEW_CORPUS => {
            where_parts.push(format!("{alias}.in_review = 1"));
            if let Some(sid) = review_set_id {
                where_parts.push(format!("{alias}.review_set_id = ?"));
                params.push(Value::Text(sid.to_string()));
            }
        }
        SCOPE_ENTIRE_MATTER => {
            // Extracted-like statuses only (not discovered/expanded inventory stubs).
            where_parts.push(format!("{alias}.status IN (?, ?, ?)"));
            params.push(Value::Text(item_status::EXTRACTED.into()));
            params.push(Value::Text(item_status::PARTIAL.into()));
            params.push(Value::Text(item_status::NORMALIZED.into()));
        }
        other => {
            return Err(Error::Other(format!("invalid filter scope '{other}'")));
        }
    }
    Ok(())
}

fn push_condition(
    alias: &str,
    cond: &FilterCondition,
    matter_id: &str,
    where_parts: &mut Vec<String>,
    params: &mut Vec<Value>,
) -> Result<()> {
    let field = cond.field.trim();
    let op = cond.op.trim();
    if field.is_empty() || op.is_empty() {
        return Err(Error::Other(
            "filter condition field and op cannot be empty".into(),
        ));
    }

    match (field, op) {
        ("in_review", "eq") => {
            let v = bool_or_int_value(cond, "in_review")?;
            where_parts.push(format!("{alias}.in_review = ?"));
            params.push(Value::Integer(v));
        }
        ("code_missing", "eq") => {
            let want = bool_value(cond, "code_missing")?;
            if want {
                where_parts.push(format!(
                    "NOT EXISTS (SELECT 1 FROM item_codes ic WHERE ic.item_id = {alias}.id)"
                ));
            } else {
                where_parts.push(format!(
                    "EXISTS (SELECT 1 FROM item_codes ic WHERE ic.item_id = {alias}.id)"
                ));
            }
        }
        ("code", "any_of") => {
            let keys = require_values(cond, "code any_of")?;
            let placeholders = sql_placeholders(keys.len());
            where_parts.push(format!(
                "EXISTS (SELECT 1 FROM item_codes ic \
                 JOIN code_definitions cd ON cd.id = ic.code_id \
                 WHERE ic.item_id = {alias}.id AND cd.matter_id = ? \
                 AND cd.key IN ({placeholders}))"
            ));
            params.push(Value::Text(matter_id.to_string()));
            for k in keys {
                params.push(Value::Text(k));
            }
        }
        ("code", "all_of") => {
            let keys = require_values(cond, "code all_of")?;
            // Each key must be present (AND of EXISTS).
            for k in keys {
                where_parts.push(format!(
                    "EXISTS (SELECT 1 FROM item_codes ic \
                     JOIN code_definitions cd ON cd.id = ic.code_id \
                     WHERE ic.item_id = {alias}.id AND cd.matter_id = ? AND cd.key = ?)"
                ));
                params.push(Value::Text(matter_id.to_string()));
                params.push(Value::Text(k));
            }
        }
        ("code", "none_of") => {
            let keys = require_values(cond, "code none_of")?;
            let placeholders = sql_placeholders(keys.len());
            where_parts.push(format!(
                "NOT EXISTS (SELECT 1 FROM item_codes ic \
                 JOIN code_definitions cd ON cd.id = ic.code_id \
                 WHERE ic.item_id = {alias}.id AND cd.matter_id = ? \
                 AND cd.key IN ({placeholders}))"
            ));
            params.push(Value::Text(matter_id.to_string()));
            for k in keys {
                params.push(Value::Text(k));
            }
        }
        ("custodian", "eq")
        | ("from_addr", "eq")
        | ("file_category", "eq")
        | ("role", "eq")
        | ("dedup_role", "eq")
        | ("cull_status", "eq")
        | ("mime_type", "eq") => {
            let col = field;
            let v = require_string_value(cond, field)?;
            where_parts.push(format!("{alias}.{col} = ?"));
            params.push(Value::Text(v));
        }
        ("custodian", "in")
        | ("file_category", "in")
        | ("dedup_role", "in")
        | ("cull_status", "in") => {
            let col = field;
            let vals = require_values(cond, &format!("{field} in"))?;
            let placeholders = sql_placeholders(vals.len());
            where_parts.push(format!("{alias}.{col} IN ({placeholders})"));
            for v in vals {
                params.push(Value::Text(v));
            }
        }
        ("custodian", "contains")
        | ("from_addr", "contains")
        | ("subject", "contains")
        | ("path", "contains") => {
            let col = field;
            let v = require_string_value(cond, field)?;
            // Case-fold contains via LOWER + bound LIKE pattern (no SQL concat of user text).
            let pattern = format!("%{}%", escape_like_pattern(&v.to_lowercase()));
            where_parts.push(format!(
                "LOWER(COALESCE({alias}.{col}, '')) LIKE ? ESCAPE '\\'"
            ));
            params.push(Value::Text(pattern));
        }
        ("mime_type", "prefix") => {
            let v = require_string_value(cond, "mime_type")?;
            let pattern = format!("{}%", escape_like_pattern(&v));
            where_parts.push(format!(
                "COALESCE({alias}.mime_type, '') LIKE ? ESCAPE '\\'"
            ));
            params.push(Value::Text(pattern));
        }
        ("size_bytes", "gte") | ("size_bytes", "lte") => {
            let n = require_i64_value(cond, "size_bytes")?;
            let cmp = if op == "gte" { ">=" } else { "<=" };
            where_parts.push(format!("{alias}.size_bytes {cmp} ?"));
            params.push(Value::Integer(n));
        }
        ("size_bytes", "between") => {
            let (lo, hi) = require_i64_between(cond, "size_bytes")?;
            where_parts.push(format!(
                "{alias}.size_bytes >= ? AND {alias}.size_bytes <= ?"
            ));
            params.push(Value::Integer(lo));
            params.push(Value::Integer(hi));
        }
        ("sent_at", "gte" | "lte" | "between")
        | ("received_at", "gte" | "lte" | "between")
        | ("best_effort_date", "gte" | "lte" | "between") => {
            push_date_condition(alias, field, op, cond, where_parts, params)?;
        }
        ("has_text", "eq") => {
            let want = bool_value(cond, "has_text")?;
            if want {
                where_parts.push(format!("{alias}.text_sha256 IS NOT NULL"));
            } else {
                where_parts.push(format!("{alias}.text_sha256 IS NULL"));
            }
        }
        ("has_notes", "eq") => {
            let want = bool_value(cond, "has_notes")?;
            // Prefer denormalized count (schema v11); EXISTS is equivalent if counts lag.
            if want {
                where_parts.push(format!("{alias}.note_count > 0"));
            } else {
                where_parts.push(format!(
                    "({alias}.note_count = 0 OR {alias}.note_count IS NULL)"
                ));
            }
        }
        ("has_highlights", "eq") => {
            let want = bool_value(cond, "has_highlights")?;
            if want {
                where_parts.push(format!("{alias}.highlight_count > 0"));
            } else {
                where_parts.push(format!(
                    "({alias}.highlight_count = 0 OR {alias}.highlight_count IS NULL)"
                ));
            }
        }
        ("has_redactions", "eq") => {
            let want = bool_value(cond, "has_redactions")?;
            if want {
                where_parts.push(format!("{alias}.redaction_count > 0"));
            } else {
                where_parts.push(format!(
                    "({alias}.redaction_count = 0 OR {alias}.redaction_count IS NULL)"
                ));
            }
        }
        ("pdf_needs_ocr", "eq") => {
            let want = bool_value(cond, "pdf_needs_ocr")?;
            if want {
                where_parts.push(format!("{alias}.pdf_needs_ocr = 1"));
            } else {
                where_parts.push(format!(
                    "({alias}.pdf_needs_ocr = 0 OR {alias}.pdf_needs_ocr IS NULL)"
                ));
            }
        }
        ("redacted_text_stale", "eq") => {
            let want = bool_value(cond, "redacted_text_stale")?;
            // Stale when redactions exist and artifact is missing, or source digest
            // no longer matches the body CAS used as source (prefer text_sha256;
            // else html_sha256 when plain text is absent).
            let stale_pred = format!(
                "({alias}.redaction_count > 0 AND (\
                    {alias}.redacted_text_sha256 IS NULL \
                    OR ({alias}.redacted_source_digest IS NOT NULL \
                        AND {alias}.text_sha256 IS NOT NULL \
                        AND {alias}.redacted_source_digest != {alias}.text_sha256) \
                    OR ({alias}.redacted_source_digest IS NOT NULL \
                        AND {alias}.text_sha256 IS NULL \
                        AND {alias}.html_sha256 IS NOT NULL \
                        AND {alias}.redacted_source_digest != {alias}.html_sha256)\
                ))"
            );
            if want {
                where_parts.push(stale_pred);
            } else {
                where_parts.push(format!("NOT {stale_pred}"));
            }
        }
        ("note_text", "contains") => {
            let v = require_string_value(cond, "note_text")?;
            let pattern = format!("%{}%", escape_like_pattern(&v.to_lowercase()));
            where_parts.push(format!(
                "EXISTS (SELECT 1 FROM item_notes n \
                 WHERE n.item_id = {alias}.id AND n.matter_id = ? \
                 AND LOWER(n.body) LIKE ? ESCAPE '\\')"
            ));
            params.push(Value::Text(matter_id.to_string()));
            params.push(Value::Text(pattern));
        }
        ("privilege_withhold", "eq") => {
            let want = bool_value(cond, "privilege_withhold")?;
            // Prefer denormalized cache (schema v12); EXISTS as equivalent fallback.
            if want {
                where_parts.push(format!(
                    "({alias}.privilege_withhold = 1 OR EXISTS (\
                     SELECT 1 FROM item_privilege ip \
                     WHERE ip.item_id = {alias}.id AND ip.matter_id = ? AND ip.withhold = 1))"
                ));
                params.push(Value::Text(matter_id.to_string()));
            } else {
                where_parts.push(format!(
                    "({alias}.privilege_withhold = 0 OR {alias}.privilege_withhold IS NULL) \
                     AND NOT EXISTS (\
                     SELECT 1 FROM item_privilege ip \
                     WHERE ip.item_id = {alias}.id AND ip.matter_id = ? AND ip.withhold = 1)"
                ));
                params.push(Value::Text(matter_id.to_string()));
            }
        }
        ("privilege_status", "any_of") => {
            let keys = require_values(cond, "privilege_status any_of")?;
            let placeholders = sql_placeholders(keys.len());
            where_parts.push(format!(
                "EXISTS (SELECT 1 FROM item_privilege ip \
                 WHERE ip.item_id = {alias}.id AND ip.matter_id = ? \
                 AND ip.status IN ({placeholders}))"
            ));
            params.push(Value::Text(matter_id.to_string()));
            for k in keys {
                params.push(Value::Text(k));
            }
        }
        ("privilege_log_ready", "eq") => {
            let want = bool_value(cond, "privilege_log_ready")?;
            // Ready: include_on_log=1 AND trim(description) != ''.
            if want {
                where_parts.push(format!(
                    "EXISTS (SELECT 1 FROM item_privilege ip \
                     WHERE ip.item_id = {alias}.id AND ip.matter_id = ? \
                     AND ip.include_on_log = 1 \
                     AND TRIM(ip.description) != '')"
                ));
            } else {
                // Incomplete / not ready: has privilege row that is include_on_log
                // with blank description, OR no ready row (for asserted incomplete chip
                // combine with privilege_status).
                where_parts.push(format!(
                    "EXISTS (SELECT 1 FROM item_privilege ip \
                     WHERE ip.item_id = {alias}.id AND ip.matter_id = ? \
                     AND ip.include_on_log = 1 \
                     AND TRIM(ip.description) = '')"
                ));
            }
            params.push(Value::Text(matter_id.to_string()));
        }
        _ => {
            return Err(Error::Other(format!(
                "unsupported filter field/op: '{field}' / '{op}'"
            )));
        }
    }
    Ok(())
}

fn push_date_condition(
    alias: &str,
    field: &str,
    op: &str,
    cond: &FilterCondition,
    where_parts: &mut Vec<String>,
    params: &mut Vec<Value>,
) -> Result<()> {
    // Normalize item timestamps to UTC epoch ms via desk_utc_epoch_ms so
    // offset-bearing stored values and subseconds compare correctly.
    // best_effort_date: first usable of sent_at / received_at after normalize.
    let expr = match field {
        "best_effort_date" => format!(
            "COALESCE({fn}({alias}.sent_at), {fn}({alias}.received_at))",
            fn = DESK_UTC_EPOCH_MS_FN
        ),
        "sent_at" | "received_at" => format!("{DESK_UTC_EPOCH_MS_FN}({alias}.{field})"),
        _ => unreachable!(),
    };
    match op {
        "gte" => {
            let start = require_date_bound(
                cond.start
                    .as_deref()
                    .or_else(|| cond.value.as_ref().and_then(|v| v.as_str())),
                "start",
            )?;
            where_parts.push(format!("{expr} >= ?"));
            params.push(Value::Integer(start));
        }
        "lte" => {
            // Inclusive upper bound for lte alone (between uses exclusive end).
            let end = require_date_bound(
                cond.end
                    .as_deref()
                    .or_else(|| cond.value.as_ref().and_then(|v| v.as_str())),
                "end",
            )?;
            where_parts.push(format!("{expr} <= ?"));
            params.push(Value::Integer(end));
        }
        "between" => {
            let start = require_date_bound(cond.start.as_deref(), "start")?;
            let end = require_date_bound(cond.end.as_deref(), "end")?;
            // Start inclusive, end exclusive (match cull 0024).
            where_parts.push(format!("{expr} >= ? AND {expr} < ?"));
            params.push(Value::Integer(start));
            params.push(Value::Integer(end));
        }
        _ => {
            return Err(Error::Other(format!(
                "unsupported date op '{op}' for field '{field}'"
            )));
        }
    }
    Ok(())
}

/// Register filter-related SQLite UDFs on a matter connection.
///
/// Called from [`crate::schema::configure_connection`] so every open/create
/// path can evaluate compiled date filters.
pub fn register_filter_functions(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        DESK_UTC_EPOCH_MS_FN,
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let raw: Option<String> = ctx.get(0)?;
            match raw {
                None => Ok(None::<i64>),
                Some(s) => Ok(stored_instant_to_epoch_ms(&s)),
            }
        },
    )?;
    Ok(())
}

/// Parse an RFC3339 timestamp that **must** include an offset or `Z`.
///
/// Naive formats are rejected. Returns UTC instant.
pub fn parse_bound_instant(s: &str) -> Result<DateTime<Utc>> {
    let t = s.trim();
    if t.is_empty() {
        return Err(Error::Other(
            "date bound is empty; require RFC3339 with offset or Z".into(),
        ));
    }
    if is_naive_datetime(t) {
        return Err(Error::Other(format!(
            "date bound must include timezone offset or Z (got naive '{t}')"
        )));
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = t.parse::<DateTime<FixedOffset>>() {
        return Ok(dt.with_timezone(&Utc));
    }
    Err(Error::Other(format!(
        "invalid RFC3339 date bound '{t}' (offset or Z required)"
    )))
}

/// Parse a stored item timestamp for comparison (best-effort, cull-aligned).
///
/// Accepts offset-bearing RFC3339; treats naive forms as UTC (extract writes Z,
/// but legacy / hand-inserted rows may omit it).
pub fn parse_item_instant(s: &str) -> Option<DateTime<Utc>> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Some(dt.with_timezone(&Utc));
    }
    // Legacy: treat trailing-naive as UTC for *item* fields only.
    if let Ok(dt) = DateTime::parse_from_rfc3339(&format!("{t}Z")) {
        return Some(dt.with_timezone(&Utc));
    }
    None
}

/// Convert a stored item timestamp to Unix epoch milliseconds (UTC) for SQL compare.
///
/// Returns `None` for empty/unparseable values (SQL NULL → no match).
/// Preserves subsecond precision (millis).
pub fn stored_instant_to_epoch_ms(s: &str) -> Option<i64> {
    parse_item_instant(s).map(|dt| dt.timestamp_millis())
}

/// Alias retained for callers that used the pre-epoch-ms text normalizer name.
#[inline]
pub fn normalize_stored_instant_for_compare(s: &str) -> Option<i64> {
    stored_instant_to_epoch_ms(s)
}

fn require_date_bound(raw: Option<&str>, label: &str) -> Result<i64> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Err(Error::Other(format!(
            "date condition missing {label} bound (RFC3339 with offset required)"
        )));
    };
    let dt = parse_bound_instant(s)?;
    Ok(dt.timestamp_millis())
}

fn is_naive_datetime(s: &str) -> bool {
    if s.ends_with('Z') || s.ends_with('z') {
        return false;
    }
    let bytes = s.as_bytes();
    if let Some(pos) = s.rfind(['+', '-']) {
        if let Some(tpos) = s.find('T').or_else(|| s.find('t')) {
            if pos > tpos {
                let rest = &bytes[pos + 1..];
                if rest.len() >= 4 {
                    return false;
                }
            }
        }
    }
    true
}

fn escape_like_pattern(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            other => out.push(other),
        }
    }
    out
}

fn sql_placeholders(n: usize) -> String {
    (0..n).map(|_| "?").collect::<Vec<_>>().join(", ")
}

fn require_values(cond: &FilterCondition, ctx: &str) -> Result<Vec<String>> {
    let vals = cond
        .values
        .as_ref()
        .map(|v| {
            v.iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if vals.is_empty() {
        return Err(Error::Other(format!(
            "filter {ctx} requires non-empty 'values' array"
        )));
    }
    Ok(vals)
}

fn require_string_value(cond: &FilterCondition, field: &str) -> Result<String> {
    if let Some(v) = cond.value.as_ref() {
        if let Some(s) = v.as_str() {
            let t = s.trim();
            if !t.is_empty() {
                return Ok(t.to_string());
            }
        }
        if let Some(n) = v.as_i64() {
            return Ok(n.to_string());
        }
    }
    // Allow single-element values as value.
    if let Some(vals) = cond.values.as_ref() {
        if vals.len() == 1 {
            let t = vals[0].trim();
            if !t.is_empty() {
                return Ok(t.to_string());
            }
        }
    }
    Err(Error::Other(format!(
        "filter field '{field}' requires a non-empty string 'value'"
    )))
}

fn require_i64_value(cond: &FilterCondition, field: &str) -> Result<i64> {
    if let Some(v) = cond.value.as_ref() {
        if let Some(n) = v.as_i64() {
            return Ok(n);
        }
        if let Some(s) = v.as_str() {
            if let Ok(n) = s.trim().parse::<i64>() {
                return Ok(n);
            }
        }
    }
    Err(Error::Other(format!(
        "filter field '{field}' requires an integer 'value'"
    )))
}

fn require_i64_between(cond: &FilterCondition, field: &str) -> Result<(i64, i64)> {
    let lo = cond
        .start
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .or_else(|| cond.value.as_ref().and_then(|v| v.as_i64()));
    let hi = cond
        .end
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok());
    match (lo, hi) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(Error::Other(format!(
            "filter field '{field}' between requires integer start and end"
        ))),
    }
}

fn bool_value(cond: &FilterCondition, field: &str) -> Result<bool> {
    if let Some(v) = cond.value.as_ref() {
        if let Some(b) = v.as_bool() {
            return Ok(b);
        }
        if let Some(n) = v.as_i64() {
            return Ok(n != 0);
        }
        if let Some(s) = v.as_str() {
            match s.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => return Ok(true),
                "false" | "0" | "no" => return Ok(false),
                _ => {}
            }
        }
    }
    // Default true when omitted for code_missing-style filters.
    if cond.value.is_none() {
        return Ok(true);
    }
    Err(Error::Other(format!(
        "filter field '{field}' requires boolean 'value'"
    )))
}

fn bool_or_int_value(cond: &FilterCondition, field: &str) -> Result<i64> {
    Ok(if bool_value(cond, field)? { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_spec_serde_roundtrip() {
        let spec = FilterSpec {
            version: 1,
            scope: SCOPE_REVIEW_CORPUS.into(),
            include_family: true,
            conditions: vec![
                FilterCondition {
                    field: "custodian".into(),
                    op: "eq".into(),
                    value: Some(serde_json::json!("alice@example.com")),
                    values: None,
                    start: None,
                    end: None,
                },
                FilterCondition {
                    field: "code".into(),
                    op: "any_of".into(),
                    value: None,
                    values: Some(vec!["responsive".into()]),
                    start: None,
                    end: None,
                },
            ],
        };
        let json = serde_json::to_string(&spec).expect("ser");
        let back: FilterSpec = serde_json::from_str(&json).expect("de");
        assert_eq!(back, spec);
    }

    #[test]
    fn parse_bound_instant_requires_offset() {
        assert!(parse_bound_instant("2023-01-01T00:00:00-05:00").is_ok());
        assert!(parse_bound_instant("2023-01-01T00:00:00Z").is_ok());
        assert!(parse_bound_instant("2023-01-01T00:00:00").is_err());
        assert!(parse_bound_instant("2023-01-01").is_err());
    }

    #[test]
    fn stored_instant_to_epoch_ms_converts_offset() {
        // 00:00-05:00 == 05:00Z
        let expected = parse_bound_instant("2023-01-01T05:00:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(
            stored_instant_to_epoch_ms("2023-01-01T00:00:00-05:00"),
            Some(expected)
        );
        assert_eq!(
            stored_instant_to_epoch_ms("2023-01-01T05:00:00Z"),
            Some(expected)
        );
        // Naive treated as UTC (item fields only).
        assert_eq!(
            stored_instant_to_epoch_ms("2023-01-01T05:00:00"),
            Some(expected)
        );
        // Subseconds preserved.
        let with_frac = parse_bound_instant("2023-01-01T00:00:00.100Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(
            stored_instant_to_epoch_ms("2023-01-01T00:00:00.100Z"),
            Some(with_frac)
        );
        assert_ne!(
            stored_instant_to_epoch_ms("2023-01-01T00:00:00.100Z"),
            stored_instant_to_epoch_ms("2023-01-01T00:00:00.500Z")
        );
        assert!(stored_instant_to_epoch_ms("not-a-date").is_none());
    }

    #[test]
    fn compile_date_wraps_desk_utc_epoch_ms() {
        let bound = "2023-01-01T00:00:00Z";
        let expected_ms = parse_bound_instant(bound).unwrap().timestamp_millis();
        let spec = FilterSpec {
            conditions: vec![FilterCondition {
                field: "sent_at".into(),
                op: "gte".into(),
                value: Some(serde_json::json!(bound)),
                values: None,
                start: None,
                end: None,
            }],
            ..FilterSpec::default()
        };
        let compiled = compile_filter(&spec, "mat1", None).expect("compile");
        assert!(
            compiled.list_sql.contains(DESK_UTC_EPOCH_MS_FN),
            "expected {DESK_UTC_EPOCH_MS_FN} in SQL: {}",
            compiled.list_sql
        );
        assert!(
            compiled
                .params
                .iter()
                .any(|p| matches!(p, Value::Integer(ms) if *ms == expected_ms)),
            "bound must be epoch millis {expected_ms}: {:?}",
            compiled.params
        );
    }

    #[test]
    fn compile_uses_placeholders_not_user_text() {
        let spec = FilterSpec {
            conditions: vec![FilterCondition {
                field: "path".into(),
                op: "contains".into(),
                value: Some(serde_json::json!("foo' OR '1'='1")),
                values: None,
                start: None,
                end: None,
            }],
            ..FilterSpec::default()
        };
        let compiled = compile_filter(&spec, "mat1", None).expect("compile");
        assert!(
            !compiled.list_sql.contains("OR '1'"),
            "user text must not appear in SQL: {}",
            compiled.list_sql
        );
        assert!(compiled.list_sql.contains('?'));
        assert!(compiled.params.iter().any(|p| matches!(
            p,
            Value::Text(t) if t.contains("foo")
        )));
    }

    #[test]
    fn compile_include_family_uses_hits_cte() {
        let spec = FilterSpec {
            include_family: true,
            conditions: vec![FilterCondition {
                field: "subject".into(),
                op: "contains".into(),
                value: Some(serde_json::json!("Project")),
                values: None,
                start: None,
                end: None,
            }],
            ..FilterSpec::default()
        };
        let compiled = compile_filter(&spec, "mat1", Some("rs1")).expect("compile");
        assert!(compiled.list_sql.contains("WITH hits AS"));
        // Subject predicate only inside hits (before outer select).
        let hits_end = compiled.list_sql.find("SELECT DISTINCT").expect("outer");
        let hits = &compiled.list_sql[..hits_end];
        assert!(hits.to_lowercase().contains("subject") || hits.contains("LIKE"));
        let outer = &compiled.list_sql[hits_end..];
        assert!(
            !outer.to_ascii_lowercase().contains("like ?"),
            "outer must not re-apply LIKE conditions"
        );
    }
}
