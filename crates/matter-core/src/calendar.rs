//! Calendar / ICS extract bookkeeping (schema v16 / track 0035).
//!
//! Apply path puts synthesized review text into CAS (`text_sha256`), records
//! calendar fields + `ics_*` job columns, invalidates redacted artifact on body
//! change (0032), and clears FTS bookkeeping so 0029 re-indexes. **Never**
//! rewrites native CAS.
//!
//! Multi-event ICS container expansion creates child items via
//! [`Matter::insert_item`]; this module records parent/child bookkeeping and
//! lists ICS-eligible candidates.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::matter::{now_rfc3339, Matter};

/// `ics_extract_status` values.
pub mod ics_extract_status {
    pub const OK: &str = "ok";
    pub const SKIPPED: &str = "skipped";
    pub const ERROR: &str = "error";
}

/// Input for [`Matter::apply_ics_extract`].
#[derive(Debug, Clone, Default)]
pub struct ApplyIcsExtractInput {
    pub item_id: String,
    /// When true, re-extract even if already extracted for the same native.
    pub force: bool,
    /// Synthesized review text. `None` for error/skip-only bookkeeping.
    pub text: Option<String>,
    pub method: Option<String>,
    /// `ok` | `skipped` | `error`
    pub status: Option<String>,
    /// Short error code/message when status is error (or truncated note).
    pub error: Option<String>,
    /// Native digest used for this extract attempt.
    pub source_native_sha256: Option<String>,
    /// Optional file_category refine (`calendar` / `archive`).
    pub file_category: Option<String>,
    /// When true and `file_category` is Some, set file_category.
    pub refine_file_category: bool,
    // --- calendar fields (optional partial overwrite) ---
    pub message_class: Option<String>,
    pub cal_start_at: Option<String>,
    pub cal_end_at: Option<String>,
    pub cal_all_day: Option<i64>,
    pub cal_location: Option<String>,
    pub cal_organizer: Option<String>,
    pub cal_attendees_json: Option<String>,
    pub cal_busy_status: Option<String>,
    pub cal_is_recurring: Option<i64>,
    pub cal_recurrence_id: Option<String>,
    pub cal_uid: Option<String>,
    pub cal_extract_method: Option<String>,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub to_addrs_json: Option<String>,
    /// When set, written to `sent_at` (typically `cal_start_at` fallback).
    pub sent_at: Option<String>,
    /// Optional extra_json merge/replace (full replace when Some).
    pub extra_json: Option<String>,
}

/// Result of applying ICS extract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IcsExtractApplyResult {
    /// Idempotent skip — successful terminal for same native and not force.
    Skipped,
    /// Text CAS written and/or calendar columns updated.
    Applied {
        text_sha256: Option<String>,
        text_changed: bool,
    },
    /// Error bookkeeping only (no successful source claim).
    Error { error: String },
}

/// Thin candidate row for `ics_extract` job listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IcsCandidate {
    pub id: String,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub native_sha256: Option<String>,
    pub text_sha256: Option<String>,
    pub ics_source_native_sha256: Option<String>,
    pub ics_extract_status: Option<String>,
    pub file_category: Option<String>,
    pub parent_item_id: Option<String>,
}

fn is_successful_terminal(status: Option<&str>) -> bool {
    matches!(
        status,
        Some(ics_extract_status::OK) | Some(ics_extract_status::SKIPPED)
    )
}

