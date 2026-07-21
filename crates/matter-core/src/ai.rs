//! AI provider config + first-pass code suggestions (schema v30 / track 0051)
//! + grounded citations (schema v31 / track 0052).
//!
//! Suggestions are **never** final codes — human accept promotes via
//! [`Matter::apply_codes`]. API keys are **not** stored in SQLite.
//!
//! Citation offsets are UTF-8 **byte** indices into the scanned item text
//! (entity-track style). Quotes are stored in full (no hard truncate).
//! Accept audit stores **offset pointers only** — never quote cleartext.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::matter::{new_id, now_rfc3339, ApplyCodesInput, CodeDef, Matter};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Suggestion type for first-pass coding.
pub const AI_SUGGESTION_TYPE_CODE: &str = "code";

/// Suggestion status: awaiting human review.
pub const AI_SUGGESTION_PENDING: &str = "pending";
/// Human accepted → promoted to `item_codes`.
pub const AI_SUGGESTION_ACCEPTED: &str = "accepted";
/// Human rejected.
pub const AI_SUGGESTION_REJECTED: &str = "rejected";
/// Replaced by a newer suggestion run for the same item fingerprint.
pub const AI_SUGGESTION_SUPERSEDED: &str = "superseded";

/// Provider kind stored on matter / suggestions: disabled / none.
pub const AI_PROVIDER_NONE: &str = "none";
/// Deterministic mock (CI / offline tests).
pub const AI_PROVIDER_MOCK: &str = "mock";
/// OpenAI-compatible HTTP (`/v1/chat/completions`).
pub const AI_PROVIDER_OPENAI_COMPATIBLE: &str = "openai_compatible";

/// Citation verify: offsets in range and normalized quote matches (or re-found).
pub const VERIFY_MATCHED: &str = "matched";
/// Reserved / currently unused as a **stored** status.
///
/// Intended for intermediate "offsets wrong but re-find may recover" signaling.
/// `matter-ai` verify repairs to [`VERIFY_MATCHED`] or falls through to
/// [`VERIFY_QUOTE_NOT_FOUND`] and does not emit this value today.
pub const VERIFY_OFFSET_MISMATCH: &str = "offset_mismatch";
/// Quote not uniquely found in current text (or spliced/ellipsis invalid).
pub const VERIFY_QUOTE_NOT_FOUND: &str = "quote_not_found";
/// Not yet verified against body text.
pub const VERIFY_UNCHECKED: &str = "unchecked";

/// Soft cap on **count** of citations per suggestion (not quote length).
pub const MAX_CITATIONS_PER_SUGGESTION: usize = 5;

/// Continuous UTF-8 body cap for citation re-verify at accept / Desk display
/// coordinate space (matches Desk `BODY_DISPLAY_CAP_BYTES` = 2 MiB).
pub const AI_VERIFY_TEXT_MAX_BYTES: u64 = 2 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Matter-scoped AI configuration (no API key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiMatterConfig {
    pub ai_enabled: bool,
    pub ai_allow_remote: bool,
    pub ai_base_url: Option<String>,
    pub ai_model: Option<String>,
    /// `none` | `mock` | `openai_compatible` (or empty → treat as none).
    pub ai_provider_kind: Option<String>,
}

/// Update matter AI config. API keys never accepted here.
#[derive(Debug, Clone)]
pub struct UpdateAiMatterConfigInput<'a> {
    pub enabled: bool,
    pub allow_remote: bool,
    pub base_url: Option<&'a str>,
    pub model: Option<&'a str>,
    pub provider_kind: Option<&'a str>,
}

/// Thin candidate for `ai_suggest_codes` pagination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiSuggestCandidate {
    pub id: String,
    pub text_sha256: Option<String>,
    pub in_review: Option<i64>,
}

