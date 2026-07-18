//! Text redaction regions + true redacted produce artifact (schema v13 / track 0032).
//!
//! Stand-off ranges on Review display text (same coordinate system as highlights).
//! **Separate** from `item_highlights` — black paint, permanent produce removal.
//!
//! ## True redaction contract
//!
//! 1. Collect active intervals as `[start, end)` UTF-8 **char** ranges.
//! 2. **MERGE (union)** all intervals before any string mutation.
//! 3. Replace each merged span once with the fixed token [`REDACTED_TOKEN`].
//!
//! Output must not contain any redacted `exact_quote` as a contiguous substring.
//! Original `text_sha256` / native CAS is never rewritten.
//!
//! ## Production contract for **0040**
//!
//! - When `redaction_count > 0`, produce **must** use `redacted_text_sha256` CAS
//!   (or fail closed / force regenerate). **Never** emit original `text_sha256`
//!   body as the produced text while redactions exist.
//! - When body digest changes (`text_sha256` **or** `html_sha256`), this track
//!   **NULLs** `redacted_text_sha256` so a stale artifact cannot be produced by
//!   sha alone.
//! - Full native withhold still follows **0031**; redacted text may be an allowed
//!   partial produce path when withhold=1 and a redacted artifact is present.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::audit::{self, AuditEventInput};
use crate::error::{Error, Result};
use crate::matter::{
    new_id, normalize_actor, now_rfc3339, re_resolve_whitespace_normalized, utf8_char_slice,
    ItemHighlight, Matter, HIGHLIGHT_CONTEXT_CHARS, HIGHLIGHT_QUOTE_MAX_BYTES,
};
use crate::privilege::{privilege_basis, privilege_status, UpsertItemPrivilegeInput};

// ---------------------------------------------------------------------------
// Constants / vocabulary
// ---------------------------------------------------------------------------

/// Fixed produce token replacing each **merged** redacted span (P0 lock).
pub const REDACTED_TOKEN: &str = "[REDACTED]";

/// Max UTF-8 byte length for a stored redaction exact_quote (same as highlights).
pub const REDACTION_QUOTE_MAX_BYTES: usize = HIGHLIGHT_QUOTE_MAX_BYTES;

/// Context chars captured for prefix/suffix re-resolve.
pub const REDACTION_CONTEXT_CHARS: usize = HIGHLIGHT_CONTEXT_CHARS;

/// Redaction reason vocabulary.
pub mod redaction_reason {
    pub const PRIVILEGE: &str = "privilege";
    pub const PII: &str = "pii";
    pub const CONFIDENTIAL: &str = "confidential";
    pub const OTHER: &str = "other";

    pub const ALL: &[&str] = &[PRIVILEGE, PII, CONFIDENTIAL, OTHER];
}

/// Redaction status vocabulary (`active` | `stale`).
pub mod redaction_status {
    pub const ACTIVE: &str = "active";
    pub const STALE: &str = "stale";
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Stand-off text redaction region on Review display text (schema v13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRedaction {
    pub id: String,
    pub item_id: String,
    pub matter_id: String,
    /// Inclusive UTF-8 **char** index into display body at create (or last resolve).
    pub start_utf8: i64,
    /// Exclusive UTF-8 char index; `end > start`.
    pub end_utf8: i64,
    /// Raw substring of display body at create (not re-normalized on store).
    pub exact_quote: String,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    /// Digest of display text used when created (`text_sha256` or synthetic).
    pub body_digest: String,
    /// `privilege` | `pii` | `confidential` | `other`.
    pub reason: String,
    /// Optional free-form stamp label (metadata only; not burned into produce token).
    pub label: Option<String>,
    /// `active` or `stale`.
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
}

/// Input for [`Matter::create_redaction`].
#[derive(Debug, Clone)]
pub struct CreateRedactionInput {
    pub item_id: String,
    /// Inclusive UTF-8 char index into [`Self::display_body`].
    pub start_utf8: i64,
    /// Exclusive UTF-8 char index.
    pub end_utf8: i64,
    /// Must equal the char-slice of `display_body` at `[start, end)`.
    pub exact_quote: String,
    /// Full display body currently shown (for validation + prefix/suffix).
    pub display_body: String,
    /// Digest of the display body (prefer item `text_sha256`, else synthetic).
    pub body_digest: String,
    /// `privilege` | `pii` | `confidential` | `other`.
    pub reason: String,
    /// Optional stamp label (metadata only).
    pub label: Option<String>,
    pub actor: String,
}

/// Paint-ready range after digest check / whitespace re-resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRedaction {
    pub redaction_id: String,
    pub start_utf8: i64,
    pub end_utf8: i64,
    /// Effective status for paint (`active` | `stale`).
    pub status: String,
    /// True when re-resolve found a range different from stored offsets.
    pub remapped: bool,
    pub reason: String,
}

/// Result of [`Matter::regenerate_redacted_text`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedTextResult {
    pub item_id: String,
    /// CAS digest of redacted text, or `None` when cleared (no active ranges).
    pub redacted_text_sha256: Option<String>,
    pub redacted_source_digest: Option<String>,
    pub redacted_text_at: Option<String>,
    /// Number of active regions applied after resolve + merge.
    pub region_count: u64,
    /// Number of regions that resolved stale (skipped for produce).
    pub stale_count: u64,
    /// When true, operator should re-anchor stale regions.
    pub has_stale: bool,
}

