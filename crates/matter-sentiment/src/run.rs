//! Resumable `sentiment` job: fingerprint-aware skip / threshold relabel / full rescore.

use std::time::Instant;

use matter_core::{
    AuditEventInput, ClearItemSentimentInput, Matter, RelabelItemSentimentInput,
    SentimentCandidate, WriteItemSentimentInput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::aggregate::{aggregate_units, polarity_from_compound};
use crate::error::{Result, SentimentError};
use crate::method::METHOD_VADER_LEXICON_V1;
use crate::params::SentimentParams;
use crate::prep::strip_headers_and_disclaimers;
use crate::score::score_unit;
use crate::units::split_units;

/// Job kind string for process-runner.
pub const JOB_KIND_SENTIMENT: &str = "sentiment";
/// Checkpoint stage name.
pub const SENTIMENT_STAGE: &str = "sentiment";

/// Summary after a sentiment run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentimentSummary {
    pub completed_count: u64,
    pub scanned_count: u64,
    pub skipped_count: u64,
    pub relabeled_count: u64,
    pub error_count: u64,
    pub unscored_count: u64,
}

/// Full success payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SentimentReport {
    pub completed_count: u64,
    pub scanned_count: u64,
    pub skipped_count: u64,
    pub relabeled_count: u64,
    pub error_count: u64,
    pub unscored_count: u64,
    pub method: String,
    pub pos_threshold: f64,
    pub neg_threshold: f64,
}

/// Outcome of [`run_sentiment`].
#[derive(Debug, Clone, PartialEq)]
pub enum SentimentOutcome {
    Succeeded(SentimentReport),
    Paused(SentimentSummary),
    Failed {
        message: String,
        summary: SentimentSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    scanned_count: u64,
    skipped_count: u64,
    #[serde(default)]
    relabeled_count: u64,
    error_count: u64,
    #[serde(default)]
    unscored_count: u64,
    /// When true, matter-wide reset wipe already applied.
    #[serde(default)]
    reset_done: bool,
    params: serde_json::Value,
}

/// Run offline sentiment scoring on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between items.
///
/// **Idempotency (`reset: false`):**
/// - Skip (scored): text digest + VADER method + compound present + thresholds match.
/// - Skip (unscored-empty): text digest match + polarity/compound/method all NULL
///   (prior empty-after-strip attempt fingerprint).
/// - Relabel (no CAS): text + method + compound match but thresholds differ.
/// - Full rescore when text differs, method differs, or `reset`.
/// - Empty after strip / empty units / no aggregate: **clear** prior scores to NULL
///   and fingerprint the digest (unscored ≠ neutral).
pub fn run_sentiment(
    matter: &Matter,
    job_id: &str,
    params: &SentimentParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<SentimentOutcome> {
    let started = Instant::now();
    let result = run_body(matter, job_id, params, cancel, &progress);

    match &result {
        Ok(SentimentOutcome::Succeeded(r)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "sentiment.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "scanned_count": r.scanned_count,
                    "skipped_count": r.skipped_count,
                    "relabeled_count": r.relabeled_count,
                    "error_count": r.error_count,
                    "unscored_count": r.unscored_count,
                    "completed_count": r.completed_count,
                    "method": r.method,
                    "pos_threshold": r.pos_threshold,
                    "neg_threshold": r.neg_threshold,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                let message = format!("audit complete failed: {e}");
                let summary = summary_from_report(r);
                let _ = matter.append_audit(AuditEventInput {
                    actor: "system".into(),
                    action: "sentiment.fail".into(),
                    entity: format!("job:{job_id}"),
                    params_json: fail_audit_params(&message, &summary).to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                });
                return Ok(SentimentOutcome::Failed { message, summary });
            }
        }
        Ok(SentimentOutcome::Paused(_)) => {}
        Ok(SentimentOutcome::Failed { message, summary }) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "sentiment.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(message, summary).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
        Err(e) => {
            let empty = SentimentSummary::default();
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "sentiment.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(&e.to_string(), &empty).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
    }

    result
}

fn summary_from_report(r: &SentimentReport) -> SentimentSummary {
    SentimentSummary {
        completed_count: r.completed_count,
        scanned_count: r.scanned_count,
        skipped_count: r.skipped_count,
        relabeled_count: r.relabeled_count,
        error_count: r.error_count,
        unscored_count: r.unscored_count,
    }
}

fn fail_audit_params(message: &str, summary: &SentimentSummary) -> serde_json::Value {
    json!({
        "error": message,
        "completed_count": summary.completed_count,
        "scanned_count": summary.scanned_count,
        "skipped_count": summary.skipped_count,
        "relabeled_count": summary.relabeled_count,
        "error_count": summary.error_count,
        "unscored_count": summary.unscored_count,
    })
}

fn run_body(
    matter: &Matter,
    job_id: &str,
    params: &SentimentParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<SentimentOutcome> {
    params.validate()?;

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate()?;
    let params_json = serde_json::to_value(&effective)
        .map_err(|e| SentimentError::other(format!("serialize sentiment params: {e}")))?;

    let resuming = prior.as_ref().is_some_and(|p| p.completed_count > 0);
    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "sentiment.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "resume": resuming,
            "method": METHOD_VADER_LEXICON_V1,
            "pos_threshold": effective.pos_threshold,
            "neg_threshold": effective.neg_threshold,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    run_inner(
        matter,
        job_id,
        &effective,
        cancel,
        progress,
        &params_json,
        prior,
    )
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, SENTIMENT_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(SentimentError::other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &SentimentParams,
    prior: Option<&CheckpointCursor>,
) -> Result<SentimentParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<SentimentParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(SentimentError::other(format!(
                        "checkpoint params unreadable: {e}"
                    )));
                }
            }
        }
    }
    Ok(call_site.clone())
}

fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &SentimentParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<SentimentOutcome> {
    let mut summary = SentimentSummary::default();
    let mut cursor_index = 0u64;
    let mut last_item_id: Option<String> = None;
    let mut reset_done = false;

    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        last_item_id = p.last_item_id;
        summary.completed_count = p.completed_count;
        summary.scanned_count = p.scanned_count;
        summary.skipped_count = p.skipped_count;
        summary.relabeled_count = p.relabeled_count;
        summary.error_count = p.error_count;
        summary.unscored_count = p.unscored_count;
        reset_done = p.reset_done;
    }

    let fail = |summary: SentimentSummary, e: SentimentError| -> Result<SentimentOutcome> {
        Ok(SentimentOutcome::Failed {
            message: e.to_string(),
            summary,
        })
    };

    if params.reset && !reset_done {
        if let Err(e) = matter.clear_sentiment_for_matter() {
            return fail(summary, e.into());
        }
        reset_done = true;
        if let Err(e) = write_checkpoint(
            matter,
            job_id,
            cursor_index,
            &summary,
            params_json,
            last_item_id.as_deref(),
            reset_done,
        ) {
            return fail(summary, e);
        }
    }

    let batch = params.batch_size.max(1) as u64;
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                params_json,
                last_item_id.as_deref(),
                reset_done,
            ) {
                return fail(summary, e);
            }
            progress(summary.completed_count);
            return Ok(SentimentOutcome::Paused(summary));
        }

        let candidates = match matter.list_sentiment_candidates(last_item_id.as_deref(), batch) {
            Ok(c) => c,
            Err(e) => return fail(summary, e.into()),
        };
        if candidates.is_empty() {
            break;
        }

        for cand in candidates {
            if cancel.map(|c| c()).unwrap_or(false) {
                if let Err(e) = write_checkpoint(
                    matter,
                    job_id,
                    cursor_index,
                    &summary,
                    params_json,
                    last_item_id.as_deref(),
                    reset_done,
                ) {
                    return fail(summary, e);
                }
                progress(summary.completed_count);
                return Ok(SentimentOutcome::Paused(summary));
            }

            if let Err(e) = process_one(matter, job_id, &cand, params, &mut summary) {
                return fail(summary, e);
            }
            cursor_index += 1;
            last_item_id = Some(cand.id.clone());
            progress(summary.completed_count);
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                params_json,
                last_item_id.as_deref(),
                reset_done,
            ) {
                return fail(summary, e);
            }
        }
    }

    Ok(SentimentOutcome::Succeeded(SentimentReport {
        completed_count: summary.completed_count,
        scanned_count: summary.scanned_count,
        skipped_count: summary.skipped_count,
        relabeled_count: summary.relabeled_count,
        error_count: summary.error_count,
        unscored_count: summary.unscored_count,
        method: METHOD_VADER_LEXICON_V1.into(),
        pos_threshold: params.pos_threshold,
        neg_threshold: params.neg_threshold,
    }))
}

