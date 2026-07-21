//! Transcript bookkeeping (schema v32 / track 0053).
//!
//! Apply path puts STT plain text into CAS via `text_sha256`, using a
//! **concatenate** policy when prior body/metadata text exists:
//!
//! `{existing}\n\n--- TRANSCRIPT ---\n\n{whisper_output}`
//!
//! Never blind-replaces non-empty `text_sha256`. Transcribe **attachment
//! child** items only at the job layer — this module does not walk parents.
//! Clears FTS + redacted-text bookkeeping on text change (OCR pattern).
//! **Never** rewrites native CAS.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::matter::{now_rfc3339, Matter};

/// Separator inserted between prior text and STT output (spec §3.6.1).
pub const TRANSCRIPT_MARKER: &str = "--- TRANSCRIPT ---";

/// `transcript_status` values.
pub mod transcript_status {
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
    pub const SKIPPED: &str = "skipped";
    pub const PENDING: &str = "pending";
    pub const DISABLED: &str = "disabled";
}

/// Input for [`Matter::apply_transcript_text`].
#[derive(Debug, Clone)]
pub struct ApplyTranscriptInput {
    pub item_id: String,
    /// When true, re-transcribe even if already done for the same native.
    pub force: bool,
    /// STT plain text. `None` for error/skip-only bookkeeping.
    pub text: Option<String>,
    /// Engine id (e.g. `whisper_cli`, `mock`).
    pub engine: Option<String>,
    /// Model id / path label.
    pub model: Option<String>,
    /// Detected or requested language (e.g. `en`).
    pub language: Option<String>,
    /// `done` | `failed` | `skipped` | `pending` | `disabled`
    pub status: Option<String>,
    /// Short error code/message when status is failed/skipped.
    pub error: Option<String>,
    /// Native digest used for this STT attempt.
    pub source_native_sha256: Option<String>,
    /// Job that produced this transcript (optional bookkeeping).
    pub job_id: Option<String>,
}

/// Result of applying transcript text.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptApplyResult {
    /// Idempotent skip — done for same native and not force.
    Skipped,
    /// Text CAS written and columns updated.
    Applied {
        text_sha256: String,
        text_changed: bool,
    },
    /// Error bookkeeping only (retryable; does not claim native for success skip).
    Error { error: String },
}

/// Thin candidate row for the `transcribe` job listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptCandidate {
    pub id: String,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub native_sha256: Option<String>,
    pub text_sha256: Option<String>,
    pub transcript_native_sha256: Option<String>,
    pub transcript_status: Option<String>,
    pub file_category: Option<String>,
    pub parent_item_id: Option<String>,
    pub role: Option<String>,
}

/// Strip any prior `--- TRANSCRIPT ---` section (and following text).
///
/// Returns the pre-transcript body (trimmed end). If the marker is absent,
/// returns the full existing text.
pub fn strip_transcript_section(existing: &str) -> String {
    match existing.find(TRANSCRIPT_MARKER) {
        Some(idx) => existing[..idx].trim_end().to_string(),
        None => existing.to_string(),
    }
}

/// Combine prior body/metadata with a new transcript (concat policy).
///
/// - Empty / missing prior → transcript only.
/// - Prior present → strip old transcript section, then append marker + new text.
pub fn combine_with_transcript(existing: Option<&str>, transcript: &str) -> String {
    let prior = existing
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(strip_transcript_section)
        .filter(|s| !s.is_empty());
    match prior {
        None => transcript.to_string(),
        Some(pre) => format!("{pre}\n\n{TRANSCRIPT_MARKER}\n\n{transcript}"),
    }
}

/// Permanent digest skip is only for **done**. `skipped` is not terminal —
/// tool-missing skips must remain retriable (track 0053 / Codex P2).
fn is_successful_terminal(status: Option<&str>) -> bool {
    matches!(status, Some(transcript_status::DONE))
}

