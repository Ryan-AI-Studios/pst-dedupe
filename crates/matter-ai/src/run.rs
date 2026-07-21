//! Resumable `ai_suggest_codes` job: suggestions only (never final `item_codes`).

use std::time::Instant;

use matter_core::{
    catalog_content_hash, AiSuggestCandidate, AuditEventInput, InsertAiCitationInput,
    InsertAiSuggestionInput, InsertAiSuggestionRunInput, Matter, AI_PROVIDER_NONE,
    AI_SUGGESTION_TYPE_CODE, MAX_CITATIONS_PER_SUGGESTION,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{AiError, Result};
use crate::params::AiSuggestCodesParams;
use crate::parse::extract_code_suggestions;
use crate::prompt::{build_suggest_codes_v2, PROMPT_TEMPLATE_SUGGEST_CODES_V2};
use crate::provider::{AiProvider, AiProviderKind, MockAiProvider, OpenAiCompatibleProvider};
use crate::secrets::resolve_api_key_optional;
use crate::truncate::middle_drop;
use crate::verify::verify_citation_for_storage;

#[cfg(test)]
use crate::truncate::assemble_head_tail;

/// Job kind string for process-runner.
pub const JOB_KIND_AI_SUGGEST_CODES: &str = "ai_suggest_codes";
/// Checkpoint stage name.
pub const AI_SUGGEST_STAGE: &str = "ai_suggest_codes";

/// Summary after a suggest run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiSuggestSummary {
    pub completed_count: u64,
    pub suggested_count: u64,
    pub skipped_count: u64,
    pub withheld_count: u64,
    pub error_count: u64,
    pub suggestion_rows: u64,
}

/// Full success payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSuggestReport {
    pub completed_count: u64,
    pub suggested_count: u64,
    pub skipped_count: u64,
    pub withheld_count: u64,
    pub error_count: u64,
    pub suggestion_rows: u64,
    pub provider_kind: String,
    pub model: String,
    pub is_remote: bool,
    pub prompt_template_id: String,
}