fn thresholds_match(cand: &SentimentCandidate, params: &SentimentParams) -> bool {
    match (cand.sentiment_pos_threshold, cand.sentiment_neg_threshold) {
        (Some(p), Some(n)) => {
            // Exact match for snapshotted params (serde-stable f64 constants).
            p == params.pos_threshold && n == params.neg_threshold
        }
        _ => false,
    }
}

/// Body digest equals last scanned digest (both non-empty after trim).
fn text_digest_matches(cand: &SentimentCandidate) -> bool {
    let text = cand
        .text_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let scanned = cand
        .sentiment_scanned_text_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (text, scanned) {
        (Some(t), Some(s)) => t == s,
        _ => false,
    }
}

/// Full scored fingerprint: text match + VADER method + compound present.
fn scored_fingerprint_match(cand: &SentimentCandidate) -> bool {
    text_digest_matches(cand)
        && cand.sentiment_method.as_deref() == Some(METHOD_VADER_LEXICON_V1)
        && cand.sentiment_compound.is_some()
}

/// Empty-attempt fingerprint: same text already decided unscored (no scores).
fn unscored_fingerprint_match(cand: &SentimentCandidate) -> bool {
    text_digest_matches(cand)
        && cand.sentiment_polarity.is_none()
        && cand.sentiment_compound.is_none()
        && cand.sentiment_method.is_none()
}

/// Mark item unscored, clearing any prior scores; fingerprint when digest known.
fn mark_unscored(
    matter: &Matter,
    job_id: &str,
    item_id: &str,
    digest: Option<&str>,
    summary: &mut SentimentSummary,
) -> Result<()> {
    let scanned_at = Matter::sentiment_scan_now();
    matter.clear_item_sentiment(ClearItemSentimentInput {
        item_id,
        scanned_text_sha256: digest,
        job_id: Some(job_id),
        scanned_at: Some(&scanned_at),
    })?;
    summary.unscored_count += 1;
    summary.completed_count += 1;
    Ok(())
}

