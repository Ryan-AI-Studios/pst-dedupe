//! Resumable `classify` job.

use std::collections::BTreeMap;
use std::time::Instant;

use matter_core::{
    category_status, classify_candidate_needs_work, ApplyClassificationInput, AuditEventInput,
    CategoryApplyResult, Matter,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::category::TAXONOMY_V1;
use crate::classify::{classify, ClassifyInput};
use crate::error::{Error, Result};
use crate::magic::MAGIC_HEAD_MAX;
use crate::params::ClassifyParams;

/// Job kind string for process-runner.
pub const JOB_KIND_CLASSIFY: &str = "classify";
/// Checkpoint stage name.
pub const CLASSIFY_STAGE: &str = "classify";

/// Summary counts after a classify run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifySummary {
    pub completed_count: u64,
    pub classified_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
    /// Counts by category string.
    #[serde(default)]
    pub by_category: BTreeMap<String, u64>,
    /// Counts by method string.
    #[serde(default)]
    pub by_method: BTreeMap<String, u64>,
}

/// Outcome of [`run_classify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyOutcome {
    Succeeded(ClassifySummary),
    Paused(ClassifySummary),
    Failed {
        message: String,
        summary: ClassifySummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    classified_count: u64,
    skipped_count: u64,
    error_count: u64,
    #[serde(default)]
    by_category: BTreeMap<String, u64>,
    #[serde(default)]
    by_method: BTreeMap<String, u64>,
    params: serde_json::Value,
}

/// Run file-category classification on `matter` for the runner-created `job_id`.
///
/// **CPU/IO-bound** — call only on the matter worker thread.
pub fn run_classify(
    matter: &Matter,
    job_id: &str,
    params: &ClassifyParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<ClassifyOutcome> {
    let started = Instant::now();
    let result = run_classify_body(matter, job_id, params, cancel, &progress);

    match &result {
        Ok(ClassifyOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "classify.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "classified_count": s.classified_count,
                    "skipped_count": s.skipped_count,
                    "error_count": s.error_count,
                    "completed_count": s.completed_count,
                    "by_category": s.by_category,
                    "by_method": s.by_method,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                // Complete audit failed: still emit classify.fail with full counts.
                let message = format!("audit complete failed: {e}");
                let _ = matter.append_audit(AuditEventInput {
                    actor: "system".into(),
                    action: "classify.fail".into(),
                    entity: format!("job:{job_id}"),
                    params_json: fail_audit_params_json(&message, s).to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                });
                return Ok(ClassifyOutcome::Failed {
                    message,
                    summary: s.clone(),
                });
            }
        }
        Ok(ClassifyOutcome::Paused(_)) => {}
        Ok(ClassifyOutcome::Failed { message, summary }) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "classify.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params_json(message, summary).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
        Err(e) => {
            let empty = ClassifySummary::default();
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "classify.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params_json(&e.to_string(), &empty).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
    }

    result
}

fn run_classify_body(
    matter: &Matter,
    job_id: &str,
    params: &ClassifyParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<ClassifyOutcome> {
    params.validate().map_err(Error::InvalidParams)?;

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(Error::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "classify.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({ "params": params_json }).to_string(),
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
    let Some(cp) = matter.get_checkpoint(job_id, CLASSIFY_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(Error::other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &ClassifyParams,
    prior: Option<&CheckpointCursor>,
) -> Result<ClassifyParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<ClassifyParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(Error::other(format!("checkpoint params unreadable: {e}")));
                }
            }
        }
    }
    Ok(call_site.clone())
}

fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &ClassifyParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<ClassifyOutcome> {
    let mut summary = ClassifySummary::default();
    // cursor_index = completed counter for display/checkpoint; listing uses last_item_id keyset.
    let mut cursor_index = 0u64;
    let mut last_item_id: Option<String> = None;
    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        last_item_id = p.last_item_id;
        summary.completed_count = p.completed_count;
        summary.classified_count = p.classified_count;
        summary.skipped_count = p.skipped_count;
        summary.error_count = p.error_count;
        summary.by_category = p.by_category;
        summary.by_method = p.by_method;
    }

    // Convert mid-run operational errors into Failed with the live summary so
    // classify.fail audit counts remain truthful after partial progress.
    let fail = |summary: ClassifySummary, e: Error| -> Result<ClassifyOutcome> {
        Ok(ClassifyOutcome::Failed {
            message: e.to_string(),
            summary,
        })
    };

    let batch = params.batch_size.max(1);
    // Keyset pagination: after_id = last processed id (not OFFSET on a shrinking set).
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                params_json,
                last_item_id.as_deref(),
            ) {
                return fail(summary, e);
            }
            progress(summary.completed_count);
            return Ok(ClassifyOutcome::Paused(summary));
        }

        let candidates = match matter.list_classify_candidates(
            last_item_id.as_deref(),
            batch as u64,
            params.force,
            params.in_review_only,
        ) {
            Ok(c) => c,
            Err(e) => {
                return fail(summary, Error::other(e.to_string()));
            }
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
                ) {
                    return fail(summary, e);
                }
                progress(summary.completed_count);
                return Ok(ClassifyOutcome::Paused(summary));
            }

            if let Err(e) = process_one(matter, &cand, params, &mut summary) {
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
            ) {
                return fail(summary, e);
            }
        }
    }

    Ok(ClassifyOutcome::Succeeded(summary))
}

/// Shared `classify.fail` audit payload (same count shape as `classify.complete` where possible).
fn fail_audit_params_json(message: &str, summary: &ClassifySummary) -> serde_json::Value {
    json!({
        "error": message,
        "completed_count": summary.completed_count,
        "classified_count": summary.classified_count,
        "skipped_count": summary.skipped_count,
        "error_count": summary.error_count,
        "by_category": summary.by_category,
        "by_method": summary.by_method,
    })
}

fn process_one(
    matter: &Matter,
    cand: &matter_core::ClassifyCandidate,
    params: &ClassifyParams,
    summary: &mut ClassifySummary,
) -> Result<()> {
    // Defense-in-depth: SQL already filters when force=false; still skip clean rows.
    if !classify_candidate_needs_work(cand, params.force) {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    // Head bytes optional (prefix only — never full CAS load for large natives).
    let head = if params.use_magic {
        if let Some(native) = cand.native_sha256.as_deref() {
            match matter.read_cas_prefix(native, MAGIC_HEAD_MAX) {
                Ok(b) if !b.is_empty() => Some(b),
                _ => None, // unreadable / empty CAS → path/mime fallback
            }
        } else {
            None
        }
    } else {
        None
    };

    // When force, do not respect extractor refine.
    let respect = params.respect_extractor_refine && !params.force;

    let result = classify(&ClassifyInput {
        path: cand.path.as_deref(),
        mime_type: cand.mime_type.as_deref(),
        role: cand.role.as_deref(),
        message_class: cand.message_class.as_deref(),
        head_bytes: head.as_deref(),
        current_category: cand.file_category.as_deref(),
        respect_extractor_refine: respect,
    });

    let apply = matter.apply_classification(ApplyClassificationInput {
        item_id: cand.id.clone(),
        force: params.force,
        category: result.category.as_str().to_string(),
        method: result.method.as_str().to_string(),
        taxonomy: TAXONOMY_V1.to_string(),
        mime_type: result.mime_type.clone(),
        status: Some(category_status::OK.to_string()),
        error: None,
    })?;

    match apply {
        CategoryApplyResult::Skipped => {
            summary.skipped_count += 1;
        }
        CategoryApplyResult::Applied { .. } => {
            summary.classified_count += 1;
            *summary
                .by_category
                .entry(result.category.as_str().to_string())
                .or_insert(0) += 1;
            *summary
                .by_method
                .entry(result.method.as_str().to_string())
                .or_insert(0) += 1;
        }
        CategoryApplyResult::Error { .. } => {
            summary.error_count += 1;
        }
    }
    summary.completed_count += 1;
    Ok(())
}

fn write_checkpoint(
    matter: &Matter,
    job_id: &str,
    cursor_index: u64,
    summary: &ClassifySummary,
    params_json: &serde_json::Value,
    last_item_id: Option<&str>,
) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index,
        last_item_id: last_item_id.map(|s| s.to_string()),
        completed_count: summary.completed_count,
        classified_count: summary.classified_count,
        skipped_count: summary.skipped_count,
        error_count: summary.error_count,
        by_category: summary.by_category.clone(),
        by_method: summary.by_method.clone(),
        params: params_json.clone(),
    };
    let json = serde_json::to_string(&cursor).map_err(|e| Error::other(e.to_string()))?;
    matter.put_checkpoint(
        job_id,
        CLASSIFY_STAGE,
        &json,
        summary.completed_count as i64,
    )?;
    Ok(())
}
