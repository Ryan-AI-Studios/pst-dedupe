//! OCR bookkeeping (schema v17 / track 0036).
//!
//! Apply path puts OCR plain text into CAS (`ocr_text_sha256` + review
//! `text_sha256`), clears `pdf_needs_ocr` on success, invalidates redacted
//! artifact on body change (0032), and clears FTS bookkeeping so 0029
//! re-indexes. **Never** rewrites native CAS.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::matter::{now_rfc3339, Matter};

/// `ocr_status` values.
pub mod ocr_status {
    pub const OK: &str = "ok";
    pub const ERROR: &str = "error";
    pub const SKIPPED: &str = "skipped";
    pub const DISABLED: &str = "disabled";
}

/// Input for [`Matter::apply_ocr_text`].
#[derive(Debug, Clone)]
pub struct ApplyOcrTextInput {
    pub item_id: String,
    /// When true, re-OCR even if already ok for the same native.
    pub force: bool,
    /// OCR plain text. `None` for error/skip-only bookkeeping.
    pub text: Option<String>,
    /// Engine id + version (e.g. `tesseract_cli 5.3.0`).
    pub engine: Option<String>,
    /// Language pack string (e.g. `eng`).
    pub lang: Option<String>,
    /// `ok` | `error` | `skipped` | `disabled`
    pub status: Option<String>,
    /// Short error code/message when status is error/skipped.
    pub error: Option<String>,
    /// Native digest used for this OCR attempt.
    pub source_native_sha256: Option<String>,
    /// Pages OCR'd (or attempted).
    pub page_count: Option<i64>,
    /// Mean confidence if available.
    pub confidence: Option<f64>,
}

/// Result of applying OCR text.
#[derive(Debug, Clone, PartialEq)]
pub enum OcrApplyResult {
    /// Idempotent skip — ok for same native and not force.
    Skipped,
    /// Text CAS written and columns updated.
    Applied {
        text_sha256: String,
        ocr_text_sha256: String,
        text_changed: bool,
    },
    /// Error bookkeeping only (no successful source claim for retryables).
    Error { error: String },
}

/// Thin candidate row for the `ocr` job listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrCandidate {
    pub id: String,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub native_sha256: Option<String>,
    pub text_sha256: Option<String>,
    pub ocr_source_native_sha256: Option<String>,
    pub ocr_status: Option<String>,
    pub pdf_needs_ocr: i64,
    pub file_category: Option<String>,
    pub redaction_count: i64,
}

fn is_successful_terminal(status: Option<&str>) -> bool {
    matches!(status, Some(ocr_status::OK) | Some(ocr_status::SKIPPED))
}

