//! PDF text extract bookkeeping (schema v15 / track 0034).
//!
//! Apply path puts extracted plain text into CAS (`text_sha256`) when non-empty,
//! records `pdf_*` columns (including `pdf_needs_ocr` for empty/low-text),
//! invalidates redacted artifact on body change (0032), and clears FTS
//! bookkeeping so 0029 re-indexes. **Never** rewrites native CAS.
//!
//! Page rasterization / preview CAS is **not** in P0 (deferred).

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::matter::{now_rfc3339, Matter};

/// `pdf_extract_status` values.
pub mod pdf_extract_status {
    pub const OK: &str = "ok";
    pub const LOW_TEXT: &str = "low_text";
    pub const EMPTY: &str = "empty";
    pub const SKIPPED: &str = "skipped";
    pub const ERROR: &str = "error";
}

/// Input for [`Matter::apply_pdf_text`].
#[derive(Debug, Clone)]
pub struct ApplyPdfTextInput {
    pub item_id: String,
    /// When true, re-extract even if already extracted for the same native.
    pub force: bool,
    /// Extracted plain text. `None` for empty/error/skip-only bookkeeping.
    pub text: Option<String>,
    pub method: Option<String>,
    /// `ok` | `low_text` | `empty` | `skipped` | `error`
    pub status: Option<String>,
    /// Short error code/message when status is error (or truncated note).
    pub error: Option<String>,
    /// Native digest used for this extract attempt.
    pub source_native_sha256: Option<String>,
    /// True when text was truncated at the output cap.
    pub partial: bool,
    /// Pages seen/total if known.
    pub page_count: Option<i64>,
    /// 0/1 OCR candidate flag; when `None`, derived from status for terminals.
    pub needs_ocr: Option<i64>,
    /// Optional file_category refine (`pdf`).
    pub file_category: Option<String>,
    /// When true and `file_category` is Some, set file_category if cheap.
    pub refine_file_category: bool,
}

/// Result of applying PDF text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfExtractApplyResult {
    /// Idempotent skip — successful terminal for same native and not force.
    Skipped,
    /// Text CAS written and columns updated (`ok`).
    Applied {
        text_sha256: String,
        text_changed: bool,
    },
    /// Low-text: text CAS written, `pdf_needs_ocr=1`.
    LowText {
        text_sha256: String,
        text_changed: bool,
    },
    /// Zero non-ws text — text_sha256 left NULL, needs_ocr=1, source set.
    Empty { error: String },
    /// Error bookkeeping only (no text write; source not claimed).
    Error { error: String },
}

/// Thin candidate row for `pdf_extract` job listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfCandidate {
    pub id: String,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub native_sha256: Option<String>,
    pub text_sha256: Option<String>,
    pub pdf_source_native_sha256: Option<String>,
    pub pdf_extract_status: Option<String>,
    pub pdf_needs_ocr: i64,
    pub file_category: Option<String>,
}

fn is_successful_terminal(status: Option<&str>) -> bool {
    matches!(
        status,
        Some(pdf_extract_status::OK)
            | Some(pdf_extract_status::SKIPPED)
            | Some(pdf_extract_status::LOW_TEXT)
            | Some(pdf_extract_status::EMPTY)
    )
}