// ---------------------------------------------------------------------------
// Pure builders
// ---------------------------------------------------------------------------

/// Merge (union) intervals as `[start, end)` char ranges.
///
/// Overlapping **and adjacent** ranges collapse. Empty / inverted ranges are dropped.
/// **Mandatory** before any string mutation in [`build_redacted_text`].
pub fn merge_redaction_intervals(ranges: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut sorted: Vec<(i64, i64)> = ranges
        .iter()
        .copied()
        .filter(|(s, e)| *e > *s && *s >= 0)
        .collect();
    if sorted.is_empty() {
        return Vec::new();
    }
    sorted.sort_by_key(|(s, _)| *s);
    let mut out: Vec<(i64, i64)> = Vec::with_capacity(sorted.len());
    let mut cur = sorted[0];
    for &(s, e) in &sorted[1..] {
        if s <= cur.1 {
            // Overlap or adjacent — union.
            cur.1 = cur.1.max(e);
        } else {
            out.push(cur);
            cur = (s, e);
        }
    }
    out.push(cur);
    out
}

/// Build redacted display text: **merge intervals first**, then replace each
/// merged span with [`REDACTED_TOKEN`].
///
/// Ranges are UTF-8 **char** indices into `display_body`. Out-of-bounds ends are
/// clamped to body length. Empty input ranges → body returned unchanged.
pub fn build_redacted_text(display_body: &str, ranges: &[(i64, i64)]) -> String {
    let body_chars = display_body.chars().count() as i64;
    if body_chars == 0 {
        return display_body.to_string();
    }
    let clamped: Vec<(i64, i64)> = ranges
        .iter()
        .map(|&(s, e)| {
            let s = s.max(0).min(body_chars);
            let e = e.max(0).min(body_chars);
            (s, e)
        })
        .filter(|(s, e)| e > s)
        .collect();
    let merged = merge_redaction_intervals(&clamped);
    if merged.is_empty() {
        return display_body.to_string();
    }

    let chars: Vec<char> = display_body.chars().collect();
    let mut out = String::with_capacity(display_body.len());
    let mut i: i64 = 0;
    for &(s, e) in &merged {
        if i < s {
            let start = i as usize;
            let end = s as usize;
            out.extend(chars[start..end].iter());
        }
        out.push_str(REDACTED_TOKEN);
        i = e;
    }
    if (i as usize) < chars.len() {
        out.extend(chars[i as usize..].iter());
    }
    out
}

