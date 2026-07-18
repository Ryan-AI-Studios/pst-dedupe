//! Resumable `office_extract` job.

use std::time::Instant;

use chrono::Utc;
use matter_core::{
    ApplyOfficeTextInput, AuditEventInput, Matter, OfficeCandidate, OfficeExtractApplyResult,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};
use crate::extract::extract_office_catch_unwind;
use crate::params::OfficeExtractParams;

/// Job kind string for process-runner.
pub const JOB_KIND_OFFICE_EXTRACT: &str = "office_extract";
/// Checkpoint stage name.
pub const OFFICE_EXTRACT_STAGE: &str = "office_extract";

/// Summary counts after an office extract run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeExtractSummary {
    pub completed_count: u64,
    pub extracted_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
}

/// Outcome of [`run_office_extract`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfficeExtractOutcome {
    Succeeded(OfficeExtractSummary),
    Paused(OfficeExtractSummary),
    Failed {
        message: String,
        summary: OfficeExtractSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    /// Index into the stable candidate list at last checkpoint.
    cursor_index: u64,
    /// Last fully processed item id (optional breadcrumb).
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    extracted_count: u64,
    skipped_count: u64,
    error_count: u64,
    params: serde_json::Value,
}

/// Run office extract on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between items.
/// Calls `progress(completed_count)` after each item.
pub fn run_office_extract(
    matter: &Matter,
    job_id: &str,
    params: &OfficeExtractParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<OfficeExtractOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(Error::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "office_extract.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({ "params": params_json }).to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_inner(
        matter,
        job_id,
        &effective,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(OfficeExtractOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "office_extract.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "extracted_count": s.extracted_count,
                    "skipped_count": s.skipped_count,
                    "error_count": s.error_count,
                    "completed_count": s.completed_count,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(OfficeExtractOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(OfficeExtractOutcome::Paused(_)) => {}
        Ok(OfficeExtractOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "office_extract.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "extracted_count": summary.extracted_count,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(Error::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "office_extract.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(Error::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, OFFICE_EXTRACT_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(Error::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &OfficeExtractParams,
    prior: Option<&CheckpointCursor>,
) -> Result<OfficeExtractParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<OfficeExtractParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(Error::Other(format!("checkpoint params unreadable: {e}")));
                }
            }
        }
    }
    Ok(call_site.clone())
}

fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &OfficeExtractParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<OfficeExtractOutcome> {
    let mut summary = OfficeExtractSummary::default();
    let mut cursor_index = 0u64;
    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        summary.completed_count = p.completed_count;
        summary.extracted_count = p.extracted_count;
        summary.skipped_count = p.skipped_count;
        summary.error_count = p.error_count;
    }

    // Page a **stable** office-eligible list with OFFSET = cursor_index.
    // Do not filter out already-extracted rows in SQL: a shrinking pending list
    // plus advancing OFFSET silently skips remaining candidates after successes.
    // Non-force idempotent skip is applied in process_one / apply_office_text.
    let batch = params.batch_size.max(1);
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            write_checkpoint(matter, job_id, cursor_index, &summary, params_json, None)?;
            progress(summary.completed_count);
            return Ok(OfficeExtractOutcome::Paused(summary));
        }

        let candidates = matter.list_office_candidates(cursor_index, batch as u64, params.force)?;
        if candidates.is_empty() {
            break;
        }

        for cand in candidates {
            if cancel.map(|c| c()).unwrap_or(false) {
                write_checkpoint(
                    matter,
                    job_id,
                    cursor_index,
                    &summary,
                    params_json,
                    Some(&cand.id),
                )?;
                progress(summary.completed_count);
                return Ok(OfficeExtractOutcome::Paused(summary));
            }

            // Format filter from params
            if let Some(fmt) = format_hint(&cand) {
                if !params.allows_format(fmt) {
                    summary.skipped_count += 1;
                    summary.completed_count += 1;
                    cursor_index += 1;
                    progress(summary.completed_count);
                    continue;
                }
            }

            process_one(matter, &cand, params.force, &mut summary)?;
            cursor_index += 1;
            progress(summary.completed_count);

            // Checkpoint every item (cheap) so cancel/resume is tight.
            write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                params_json,
                Some(&cand.id),
            )?;
        }
    }

    Ok(OfficeExtractOutcome::Succeeded(summary))
}

