//! File-category bookkeeping (schema v18 / track 0037).
//!
//! Apply path updates `file_category` + `category_*` columns and optionally
//! refines `mime_type` when stronger. **Never** mutates role, parent, CAS, or text.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::matter::{now_rfc3339, Matter};

/// `category_status` values.
pub mod category_status {
    pub const OK: &str = "ok";
    pub const SKIPPED: &str = "skipped";
    pub const ERROR: &str = "error";
}

/// Input for [`Matter::apply_classification`].
#[derive(Debug, Clone)]
pub struct ApplyClassificationInput {
    pub item_id: String,
    /// When true, overwrite even if category+method+taxonomy unchanged.
    pub force: bool,
    /// Canonical category string (required for success path).
    pub category: String,
    /// Method string (`message_class`, `magic`, …).
    pub method: String,
    /// Taxonomy id (e.g. `taxonomy_v1`).
    pub taxonomy: String,
    /// Optional mime refine (only applied when stronger than current empty/generic).
    pub mime_type: Option<String>,
    /// `ok` | `skipped` | `error`
    pub status: Option<String>,
    pub error: Option<String>,
}

/// Result of applying classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryApplyResult {
    /// Idempotent skip — same category+method+taxonomy and not force.
    Skipped,
    /// Columns updated.
    Applied {
        category: String,
        method: String,
        mime_changed: bool,
    },
    /// Error bookkeeping only.
    Error { error: String },
}

/// Thin candidate row for the `classify` job listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassifyCandidate {
    pub id: String,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub file_category: Option<String>,
    pub role: Option<String>,
    pub message_class: Option<String>,
    pub native_sha256: Option<String>,
    pub category_method: Option<String>,
    pub category_taxonomy: Option<String>,
    pub category_status: Option<String>,
}

fn is_generic_mime(mime: Option<&str>) -> bool {
    match mime.map(str::trim).filter(|s| !s.is_empty()) {
        None => true,
        Some(m) => {
            let base = m.split(';').next().unwrap_or(m).trim().to_ascii_lowercase();
            matches!(
                base.as_str(),
                "application/octet-stream"
                    | "application/zip"
                    | "application/x-zip-compressed"
                    | "application/x-ole-storage"
                    | "application/cdfv2"
                    | "binary/octet-stream"
            )
        }
    }
}

impl Matter {
    /// Apply classification metadata. Never mutates role, parent, CAS digests, or text.
    pub fn apply_classification(
        &self,
        input: ApplyClassificationInput,
    ) -> Result<CategoryApplyResult> {
        let item = self.get_item(&input.item_id)?;
        let now = now_rfc3339();

        // Error-only bookkeeping.
        if input
            .status
            .as_deref()
            .is_some_and(|s| s == category_status::ERROR)
            || (input.category.trim().is_empty() && input.error.is_some())
        {
            let err = input
                .error
                .clone()
                .unwrap_or_else(|| "classify_error".into());
            self.connection().execute(
                "UPDATE items SET category_status = ?1, category_error = ?2, \
                        categorized_at = ?3 \
                 WHERE id = ?4 AND matter_id = ?5",
                params![category_status::ERROR, err, now, input.item_id, self.id()],
            )?;
            return Ok(CategoryApplyResult::Error { error: err });
        }

        let category = input.category.trim().to_string();
        let method = input.method.trim().to_string();
        let taxonomy = input.taxonomy.trim().to_string();
        if category.is_empty() || method.is_empty() || taxonomy.is_empty() {
            let err = "missing category/method/taxonomy".into();
            self.connection().execute(
                "UPDATE items SET category_status = ?1, category_error = ?2, \
                        categorized_at = ?3 \
                 WHERE id = ?4 AND matter_id = ?5",
                params![category_status::ERROR, err, now, input.item_id, self.id()],
            )?;
            return Ok(CategoryApplyResult::Error { error: err });
        }

        // Idempotent skip when unchanged (unless force).
        let same_category = item.file_category.as_deref() == Some(category.as_str());
        let same_method = item.category_method.as_deref() == Some(method.as_str());
        let same_taxonomy = item.category_taxonomy.as_deref() == Some(taxonomy.as_str());
        if !input.force && same_category && same_method && same_taxonomy {
            self.connection().execute(
                "UPDATE items SET category_status = ?1, categorized_at = ?2, \
                        category_error = NULL \
                 WHERE id = ?3 AND matter_id = ?4",
                params![category_status::SKIPPED, now, input.item_id, self.id()],
            )?;
            return Ok(CategoryApplyResult::Skipped);
        }

        // Mime refine: only fill empty/generic.
        let mime_changed = input.mime_type.as_ref().is_some_and(|new_mime| {
            let nm = new_mime.trim();
            !nm.is_empty() && is_generic_mime(item.mime_type.as_deref())
        });
        let new_mime = if mime_changed {
            input.mime_type.as_deref().map(|s| s.trim().to_string())
        } else {
            None
        };

        if let Some(ref mime) = new_mime {
            self.connection().execute(
                "UPDATE items SET \
                    file_category = ?1, \
                    category_method = ?2, \
                    category_taxonomy = ?3, \
                    category_status = ?4, \
                    category_error = NULL, \
                    categorized_at = ?5, \
                    mime_type = ?6 \
                 WHERE id = ?7 AND matter_id = ?8",
                params![
                    category,
                    method,
                    taxonomy,
                    category_status::OK,
                    now,
                    mime,
                    input.item_id,
                    self.id()
                ],
            )?;
        } else {
            self.connection().execute(
                "UPDATE items SET \
                    file_category = ?1, \
                    category_method = ?2, \
                    category_taxonomy = ?3, \
                    category_status = ?4, \
                    category_error = NULL, \
                    categorized_at = ?5 \
                 WHERE id = ?6 AND matter_id = ?7",
                params![
                    category,
                    method,
                    taxonomy,
                    category_status::OK,
                    now,
                    input.item_id,
                    self.id()
                ],
            )?;
        }

        Ok(CategoryApplyResult::Applied {
            category,
            method,
            mime_changed,
        })
    }