/// Resolve one redaction against current display text (fast path + whitespace re-resolve).
pub fn resolve_redaction_against_body(
    red: &ItemRedaction,
    display_body: &str,
    display_digest: &str,
) -> ResolvedRedaction {
    // Reuse highlight re-resolve by projecting to a temporary highlight row.
    let as_hl = ItemHighlight {
        id: red.id.clone(),
        item_id: red.item_id.clone(),
        matter_id: red.matter_id.clone(),
        start_utf8: red.start_utf8,
        end_utf8: red.end_utf8,
        exact_quote: red.exact_quote.clone(),
        prefix: red.prefix.clone(),
        suffix: red.suffix.clone(),
        body_digest: red.body_digest.clone(),
        color: String::new(),
        status: red.status.clone(),
        created_at: red.created_at.clone(),
        updated_at: red.updated_at.clone(),
        created_by: red.created_by.clone().unwrap_or_default(),
    };

    // Fast path: digest matches → prefer stored offsets.
    if red.body_digest == display_digest {
        let start = red.start_utf8;
        let end = red.end_utf8;
        let body_chars = display_body.chars().count() as i64;
        if start >= 0 && end > start && end <= body_chars {
            if let Some(slice) = utf8_char_slice(display_body, start as usize, end as usize) {
                if slice == red.exact_quote {
                    return ResolvedRedaction {
                        redaction_id: red.id.clone(),
                        start_utf8: start,
                        end_utf8: end,
                        status: redaction_status::ACTIVE.to_string(),
                        remapped: false,
                        reason: red.reason.clone(),
                    };
                }
            }
        }
    }

    match re_resolve_whitespace_normalized(&as_hl, display_body) {
        Some((start, end)) => ResolvedRedaction {
            redaction_id: red.id.clone(),
            start_utf8: start,
            end_utf8: end,
            status: redaction_status::ACTIVE.to_string(),
            remapped: true,
            reason: red.reason.clone(),
        },
        None => ResolvedRedaction {
            redaction_id: red.id.clone(),
            start_utf8: red.start_utf8,
            end_utf8: red.end_utf8,
            status: redaction_status::STALE.to_string(),
            remapped: false,
            reason: red.reason.clone(),
        },
    }
}

fn validate_reason(reason: &str) -> Result<()> {
    if redaction_reason::ALL.contains(&reason) {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "invalid redaction reason '{reason}'; expected one of: {}",
            redaction_reason::ALL.join(", ")
        )))
    }
}

fn truncate_for_audit(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn map_redaction_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemRedaction> {
    Ok(ItemRedaction {
        id: row.get(0)?,
        item_id: row.get(1)?,
        matter_id: row.get(2)?,
        start_utf8: row.get(3)?,
        end_utf8: row.get(4)?,
        exact_quote: row.get(5)?,
        prefix: row.get(6)?,
        suffix: row.get(7)?,
        body_digest: row.get(8)?,
        reason: row.get(9)?,
        label: row.get(10)?,
        status: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        created_by: row.get(14)?,
    })
}

const REDACTION_SELECT: &str = "id, item_id, matter_id, start_utf8, end_utf8, exact_quote, \
    prefix, suffix, body_digest, reason, label, status, created_at, updated_at, created_by";

fn null_redacted_artifact_sql(
    conn: &rusqlite::Connection,
    item_id: &str,
    matter_id: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE items SET redacted_text_sha256 = NULL, redacted_text_at = NULL, \
                redacted_source_digest = NULL \
         WHERE id = ?1 AND matter_id = ?2",
        params![item_id, matter_id],
    )?;
    Ok(())
}