/// Outcome of [`run_ai_suggest_codes`].
#[derive(Debug, Clone, PartialEq)]
pub enum AiSuggestOutcome {
    Succeeded(AiSuggestReport),
    Paused(AiSuggestSummary),
    Failed {
        message: String,
        summary: AiSuggestSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    suggested_count: u64,
    skipped_count: u64,
    #[serde(default)]
    withheld_count: u64,
    error_count: u64,
    #[serde(default)]
    suggestion_rows: u64,
    #[serde(default)]
    processed_items: u64,
    params: serde_json::Value,
}

/// Run first-pass code suggestions. Writes **only** `item_ai_suggestions`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between items.
pub fn run_ai_suggest_codes(
    matter: &Matter,
    job_id: &str,
    params: &AiSuggestCodesParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<AiSuggestOutcome> {
    let provider = resolve_provider(matter)?;
    run_ai_suggest_codes_with_provider(matter, job_id, params, provider.as_ref(), cancel, progress)
}

/// Same as [`run_ai_suggest_codes`] with an explicit provider (tests).
pub fn run_ai_suggest_codes_with_provider(
    matter: &Matter,
    job_id: &str,
    params: &AiSuggestCodesParams,
    provider: &dyn AiProvider,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<AiSuggestOutcome> {
    let started = Instant::now();
    let result = run_body(matter, job_id, params, provider, cancel, &progress);

    match &result {
        Ok(AiSuggestOutcome::Succeeded(r)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ai_suggest_codes.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "suggested_count": r.suggested_count,
                    "skipped_count": r.skipped_count,
                    "withheld_count": r.withheld_count,
                    "error_count": r.error_count,
                    "completed_count": r.completed_count,
                    "suggestion_rows": r.suggestion_rows,
                    "provider_kind": r.provider_kind,
                    "model": r.model,
                    "is_remote": r.is_remote,
                    "prompt_template_id": r.prompt_template_id,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                let message = format!("audit complete failed: {e}");
                let summary = summary_from_report(r);
                let _ = matter.append_audit(AuditEventInput {
                    actor: "system".into(),
                    action: "ai_suggest_codes.fail".into(),
                    entity: format!("job:{job_id}"),
                    params_json: fail_audit_params(&message, &summary).to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                });
                return Ok(AiSuggestOutcome::Failed { message, summary });
            }
        }
        Ok(AiSuggestOutcome::Paused(_)) => {}
        Ok(AiSuggestOutcome::Failed { message, summary }) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ai_suggest_codes.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(message, summary).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
        Err(e) => {
            let empty = AiSuggestSummary::default();
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ai_suggest_codes.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(&e.to_string(), &empty).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
    }

    result
}

/// Build provider from matter AI config.
pub fn resolve_provider(matter: &Matter) -> Result<Box<dyn AiProvider>> {
    let cfg = matter.get_ai_config()?;
    if !cfg.ai_enabled {
        return Err(AiError::AiDisabled);
    }
    let kind_str = cfg
        .ai_provider_kind
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(AI_PROVIDER_NONE);
    let kind = AiProviderKind::parse(kind_str)
        .ok_or_else(|| AiError::InvalidParams(format!("unknown ai_provider_kind '{kind_str}'")))?;
    match kind {
        AiProviderKind::None => Err(AiError::AiDisabled),
        AiProviderKind::Mock => Ok(Box::new(MockAiProvider::new())),
        AiProviderKind::OpenAiCompatible => {
            let base = cfg
                .ai_base_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    AiError::InvalidParams(
                        "ai_base_url required for openai_compatible provider".into(),
                    )
                })?;
            let key = resolve_api_key_optional()?;
            let p = OpenAiCompatibleProvider::new(base, key, cfg.ai_allow_remote)?;
            if p.is_remote() && !cfg.ai_allow_remote {
                return Err(AiError::RemoteBlocked);
            }
            Ok(Box::new(p))
        }
    }
}

fn summary_from_report(r: &AiSuggestReport) -> AiSuggestSummary {
    AiSuggestSummary {
        completed_count: r.completed_count,
        suggested_count: r.suggested_count,
        skipped_count: r.skipped_count,
        withheld_count: r.withheld_count,
        error_count: r.error_count,
        suggestion_rows: r.suggestion_rows,
    }
}

fn fail_audit_params(message: &str, summary: &AiSuggestSummary) -> serde_json::Value {
    json!({
        "error": message,
        "completed_count": summary.completed_count,
        "suggested_count": summary.suggested_count,
        "skipped_count": summary.skipped_count,
        "withheld_count": summary.withheld_count,
        "error_count": summary.error_count,
        "suggestion_rows": summary.suggestion_rows,
    })
}

fn run_body(
    matter: &Matter,
    job_id: &str,
    params: &AiSuggestCodesParams,
    provider: &dyn AiProvider,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<AiSuggestOutcome> {
    params.validate()?;

    // Fail closed if AI disabled even when provider injected.
    let cfg = matter.get_ai_config()?;
    if !cfg.ai_enabled {
        return Ok(AiSuggestOutcome::Failed {
            message: AiError::AiDisabled.to_string(),
            summary: AiSuggestSummary::default(),
        });
    }
    if provider.is_remote() && !cfg.ai_allow_remote {
        return Ok(AiSuggestOutcome::Failed {
            message: AiError::RemoteBlocked.to_string(),
            summary: AiSuggestSummary::default(),
        });
    }

    let model = cfg
        .ai_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(match provider.kind() {
            AiProviderKind::Mock => "mock",
            _ => "default",
        })
        .to_string();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate()?;
    let params_json = serde_json::to_value(&effective)
        .map_err(|e| AiError::other(format!("serialize params: {e}")))?;

    let resuming = prior.as_ref().is_some_and(|p| p.completed_count > 0);
    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ai_suggest_codes.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "resume": resuming,
            "provider_kind": provider.kind().as_str(),
            "model": model,
            "is_remote": provider.is_remote(),
            "prompt_template_id": PROMPT_TEMPLATE_SUGGEST_CODES_V2,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    run_inner(
        matter,
        job_id,
        &effective,
        provider,
        &model,
        cancel,
        progress,
        &params_json,
        prior,
    )
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, AI_SUGGEST_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(AiError::other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &AiSuggestCodesParams,
    prior: Option<&CheckpointCursor>,
) -> Result<AiSuggestCodesParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<AiSuggestCodesParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(AiError::other(format!("checkpoint params unreadable: {e}")));
                }
            }
        }
    }
    Ok(call_site.clone())
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &AiSuggestCodesParams,
    provider: &dyn AiProvider,
    model: &str,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<AiSuggestOutcome> {
    let mut summary = AiSuggestSummary::default();
    let mut cursor_index = 0u64;
    let mut last_item_id: Option<String> = None;
    let mut processed_items = 0u64;

    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        last_item_id = p.last_item_id;
        summary.completed_count = p.completed_count;
        summary.suggested_count = p.suggested_count;
        summary.skipped_count = p.skipped_count;
        summary.withheld_count = p.withheld_count;
        summary.error_count = p.error_count;
        summary.suggestion_rows = p.suggestion_rows;
        processed_items = p.processed_items;
    }

    let fail = |summary: AiSuggestSummary, e: AiError| -> Result<AiSuggestOutcome> {
        Ok(AiSuggestOutcome::Failed {
            message: e.to_string(),
            summary,
        })
    };

    let catalog = match matter.list_code_definitions() {
        Ok(c) => c,
        Err(e) => return fail(summary, e.into()),
    };
    let cat_hash = catalog_content_hash(&catalog);
    let template = PROMPT_TEMPLATE_SUGGEST_CODES_V2;
    let is_remote = provider.is_remote();
    let provider_kind = provider.kind().as_str().to_string();

    let batch = params.batch_size.max(1) as u64;
    let max_items = params.max_items;

    loop {
        if processed_items >= max_items {
            break;
        }
        if cancel.map(|c| c()).unwrap_or(false) {
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                processed_items,
                params_json,
                last_item_id.as_deref(),
            ) {
                return fail(summary, e);
            }
            progress(summary.completed_count);
            return Ok(AiSuggestOutcome::Paused(summary));
        }

        let remaining = max_items.saturating_sub(processed_items);
        let page = batch.min(remaining);
        let candidates = match matter.list_ai_suggest_candidates(
            last_item_id.as_deref(),
            page,
            params.in_review_only(),
        ) {
            Ok(c) => c,
            Err(e) => return fail(summary, e.into()),
        };
        if candidates.is_empty() {
            break;
        }

        for cand in candidates {
            if processed_items >= max_items {
                break;
            }
            if cancel.map(|c| c()).unwrap_or(false) {
                if let Err(e) = write_checkpoint(
                    matter,
                    job_id,
                    cursor_index,
                    &summary,
                    processed_items,
                    params_json,
                    last_item_id.as_deref(),
                ) {
                    return fail(summary, e);
                }
                progress(summary.completed_count);
                return Ok(AiSuggestOutcome::Paused(summary));
            }

            if let Err(e) = process_one(
                matter,
                job_id,
                &cand,
                params,
                provider,
                model,
                &provider_kind,
                is_remote,
                template,
                &cat_hash,
                &catalog,
                &mut summary,
            ) {
                // Item-level provider/parse failures count as errors and continue.
                if is_item_level_error(&e) {
                    summary.error_count += 1;
                    summary.completed_count += 1;
                    let _ = matter.record_item_error(matter_core::ItemErrorInput {
                        item_id: Some(cand.id.clone()),
                        source_id: None,
                        job_id: Some(job_id.into()),
                        stage: AI_SUGGEST_STAGE.into(),
                        code: "ai_item".into(),
                        message: e.to_string(),
                        detail: None,
                    });
                } else {
                    return fail(summary, e);
                }
            }
            processed_items += 1;
            cursor_index += 1;
            last_item_id = Some(cand.id.clone());
            progress(summary.completed_count);
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                processed_items,
                params_json,
                last_item_id.as_deref(),
            ) {
                return fail(summary, e);
            }
        }
    }

    // Run meta (counts only — no bodies/keys). Job must not succeed if this fails.
    if let Err(e) = matter.insert_ai_suggestion_run(InsertAiSuggestionRunInput {
        job_id: Some(job_id),
        provider_kind: &provider_kind,
        model: Some(model),
        prompt_template_id: template,
        is_remote,
        item_count: summary.completed_count as i64,
        suggestion_count: summary.suggestion_rows as i64,
    }) {
        return fail(summary, e.into());
    }

    Ok(AiSuggestOutcome::Succeeded(AiSuggestReport {
        completed_count: summary.completed_count,
        suggested_count: summary.suggested_count,
        skipped_count: summary.skipped_count,
        withheld_count: summary.withheld_count,
        error_count: summary.error_count,
        suggestion_rows: summary.suggestion_rows,
        provider_kind,
        model: model.to_string(),
        is_remote,
        prompt_template_id: template.to_string(),
    }))
}

