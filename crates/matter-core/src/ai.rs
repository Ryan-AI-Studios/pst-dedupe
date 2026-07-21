//! AI provider config + first-pass code suggestions (schema v30 / track 0051).
//!
//! Suggestions are **never** final codes — human accept promotes via
//! [`Matter::apply_codes`]. API keys are **not** stored in SQLite.

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
                    catalog_content_hash, status, job_id, created_at, resolved_at, resolved_by \
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
                    catalog_content_hash, status, job_id, created_at, resolved_at, resolved_by \
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
                        catalog_content_hash, status, job_id, created_at, resolved_at, resolved_by \
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

    /// Accept a pending suggestion: apply code via [`Matter::apply_codes`], mark accepted.
    ///
    /// Audit: `coding.apply` (via apply_codes) + `ai_suggestion.accept` with source detail.
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
        self.apply_codes(ApplyCodesInput {
            item_ids: vec![sugg.item_id.clone()],
            add_code_ids: vec![code_id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: actor_s.clone(),
        })?;
        let now = now_rfc3339();
        self.connection().execute(
            "UPDATE item_ai_suggestions SET status = ?1, resolved_at = ?2, resolved_by = ?3, \
             code_id = COALESCE(code_id, ?4) \
             WHERE id = ?5 AND matter_id = ?6",
            params![
                AI_SUGGESTION_ACCEPTED,
                now,
                actor_s,
                code_id,
                suggestion_id,
                self.id()
            ],
        )?;
        self.append_audit(crate::audit::AuditEventInput {
            actor: actor_s,
            action: "ai_suggestion.accept".into(),
            entity: format!("item:{}", sugg.item_id),
            params_json: serde_json::json!({
                "suggestion_id": suggestion_id,
                "code_id": code_id,
                "code_name": sugg.code_name,
                "source": "ai_suggestion",
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
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