impl Matter {
    /// Apply STT result: put combined text into CAS when non-empty, set
    /// `transcript_*` columns, clear FTS + redacted-text bookkeeping on body change.
    ///
    /// **Never** rewrites native CAS. **Never** blind-replaces non-empty prior text.
    pub fn apply_transcript_text(
        &self,
        input: ApplyTranscriptInput,
    ) -> Result<TranscriptApplyResult> {
        let item = self.get_item(&input.item_id)?;
        let native = item
            .native_sha256
            .clone()
            .or(input.source_native_sha256.clone());

        // Idempotent skip for **done** covering this native (spec §3.6).
        // Do **not** demote `done` → `skipped` — that would break permanent digest
        // skip on the next run once `skipped` is no longer treated as terminal.
        let prior_success_for_native = item.transcript_native_sha256.is_some()
            && item.transcript_native_sha256 == native
            && is_successful_terminal(item.transcript_status.as_deref());
        if !input.force
            && prior_success_for_native
            && (input.text.is_some()
                || input
                    .status
                    .as_deref()
                    .is_some_and(|s| s == transcript_status::SKIPPED)
                || input.status.is_none())
        {
            return Ok(TranscriptApplyResult::Skipped);
        }

        // Bookkeeping without text payload: error / skipped / disabled.
        if input.text.is_none() {
            let status = input
                .status
                .clone()
                .unwrap_or_else(|| transcript_status::FAILED.into());
            let err = input.error.clone().unwrap_or_else(|| status.clone());
            let now = now_rfc3339();

            if status == transcript_status::SKIPPED || status == transcript_status::DISABLED {
                self.connection().execute(
                    "UPDATE items SET transcript_status = ?1, transcript_at = ?2, \
                            transcript_error = COALESCE(?3, transcript_error), \
                            transcript_engine = COALESCE(?4, transcript_engine), \
                            transcript_model = COALESCE(?5, transcript_model), \
                            transcript_language = COALESCE(?6, transcript_language), \
                            transcript_native_sha256 = COALESCE(?7, transcript_native_sha256), \
                            transcript_job_id = COALESCE(?8, transcript_job_id) \
                     WHERE id = ?9 AND matter_id = ?10",
                    params![
                        status,
                        now,
                        input.error,
                        input.engine,
                        input.model,
                        input.language,
                        input.source_native_sha256,
                        input.job_id,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(TranscriptApplyResult::Skipped);
            }

            // Failed: do **not** overwrite transcript_native_sha256 (retryable).
            self.connection().execute(
                "UPDATE items SET transcript_status = ?1, transcript_error = ?2, \
                        transcript_at = ?3, \
                        transcript_engine = COALESCE(?4, transcript_engine), \
                        transcript_model = COALESCE(?5, transcript_model), \
                        transcript_language = COALESCE(?6, transcript_language), \
                        transcript_job_id = COALESCE(?7, transcript_job_id) \
                 WHERE id = ?8 AND matter_id = ?9",
                params![
                    status,
                    err,
                    now,
                    input.engine,
                    input.model,
                    input.language,
                    input.job_id,
                    input.item_id,
                    self.id()
                ],
            )?;
            return Ok(TranscriptApplyResult::Error { error: err });
        }

        let Some(transcript) = input.text else {
            return Ok(TranscriptApplyResult::Error {
                error: "internal: missing text payload".into(),
            });
        };
        if transcript.is_empty() {
            return self.apply_transcript_text(ApplyTranscriptInput {
                item_id: input.item_id,
                force: input.force,
                text: None,
                engine: input.engine,
                model: input.model,
                language: input.language,
                status: Some(transcript_status::FAILED.into()),
                error: input.error.or_else(|| Some("transcript_empty_text".into())),
                source_native_sha256: input.source_native_sha256,
                job_id: input.job_id,
            });
        }

        // Load prior body text when present so we can concatenate.
        // Fail closed on CAS read / invalid UTF-8 — never treat missing prior as
        // empty and blind-replace with transcript-only (integrity, Codex P2).
        let prior_text = match item.text_sha256.as_deref() {
            Some(sha) if !sha.is_empty() => match self.get_bytes(sha) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => Some(s),
                    Err(_) => {
                        return self.apply_transcript_text(ApplyTranscriptInput {
                            item_id: input.item_id,
                            force: true,
                            text: None,
                            engine: input.engine,
                            model: input.model,
                            language: input.language,
                            status: Some(transcript_status::FAILED.into()),
                            error: Some("transcript_prior_text_invalid_utf8".into()),
                            // Retryable integrity error — do not claim native for success skip.
                            source_native_sha256: None,
                            job_id: input.job_id,
                        });
                    }
                },
                Err(e) => {
                    return self.apply_transcript_text(ApplyTranscriptInput {
                        item_id: input.item_id,
                        force: true,
                        text: None,
                        engine: input.engine,
                        model: input.model,
                        language: input.language,
                        status: Some(transcript_status::FAILED.into()),
                        error: Some(format!("transcript_prior_text_cas_error: {e}")),
                        source_native_sha256: None,
                        job_id: input.job_id,
                    });
                }
            },
            _ => None,
        };

        let combined = combine_with_transcript(prior_text.as_deref(), &transcript);
        let text_sha = self.put_bytes(combined.as_bytes())?;
        let text_changed = item.text_sha256.as_deref() != Some(text_sha.as_str());
        let now = now_rfc3339();
        let status = input
            .status
            .unwrap_or_else(|| transcript_status::DONE.into());

        self.connection().execute(
            "UPDATE items SET \
                text_sha256 = ?1, \
                transcript_status = ?2, \
                transcript_engine = ?3, \
                transcript_model = ?4, \
                transcript_language = ?5, \
                transcript_native_sha256 = ?6, \
                transcript_at = ?7, \
                transcript_error = ?8, \
                transcript_job_id = COALESCE(?9, transcript_job_id), \
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
                input.engine,
                input.model,
                input.language,
                native,
                now,
                input.error,
                input.job_id,
                input.item_id,
                self.id(),
            ],
        )?;