fn is_item_level_error(e: &AiError) -> bool {
    matches!(
        e,
        AiError::JsonParse(_) | AiError::Provider(_) | AiError::Http(_)
    )
}

#[allow(clippy::too_many_arguments)]
fn process_one(
    matter: &Matter,
    job_id: &str,
    cand: &AiSuggestCandidate,
    params: &AiSuggestCodesParams,
    provider: &dyn AiProvider,
    model: &str,
    provider_kind: &str,
    is_remote: bool,
    template: &str,
    catalog_hash: &str,
    catalog: &[matter_core::CodeDef],
    summary: &mut AiSuggestSummary,
) -> Result<()> {
    // Skip withheld (privilege hold) — fail-closed P0.
    if matter.item_is_withheld(&cand.id)? {
        summary.withheld_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    let digest = cand
        .text_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(digest) = digest else {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    if !params.reset
        && matter.has_matching_ai_suggestion_fingerprint(
            &cand.id,
            digest,
            model,
            template,
            catalog_hash,
        )?
    {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    // Continuous CAS prefix for verify + Desk coordinate space (not head+tail).
    // Head+tail synthetic strings must never be used for stored offsets — they map
    // incorrectly relative to the original CAS / Desk full body.
    // Prompt path: middle_drop on the continuous text so head+tail of the *loaded*
    // window survive in the model input when over max_text_bytes.
    let text = load_text_continuous(matter, digest, MAX_VERIFY_TEXT_BYTES)?;
    let prepared = middle_drop(&text, params.max_text_bytes as usize);
    let prepared_was_truncated = prepared != text;

    let req = build_suggest_codes_v2(
        model,
        catalog,
        &prepared,
        params.temperature,
        Some(params.max_tokens),
    );
    let resp = provider.complete(req)?;
    let parsed = extract_code_suggestions(&resp.content)?;

    // Supersede prior pending for this item when writing new suggestions.
    let _ = matter.supersede_pending_ai_suggestions(&cand.id, "system")?;

    let mut wrote = 0u64;
    for s in &parsed {
        let code_name = s.display_name().unwrap_or("unknown");
        // Resolve code_id from catalog when possible.
        let code_id = s
            .code_id
            .as_deref()
            .filter(|id| catalog.iter().any(|d| d.id == *id))
            .or_else(|| {
                catalog
                    .iter()
                    .find(|d| {
                        d.key.eq_ignore_ascii_case(code_name)
                            || d.label.eq_ignore_ascii_case(code_name)
                    })
                    .map(|d| d.id.as_str())
            });
        let sid = matter.insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &cand.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id,
            code_name,
            confidence: s.confidence,
            rationale: s.rationale_short.as_deref(),
            provider_kind,
            model,
            prompt_template_id: template,
            is_remote,
            text_sha256: Some(digest),
            catalog_content_hash: Some(catalog_hash),
            job_id: Some(job_id),
        })?;

        // Verify + persist citations against **full loaded text** (Desk coordinate space).
        // Cap count only; quotes stored in full. When middle-drop truncated the prompt,
        // model offsets are ignored and quote re-find runs on full text only.
        struct VerifiedCite {
            quote: String,
            start: Option<i64>,
            end: Option<i64>,
            field: String,
            status: String,
        }
        let mut verified: Vec<VerifiedCite> = Vec::new();
        for c in s.citations.iter().take(MAX_CITATIONS_PER_SUGGESTION) {
            let v = verify_citation_for_storage(
                &c.quote,
                c.start_offset,
                c.end_offset,
                &text,
                prepared_was_truncated,
            );
            let field = c
                .field
                .as_deref()
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .unwrap_or("text")
                .to_string();
            // Prefer verified offsets; only fall back to model offsets when verify
            // returned none **and** we did not truncate (prepared == full).
            let (start, end) = if prepared_was_truncated {
                (v.start_offset, v.end_offset)
            } else {
                (
                    v.start_offset.or(c.start_offset),
                    v.end_offset.or(c.end_offset),
                )
            };
            verified.push(VerifiedCite {
                quote: c.quote.clone(), // full quote
                start,
                end,
                field,
                status: v.status,
            });
        }
        let cite_inputs: Vec<InsertAiCitationInput<'_>> = verified
            .iter()
            .enumerate()
            .map(|(ord, c)| InsertAiCitationInput {
                suggestion_id: &sid,
                item_id: &cand.id,
                ordinal: ord as i64,
                quote: &c.quote,
                start_offset: c.start,
                end_offset: c.end,
                field: &c.field,
                verify_status: &c.status,
            })
            .collect();
        if !cite_inputs.is_empty() {
            matter.insert_ai_suggestion_citations(&cite_inputs)?;
        }
        wrote += 1;
    }

    // Residual (P3-4 deferred): empty model results leave no fingerprint row, so a
    // later run may re-call the provider for the same item/catalog/model. Acceptable
    // for mock/empty catalog; a dedicated empty-result marker is product follow-up.
    if wrote > 0 {
        summary.suggested_count += 1;
        summary.suggestion_rows += wrote;
    } else {
        summary.skipped_count += 1;
    }
    summary.completed_count += 1;
    Ok(())
}

/// Continuous body load for verify + prompt base — matches Desk display space.
///
/// - Blob ≤ `max_bytes`: full UTF-8 (lossy) decode.
/// - Blob > `max_bytes`: **first** `max_bytes` only (UTF-8-safe prefix). Never
///   head+marker+tail — synthetic coordinates must not be stored as offsets.
///
/// Aligns with Desk `BODY_DISPLAY_CAP_BYTES` / matter-core `AI_VERIFY_TEXT_MAX_BYTES`.
const MAX_VERIFY_TEXT_BYTES: u64 = matter_core::AI_VERIFY_TEXT_MAX_BYTES;

fn load_text_continuous(matter: &Matter, digest: &str, max_bytes: u64) -> Result<String> {
    let len = matter.cas_len(digest)?;
    if len == 0 {
        return Ok(String::new());
    }
    if len <= max_bytes {
        let bytes = matter.get_bytes(digest)?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    // Continuous prefix only (Desk-compatible offset space).
    let bytes = matter.read_cas_prefix(digest, max_bytes as usize)?;
    Ok(utf8_prefix(&bytes))
}

/// Optional prompt-only head+tail load for extremely large CAS blobs when a
/// caller wants true document endings in the **prompt** without affecting
/// verify offsets. Not used by the default suggest path (continuous + middle_drop).
#[cfg(test)]
fn load_text_capped(matter: &Matter, digest: &str, max_bytes: u64) -> Result<String> {
    let len = matter.cas_len(digest)?;
    if len <= max_bytes {
        let bytes = matter.get_bytes(digest)?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    load_text_head_tail(matter, digest, len, max_bytes)
}

/// Read first/last `max_bytes/2` from a large CAS blob and join with truncation marker.
/// **Prompt-only** — never use for citation verify / stored offsets.
#[cfg(test)]
fn load_text_head_tail(
    matter: &Matter,
    digest: &str,
    file_len: u64,
    max_bytes: u64,
) -> Result<String> {
    use std::io::{Read, Seek, SeekFrom};

    let half = (max_bytes / 2) as usize;
    if half == 0 {
        return Ok(String::new());
    }

    let mut file = matter.cas().open_read(digest)?;

    let mut head_buf = vec![0u8; half];
    let n_head = file.read(&mut head_buf).map_err(matter_core::Error::from)?;
    head_buf.truncate(n_head);
    let head = utf8_prefix(&head_buf);

    let tail_start = file_len.saturating_sub(half as u64);
    file.seek(SeekFrom::Start(tail_start))
        .map_err(matter_core::Error::from)?;
    let mut tail_buf = vec![0u8; half];
    let n_tail = file.read(&mut tail_buf).map_err(matter_core::Error::from)?;
    tail_buf.truncate(n_tail);
    let tail = utf8_suffix(&tail_buf);

    Ok(assemble_head_tail(&head, &tail))
}

/// Decode as much of a prefix as is valid UTF-8 (drop incomplete trailing bytes).
fn utf8_prefix(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(e) => {
            let valid = e.valid_up_to();
            match std::str::from_utf8(&bytes[..valid]) {
                Ok(s) => s.to_string(),
                Err(_) => String::new(),
            }
        }
    }
}

/// Decode a suffix starting at the first valid UTF-8 char boundary.
#[cfg(test)]
fn utf8_suffix(bytes: &[u8]) -> String {
    // Multi-byte UTF-8 is at most 4 bytes; scan a small window then the rest.
    let scan = bytes.len().min(4);
    for start in 0..scan {
        if let Ok(s) = std::str::from_utf8(&bytes[start..]) {
            return s.to_string();
        }
    }
    for start in scan..bytes.len() {
        if let Ok(s) = std::str::from_utf8(&bytes[start..]) {
            return s.to_string();
        }
    }
    String::new()
}

fn write_checkpoint(
    matter: &Matter,
    job_id: &str,
    cursor_index: u64,
    summary: &AiSuggestSummary,
    processed_items: u64,
    params_json: &serde_json::Value,
    last_item_id: Option<&str>,
) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index,
        last_item_id: last_item_id.map(|s| s.to_string()),
        completed_count: summary.completed_count,
        suggested_count: summary.suggested_count,
        skipped_count: summary.skipped_count,
        withheld_count: summary.withheld_count,
        error_count: summary.error_count,
        suggestion_rows: summary.suggestion_rows,
        processed_items,
        params: params_json.clone(),
    };
    let json = serde_json::to_string(&cursor)
        .map_err(|e| AiError::other(format!("checkpoint serialize: {e}")))?;
    matter.put_checkpoint(
        job_id,
        AI_SUGGEST_STAGE,
        &json,
        summary.completed_count as i64,
    )?;
    Ok(())
}