/// One stored AI suggestion row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemAiSuggestion {
    pub id: String,
    pub matter_id: String,
    pub item_id: String,
    pub suggestion_type: String,
    pub code_id: Option<String>,
    pub code_name: String,
    pub confidence: Option<f64>,
    pub rationale: Option<String>,
    pub provider_kind: String,
    pub model: String,
    pub prompt_template_id: String,
    pub is_remote: bool,
    pub text_sha256: Option<String>,
    pub catalog_content_hash: Option<String>,
    pub status: String,
    pub job_id: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub resolved_by: Option<String>,
    /// Number of citation rows (schema v31+).
    pub citations_count: i64,
}

/// One grounded citation for an AI suggestion (schema v31).
///
/// Offsets are UTF-8 **byte** indices into the scanned field text. Quotes are
/// stored in full — never hard-truncated on insert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemAiSuggestionCitation {
    pub id: String,
    pub suggestion_id: String,
    pub matter_id: String,
    pub item_id: String,
    pub ordinal: i64,
    pub quote: String,
    pub start_offset: Option<i64>,
    pub end_offset: Option<i64>,
    pub field: String,
    pub verify_status: String,
    pub created_at: String,
}

/// Insert one citation row (quote stored in full).
#[derive(Debug, Clone)]
pub struct InsertAiCitationInput<'a> {
    pub suggestion_id: &'a str,
    pub item_id: &'a str,
    pub ordinal: i64,
    pub quote: &'a str,
    pub start_offset: Option<i64>,
    pub end_offset: Option<i64>,
    pub field: &'a str,
    pub verify_status: &'a str,
}

/// Insert one pending suggestion.
#[derive(Debug, Clone)]
pub struct InsertAiSuggestionInput<'a> {
    pub item_id: &'a str,
    pub suggestion_type: &'a str,
    pub code_id: Option<&'a str>,
    pub code_name: &'a str,
    pub confidence: Option<f64>,
    pub rationale: Option<&'a str>,
    pub provider_kind: &'a str,
    pub model: &'a str,
    pub prompt_template_id: &'a str,
    pub is_remote: bool,
    pub text_sha256: Option<&'a str>,
    pub catalog_content_hash: Option<&'a str>,
    pub job_id: Option<&'a str>,
}

/// Meta row for one suggestion job run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiSuggestionRun {
    pub id: String,
    pub matter_id: String,
    pub job_id: Option<String>,
    pub provider_kind: String,
    pub model: Option<String>,
    pub prompt_template_id: String,
    pub is_remote: bool,
    pub item_count: i64,
    pub suggestion_count: i64,
    pub created_at: String,
}

/// Insert run meta.
#[derive(Debug, Clone)]
pub struct InsertAiSuggestionRunInput<'a> {
    pub job_id: Option<&'a str>,
    pub provider_kind: &'a str,
    pub model: Option<&'a str>,
    pub prompt_template_id: &'a str,
    pub is_remote: bool,
    pub item_count: i64,
    pub suggestion_count: i64,
}

// ---------------------------------------------------------------------------
// Fingerprint helpers
// ---------------------------------------------------------------------------

/// Hash of active code catalog content (id, key, label, guidance) for skip invalidation.
pub fn catalog_content_hash(defs: &[CodeDef]) -> String {
    let mut active: Vec<&CodeDef> = defs.iter().filter(|d| d.is_active != 0).collect();
    active.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut hasher = Sha256::new();
    for d in active {
        hasher.update(d.id.as_bytes());
        hasher.update(b"\0");
        hasher.update(d.key.as_bytes());
        hasher.update(b"\0");
        hasher.update(d.label.as_bytes());
        hasher.update(b"\0");
        let g = d
            .guidance
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(d.label.as_str());
        hasher.update(g.as_bytes());
        hasher.update(b"\0");
    }
    hex_encode(hasher.finalize().as_slice())
}