fn process_one(
    matter: &Matter,
    job_id: &str,
    cand: &SentimentCandidate,
    params: &SentimentParams,
    summary: &mut SentimentSummary,
) -> Result<()> {
    if !params.reset {
        // Full skip (scored path): text + method + compound + thresholds.
        if scored_fingerprint_match(cand) && thresholds_match(cand, params) {
            summary.skipped_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
        // Unscored skip: already decided empty/unscored for this text digest.
        if unscored_fingerprint_match(cand) {
            summary.skipped_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
        // Relabel only when compound present, method match, thresholds differ.
        if scored_fingerprint_match(cand) && !thresholds_match(cand, params) {
            if let Some(compound) = cand.sentiment_compound {
                let polarity =
                    polarity_from_compound(compound, params.pos_threshold, params.neg_threshold);
                let scanned_at = Matter::sentiment_scan_now();
                matter.relabel_item_sentiment(RelabelItemSentimentInput {
                    item_id: &cand.id,
                    polarity,
                    pos_threshold: params.pos_threshold,
                    neg_threshold: params.neg_threshold,
                    job_id: Some(job_id),
                    scanned_at: &scanned_at,
                })?;
                summary.relabeled_count += 1;
                summary.completed_count += 1;
                return Ok(());
            }
            // Missing compound despite scored match → fall through to full rescore.
        }
    }

    let Some(digest) = cand
        .text_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        // No body digest — clear prior scores if any; leave unscored.
        return mark_unscored(matter, job_id, &cand.id, None, summary);
    };

    let text = match load_text_capped(matter, digest, params.max_text_bytes) {
        Ok(t) => t,
        Err(e) => {
            let _ = matter.record_item_error(matter_core::ItemErrorInput {
                item_id: Some(cand.id.clone()),
                source_id: None,
                job_id: Some(job_id.to_string()),
                stage: SENTIMENT_STAGE.into(),
                code: "sentiment_text_load".into(),
                message: e.to_string(),
                detail: None,
            });
            // Fail-closed: clear prior scores so filters/Desk do not show tone for
            // text that was not successfully scored. Do **not** fingerprint the
            // failed digest as a successful empty-attempt (retry on next run).
            let scanned_at = Matter::sentiment_scan_now();
            matter.clear_item_sentiment(ClearItemSentimentInput {
                item_id: &cand.id,
                scanned_text_sha256: None,
                job_id: Some(job_id),
                scanned_at: Some(&scanned_at),
            })?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    let prepared = strip_headers_and_disclaimers(&text);
    if prepared.trim().is_empty() {
        // Empty after strip — clear prior scores; fingerprint empty-attempt.
        return mark_unscored(matter, job_id, &cand.id, Some(digest), summary);
    }

    let unit_texts = split_units(&prepared, params.max_units);
    if unit_texts.is_empty() {
        return mark_unscored(matter, job_id, &cand.id, Some(digest), summary);
    }

    let unit_scores: Vec<_> = unit_texts.iter().map(|u| score_unit(u)).collect();
    let Some(agg) = aggregate_units(&unit_scores, params.pos_threshold, params.neg_threshold)
    else {
        return mark_unscored(matter, job_id, &cand.id, Some(digest), summary);
    };

    let scanned_at = Matter::sentiment_scan_now();
    matter.write_item_sentiment(WriteItemSentimentInput {
        item_id: &cand.id,
        compound: agg.compound,
        compound_min: agg.compound_min,
        compound_max: agg.compound_max,
        pos: agg.pos,
        neu: agg.neu,
        neg: agg.neg,
        polarity: agg.polarity,
        method: METHOD_VADER_LEXICON_V1,
        pos_threshold: params.pos_threshold,
        neg_threshold: params.neg_threshold,
        scanned_text_sha256: digest,
        job_id: Some(job_id),
        scanned_at: &scanned_at,
    })?;

    summary.scanned_count += 1;
    summary.completed_count += 1;
    Ok(())
}

/// Load CAS text, truncating to `max_bytes` when larger.
fn load_text_capped(matter: &Matter, digest: &str, max_bytes: u64) -> Result<String> {
    match matter.get_bytes_capped(digest, max_bytes) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(matter_core::Error::Other(msg)) if msg.contains("exceeds cap") => {
            let mut file = matter.open_read(digest)?;
            let mut buf = vec![0u8; max_bytes as usize];
            use std::io::Read;
            let n = file.read(&mut buf).map_err(matter_core::Error::from)?;
            buf.truncate(n);
            Ok(String::from_utf8_lossy(&buf).into_owned())
        }
        Err(e) => Err(e.into()),
    }
}

fn write_checkpoint(
    matter: &Matter,
    job_id: &str,
    cursor_index: u64,
    summary: &SentimentSummary,
    params_json: &serde_json::Value,
    last_item_id: Option<&str>,
    reset_done: bool,
) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index,
        last_item_id: last_item_id.map(|s| s.to_string()),
        completed_count: summary.completed_count,
        scanned_count: summary.scanned_count,
        skipped_count: summary.skipped_count,
        relabeled_count: summary.relabeled_count,
        error_count: summary.error_count,
        unscored_count: summary.unscored_count,
        reset_done,
        params: params_json.clone(),
    };
    let json = serde_json::to_string(&cursor).map_err(|e| SentimentError::other(e.to_string()))?;
    matter.put_checkpoint(
        job_id,
        SENTIMENT_STAGE,
        &json,
        summary.completed_count as i64,
    )?;
    Ok(())
}