/// Provider kind string helpers for audit (re-export constants).
pub use matter_core::{
    AI_PROVIDER_MOCK as PROVIDER_MOCK, AI_PROVIDER_NONE as PROVIDER_NONE,
    AI_PROVIDER_OPENAI_COMPATIBLE as PROVIDER_OPENAI,
};

#[cfg(test)]
mod load_text_tests {
    use super::*;
    use matter_core::{item_role, item_status, ItemInput};
    use tempfile::TempDir;

    fn utf8_tempdir() -> (TempDir, camino::Utf8PathBuf) {
        let tmp = TempDir::new().expect("tempdir");
        let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
        (tmp, base)
    }

    #[test]
    fn load_text_continuous_prefix_not_head_tail() {
        let (_tmp, base) = utf8_tempdir();
        let matter = Matter::create(base.join("m"), "continuous").expect("create");

        // Cap smaller than body: continuous must be prefix only (no synthetic tail).
        const CAP: u64 = 4_000;
        let head = "HEAD_ONLY_PREFIX ".repeat(200); // ~3.4k
        let mid = "MID_FILL_XXXX ".repeat(20_000); // ~280k
        let tail = " TAIL_UNIQUE_ZZZ9 hot confidential ending";
        let body = format!("{head}{mid}{tail}");
        assert!(body.len() as u64 > CAP, "fixture must exceed load cap");

        let digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");
        let loaded = load_text_continuous(&matter, &digest, CAP).expect("load");
        assert!(
            loaded.contains("HEAD_ONLY_PREFIX"),
            "head missing: {}",
            &loaded[..80.min(loaded.len())]
        );
        assert!(
            !loaded.contains("TAIL_UNIQUE_ZZZ9"),
            "continuous load must not splice true tail into synthetic space"
        );
        assert!(
            !loaded.contains("[TRUNCATED]"),
            "continuous load must not inject middle-drop marker"
        );
        assert!(
            loaded.len() as u64 <= CAP + 4,
            "oversize continuous load should cap near max (len={})",
            loaded.len()
        );
        // Offsets in continuous text match original CAS prefix.
        assert_eq!(&body[..loaded.len()], loaded.as_str());
    }