impl Matter {
    /// Apply OCR result: put text CAS when non-empty, set `ocr_*` columns,
    /// clear `pdf_needs_ocr` on success, invalidate redacted artifact + clear
    /// FTS bookkeeping on text change.
    ///
    /// **Never** rewrites native CAS.
    pub fn apply_ocr_text(&self, input: ApplyOcrTextInput) -> Result<OcrApplyResult> {
        let item = self.get_item(&input.item_id)?;
        let native = item
            .native_sha256
            .clone()
            .or(input.source_native_sha256.clone());

        // Idempotent skip for successful terminals covering this native.
        let prior_success_for_native = item.ocr_source_native_sha256.is_some()
            && item.ocr_source_native_sha256 == native
            && is_successful_terminal(item.ocr_status.as_deref());
        if !input.force
            && prior_success_for_native
            && (input.text.is_some()
                || input
                    .status
                    .as_deref()
                    .is_some_and(|s| s == ocr_status::SKIPPED)
                || input.status.is_none())
        {
            let now = now_rfc3339();
            self.connection().execute(
                "UPDATE items SET ocr_status = ?1, ocr_at = ?2 \
                 WHERE id = ?3 AND matter_id = ?4",
                params![ocr_status::SKIPPED, now, input.item_id, self.id()],
            )?;
            return Ok(OcrApplyResult::Skipped);
        }

        // Bookkeeping without text payload: error / skipped / disabled.
        if input.text.is_none() {
            let status = input
                .status
                .clone()
                .unwrap_or_else(|| ocr_status::ERROR.into());
            let err = input.error.clone().unwrap_or_else(|| status.clone());
            let now = now_rfc3339();

            if status == ocr_status::SKIPPED || status == ocr_status::DISABLED {
                // Skipped/disabled can claim source so we don't re-scan forever
                // (e.g. redactions present). Disabled is rare at item level.
                self.connection().execute(
                    "UPDATE items SET ocr_status = ?1, ocr_at = ?2, \
                            ocr_error = COALESCE(?3, ocr_error), \
                            ocr_engine = COALESCE(?4, ocr_engine), \
                            ocr_lang = COALESCE(?5, ocr_lang), \
                            ocr_source_native_sha256 = COALESCE(?6, ocr_source_native_sha256), \
                            ocr_page_count = COALESCE(?7, ocr_page_count) \
                     WHERE id = ?8 AND matter_id = ?9",
                    params![
                        status,
                        now,
                        input.error,
                        input.engine,
                        input.lang,
                        input.source_native_sha256,
                        input.page_count,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(OcrApplyResult::Skipped);
            }

            // Error: do **not** overwrite ocr_source_native_sha256 (retryable)
            // and do **not** wipe pdf_needs_ocr.
            self.connection().execute(
                "UPDATE items SET ocr_status = ?1, ocr_error = ?2, \
                        ocr_at = ?3, \
                        ocr_engine = COALESCE(?4, ocr_engine), \
                        ocr_lang = COALESCE(?5, ocr_lang), \
                        ocr_page_count = COALESCE(?6, ocr_page_count), \
                        ocr_confidence = COALESCE(?7, ocr_confidence) \
                 WHERE id = ?8 AND matter_id = ?9",
                params![
                    status,
                    err,
                    now,
                    input.engine,
                    input.lang,
                    input.page_count,
                    input.confidence,
                    input.item_id,
                    self.id()
                ],
            )?;
            return Ok(OcrApplyResult::Error { error: err });
        }

        let Some(text) = input.text else {
            return Ok(OcrApplyResult::Error {
                error: "internal: missing text payload".into(),
            });
        };
        if text.is_empty() {
            return self.apply_ocr_text(ApplyOcrTextInput {
                item_id: input.item_id,
                force: input.force,
                text: None,
                engine: input.engine,
                lang: input.lang,
                status: Some(ocr_status::ERROR.into()),
                error: input.error.or_else(|| Some("ocr_empty_text".into())),
                source_native_sha256: input.source_native_sha256,
                page_count: input.page_count,
                confidence: input.confidence,
            });
        }

        let text_sha = self.put_bytes(text.as_bytes())?;
        let text_changed = item.text_sha256.as_deref() != Some(text_sha.as_str());
        let now = now_rfc3339();
        let status = input.status.unwrap_or_else(|| ocr_status::OK.into());
        let engine = input.engine;
        let lang = input.lang;
        let err = input.error;
        let conf = input.confidence;

        // Success: set review body to OCR text, clear needs-OCR, invalidate
        // redacted artifact + FTS bookkeeping always (OCR is the new body).
        self.connection().execute(
            "UPDATE items SET \
                text_sha256 = ?1, \
                ocr_text_sha256 = ?1, \
                ocr_status = ?2, \
                ocr_engine = ?3, \
                ocr_lang = ?4, \
                ocr_source_native_sha256 = ?5, \
                ocr_at = ?6, \
                ocr_error = ?7, \
                ocr_page_count = COALESCE(?8, ocr_page_count), \
                ocr_confidence = ?9, \
                pdf_needs_ocr = 0, \
                redacted_text_sha256 = NULL, \
                redacted_text_at = NULL, \
                redacted_source_digest = NULL, \
                fts_text_sha256 = NULL, \
                fts_indexed_at = NULL, \
                fts_error = NULL \
             WHERE id = ?10 AND matter_id = ?11",
            params![
                text_sha,
                status,
                engine,
                lang,
                native,
                now,
                err,
                input.page_count,
                conf,
                input.item_id,
                self.id(),
            ],
        )?;

        Ok(OcrApplyResult::Applied {
            text_sha256: text_sha.clone(),
            ocr_text_sha256: text_sha,
            text_changed,
        })
    }

    /// List OCR-eligible candidates for the OCR job.
    ///
    /// Stable ordered set: items with `native_sha256` NOT NULL and either:
    /// - image path/mime/file_category (png/jpeg/jpg/tiff/tif/webp), or
    /// - `pdf_needs_ocr = 1` with PDF-ish path/mime/file_category, or
    /// - when `force`: prior `ocr_status` is not null (re-OCR).
    ///
    /// Does **not** filter on successful OCR status — callers skip in-process.
    /// Using a shrinking "pending only" list with SQL OFFSET is incorrect.
    pub fn list_ocr_candidates(
        &self,
        offset: u64,
        limit: u64,
        force: bool,
    ) -> Result<Vec<OcrCandidate>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let force_i: i64 = if force { 1 } else { 0 };
        // Note: no `--` SQL comments (Rust `\` line joins collapse newlines and
        // would comment out the rest of the statement).
        let sql = "SELECT id, path, mime_type, native_sha256, text_sha256, \
                    ocr_source_native_sha256, ocr_status, \
                    IFNULL(pdf_needs_ocr, 0), file_category, \
                    IFNULL(redaction_count, 0) \
             FROM items \
             WHERE matter_id = ?1 \
               AND native_sha256 IS NOT NULL \
               AND ( \
                 lower(IFNULL(path, '')) LIKE '%.png' \
                 OR lower(IFNULL(path, '')) LIKE '%.jpg' \
                 OR lower(IFNULL(path, '')) LIKE '%.jpeg' \
                 OR lower(IFNULL(path, '')) LIKE '%.tif' \
                 OR lower(IFNULL(path, '')) LIKE '%.tiff' \
                 OR lower(IFNULL(path, '')) LIKE '%.webp' \
                 OR IFNULL(mime_type, '') LIKE 'image/png%' \
                 OR IFNULL(mime_type, '') LIKE 'image/jpeg%' \
                 OR IFNULL(mime_type, '') LIKE 'image/jpg%' \
                 OR IFNULL(mime_type, '') LIKE 'image/tiff%' \
                 OR IFNULL(mime_type, '') LIKE 'image/webp%' \
                 OR lower(IFNULL(file_category, '')) = 'image' \
                 OR ( \
                   IFNULL(pdf_needs_ocr, 0) = 1 \
                   AND ( \
                     lower(IFNULL(path, '')) LIKE '%.pdf' \
                     OR IFNULL(mime_type, '') LIKE 'application/pdf%' \
                     OR lower(IFNULL(file_category, '')) = 'pdf' \
                   ) \
                 ) \
                 OR ( \
                   ?2 = 1 \
                   AND ocr_status IS NOT NULL \
                 ) \
               ) \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?3 OFFSET ?4";
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params![self.id(), force_i, limit_i, offset as i64], |row| {
            Ok(OcrCandidate {
                id: row.get(0)?,
                path: row.get(1)?,
                mime_type: row.get(2)?,
                native_sha256: row.get(3)?,
                text_sha256: row.get(4)?,
                ocr_source_native_sha256: row.get(5)?,
                ocr_status: row.get(6)?,
                pdf_needs_ocr: row.get(7)?,
                file_category: row.get(8)?,
                redaction_count: row.get(9)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}