        Ok(TranscriptApplyResult::Applied {
            text_sha256: text_sha,
            text_changed,
        })
    }

    /// List STT-eligible candidates for the `transcribe` job.
    ///
    /// Stable ordered set: items with `native_sha256` NOT NULL and audio/video
    /// path, mime, or file_category. Does **not** filter on successful status —
    /// callers skip in-process. Using a shrinking "pending only" list with SQL
    /// OFFSET is incorrect.
    pub fn list_transcript_candidates(
        &self,
        offset: u64,
        limit: u64,
        force: bool,
    ) -> Result<Vec<TranscriptCandidate>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let force_i: i64 = if force { 1 } else { 0 };
        // Note: no `--` SQL comments (Rust `\` line joins collapse newlines).
        let sql = "SELECT id, path, mime_type, native_sha256, text_sha256, \
                    transcript_native_sha256, transcript_status, file_category, \
                    parent_item_id, role \
             FROM items \
             WHERE matter_id = ?1 \
               AND native_sha256 IS NOT NULL \
               AND ( \
                 lower(IFNULL(path, '')) LIKE '%.wav' \
                 OR lower(IFNULL(path, '')) LIKE '%.mp3' \
                 OR lower(IFNULL(path, '')) LIKE '%.m4a' \
                 OR lower(IFNULL(path, '')) LIKE '%.flac' \
                 OR lower(IFNULL(path, '')) LIKE '%.ogg' \
                 OR lower(IFNULL(path, '')) LIKE '%.mp4' \
                 OR lower(IFNULL(path, '')) LIKE '%.mov' \
                 OR lower(IFNULL(path, '')) LIKE '%.mkv' \
                 OR lower(IFNULL(path, '')) LIKE '%.webm' \
                 OR IFNULL(mime_type, '') LIKE 'audio/%' \
                 OR IFNULL(mime_type, '') LIKE 'video/%' \
                 OR lower(IFNULL(file_category, '')) = 'audio' \
                 OR lower(IFNULL(file_category, '')) = 'video' \
                 OR ( \
                   ?2 = 1 \
                   AND transcript_status IS NOT NULL \
                 ) \
               ) \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?3 OFFSET ?4";
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params![self.id(), force_i, limit_i, offset as i64], |row| {
            Ok(TranscriptCandidate {
                id: row.get(0)?,
                path: row.get(1)?,
                mime_type: row.get(2)?,
                native_sha256: row.get(3)?,
                text_sha256: row.get(4)?,
                transcript_native_sha256: row.get(5)?,
                transcript_status: row.get(6)?,
                file_category: row.get(7)?,
                parent_item_id: row.get(8)?,
                role: row.get(9)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_without_marker_returns_full() {
        assert_eq!(strip_transcript_section("hello metadata"), "hello metadata");
    }

    #[test]
    fn strip_with_marker_keeps_pre() {
        let s = "meta line\n\n--- TRANSCRIPT ---\n\nold words";
        assert_eq!(strip_transcript_section(s), "meta line");
    }

    #[test]
    fn combine_empty_is_transcript_only() {
        assert_eq!(
            combine_with_transcript(None, "hello speech"),
            "hello speech"
        );
        assert_eq!(
            combine_with_transcript(Some(""), "hello speech"),
            "hello speech"
        );
        assert_eq!(
            combine_with_transcript(Some("   "), "hello speech"),
            "hello speech"
        );
    }

    #[test]
    fn combine_preserves_prior_and_marker() {
        let out = combine_with_transcript(Some("Title: call 1"), "speaker said hi");
        assert!(out.starts_with("Title: call 1"));
        assert!(out.contains(TRANSCRIPT_MARKER));
        assert!(out.contains("speaker said hi"));
    }

    #[test]
    fn combine_reset_replaces_old_transcript_section() {
        let prior = "Title: call 1\n\n--- TRANSCRIPT ---\n\nold words";
        let out = combine_with_transcript(Some(prior), "new words");
        assert!(out.contains("Title: call 1"));
        assert!(out.contains("new words"));
        assert!(!out.contains("old words"));
        assert_eq!(out.matches(TRANSCRIPT_MARKER).count(), 1);
    }
}