/// Choose regenerate body + source digest for produce bookkeeping.
///
/// When `text_sha256` is set, load **full** plain-text CAS bytes (ignore
/// possibly truncated `display_body`). Fail closed if CAS cannot be read.
///
/// Otherwise use `display_body` and prefer `html_sha256` as source digest when
/// present so HTML-only body changes invalidate the artifact.
fn resolve_regenerate_body_source(
    text_sha256: Option<&str>,
    html_sha256: Option<&str>,
    display_body: &str,
    load_cas: impl FnOnce(&str) -> Result<Vec<u8>>,
) -> Result<(String, String)> {
    if let Some(text_sha) = text_sha256.map(str::trim).filter(|s| !s.is_empty()) {
        let bytes = load_cas(text_sha).map_err(|e| {
            Error::Other(format!(
                "cannot load full text body for redaction regenerate \
                 (text_sha256={text_sha}): {e}"
            ))
        })?;
        let body = String::from_utf8(bytes).map_err(|_| {
            Error::Other(format!(
                "text CAS body is not valid UTF-8 (text_sha256={text_sha}); \
                 refuse partial redacted regenerate"
            ))
        })?;
        return Ok((body, text_sha.to_string()));
    }

    // No plain-text CAS: caller-supplied display body only (desk must not pass
    // a truncated pane as if it were the full source).
    let source_digest = html_sha256
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| crate::matter::display_body_digest(display_body));
    Ok((display_body.to_string(), source_digest))
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// List redactions for an item (range order).
    pub fn list_redactions(&self, item_id: &str) -> Result<Vec<ItemRedaction>> {
        self.ensure_item_in_matter(item_id)?;
        let mut stmt = self.connection().prepare(&format!(
            "SELECT {REDACTION_SELECT} FROM item_redactions \
             WHERE item_id = ?1 AND matter_id = ?2 \
             ORDER BY start_utf8 ASC, created_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map(params![item_id, self.id()], map_redaction_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Load one redaction by id.
    pub fn get_redaction(&self, redaction_id: &str) -> Result<ItemRedaction> {
        self.connection()
            .query_row(
                &format!("SELECT {REDACTION_SELECT} FROM item_redactions WHERE id = ?1"),
                params![redaction_id],
                map_redaction_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("redaction not found: {redaction_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// Create a stand-off redaction. Validates range + quote match against display body.
    ///
    /// Bumps `redaction_count`, **NULLs** redacted artifact pointers, audits
    /// `redaction.create`. When `reason=privilege`, sets privilege status
    /// `partial_redaction` with withhold=1 and include_on_log=1.
    pub fn create_redaction(&self, input: CreateRedactionInput) -> Result<ItemRedaction> {
        let actor = normalize_actor(&input.actor);
        self.ensure_item_in_matter(&input.item_id)?;

        let reason = input.reason.trim().to_string();
        validate_reason(&reason)?;

        if input.end_utf8 <= input.start_utf8 {
            return Err(Error::Other(format!(
                "redaction range invalid: end ({}) must be > start ({})",
                input.end_utf8, input.start_utf8
            )));
        }
        if input.start_utf8 < 0 {
            return Err(Error::Other("redaction start_utf8 must be >= 0".into()));
        }
        let start = input.start_utf8 as usize;
        let end = input.end_utf8 as usize;
        let body_chars = input.display_body.chars().count();
        if end > body_chars {
            return Err(Error::Other(format!(
                "redaction end_utf8 ({end}) exceeds display body char length ({body_chars})"
            )));
        }
        let slice = match utf8_char_slice(&input.display_body, start, end) {
            Some(s) => s,
            None => {
                return Err(Error::Other(
                    "redaction range does not map to a valid char slice".into(),
                ));
            }
        };
        if slice != input.exact_quote {
            return Err(Error::Other(
                "exact_quote does not match display body at [start_utf8, end_utf8)".into(),
            ));
        }
        if input.exact_quote.len() > REDACTION_QUOTE_MAX_BYTES {
            return Err(Error::Other(format!(
                "exact_quote exceeds max size of {REDACTION_QUOTE_MAX_BYTES} bytes"
            )));
        }
        if input.body_digest.trim().is_empty() {
            return Err(Error::Other("body_digest cannot be empty".into()));
        }

        let label = input
            .label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let prefix = utf8_char_slice(
            &input.display_body,
            start.saturating_sub(REDACTION_CONTEXT_CHARS),
            start,
        )
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
        let suffix = utf8_char_slice(
            &input.display_body,
            end,
            (end + REDACTION_CONTEXT_CHARS).min(body_chars),
        )
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

        let id = new_id("rdx");
        let now = now_rfc3339();
        let quote_for_audit = truncate_for_audit(&input.exact_quote, 512);
        let params_json = serde_json::json!({
            "redaction_id": id,
            "item_id": input.item_id,
            "start_utf8": input.start_utf8,
            "end_utf8": input.end_utf8,
            "reason": reason,
            "label": label,
            "quote": quote_for_audit,
        })
        .to_string();

        self.with_transaction(|conn| {
            conn.execute(
                "INSERT INTO item_redactions \
                 (id, item_id, matter_id, start_utf8, end_utf8, exact_quote, prefix, suffix, \
                  body_digest, reason, label, status, created_at, updated_at, created_by) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    id,
                    input.item_id,
                    self.id(),
                    input.start_utf8,
                    input.end_utf8,
                    input.exact_quote,
                    prefix,
                    suffix,
                    input.body_digest,
                    reason,
                    label,
                    redaction_status::ACTIVE,
                    now,
                    now,
                    actor
                ],
            )?;
            conn.execute(
                "UPDATE items SET redaction_count = redaction_count + 1 \
                 WHERE id = ?1 AND matter_id = ?2",
                params![input.item_id, self.id()],
            )?;
            null_redacted_artifact_sql(conn, &input.item_id, self.id())?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "redaction.create".into(),
                    entity: format!("redaction:{id}"),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;

        // Privilege hook (separate txn via public privilege API).
        if reason == redaction_reason::PRIVILEGE {
            self.apply_privilege_redaction_hook(&input.item_id, &actor)?;
        }

        self.get_redaction(&id)
    }

    /// Hard-delete a redaction region. Decrements count, NULLs artifact, audits.
    pub fn delete_redaction(&self, redaction_id: &str, actor: &str) -> Result<()> {
        let actor = normalize_actor(actor);
        let existing = self.get_redaction(redaction_id)?;
        if existing.matter_id != self.id() {
            return Err(Error::Other(format!(
                "redaction {redaction_id} belongs to another matter"
            )));
        }
        let now = now_rfc3339();
        let params_json = serde_json::json!({
            "redaction_id": redaction_id,
            "item_id": existing.item_id,
            "start_utf8": existing.start_utf8,
            "end_utf8": existing.end_utf8,
            "reason": existing.reason,
            "quote": truncate_for_audit(&existing.exact_quote, 512),
        })
        .to_string();

        self.with_transaction(|conn| {
            conn.execute(
                "DELETE FROM item_redactions WHERE id = ?1",
                params![redaction_id],
            )?;
            conn.execute(
                "UPDATE items SET redaction_count = MAX(0, redaction_count - 1) \
                 WHERE id = ?1 AND matter_id = ?2",
                params![existing.item_id, self.id()],
            )?;
            null_redacted_artifact_sql(conn, &existing.item_id, self.id())?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "redaction.delete".into(),
                    entity: format!("redaction:{redaction_id}"),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })
    }

    /// Resolve redactions for paint / regenerate against the current display body.
    ///
    /// When `body_digest` matches, uses stored offsets. On mismatch, applies
    /// whitespace-normalized quote re-resolve. Optionally persists `status=stale`.
    pub fn resolve_redactions(
        &self,
        item_id: &str,
        display_body: &str,
        display_digest: &str,
        persist_stale: bool,
    ) -> Result<Vec<ResolvedRedaction>> {
        let redactions = self.list_redactions(item_id)?;
        let mut out = Vec::with_capacity(redactions.len());
        let mut stale_ids: Vec<String> = Vec::new();
        for red in redactions {
            let resolved = resolve_redaction_against_body(&red, display_body, display_digest);
            if resolved.status == redaction_status::STALE
                && red.status != redaction_status::STALE
                && persist_stale
            {
                stale_ids.push(red.id.clone());
            }
            out.push(resolved);
        }
        if persist_stale && !stale_ids.is_empty() {
            let now = now_rfc3339();
            self.with_transaction(|conn| {
                for id in &stale_ids {
                    conn.execute(
                        "UPDATE item_redactions SET status = ?1, updated_at = ?2 WHERE id = ?3",
                        params![redaction_status::STALE, now, id],
                    )?;
                }
                Ok(())
            })?;
        }
        Ok(out)
    }

    /// NULL redacted artifact bookkeeping for an item (defense-in-depth).
    ///
    /// Called when body digest changes or create/delete invalidates the produce pointer.
    pub fn invalidate_redacted_artifact(&self, item_id: &str) -> Result<()> {
        self.ensure_item_in_matter(item_id)?;
        null_redacted_artifact_sql(self.connection(), item_id, self.id())
    }

    /// Regenerate the true redacted text CAS artifact from active ranges.
    ///
    /// Resolves regions first; applies **active** only (P0). Warns via
    /// [`RedactedTextResult::has_stale`] when any resolve as stale. Empty active
    /// set clears the artifact pointer (no CAS write).
    ///
    /// ## Body source (fail-closed against truncated UI panes)
    ///
    /// When the item has `text_sha256`, the **full** plain-text CAS blob is
    /// loaded and used for resolve + build. The caller's `display_body` is
    /// ignored so a truncated Review pane cannot produce a partial artifact
    /// labeled with the full-body source digest.
    ///
    /// When `text_sha256` is absent, `display_body` is used and
    /// `redacted_source_digest` is set to `html_sha256` when present (so HTML
    /// body re-extract invalidates), else a synthetic digest of `display_body`.
    pub fn regenerate_redacted_text(
        &self,
        item_id: &str,
        display_body: &str,
        actor: &str,
    ) -> Result<RedactedTextResult> {
        let actor = normalize_actor(actor);
        self.ensure_item_in_matter(item_id)?;

        let item = self.get_item(item_id)?;
        let (body_owned, source_digest) = resolve_regenerate_body_source(
            item.text_sha256.as_deref(),
            item.html_sha256.as_deref(),
            display_body,
            |digest| self.get_bytes(digest),
        )?;
        let body = body_owned.as_str();

        let resolved = self.resolve_redactions(item_id, body, &source_digest, true)?;
        let stale_count = resolved
            .iter()
            .filter(|r| r.status == redaction_status::STALE)
            .count() as u64;
        let active_ranges: Vec<(i64, i64)> = resolved
            .iter()
            .filter(|r| r.status == redaction_status::ACTIVE)
            .map(|r| (r.start_utf8, r.end_utf8))
            .collect();
        let region_count = active_ranges.len() as u64;
        let has_stale = stale_count > 0;
        let now = now_rfc3339();

        if region_count == 0 {
            // Clear artifact — no produce text when nothing active to redact.
            self.with_transaction(|conn| {
                null_redacted_artifact_sql(conn, item_id, self.id())?;
                let params_json = serde_json::json!({
                    "item_id": item_id,
                    "region_count": 0,
                    "stale_count": stale_count,
                    "output_sha": serde_json::Value::Null,
                    "source_digest": source_digest,
                    "cleared": true,
                })
                .to_string();
                audit::append_event(
                    conn,
                    &AuditEventInput {
                        actor: actor.clone(),
                        action: "redaction.regenerate".into(),
                        entity: format!("item:{item_id}"),
                        params_json,
                        tool_version: env!("CARGO_PKG_VERSION").into(),
                    },
                    &now,
                )?;
                Ok(())
            })?;
            return Ok(RedactedTextResult {
                item_id: item_id.to_string(),
                redacted_text_sha256: None,
                redacted_source_digest: None,
                redacted_text_at: None,
                region_count: 0,
                stale_count,
                has_stale,
            });
        }

        let redacted = build_redacted_text(body, &active_ranges);
        let sha = self.put_bytes(redacted.as_bytes())?;

        self.with_transaction(|conn| {
            conn.execute(
                "UPDATE items SET redacted_text_sha256 = ?1, redacted_text_at = ?2, \
                        redacted_source_digest = ?3 \
                 WHERE id = ?4 AND matter_id = ?5",
                params![sha, now, source_digest, item_id, self.id()],
            )?;
            let params_json = serde_json::json!({
                "item_id": item_id,
                "region_count": region_count,
                "stale_count": stale_count,
                "output_sha": sha,
                "source_digest": source_digest,
                "cleared": false,
            })
            .to_string();
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "redaction.regenerate".into(),
                    entity: format!("item:{item_id}"),
                    params_json,
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;

        Ok(RedactedTextResult {
            item_id: item_id.to_string(),
            redacted_text_sha256: Some(sha),
            redacted_source_digest: Some(source_digest),
            redacted_text_at: Some(now),
            region_count,
            stale_count,
            has_stale,
        })
    }

    /// When reason=privilege: ensure privilege row is `partial_redaction` + withhold.
    fn apply_privilege_redaction_hook(&self, item_id: &str, actor: &str) -> Result<()> {
        let existing = self.get_item_privilege(item_id)?;
        match existing {
            Some(p) => {
                // Always set partial_redaction + withhold when reason=privilege (P0).
                if p.status == privilege_status::PARTIAL_REDACTION
                    && p.withhold == 1
                    && p.include_on_log == 1
                {
                    return Ok(());
                }
                self.upsert_item_privilege(UpsertItemPrivilegeInput {
                    item_id: item_id.to_string(),
                    basis: p.basis,
                    description: p.description,
                    status: privilege_status::PARTIAL_REDACTION.to_string(),
                    withhold: true,
                    include_on_log: true,
                    actor: actor.to_string(),
                })?;
            }
            None => {
                self.upsert_item_privilege(UpsertItemPrivilegeInput {
                    item_id: item_id.to_string(),
                    basis: privilege_basis::ATTORNEY_CLIENT.to_string(),
                    description: String::new(),
                    status: privilege_status::PARTIAL_REDACTION.to_string(),
                    withhold: true,
                    include_on_log: true,
                    actor: actor.to_string(),
                })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_overlapping_and_adjacent() {
        let merged = merge_redaction_intervals(&[(10, 18), (15, 21), (30, 35), (35, 40)]);
        assert_eq!(merged, vec![(10, 21), (30, 40)]);
    }

    #[test]
    fn merge_drops_empty() {
        assert!(merge_redaction_intervals(&[(5, 5), (3, 1)]).is_empty());
    }

    #[test]
    fn build_merges_before_replace_single_token() {
        let body = "0123456789ABCDEFGHIJ"; // 20 chars
                                           // [10..18] and [15..21] → [10..20] → one token (clamped to end)
        let out = build_redacted_text(body, &[(10, 18), (15, 21)]);
        assert_eq!(out, format!("0123456789{REDACTED_TOKEN}"));
        assert!(!out.contains("ABCDEFGH"));
    }

    #[test]
    fn build_true_redact_removes_quote() {
        let body = "Hello SECRET sauce today";
        let start = 6i64;
        let end = 12i64;
        let quote = "SECRET";
        assert_eq!(
            utf8_char_slice(body, start as usize, end as usize),
            Some(quote)
        );
        let out = build_redacted_text(body, &[(start, end)]);
        assert_eq!(out, format!("Hello {REDACTED_TOKEN} sauce today"));
        assert!(!out.contains(quote));
    }

    #[test]
    fn build_empty_ranges_unchanged() {
        let body = "keep me";
        assert_eq!(build_redacted_text(body, &[]), body);
    }

    #[test]
    fn build_utf8_multibyte_safe() {
        let body = "café SECRET 日本語";
        // Find SECRET
        let start = body.find("SECRET").unwrap();
        let start_chars = body[..start].chars().count() as i64;
        let end_chars = start_chars + "SECRET".chars().count() as i64;
        let out = build_redacted_text(body, &[(start_chars, end_chars)]);
        assert!(!out.contains("SECRET"));
        assert!(out.contains("café"));
        assert!(out.contains("日本語"));
        assert!(out.contains(REDACTED_TOKEN));
    }

    #[test]
    fn regenerate_body_prefers_full_text_cas_over_display() {
        let full = "full body SECRET remainder that is long";
        let truncated = "full body SECRET rem";
        let digest = "a".repeat(64);
        let (body, source) = resolve_regenerate_body_source(Some(&digest), None, truncated, |d| {
            assert_eq!(d, digest);
            Ok(full.as_bytes().to_vec())
        })
        .expect("load");
        assert_eq!(body, full);
        assert_eq!(source, digest);
        assert_ne!(body, truncated);
    }

    #[test]
    fn regenerate_body_fails_closed_when_text_cas_missing() {
        let digest = "b".repeat(64);
        let err = resolve_regenerate_body_source(Some(&digest), None, "partial", |_d| {
            Err(Error::Other("blob missing".into()))
        })
        .expect_err("must fail closed");
        let msg = err.to_string();
        assert!(
            msg.contains("cannot load full text body") || msg.contains("blob missing"),
            "{msg}"
        );
    }

    #[test]
    fn regenerate_body_uses_html_sha_when_no_text() {
        let html = "c".repeat(64);
        let display = "stripped display body";
        let (body, source) = resolve_regenerate_body_source(None, Some(&html), display, |_| {
            unreachable!("no text cas")
        })
        .expect("display path");
        assert_eq!(body, display);
        assert_eq!(source, html);
    }
}