    #[test]
    fn load_text_head_tail_prompt_only_still_works() {
        // Head+tail helper remains available for prompt experiments; not for verify.
        let (_tmp, base) = utf8_tempdir();
        let matter = Matter::create(base.join("m"), "head-tail").expect("create");
        const CAP: u64 = 4_000;
        let head = "HEAD_ONLY_PREFIX ".repeat(200);
        let mid = "MID_FILL_XXXX ".repeat(20_000);
        let tail = " TAIL_UNIQUE_ZZZ9 hot confidential ending";
        let body = format!("{head}{mid}{tail}");
        let digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");
        let loaded = load_text_capped(&matter, &digest, CAP).expect("load");
        assert!(loaded.contains("TAIL_UNIQUE_ZZZ9"));
        assert!(loaded.contains("[TRUNCATED]"));
    }

    #[test]
    fn continuous_verify_offsets_match_cas_not_synthetic() {
        // Regression P1-1: quote near end of continuous region must store offsets
        // into continuous text — not into a head+marker+tail reconstruction where
        // a tail quote would land at a wrong (small) index after the marker.
        let (_tmp, base) = utf8_tempdir();
        let matter = Matter::create(base.join("m"), "offset-space").expect("create");

        // ~300KB body: distinctive quote near end of first 200KB continuous region
        // (well inside 2 MiB Desk cap, past a tiny head-only half of head+tail).
        let prefix = "AAAA_FILL_".repeat(18_000); // ~180k
        let quote = "UNIQUE_CONTINUOUS_QUOTE_HOT_XYZ";
        let after = " trailing filler ".repeat(5_000); // push total > 200k
        let body = format!("{prefix}{quote}{after}");
        assert!(body.len() > 200_000);

        let digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");
        let continuous =
            load_text_continuous(&matter, &digest, MAX_VERIFY_TEXT_BYTES).expect("load");
        assert!(
            continuous.contains(quote),
            "quote must be inside continuous verify window"
        );
        let expected = continuous.find(quote).expect("pos") as i64;

        // Synthetic head+tail with a small cap would place the true tail (not our
        // mid-body quote) at a different coordinate system — prove continuous
        // verify finds the real CAS offset.
        let r = verify_citation_for_storage(quote, None, None, &continuous, true);
        assert_eq!(r.status, matter_core::VERIFY_MATCHED);
        assert_eq!(r.start_offset, Some(expected));
        assert_eq!(r.end_offset, Some(expected + quote.len() as i64));
        // Same offset in original body (continuous == body for < 2 MiB).
        assert_eq!(body.find(quote).map(|p| p as i64), Some(expected));

        // Contrast: head+tail synthetic of 4k would NOT contain this mid-body quote
        // at the same index (quote is past first 2k of a 4k head+tail load).
        let synthetic = load_text_capped(&matter, &digest, 4_000).expect("synthetic");
        assert!(
            !synthetic.contains(quote) || synthetic.find(quote) != continuous.find(quote),
            "synthetic head+tail must not share continuous mid-body offset space"
        );
    }