    /// List items for the `classify` job via **keyset** pagination (`ORDER BY id ASC`).
    ///
    /// - `after_id`: exclusive lower bound on `id` (`None` = start). Resume with
    ///   the last processed item id — do **not** use SQL OFFSET on a shrinking set.
    /// - `force = false`: only rows that need classify work (matches
    ///   [`classify_candidate_needs_work`]).
    /// - `force = true`: all rows matching the review filter (reclassify all).
    /// - `in_review_only`: when true, only `in_review = 1` items.
    pub fn list_classify_candidates(
        &self,
        after_id: Option<&str>,
        limit: u64,
        force: bool,
        in_review_only: bool,
    ) -> Result<Vec<ClassifyCandidate>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let review_i: i64 = if in_review_only { 1 } else { 0 };
        let force_i: i64 = if force { 1 } else { 0 };

        // No `--` SQL comments (Rust line joins collapse newlines).
        // Candidate predicate when force=0 mirrors classify_candidate_needs_work.
        let sql = "SELECT id, path, mime_type, file_category, role, message_class, \
                    native_sha256, category_method, category_taxonomy, category_status \
             FROM items \
             WHERE matter_id = ?1 \
               AND ( \
                 ?2 = 0 \
                 OR IFNULL(in_review, 0) = 1 \
               ) \
               AND ( \
                 ?3 = 1 \
                 OR ( \
                   file_category IS NULL \
                   OR trim(file_category) = '' \
                   OR lower(file_category) IN ('attachment', 'other', 'unrecognized') \
                   OR category_taxonomy IS NULL \
                   OR trim(category_taxonomy) = '' \
                   OR category_taxonomy != 'taxonomy_v1' \
                 ) \
               ) \
               AND (?4 IS NULL OR id > ?4) \
             ORDER BY id ASC \
             LIMIT ?5";
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(
            params![self.id(), review_i, force_i, after_id, limit_i],
            |row| {
                Ok(ClassifyCandidate {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    mime_type: row.get(2)?,
                    file_category: row.get(3)?,
                    role: row.get(4)?,
                    message_class: row.get(5)?,
                    native_sha256: row.get(6)?,
                    category_method: row.get(7)?,
                    category_taxonomy: row.get(8)?,
                    category_status: row.get(9)?,
                })
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// Whether a listed row needs classify work when `force` is false.
pub fn classify_candidate_needs_work(cand: &ClassifyCandidate, force: bool) -> bool {
    if force {
        return true;
    }
    match cand.file_category.as_deref().map(str::trim) {
        None => return true,
        Some(fc) => {
            let lower = fc.to_ascii_lowercase();
            if lower.is_empty()
                || lower == "attachment"
                || lower == "other"
                || lower == "unrecognized"
            {
                return true;
            }
        }
    }
    match cand.category_taxonomy.as_deref() {
        None => true,
        Some(t) if t != "taxonomy_v1" => true,
        Some(_) => false,
    }
}
