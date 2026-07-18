//! Office OOXML text extract bookkeeping (schema v14 / track 0033).
//!
//! Apply path puts extracted plain text into CAS (`text_sha256`), records
//! `office_*` columns, invalidates redacted artifact on body change (0032),
//! and clears FTS bookkeeping so 0029 re-indexes. **Never** rewrites native CAS.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::matter::{now_rfc3339, Matter};

/// `office_extract_status` values.
pub mod office_extract_status {
    pub const OK: &str = "ok";
    pub const SKIPPED: &str = "skipped";
    pub const ERROR: &str = "error";
}

/// Input for [`Matter::apply_office_text`].
#[derive(Debug, Clone)]
pub struct ApplyOfficeTextInput {
    pub item_id: String,
    /// When true, re-extract even if text already set for the same native.
    pub force: bool,
    /// Extracted plain text. `None` for error/skip-only bookkeeping.
    pub text: Option<String>,
    pub method: Option<String>,
    /// `ok` | `skipped` | `error`
    pub status: Option<String>,
    /// Short error code/message when status is error (or truncated note).
    pub error: Option<String>,
    /// Native digest used for this extract attempt.
    pub source_native_sha256: Option<String>,
    /// True when text was truncated at the output cap.
    pub partial: bool,
    /// Optional file_category refine (`document` / `spreadsheet` / `presentation`).
    pub file_category: Option<String>,
    /// When true and `file_category` is Some, set file_category if cheap.
    pub refine_file_category: bool,
}

/// Result of applying office text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfficeExtractApplyResult {
    /// Idempotent skip — text already set for same native and not force.
    Skipped,
    /// Text CAS written and columns updated.
    Applied {
        text_sha256: String,
        text_changed: bool,
    },
    /// Parse produced zero text — text_sha256 left NULL, error status set.
    Empty { error: String },
    /// Error bookkeeping only (no text write).
    Error { error: String },
}

/// Thin candidate row for `office_extract` job listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeCandidate {
    pub id: String,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub native_sha256: Option<String>,
    pub text_sha256: Option<String>,
    pub office_source_native_sha256: Option<String>,
    pub office_extract_status: Option<String>,
    pub file_category: Option<String>,
}