impl Matter {
    /// Apply ICS extract result: put text CAS when non-empty, set cal_* + ics_*
    /// columns, invalidate redacted artifact + clear FTS bookkeeping on text
    /// change.
    ///
    /// **Never** rewrites native CAS.
    ///
    /// Successful terminals that set `ics_source_native_sha256`: `ok`, `skipped`.
    /// Pure `error` does **not** set source (retryable).
    ///
    /// When `sent_at` is provided, or when `cal_start_at` is set and the item's
    /// current `sent_at` is null, copies start into `sent_at` so date filters work.
    pub fn apply_ics_extract(&self, input: ApplyIcsExtractInput) -> Result<IcsExtractApplyResult> {
        let item = self.get_item(&input.item_id)?;
        let native = item
            .native_sha256
            .clone()
            .or(input.source_native_sha256.clone());

        let prior_success_for_native = item.ics_source_native_sha256.is_some()
            && item.ics_source_native_sha256 == native
            && is_successful_terminal(item.ics_extract_status.as_deref());
        if !input.force
            && prior_success_for_native
            && (input.text.is_some()
                || input
                    .status
                    .as_deref()
                    .is_some_and(|s| s == ics_extract_status::SKIPPED)
                || input.status.is_none())
        {
            let now = now_rfc3339();
            self.connection().execute(
                "UPDATE items SET ics_extract_status = ?1, ics_extracted_at = ?2 \
                 WHERE id = ?3 AND matter_id = ?4",
                params![ics_extract_status::SKIPPED, now, input.item_id, self.id()],
            )?;
            return Ok(IcsExtractApplyResult::Skipped);
        }

        // Bookkeeping without text payload: error / skipped / ok-without-text
        // (e.g. archive parent after container expansion).
        if input.text.is_none() {
            let status = input
                .status
                .clone()
                .unwrap_or_else(|| ics_extract_status::ERROR.into());
            let err = input.error.clone().unwrap_or_else(|| status.clone());
            let now = now_rfc3339();

            if status == ics_extract_status::SKIPPED {
                self.connection().execute(
                    "UPDATE items SET ics_extract_status = ?1, ics_extracted_at = ?2, \
                            ics_extract_error = COALESCE(?3, ics_extract_error), \
                            ics_source_native_sha256 = COALESCE(?4, ics_source_native_sha256) \
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
                return Ok(IcsExtractApplyResult::Skipped);
            }

            if status == ics_extract_status::OK {
                // Container parent or field-only apply without body text.
                let file_cat = if input.refine_file_category {
                    input.file_category.clone()
                } else {
                    None
                };
                let sent = resolve_sent_at(&item, &input);
                self.connection().execute(
                    "UPDATE items SET \
                            ics_extract_status = ?1, ics_extract_error = NULL, \
                            ics_extracted_at = ?2, \
                            ics_extract_method = COALESCE(?3, ics_extract_method), \
                            ics_source_native_sha256 = ?4, \
                            file_category = COALESCE(?5, file_category), \
                            message_class = COALESCE(?6, message_class), \
                            cal_start_at = COALESCE(?7, cal_start_at), \
                            cal_end_at = COALESCE(?8, cal_end_at), \
                            cal_all_day = COALESCE(?9, cal_all_day), \
                            cal_location = COALESCE(?10, cal_location), \
                            cal_organizer = COALESCE(?11, cal_organizer), \
                            cal_attendees_json = COALESCE(?12, cal_attendees_json), \
                            cal_busy_status = COALESCE(?13, cal_busy_status), \
                            cal_is_recurring = COALESCE(?14, cal_is_recurring), \
                            cal_recurrence_id = COALESCE(?15, cal_recurrence_id), \
                            cal_uid = COALESCE(?16, cal_uid), \
                            cal_extract_method = COALESCE(?17, cal_extract_method), \
                            subject = COALESCE(?18, subject), \
                            from_addr = COALESCE(?19, from_addr), \
                            to_addrs_json = COALESCE(?20, to_addrs_json), \
                            sent_at = COALESCE(?21, sent_at), \
                            extra_json = COALESCE(?22, extra_json) \
                     WHERE id = ?23 AND matter_id = ?24",
                    params![
                        ics_extract_status::OK,
                        now,
                        input.method,
                        native,
                        file_cat,
                        input.message_class,
                        input.cal_start_at,
                        input.cal_end_at,
                        input.cal_all_day,
                        input.cal_location,
                        input.cal_organizer,
                        input.cal_attendees_json,
                        input.cal_busy_status,
                        input.cal_is_recurring,
                        input.cal_recurrence_id,
                        input.cal_uid,
                        input.cal_extract_method,
                        input.subject,
                        input.from_addr,
                        input.to_addrs_json,
                        sent,
                        input.extra_json,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(IcsExtractApplyResult::Applied {
                    text_sha256: None,
                    text_changed: false,
                });
            }

            // Error: do **not** overwrite ics_source_native_sha256 (retryable).
            self.connection().execute(
                "UPDATE items SET ics_extract_status = ?1, ics_extract_error = ?2, \
                        ics_extracted_at = ?3, \
                        ics_extract_method = COALESCE(?4, ics_extract_method) \
                 WHERE id = ?5 AND matter_id = ?6",
                params![status, err, now, input.method, input.item_id, self.id()],
            )?;
            return Ok(IcsExtractApplyResult::Error { error: err });
        }

        let Some(text) = input.text else {
            return Ok(IcsExtractApplyResult::Error {
                error: "internal: missing text payload".into(),
            });
        };

        let text_sha = if text.is_empty() {
            None
        } else {
            Some(self.put_bytes(text.as_bytes())?)
        };
        let text_changed = item.text_sha256.as_deref() != text_sha.as_deref();
        let now = now_rfc3339();
        let status = input
            .status
            .unwrap_or_else(|| ics_extract_status::OK.into());
        let method = input.method;
        let err = input.error;
        let file_cat = if input.refine_file_category {
            input.file_category
        } else {
            None
        };
        let sent = resolve_sent_at(
            &item,
            &ApplyIcsExtractInput {
                cal_start_at: input.cal_start_at.clone(),
                sent_at: input.sent_at.clone(),
                ..Default::default()
            },
        );

        self.connection().execute(
            "UPDATE items SET \
                text_sha256 = COALESCE(?1, text_sha256), \
                ics_extract_status = ?2, \
                ics_extract_method = ?3, \
                ics_source_native_sha256 = ?4, \
                ics_extracted_at = ?5, \
                ics_extract_error = ?6, \
                file_category = COALESCE(?7, file_category), \
                message_class = COALESCE(?8, message_class), \
                cal_start_at = COALESCE(?9, cal_start_at), \
                cal_end_at = COALESCE(?10, cal_end_at), \
                cal_all_day = COALESCE(?11, cal_all_day), \
                cal_location = COALESCE(?12, cal_location), \
                cal_organizer = COALESCE(?13, cal_organizer), \
                cal_attendees_json = COALESCE(?14, cal_attendees_json), \
                cal_busy_status = COALESCE(?15, cal_busy_status), \
                cal_is_recurring = COALESCE(?16, cal_is_recurring), \
                cal_recurrence_id = COALESCE(?17, cal_recurrence_id), \
                cal_uid = COALESCE(?18, cal_uid), \
                cal_extract_method = COALESCE(?19, cal_extract_method), \
                subject = COALESCE(?20, subject), \
                from_addr = COALESCE(?21, from_addr), \
                to_addrs_json = COALESCE(?22, to_addrs_json), \
                sent_at = COALESCE(?23, sent_at), \
                extra_json = COALESCE(?24, extra_json), \
                redacted_text_sha256 = CASE WHEN ?25 THEN NULL ELSE redacted_text_sha256 END, \
                redacted_text_at = CASE WHEN ?25 THEN NULL ELSE redacted_text_at END, \
                redacted_source_digest = CASE WHEN ?25 THEN NULL ELSE redacted_source_digest END, \
                fts_text_sha256 = NULL, \
                fts_indexed_at = NULL, \
                fts_error = NULL \
             WHERE id = ?26 AND matter_id = ?27",
            params![
                text_sha,
                status,
                method,
                native,
                now,
                err,
                file_cat,
                input.message_class,
                input.cal_start_at,
                input.cal_end_at,
                input.cal_all_day,
                input.cal_location,
                input.cal_organizer,
                input.cal_attendees_json,
                input.cal_busy_status,
                input.cal_is_recurring,
                input.cal_recurrence_id,
                input.cal_uid,
                input.cal_extract_method,
                input.subject,
                input.from_addr,
                input.to_addrs_json,
                sent,
                input.extra_json,
                text_changed,
                input.item_id,
                self.id(),
            ],
        )?;

        Ok(IcsExtractApplyResult::Applied {
            text_sha256: text_sha,
            text_changed,
        })
    }

    /// List ICS-eligible candidates for the extract job.
    ///
    /// Stable ordered set: items with `native_sha256` NOT NULL and either:
    /// - path extension `.ics` / `.ical`,
    /// - mime contains `text/calendar`,
    /// - missing path/mime (CAS-only / sniff),
    /// - or path/mime not a known exclusive non-ICS type.
    ///
    /// Does **not** filter on existing extract status — callers skip in-process.
    pub fn list_ics_candidates(
        &self,
        offset: u64,
        limit: u64,
        _force: bool,
    ) -> Result<Vec<IcsCandidate>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let sql = "SELECT id, path, mime_type, native_sha256, text_sha256, \
                    ics_source_native_sha256, ics_extract_status, file_category, \
                    parent_item_id \
             FROM items \
             WHERE matter_id = ?1 \
               AND native_sha256 IS NOT NULL \
               AND ( \
                 lower(IFNULL(path, '')) LIKE '%.ics' \
                 OR lower(IFNULL(path, '')) LIKE '%.ical' \
                 OR IFNULL(mime_type, '') LIKE '%text/calendar%' \
                 OR path IS NULL OR path = '' \
                 OR mime_type IS NULL OR mime_type = '' \
                 OR NOT ( \
                   lower(IFNULL(path, '')) LIKE '%.pdf' \
                   OR lower(IFNULL(path, '')) LIKE '%.docx' \
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
                   OR IFNULL(mime_type, '') LIKE 'application/pdf%' \
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
            Ok(IcsCandidate {
                id: row.get(0)?,
                path: row.get(1)?,
                mime_type: row.get(2)?,
                native_sha256: row.get(3)?,
                text_sha256: row.get(4)?,
                ics_source_native_sha256: row.get(5)?,
                ics_extract_status: row.get(6)?,
                file_category: row.get(7)?,
                parent_item_id: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// Prefer explicit `sent_at`, else `cal_start_at` when item has no sent time.
fn resolve_sent_at(item: &crate::matter::Item, input: &ApplyIcsExtractInput) -> Option<String> {
    if let Some(ref s) = input.sent_at {
        return Some(s.clone());
    }
    if item.sent_at.is_none() {
        if let Some(ref start) = input.cal_start_at {
            return Some(start.clone());
        }
    }
    None
}