fn format_hint(cand: &OfficeCandidate) -> Option<&'static str> {
    let path = cand.path.as_deref()?;
    crate::detect::from_extension(path).map(|f| f.as_str())
}

fn process_one(
    matter: &Matter,
    cand: &OfficeCandidate,
    force: bool,
    summary: &mut OfficeExtractSummary,
) -> Result<()> {
    let Some(native_sha) = cand.native_sha256.as_deref() else {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    // Idempotent skip (also enforced in apply, but avoid CAS read when possible).
    if !force
        && cand.text_sha256.is_some()
        && cand.office_source_native_sha256.as_deref() == Some(native_sha)
    {
        let _ = matter.apply_office_text(ApplyOfficeTextInput {
            item_id: cand.id.clone(),
            force,
            text: None,
            method: None,
            status: Some(matter_core::office_extract_status::SKIPPED.into()),
            error: None,
            source_native_sha256: Some(native_sha.into()),
            partial: false,
            file_category: None,
            refine_file_category: false,
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    let native_bytes = match matter.get_bytes(native_sha) {
        Ok(b) => b,
        Err(e) => {
            record_error(
                matter,
                &cand.id,
                native_sha,
                &Error::Other(format!("CAS read: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    let extract_result = extract_office_catch_unwind(
        &native_bytes,
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
    );

    match extract_result {
        Ok(extracted) => {
            let apply = matter.apply_office_text(ApplyOfficeTextInput {
                item_id: cand.id.clone(),
                force,
                text: Some(extracted.text),
                method: Some(extracted.method),
                status: Some(matter_core::office_extract_status::OK.into()),
                error: if extracted.partial {
                    Some("truncated".into())
                } else {
                    None
                },
                source_native_sha256: Some(native_sha.into()),
                partial: extracted.partial,
                file_category: Some(extracted.format.file_category().into()),
                refine_file_category: true,
            })?;
            match apply {
                OfficeExtractApplyResult::Skipped => summary.skipped_count += 1,
                OfficeExtractApplyResult::Applied { .. } => summary.extracted_count += 1,
                OfficeExtractApplyResult::Empty { .. } => summary.error_count += 1,
                OfficeExtractApplyResult::Error { .. } => summary.error_count += 1,
            }
            summary.completed_count += 1;
        }
        Err(e) => {
            // Empty text / parse errors: bookkeeping without text CAS.
            record_error(matter, &cand.id, native_sha, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
        }
    }
    Ok(())
}

fn record_error(matter: &Matter, item_id: &str, native_sha: &str, err: &Error) -> Result<()> {
    let _ = matter.apply_office_text(ApplyOfficeTextInput {
        item_id: item_id.into(),
        force: true, // allow writing error bookkeeping even when text present
        text: None,
        method: None,
        status: Some(matter_core::office_extract_status::ERROR.into()),
        error: Some(format!("{}: {}", err.code(), err.short_message())),
        source_native_sha256: Some(native_sha.into()),
        partial: false,
        file_category: None,
        refine_file_category: false,
    })?;
    // Also accumulate item_errors for operators.
    let _ = matter.record_item_error(matter_core::ItemErrorInput {
        item_id: Some(item_id.into()),
        source_id: None,
        job_id: None,
        stage: OFFICE_EXTRACT_STAGE.into(),
        code: err.code().into(),
        message: err.short_message(),
        detail: None,
    });
    Ok(())
}

fn write_checkpoint(
    matter: &Matter,
    job_id: &str,
    cursor_index: u64,
    summary: &OfficeExtractSummary,
    params_json: &serde_json::Value,
    last_item_id: Option<&str>,
) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index,
        last_item_id: last_item_id.map(|s| s.to_string()),
        completed_count: summary.completed_count,
        extracted_count: summary.extracted_count,
        skipped_count: summary.skipped_count,
        error_count: summary.error_count,
        params: params_json.clone(),
    };
    let cursor_json = serde_json::to_string(&cursor).unwrap_or_else(|_| "{}".into());
    matter.put_checkpoint(
        job_id,
        OFFICE_EXTRACT_STAGE,
        &cursor_json,
        summary.completed_count as i64,
    )?;
    let _ = Utc::now(); // keep chrono linked for future timestamps if needed
    Ok(())
}