/// Composite fingerprint string for idempotent skip
/// (`text_sha256` + model + template + catalog hash).
pub fn suggestion_fingerprint(
    text_sha256: &str,
    model: &str,
    template_id: &str,
    catalog_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text_sha256.as_bytes());
    hasher.update(b"|");
    hasher.update(model.as_bytes());
    hasher.update(b"|");
    hasher.update(template_id.as_bytes());
    hasher.update(b"|");
    hasher.update(catalog_hash.as_bytes());
    hex_encode(hasher.finalize().as_slice())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Load matter AI config (defaults: disabled, no remote).
    pub fn get_ai_config(&self) -> Result<AiMatterConfig> {
        self.connection()
            .query_row(
                "SELECT ai_enabled, ai_allow_remote, ai_base_url, ai_model, ai_provider_kind \
                 FROM matters WHERE id = ?1",
                params![self.id()],
                |row| {
                    let enabled: i64 = row.get(0)?;
                    let allow: i64 = row.get(1)?;
                    Ok(AiMatterConfig {
                        ai_enabled: enabled != 0,
                        ai_allow_remote: allow != 0,
                        ai_base_url: row.get(2)?,
                        ai_model: row.get(3)?,
                        ai_provider_kind: row.get(4)?,
                    })
                },
            )
            .map_err(Error::from)
    }

    /// Update matter AI config. Never stores API keys.
    pub fn update_ai_config(&self, input: UpdateAiMatterConfigInput<'_>) -> Result<()> {
        let kind = input
            .provider_kind
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        if let Some(ref k) = kind {
            match k.as_str() {
                AI_PROVIDER_NONE | AI_PROVIDER_MOCK | AI_PROVIDER_OPENAI_COMPATIBLE => {}
                other => {
                    return Err(Error::Other(format!(
                        "invalid ai_provider_kind '{other}' (expected none|mock|openai_compatible)"
                    )));
                }
            }
        }
        let base = input
            .base_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let model = input
            .model
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let n = self.connection().execute(
            "UPDATE matters SET \
                ai_enabled = ?1, \
                ai_allow_remote = ?2, \
                ai_base_url = ?3, \
                ai_model = ?4, \
                ai_provider_kind = ?5 \
             WHERE id = ?6",
            params![
                if input.enabled { 1i64 } else { 0i64 },
                if input.allow_remote { 1i64 } else { 0i64 },
                base,
                model,
                kind,
                self.id(),
            ],
        )?;
        if n == 0 {
            return Err(Error::MatterRowMissing);
        }
        Ok(())
    }

    /// Keyset page of AI suggest candidates (prefer in_review when `in_review_only`).
    ///
    /// Rows always have non-null `text_sha256`. Withhold checks are the caller's
    /// responsibility ([`Matter::item_is_withheld`]).
    pub fn list_ai_suggest_candidates(
        &self,
        after_id: Option<&str>,
        limit: u64,
        in_review_only: bool,
    ) -> Result<Vec<AiSuggestCandidate>> {
        let lim = limit.max(1) as i64;
        let review_clause = if in_review_only {
            "AND IFNULL(in_review, 0) = 1"
        } else {
            ""
        };
        let sql = if after_id.is_some() {
            format!(
                "SELECT id, text_sha256, in_review \
                 FROM items \
                 WHERE matter_id = ?1 \
                   AND text_sha256 IS NOT NULL \
                   AND length(trim(text_sha256)) > 0 \
                   {review_clause} \
                   AND id > ?2 \
                 ORDER BY id ASC \
                 LIMIT ?3"
            )
        } else {
            format!(
                "SELECT id, text_sha256, in_review \
                 FROM items \
                 WHERE matter_id = ?1 \
                   AND text_sha256 IS NOT NULL \
                   AND length(trim(text_sha256)) > 0 \
                   {review_clause} \
                 ORDER BY id ASC \
                 LIMIT ?2"
            )
        };
        let mut stmt = self.connection().prepare(&sql)?;
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<AiSuggestCandidate> {
            Ok(AiSuggestCandidate {
                id: row.get(0)?,
                text_sha256: row.get(1)?,
                in_review: row.get(2)?,
            })
        };
        let rows = if let Some(aid) = after_id {
            stmt.query_map(params![self.id(), aid, lim], map)?
        } else {
            stmt.query_map(params![self.id(), lim], map)?
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// True when a prior suggestion matches the fingerprint (for skip).
    pub fn has_matching_ai_suggestion_fingerprint(
        &self,
        item_id: &str,
        text_sha256: &str,
        model: &str,
        template_id: &str,
        catalog_hash: &str,
    ) -> Result<bool> {
        let n: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM item_ai_suggestions \
             WHERE matter_id = ?1 AND item_id = ?2 \
               AND text_sha256 = ?3 AND model = ?4 \
               AND prompt_template_id = ?5 AND catalog_content_hash = ?6 \
               AND status IN ('pending', 'accepted', 'rejected')",
            params![
                self.id(),
                item_id,
                text_sha256,
                model,
                template_id,
                catalog_hash
            ],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    /// Insert one pending AI suggestion. Returns suggestion id.
    pub fn insert_ai_suggestion(&self, input: InsertAiSuggestionInput<'_>) -> Result<String> {
        self.ensure_item_in_matter(input.item_id)?;
        let id = new_id("ais");
        let now = now_rfc3339();
        self.connection().execute(
            "INSERT INTO item_ai_suggestions (\
                id, matter_id, item_id, suggestion_type, code_id, code_name, \
                confidence, rationale, provider_kind, model, prompt_template_id, \
                is_remote, text_sha256, catalog_content_hash, status, job_id, created_at\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                id,
                self.id(),
                input.item_id,
                input.suggestion_type,
                input.code_id,
                input.code_name,
                input.confidence,
                input.rationale,
                input.provider_kind,
                input.model,
                input.prompt_template_id,
                if input.is_remote { 1i64 } else { 0i64 },
                input.text_sha256,
                input.catalog_content_hash,
                AI_SUGGESTION_PENDING,
                input.job_id,
                now,
            ],
        )?;
        Ok(id)
    }

    /// Supersede pending suggestions for an item (before writing a new batch).
    pub fn supersede_pending_ai_suggestions(&self, item_id: &str, actor: &str) -> Result<u64> {
        self.ensure_item_in_matter(item_id)?;
        let now = now_rfc3339();
        let n = self.connection().execute(
            "UPDATE item_ai_suggestions SET status = ?1, resolved_at = ?2, resolved_by = ?3 \
             WHERE matter_id = ?4 AND item_id = ?5 AND status = ?6",
            params![
                AI_SUGGESTION_SUPERSEDED,
                now,
                actor,
                self.id(),
                item_id,
                AI_SUGGESTION_PENDING
            ],
        )?;
        Ok(n as u64)
    }

    /// List pending suggestions for one item.
    pub fn list_pending_ai_suggestions_for_item(
        &self,
        item_id: &str,
    ) -> Result<Vec<ItemAiSuggestion>> {
        self.ensure_item_in_matter(item_id)?;
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, item_id, suggestion_type, code_id, code_name, confidence, \
                    rationale, provider_kind, model, prompt_template_id, is_remote, text_sha256, \
                    catalog_content_hash, status, job_id, created_at, resolved_at, resolved_by, \
                    citations_count \
             FROM item_ai_suggestions \
             WHERE matter_id = ?1 AND item_id = ?2 AND status = ?3 \
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(
            params![self.id(), item_id, AI_SUGGESTION_PENDING],
            map_suggestion_row,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// List pending suggestions for the whole matter (newest first, capped).
    pub fn list_pending_ai_suggestions(&self, limit: u64) -> Result<Vec<ItemAiSuggestion>> {
        let lim = limit.max(1) as i64;
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, item_id, suggestion_type, code_id, code_name, confidence, \
                    rationale, provider_kind, model, prompt_template_id, is_remote, text_sha256, \
                    catalog_content_hash, status, job_id, created_at, resolved_at, resolved_by, \
                    citations_count \
             FROM item_ai_suggestions \
             WHERE matter_id = ?1 AND status = ?2 \
             ORDER BY created_at DESC, id DESC \
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            params![self.id(), AI_SUGGESTION_PENDING, lim],
            map_suggestion_row,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Load one suggestion by id.
    pub fn get_ai_suggestion(&self, suggestion_id: &str) -> Result<ItemAiSuggestion> {
        self.connection()
            .query_row(
                "SELECT id, matter_id, item_id, suggestion_type, code_id, code_name, confidence, \
                        rationale, provider_kind, model, prompt_template_id, is_remote, text_sha256, \
                        catalog_content_hash, status, job_id, created_at, resolved_at, resolved_by, \
                        citations_count \
                 FROM item_ai_suggestions WHERE id = ?1 AND matter_id = ?2",
                params![suggestion_id, self.id()],
                map_suggestion_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("ai suggestion not found: {suggestion_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// Batch-insert citation rows for one suggestion; updates `citations_count`.
    ///
    /// Quotes are stored in full (no hard truncate). Cap count at
    /// [`MAX_CITATIONS_PER_SUGGESTION`] at the call site before invoking.
    pub fn insert_ai_suggestion_citations(
        &self,
        inputs: &[InsertAiCitationInput<'_>],
    ) -> Result<Vec<String>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let suggestion_id = inputs[0].suggestion_id;
        let sugg = self.get_ai_suggestion(suggestion_id)?;
        for input in inputs {
            if input.suggestion_id != suggestion_id {
                return Err(Error::Other(
                    "insert_ai_suggestion_citations: mixed suggestion_id in batch".into(),
                ));
            }
            if input.item_id != sugg.item_id {
                return Err(Error::Other(format!(
                    "citation item_id {} does not match suggestion item {}",
                    input.item_id, sugg.item_id
                )));
            }
        }
        let now = now_rfc3339();
        let mut ids = Vec::with_capacity(inputs.len());
        for input in inputs {
            let id = new_id("aic");
            let field = if input.field.trim().is_empty() {
                "text"
            } else {
                input.field
            };
            let status = if input.verify_status.trim().is_empty() {
                VERIFY_UNCHECKED
            } else {
                input.verify_status
            };
            self.connection().execute(
                "INSERT INTO item_ai_suggestion_citations (\
                    id, suggestion_id, matter_id, item_id, ordinal, quote, \
                    start_offset, end_offset, field, verify_status, created_at\
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    id,
                    suggestion_id,
                    self.id(),
                    input.item_id,
                    input.ordinal,
                    input.quote, // full quote; do not truncate
                    input.start_offset,
                    input.end_offset,
                    field,
                    status,
                    now,
                ],
            )?;
            ids.push(id);
        }
        let count: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM item_ai_suggestion_citations \
             WHERE suggestion_id = ?1 AND matter_id = ?2",
            params![suggestion_id, self.id()],
            |row| row.get(0),
        )?;
        self.connection().execute(
            "UPDATE item_ai_suggestions SET citations_count = ?1 \
             WHERE id = ?2 AND matter_id = ?3",
            params![count, suggestion_id, self.id()],
        )?;
        Ok(ids)
    }

    /// List citations for a suggestion (ordinal ascending).
    pub fn list_ai_suggestion_citations(
        &self,
        suggestion_id: &str,
    ) -> Result<Vec<ItemAiSuggestionCitation>> {
        // Ensure suggestion belongs to this matter.
        let _ = self.get_ai_suggestion(suggestion_id)?;
        let mut stmt = self.connection().prepare(
            "SELECT id, suggestion_id, matter_id, item_id, ordinal, quote, \
                    start_offset, end_offset, field, verify_status, created_at \
             FROM item_ai_suggestion_citations \
             WHERE suggestion_id = ?1 AND matter_id = ?2 \
             ORDER BY ordinal ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![suggestion_id, self.id()], map_citation_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Accept a pending suggestion: apply code via [`Matter::apply_codes`], mark accepted.
    ///
    /// Audit: `coding.apply` (via apply_codes) + `ai_suggestion.accept` with
    /// provenance + citation **offset pointers only** (no quote cleartext).
    ///
    /// Citations are re-verified against the item's current continuous CAS text
    /// (Desk display coordinate space, capped at
    /// [`AI_VERIFY_TEXT_MAX_BYTES`]) so accept audit reflects the body at
    /// promote time — not a stale job-time status. Apply + status + accept
    /// audit commit in **one** SQLite transaction (nested `with_transaction`).
    pub fn accept_ai_suggestion(
        &self,
        suggestion_id: &str,
        actor: &str,
    ) -> Result<ItemAiSuggestion> {
        let sugg = self.get_ai_suggestion(suggestion_id)?;
        if sugg.status != AI_SUGGESTION_PENDING {
            return Err(Error::Other(format!(
                "ai suggestion {suggestion_id} is not pending (status={})",
                sugg.status
            )));
        }
        let code_id = resolve_suggestion_code_id(self, &sugg)?;
        let actor_s = {
            let t = actor.trim();
            if t.is_empty() {
                "desk".to_string()
            } else {
                t.to_string()
            }
        };
        let citations = self.list_ai_suggestion_citations(suggestion_id)?;

        // Re-verify against current continuous body (honest audit + stale body).
        let item = self.get_item(&sugg.item_id)?;
        let body_text = load_item_text_continuous_for_verify(self, item.text_sha256.as_deref())?;
        let current_digest = item.text_sha256.as_deref();
        let digest_stale = match (&sugg.text_sha256, current_digest) {
            (Some(a), Some(b)) => a != b,
            (Some(_), None) => true,
            (None, Some(_)) => true,
            (None, None) => false,
        };

        let mut citation_ptrs: Vec<serde_json::Value> = Vec::with_capacity(citations.len());
        let mut citation_unverified = false;
        for c in &citations {
            let (status, start, end) = if let Some(ref text) = body_text {
                // When digest differs, ignore stored offsets (body may have shifted).
                let (so, eo) = if digest_stale {
                    (None, None)
                } else {
                    (c.start_offset, c.end_offset)
                };
                let v = crate::ai_verify::verify_ai_citation_against_text(&c.quote, so, eo, text);
                if v.status != VERIFY_MATCHED {
                    citation_unverified = true;
                }
                (v.status, v.start_offset, v.end_offset)
            } else {
                // CAS body unavailable/unreadable: never claim matched at accept time.
                // Clear offsets so audit does not present unverifiable pointers as live.
                citation_unverified = true;
                (VERIFY_QUOTE_NOT_FOUND.to_string(), None, None)
            };
            citation_ptrs.push(serde_json::json!({
                "citation_id": c.id,
                "start_offset": start,
                "end_offset": end,
                "field": c.field,
                "verify_status": status,
            }));
        }

        // Single transaction: code apply + suggestion accepted + accept audit.
        let item_id = sugg.item_id.clone();
        let code_name = sugg.code_name.clone();
        let prompt_template_id = sugg.prompt_template_id.clone();
        let model = sugg.model.clone();
        let provider_kind = sugg.provider_kind.clone();
        let is_remote = sugg.is_remote;
        let sugg_text_sha256 = sugg.text_sha256.clone();
        let code_id_for_apply = code_id.clone();
        let actor_for_apply = actor_s.clone();

        self.with_transaction(|_conn| {
            self.apply_codes(ApplyCodesInput {
                item_ids: vec![item_id.clone()],
                add_code_ids: vec![code_id_for_apply.clone()],
                remove_code_ids: vec![],
                propagate_family: false,
                actor: actor_for_apply.clone(),
                expected_version: None,
            })?;
            let now = now_rfc3339();
            self.connection().execute(
                "UPDATE item_ai_suggestions SET status = ?1, resolved_at = ?2, resolved_by = ?3, \
                 code_id = COALESCE(code_id, ?4) \
                 WHERE id = ?5 AND matter_id = ?6",
                params![
                    AI_SUGGESTION_ACCEPTED,
                    now,
                    actor_for_apply,
                    code_id_for_apply,
                    suggestion_id,
                    self.id()
                ],
            )?;
            // Provenance: pointers only — never quote cleartext in audit.
            self.append_audit(crate::audit::AuditEventInput {
                actor: actor_for_apply,
                action: "ai_suggestion.accept".into(),
                entity: format!("item:{item_id}"),
                params_json: serde_json::json!({
                    "suggestion_id": suggestion_id,
                    "code_id": code_id_for_apply,
                    "code_name": code_name,
                    "source": "ai_suggestion",
                    "prompt_template_id": prompt_template_id,
                    "model": model,
                    "provider_kind": provider_kind,
                    "is_remote": is_remote,
                    "text_sha256": sugg_text_sha256,
                    "current_text_sha256": current_digest,
                    "text_sha256_stale": digest_stale,
                    "citations": citation_ptrs,
                    "citation_unverified": citation_unverified,
                    "cas_text_unavailable": body_text.is_none() && current_digest.is_some(),
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            })?;
            Ok(())
        })?;
        self.get_ai_suggestion(suggestion_id)
    }

    /// Reject a pending suggestion.
    pub fn reject_ai_suggestion(
        &self,
        suggestion_id: &str,
        actor: &str,
    ) -> Result<ItemAiSuggestion> {
        let sugg = self.get_ai_suggestion(suggestion_id)?;
        if sugg.status != AI_SUGGESTION_PENDING {
            return Err(Error::Other(format!(
                "ai suggestion {suggestion_id} is not pending (status={})",
                sugg.status
            )));
        }
        let actor_s = {
            let t = actor.trim();
            if t.is_empty() {
                "desk".to_string()
            } else {
                t.to_string()
            }
        };
        let now = now_rfc3339();
        self.connection().execute(
            "UPDATE item_ai_suggestions SET status = ?1, resolved_at = ?2, resolved_by = ?3 \
             WHERE id = ?4 AND matter_id = ?5",
            params![
                AI_SUGGESTION_REJECTED,
                now,
                actor_s,
                suggestion_id,
                self.id()
            ],
        )?;
        self.append_audit(crate::audit::AuditEventInput {
            actor: actor_s,
            action: "ai_suggestion.reject".into(),
            entity: format!("item:{}", sugg.item_id),
            params_json: serde_json::json!({
                "suggestion_id": suggestion_id,
                "code_name": sugg.code_name,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
        self.get_ai_suggestion(suggestion_id)
    }

    /// Insert run meta row. Returns run id.
    pub fn insert_ai_suggestion_run(
        &self,
        input: InsertAiSuggestionRunInput<'_>,
    ) -> Result<String> {
        let id = new_id("air");
        let now = now_rfc3339();
        self.connection().execute(
            "INSERT INTO ai_suggestion_runs (\
                id, matter_id, job_id, provider_kind, model, prompt_template_id, \
                is_remote, item_count, suggestion_count, created_at\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                self.id(),
                input.job_id,
                input.provider_kind,
                input.model,
                input.prompt_template_id,
                if input.is_remote { 1i64 } else { 0i64 },
                input.item_count,
                input.suggestion_count,
                now,
            ],
        )?;
        Ok(id)
    }

    /// Convenience: current UTC for AI bookkeeping.
    pub fn ai_now() -> String {
        now_rfc3339()
    }
}

/// Load continuous UTF-8 prefix of item body for citation re-verify.
///
/// Returns `Ok(None)` when there is no digest, the digest is invalid, or the
/// CAS blob is missing. Callers must treat `None` as **unverified** (never
/// trust stored `matched` without re-verification). Never errors solely because
/// a fixture used a non-hex placeholder digest.
fn load_item_text_continuous_for_verify(
    matter: &Matter,
    digest: Option<&str>,
) -> Result<Option<String>> {
    let Some(digest) = digest.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    // Invalid digest / missing blob → treat as no body (do not fail accept).
    let exists = match matter.blob_exists(digest) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if !exists {
        return Ok(None);
    }
    let len = match matter.cas_len(digest) {
        Ok(n) => n,
        Err(_) => return Ok(None),
    };
    let take = len.min(AI_VERIFY_TEXT_MAX_BYTES) as usize;
    if take == 0 {
        return Ok(Some(String::new()));
    }
    // Continuous prefix only — never head+tail synthetic for offset space.
    let bytes = if len <= AI_VERIFY_TEXT_MAX_BYTES {
        match matter.get_bytes(digest) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        }
    } else {
        match matter.read_cas_prefix(digest, take) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        }
    };
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn resolve_suggestion_code_id(matter: &Matter, sugg: &ItemAiSuggestion) -> Result<String> {
    if let Some(ref cid) = sugg.code_id {
        let def = matter.get_code_definition(cid)?;
        if def.matter_id != matter.id() {
            return Err(Error::Other(format!(
                "code definition {cid} belongs to another matter"
            )));
        }
        return Ok(def.id);
    }
    // Match code_name against key or label (case-insensitive).
    let name = sugg.code_name.trim();
    if name.is_empty() {
        return Err(Error::Other(
            "ai suggestion has no code_id and empty code_name".into(),
        ));
    }
    let defs = matter.list_code_definitions()?;
    let lower = name.to_ascii_lowercase();
    if let Some(d) = defs.iter().find(|d| d.key.eq_ignore_ascii_case(&lower)) {
        return Ok(d.id.clone());
    }
    if let Some(d) = defs
        .iter()
        .find(|d| d.label.eq_ignore_ascii_case(name) || d.label.to_ascii_lowercase() == lower)
    {
        return Ok(d.id.clone());
    }
    Err(Error::Other(format!(
        "could not resolve code_name '{name}' to a catalog definition"
    )))
}

fn map_suggestion_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemAiSuggestion> {
    let is_remote: i64 = row.get(11)?;
    Ok(ItemAiSuggestion {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        item_id: row.get(2)?,
        suggestion_type: row.get(3)?,
        code_id: row.get(4)?,
        code_name: row.get(5)?,
        confidence: row.get(6)?,
        rationale: row.get(7)?,
        provider_kind: row.get(8)?,
        model: row.get(9)?,
        prompt_template_id: row.get(10)?,
        is_remote: is_remote != 0,
        text_sha256: row.get(12)?,
        catalog_content_hash: row.get(13)?,
        status: row.get(14)?,
        job_id: row.get(15)?,
        created_at: row.get(16)?,
        resolved_at: row.get(17)?,
        resolved_by: row.get(18)?,
        citations_count: row.get(19)?,
    })
}

fn map_citation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemAiSuggestionCitation> {
    Ok(ItemAiSuggestionCitation {
        id: row.get(0)?,
        suggestion_id: row.get(1)?,
        matter_id: row.get(2)?,
        item_id: row.get(3)?,
        ordinal: row.get(4)?,
        quote: row.get(5)?,
        start_offset: row.get(6)?,
        end_offset: row.get(7)?,
        field: row.get(8)?,
        verify_status: row.get(9)?,
        created_at: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_hash_includes_guidance() {
        let a = CodeDef {
            id: "1".into(),
            matter_id: "m".into(),
            key: "hot".into(),
            label: "Hot".into(),
            group_key: "issues".into(),
            cardinality: "multi".into(),
            color: None,
            sort_order: 0,
            is_active: 1,
            created_at: String::new(),
            guidance: Some("XYZZY_PROTOCOL".into()),
        };
        let b = {
            let mut c = a.clone();
            c.guidance = Some("OTHER".into());
            c
        };
        assert_ne!(catalog_content_hash(&[a]), catalog_content_hash(&[b]));
    }

    #[test]
    fn fingerprint_stable() {
        let f1 = suggestion_fingerprint("abc", "model", "suggest_codes_v1", "cat");
        let f2 = suggestion_fingerprint("abc", "model", "suggest_codes_v1", "cat");
        let f3 = suggestion_fingerprint("abc", "model", "suggest_codes_v1", "cat2");
        assert_eq!(f1, f2);
        assert_ne!(f1, f3);
        assert_eq!(f1.len(), 64);
    }
}