impl Matter {
    /// Apply PDF extract result: put text CAS when non-empty, set pdf_* columns,
    /// invalidate redacted artifact + clear FTS bookkeeping on text change.
    ///
    /// **Never** rewrites native CAS.
    ///
    /// Successful terminal statuses that set `pdf_source_native_sha256`:
    /// `ok`, `low_text`, `empty` (and bookkeeping `skipped` for not-pdf).
    /// Pure `error` does **not** set source (retryable).
    pub fn apply_pdf_text(&self, input: ApplyPdfTextInput) -> Result<PdfExtractApplyResult> {
        let item = self.get_item(&input.item_id)?;
        let native = item
            .native_sha256
            .clone()
            .or(input.source_native_sha256.clone());

        // Idempotent skip for successful terminals covering this native.
        let prior_success_for_native = item.pdf_source_native_sha256.is_some()
            && item.pdf_source_native_sha256 == native
            && is_successful_terminal(item.pdf_extract_status.as_deref());
        if !input.force
            && prior_success_for_native
            && (input.text.is_some()
                || input
                    .status
                    .as_deref()
                    .is_some_and(|s| s == pdf_extract_status::SKIPPED)
                || input.status.is_none())
        {
            let now = now_rfc3339();
            self.connection().execute(
                "UPDATE items SET pdf_extract_status = ?1, pdf_extracted_at = ?2 \
                 WHERE id = ?3 AND matter_id = ?4",
                params![pdf_extract_status::SKIPPED, now, input.item_id, self.id()],
            )?;
            return Ok(PdfExtractApplyResult::Skipped);
        }

        // Bookkeeping without text payload: empty, error, skipped.
        if input.text.is_none() {
            let status = input
                .status
                .clone()
                .unwrap_or_else(|| pdf_extract_status::ERROR.into());
            let err = input.error.clone().unwrap_or_else(|| status.clone());
            let now = now_rfc3339();

            if status == pdf_extract_status::EMPTY {
                let needs = input.needs_ocr.unwrap_or(1);
                let file_cat = if input.refine_file_category {
                    input.file_category.clone()
                } else {
                    None
                };
                // Empty is a successful terminal: set source, needs_ocr=1, no text CAS.
                // Clear FTS bookkeeping (body became empty / no searchable text).
                // Always NULL redacted_* when text_sha256 is cleared (digest change).
                self.connection().execute(
                    "UPDATE items SET text_sha256 = NULL, \
                            pdf_extract_status = ?1, pdf_extract_error = ?2, \
                            pdf_extracted_at = ?3, \
                            pdf_extract_method = ?4, \
                            pdf_source_native_sha256 = ?5, \
                            pdf_page_count = COALESCE(?6, pdf_page_count), \
                            pdf_needs_ocr = ?7, \
                            file_category = COALESCE(?8, file_category), \
                            redacted_text_sha256 = NULL, \
                            redacted_text_at = NULL, \
                            redacted_source_digest = NULL, \
                            fts_text_sha256 = NULL, fts_indexed_at = NULL, fts_error = NULL \
                     WHERE id = ?9 AND matter_id = ?10",
                    params![
                        pdf_extract_status::EMPTY,
                        err,
                        now,
                        input.method,
                        native,
                        input.page_count,
                        needs,
                        file_cat,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(PdfExtractApplyResult::Empty { error: err });
            }

            if status == pdf_extract_status::SKIPPED {
                let needs = input.needs_ocr.unwrap_or(0);
                self.connection().execute(
                    "UPDATE items SET pdf_extract_status = ?1, pdf_extracted_at = ?2, \
                            pdf_extract_error = COALESCE(?3, pdf_extract_error), \
                            pdf_source_native_sha256 = COALESCE(?4, pdf_source_native_sha256), \
                            pdf_needs_ocr = COALESCE(?5, pdf_needs_ocr) \
                     WHERE id = ?6 AND matter_id = ?7",
                    params![
                        status,
                        now,
                        input.error,
                        input.source_native_sha256,
                        needs,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(PdfExtractApplyResult::Skipped);
            }

            // Error: do **not** overwrite pdf_source_native_sha256 (retryable)
            // and do **not** wipe pdf_needs_ocr (prior empty/low_text OCR candidacy
            // must survive a failed re-extract).
            self.connection().execute(
                "UPDATE items SET pdf_extract_status = ?1, pdf_extract_error = ?2, \
                        pdf_extracted_at = ?3, \
                        pdf_extract_method = COALESCE(?4, pdf_extract_method), \
                        pdf_page_count = COALESCE(?5, pdf_page_count) \
                 WHERE id = ?6 AND matter_id = ?7",
                params![
                    status,
                    err,
                    now,
                    input.method,
                    input.page_count,
                    input.item_id,
                    self.id()
                ],
            )?;
            return Ok(PdfExtractApplyResult::Error { error: err });
        }

        let Some(text) = input.text else {
            return Ok(PdfExtractApplyResult::Error {
                error: "internal: missing text payload".into(),
            });
        };
        if text.is_empty() {
            // Treat empty string as empty status path.
            return self.apply_pdf_text(ApplyPdfTextInput {
                item_id: input.item_id,
                force: input.force,
                text: None,
                method: input.method,
                status: Some(pdf_extract_status::EMPTY.into()),
                error: input.error.or_else(|| Some("pdf_empty_text".into())),
                source_native_sha256: input.source_native_sha256,
                partial: input.partial,
                page_count: input.page_count,
                needs_ocr: Some(input.needs_ocr.unwrap_or(1)),
                file_category: input.file_category,
                refine_file_category: input.refine_file_category,
            });
        }

        let text_sha = self.put_bytes(text.as_bytes())?;
        let text_changed = item.text_sha256.as_deref() != Some(text_sha.as_str());
        let now = now_rfc3339();
        let status = input
            .status
            .unwrap_or_else(|| pdf_extract_status::OK.into());
        let method = input.method;
        let err = if input.partial {
            Some(input.error.unwrap_or_else(|| "truncated".into()))
        } else {
            input.error
        };
        let needs = input.needs_ocr.unwrap_or_else(|| {
            if status == pdf_extract_status::LOW_TEXT {
                1
            } else {
                0
            }
        });
        let file_cat = if input.refine_file_category {
            input.file_category
        } else {
            None
        };

        self.connection().execute(
            "UPDATE items SET \
                text_sha256 = ?1, \
                pdf_extract_status = ?2, \
                pdf_extract_method = ?3, \
                pdf_source_native_sha256 = ?4, \
                pdf_extracted_at = ?5, \
                pdf_extract_error = ?6, \
                pdf_page_count = COALESCE(?7, pdf_page_count), \
                pdf_needs_ocr = ?8, \
                file_category = COALESCE(?9, file_category), \
                redacted_text_sha256 = CASE WHEN ?10 THEN NULL ELSE redacted_text_sha256 END, \
                redacted_text_at = CASE WHEN ?10 THEN NULL ELSE redacted_text_at END, \
                redacted_source_digest = CASE WHEN ?10 THEN NULL ELSE redacted_source_digest END, \
                fts_text_sha256 = NULL, \
                fts_indexed_at = NULL, \
                fts_error = NULL \
             WHERE id = ?11 AND matter_id = ?12",
            params![
                text_sha,
                status,
                method,
                native,
                now,
                err,
                input.page_count,
                needs,
                file_cat,
                text_changed,
                input.item_id,
                self.id(),
            ],
        )?;

        if status == pdf_extract_status::LOW_TEXT {
            Ok(PdfExtractApplyResult::LowText {
                text_sha256: text_sha,
                text_changed,
            })
        } else {
            Ok(PdfExtractApplyResult::Applied {
                text_sha256: text_sha,
                text_changed,
            })
        }
    }

    /// List PDF-eligible candidates for the extract job.
    ///
    /// Stable ordered set: items with `native_sha256` NOT NULL and either:
    /// - path/mime looking PDF-ish,
    /// - missing path/mime (CAS-only / sniff),
    /// - **or** path/mime not a known exclusive non-PDF type (wrong-meta sniff:
    ///   e.g. `document.bin` + `text/plain` with `%PDF-` bytes).
    ///
    /// Does **not** filter on existing extract status — callers skip in-process.
    /// Non-PDF sniffs are marked `skipped` with source so they are not re-read.
    ///
    /// Using a shrinking "pending only" list with SQL OFFSET is incorrect.
    pub fn list_pdf_candidates(
        &self,
        offset: u64,
        limit: u64,
        _force: bool,
    ) -> Result<Vec<PdfCandidate>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        // Exclusive non-PDF denylist (office OOXML/legacy, mail containers, common
        // pure image/audio). Everything else is a sniff candidate when path/mime
        // is wrong or non-PDF meta (bounded CAS read + magic check in the job).
        let sql = "SELECT id, path, mime_type, native_sha256, text_sha256, \
                    pdf_source_native_sha256, pdf_extract_status, \
                    IFNULL(pdf_needs_ocr, 0), file_category \
             FROM items \
             WHERE matter_id = ?1 \
               AND native_sha256 IS NOT NULL \
               AND ( \
                 lower(IFNULL(path, '')) LIKE '%.pdf' \
                 OR IFNULL(mime_type, '') LIKE 'application/pdf%' \
                 OR path IS NULL OR path = '' \
                 OR mime_type IS NULL OR mime_type = '' \
                 OR NOT ( \
                   lower(IFNULL(path, '')) LIKE '%.docx' \
                   OR lower(IFNULL(path, '')) LIKE '%.docm' \
                   OR lower(IFNULL(path, '')) LIKE '%.xlsx' \
                   OR lower(IFNULL(path, '')) LIKE '%.xlsm' \
                   OR lower(IFNULL(path, '')) LIKE '%.pptx' \
                   OR lower(IFNULL(path, '')) LIKE '%.pptm' \
                   OR lower(IFNULL(path, '')) LIKE '%.doc' \
                   OR lower(IFNULL(path, '')) LIKE '%.xls' \
                   OR lower(IFNULL(path, '')) LIKE '%.ppt' \
                   OR lower(IFNULL(path, '')) LIKE '%.eml' \
                   OR lower(IFNULL(path, '')) LIKE '%.msg' \
                   OR lower(IFNULL(path, '')) LIKE '%.pst' \
                   OR lower(IFNULL(path, '')) LIKE '%.ost' \
                   OR lower(IFNULL(path, '')) LIKE '%.png' \
                   OR lower(IFNULL(path, '')) LIKE '%.jpg' \
                   OR lower(IFNULL(path, '')) LIKE '%.jpeg' \
                   OR lower(IFNULL(path, '')) LIKE '%.gif' \
                   OR lower(IFNULL(path, '')) LIKE '%.webp' \
                   OR lower(IFNULL(path, '')) LIKE '%.tif' \
                   OR lower(IFNULL(path, '')) LIKE '%.tiff' \
                   OR lower(IFNULL(path, '')) LIKE '%.bmp' \
                   OR lower(IFNULL(path, '')) LIKE '%.mp3' \
                   OR lower(IFNULL(path, '')) LIKE '%.wav' \
                   OR lower(IFNULL(path, '')) LIKE '%.m4a' \
                   OR lower(IFNULL(path, '')) LIKE '%.flac' \
                   OR lower(IFNULL(path, '')) LIKE '%.mp4' \
                   OR lower(IFNULL(path, '')) LIKE '%.mov' \
                   OR lower(IFNULL(path, '')) LIKE '%.avi' \
                   OR IFNULL(mime_type, '') LIKE '%wordprocessingml%' \
                   OR IFNULL(mime_type, '') LIKE '%spreadsheetml%' \
                   OR IFNULL(mime_type, '') LIKE '%presentationml%' \
                   OR IFNULL(mime_type, '') LIKE '%officedocument%' \
                   OR IFNULL(mime_type, '') LIKE 'image/%' \
                   OR IFNULL(mime_type, '') LIKE 'audio/%' \
                   OR IFNULL(mime_type, '') LIKE 'video/%' \
                   OR lower(IFNULL(mime_type, '')) LIKE 'message/rfc822%' \
                 ) \
               ) \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?2 OFFSET ?3";
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params![self.id(), limit_i, offset as i64], |row| {
            Ok(PdfCandidate {
                id: row.get(0)?,
                path: row.get(1)?,
                mime_type: row.get(2)?,
                native_sha256: row.get(3)?,
                text_sha256: row.get(4)?,
                pdf_source_native_sha256: row.get(5)?,
                pdf_extract_status: row.get(6)?,
                pdf_needs_ocr: row.get(7)?,
                file_category: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}