    #[test]
    fn e2e_suggest_continuous_body_hot_keyword() {
        // ~280k body fully fits in 2 MiB continuous window; middle_drop keeps
        // head+tail of that window for the prompt so mock still sees "hot".
        let (_tmp, base) = utf8_tempdir();
        let matter = Matter::create(base.join("m"), "e2e-cont").expect("create");
        let head = "HEAD_ONLY_PREFIX ".repeat(200);
        let mid = "MID_FILL_XXXX ".repeat(20_000);
        let tail = " TAIL_UNIQUE_ZZZ9 hot confidential ending";
        let body = format!("{head}{mid}{tail}");
        let digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");

        matter
            .update_ai_config(matter_core::UpdateAiMatterConfigInput {
                enabled: true,
                allow_remote: false,
                base_url: None,
                model: Some("mock"),
                provider_kind: Some(matter_core::AI_PROVIDER_MOCK),
            })
            .expect("ai on");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some("huge".into()),
                text_sha256: Some(digest),
                in_review: Some(1),
                ..Default::default()
            })
            .expect("item");
        let job = matter.create_job(JOB_KIND_AI_SUGGEST_CODES).expect("job");
        let params = AiSuggestCodesParams {
            max_text_bytes: 8_000,
            ..AiSuggestCodesParams::default()
        };
        let outcome = run_ai_suggest_codes(&matter, &job.id, &params, None, |_| {}).expect("run");
        match outcome {
            AiSuggestOutcome::Succeeded(r) => {
                assert!(
                    r.suggestion_rows >= 1 || r.suggested_count >= 1,
                    "tail keyword 'hot' should reach mock via continuous+middle_drop: {r:?}"
                );
            }
            other => panic!("expected Succeeded, got {other:?}"),
        }
    }

    #[test]
    fn utf8_prefix_suffix_trim_incomplete() {
        // Leading incomplete continuation + valid tail text.
        let mut tail = vec![0x80, 0x80]; // invalid as start
        tail.extend_from_slice("TAILΩ".as_bytes());
        assert_eq!(utf8_suffix(&tail), "TAILΩ");

        // Truncated multi-byte at end of head (first byte of multi-byte seq only).
        let mut head = b"HEAD".to_vec();
        head.push(0xCE); // first byte of Ω only
        assert_eq!(utf8_prefix(&head), "HEAD");
    }
}