impl Matter {
    /// Apply office extract result: put text CAS, set office_* columns,
    /// invalidate redacted artifact + clear FTS bookkeeping on text change.
    ///
    /// **Never** rewrites native CAS.
    ///
    /// Idempotent: when `text_sha256` is already set, last **successful** extract
    /// (`office_extract_status == ok`) used this native, and `force` is false →
    /// [`OfficeExtractApplyResult::Skipped`]. Failed extracts do **not** set
    /// `office_source_native_sha256` (that column is the native of the last
    /// successful text write) so non-force runs can retry.
    pub fn apply_office_text(
        &self,
        input: ApplyOfficeTextInput,
    ) -> Result<OfficeExtractApplyResult> {
        let item = self.get_item(&input.item_id)?;
        let native = item
            .native_sha256
            .clone()
            .or(input.source_native_sha256.clone());

        // Idempotent skip only when last successful text is for this native.
        // Status may be `ok` (just extracted) or `skipped` (prior idempotent pass).
        // Status `error` means the last attempt failed → do not skip (retry).
        let prior_success_for_native = item.text_sha256.is_some()
            && item.office_source_native_sha256.is_some()
            && item.office_source_native_sha256 == native
            && matches!(
                item.office_extract_status.as_deref(),
                Some(office_extract_status::OK) | Some(office_extract_status::SKIPPED)
            );
        if !input.force && prior_success_for_native {
            // Pure skip request or re-apply of successful text without force.
            if input.text.is_some()
                || input
                    .status
                    .as_deref()
                    .is_some_and(|s| s == office_extract_status::SKIPPED)
            {
                let now = now_rfc3339();
                self.connection().execute(
                    "UPDATE items SET office_extract_status = ?1, office_extracted_at = ?2 \
                     WHERE id = ?3 AND matter_id = ?4",
                    params![
                        office_extract_status::SKIPPED,
                        now,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(OfficeExtractApplyResult::Skipped);
            }
        }

        // Error/skip-only bookkeeping (no text payload).
        if input.text.is_none() {
            let status = input
                .status
                .unwrap_or_else(|| office_extract_status::ERROR.into());
            let err = input.error.clone().unwrap_or_else(|| status.clone());
            let now = now_rfc3339();
            if status == office_extract_status::SKIPPED {
                // Skipped (e.g. not-office after sniff): may set source so the
                // job does not re-read the same native forever. Only set
                // office_extract_error when the caller provided one.
                self.connection().execute(
                    "UPDATE items SET office_extract_status = ?1, office_extracted_at = ?2, \
                            office_extract_error = COALESCE(?3, office_extract_error), \
                            office_source_native_sha256 = COALESCE(?4, office_source_native_sha256) \
                     WHERE id = ?5 AND matter_id = ?6",
                    params![
                        status,
                        now,
                        input.error,
                        input.source_native_sha256,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(OfficeExtractApplyResult::Skipped);
            }
            // Error: do **not** overwrite office_source_native_sha256 — that
            // column is defined as the native used for the last **successful**
            // text extract so failed items remain retry-eligible.
            self.connection().execute(
                "UPDATE items SET office_extract_status = ?1, office_extract_error = ?2, \
                        office_extracted_at = ?3, \
                        office_extract_method = COALESCE(?4, office_extract_method) \
                 WHERE id = ?5 AND matter_id = ?6",
                params![status, err, now, input.method, input.item_id, self.id()],
            )?;
            return Ok(OfficeExtractApplyResult::Error { error: err });
        }

        let text = input.text.unwrap();
        if text.is_empty() {
            let err = input.error.unwrap_or_else(|| "office_empty_text".into());
            let now = now_rfc3339();
            // Empty text is not a successful extract — leave prior successful source.
            self.connection().execute(
                "UPDATE items SET text_sha256 = NULL, \
                        office_extract_status = ?1, office_extract_error = ?2, \
                        office_extracted_at = ?3, \
                        office_extract_method = ?4, \
                        fts_text_sha256 = NULL, fts_indexed_at = NULL, fts_error = NULL \
                 WHERE id = ?5 AND matter_id = ?6",
                params![
                    office_extract_status::ERROR,
                    err,
                    now,
                    input.method,
                    input.item_id,
                    self.id()
                ],
            )?;
            return Ok(OfficeExtractApplyResult::Empty { error: err });
        }

        // Put text CAS (never touches native).
        let text_sha = self.put_bytes(text.as_bytes())?;
        let text_changed = item.text_sha256.as_deref() != Some(text_sha.as_str());
        let now = now_rfc3339();
        let status = input
            .status
            .unwrap_or_else(|| office_extract_status::OK.into());
        let method = input.method;
        // `partial` is surfaced via office_extract_error ("truncated") when set by the job.
        let err = if input.partial {
            Some(input.error.unwrap_or_else(|| "truncated".into()))
        } else {
            input.error
        };
        let file_cat = if input.refine_file_category {
            input.file_category
        } else {
            None
        };

        // Always clear FTS bookkeeping after a successful text write so 0029
        // incremental re-index picks it up. NULL redacted_* when digest changes.
        // office_source_native_sha256 is set only on successful text write.
        self.connection().execute(
            "UPDATE items SET \
                text_sha256 = ?1, \
                office_extract_status = ?2, \
                office_extract_method = ?3, \
                office_source_native_sha256 = ?4, \
                office_extracted_at = ?5, \
                office_extract_error = ?6, \
                file_category = COALESCE(?7, file_category), \
                redacted_text_sha256 = CASE WHEN ?8 THEN NULL ELSE redacted_text_sha256 END, \
                redacted_text_at = CASE WHEN ?8 THEN NULL ELSE redacted_text_at END, \
                redacted_source_digest = CASE WHEN ?8 THEN NULL ELSE redacted_source_digest END, \
                fts_text_sha256 = NULL, \
                fts_indexed_at = NULL, \
                fts_error = NULL \
             WHERE id = ?9 AND matter_id = ?10",
            params![
                text_sha,
                status,
                method,
                native,
                now,
                err,
                file_cat,
                text_changed,
                input.item_id,
                self.id(),
            ],
        )?;

        Ok(OfficeExtractApplyResult::Applied {
            text_sha256: text_sha,
            text_changed,
        })
    }

    /// List office-eligible candidates for the extract job.
    ///
    /// Stable ordered set: items with `native_sha256` NOT NULL and either
    /// path/mime looking office-ish **or** missing path/mime (CAS-only / sniff
    /// candidates). Does **not** filter on existing text — callers skip
    /// already-extracted items in-process (`force: false` idempotent skip).
    ///
    /// Using a shrinking "pending only" list with SQL OFFSET is incorrect:
    /// successful extracts remove rows while OFFSET advances, silently skipping
    /// remaining candidates.
    ///
    /// `force` is retained for call-site compatibility and does not change the
    /// result set (skip vs re-extract is decided when applying).
    pub fn list_office_candidates(
        &self,
        offset: u64,
        limit: u64,
        _force: bool,
    ) -> Result<Vec<OfficeCandidate>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        // Path/mime office-like OR path/mime missing so CAS-only natives can be
        // sniffed in process_one. Non-office sniffs are marked skipped with source.
        let sql = "SELECT id, path, mime_type, native_sha256, text_sha256, \
                    office_source_native_sha256, office_extract_status, file_category \
             FROM items \
             WHERE matter_id = ?1 \
               AND native_sha256 IS NOT NULL \
               AND ( \
                 lower(IFNULL(path, '')) LIKE '%.docx' OR lower(IFNULL(path, '')) LIKE '%.docm' \
                 OR lower(IFNULL(path, '')) LIKE '%.xlsx' OR lower(IFNULL(path, '')) LIKE '%.xlsm' \
                 OR lower(IFNULL(path, '')) LIKE '%.pptx' OR lower(IFNULL(path, '')) LIKE '%.pptm' \
                 OR lower(IFNULL(path, '')) LIKE '%.doc' OR lower(IFNULL(path, '')) LIKE '%.xls' \
                 OR lower(IFNULL(path, '')) LIKE '%.ppt' \
                 OR IFNULL(mime_type, '') LIKE '%wordprocessingml%' \
                 OR IFNULL(mime_type, '') LIKE '%spreadsheetml%' \
                 OR IFNULL(mime_type, '') LIKE '%presentationml%' \
                 OR IFNULL(mime_type, '') LIKE '%officedocument%' \
                 OR path IS NULL OR path = '' \
                 OR mime_type IS NULL OR mime_type = '' \
               ) \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?2 OFFSET ?3";
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params![self.id(), limit_i, offset as i64], |row| {
            Ok(OfficeCandidate {
                id: row.get(0)?,
                path: row.get(1)?,
                mime_type: row.get(2)?,
                native_sha256: row.get(3)?,
                text_sha256: row.get(4)?,
                office_source_native_sha256: row.get(5)?,
                office_extract_status: row.get(6)?,
                file_category: row.get(7)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}
