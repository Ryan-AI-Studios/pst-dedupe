//! Resumable `office_extract` job.

use std::time::Instant;

use matter_core::{
    ApplyOfficeTextInput, AuditEventInput, Matter, OfficeCandidate, OfficeExtractApplyResult,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detect;
use crate::error::{Error, Result};
use crate::extract::extract_office_catch_unwind;
use crate::limits::MAX_NATIVE_INPUT_BYTES;
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

/// Reject oversized native length before any full CAS load / extract.
///
/// Used by the job path after [`Matter::cas_len`] so hostile natives never OOM
/// the process before the limit error is recorded.
pub fn reject_oversized_native_len(len: u64) -> Result<()> {
    reject_oversized_native_len_with_max(len, MAX_NATIVE_INPUT_BYTES)
}

/// Same as [`reject_oversized_native_len`] with an injectable max (tests).
pub fn reject_oversized_native_len_with_max(len: u64, max: u64) -> Result<()> {
    if len > max {
        return Err(Error::limit(format!("native size {len} exceeds max {max}")));
    }
    Ok(())
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

            // Format filter from params (path/mime only; sniff happens in process_one).
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

/// True when a successful extract already covers this native (non-force skip).
///
/// Requires text present, source matching native, and status not `error`
/// (`ok` after extract, or `skipped` after a prior idempotent pass). Failed
/// extracts leave status=`error` without updating source so they remain retryable.
fn already_extracted_ok(cand: &OfficeCandidate, native_sha: &str, force: bool) -> bool {
    if force || cand.text_sha256.is_none() {
        return false;
    }
    if cand.office_source_native_sha256.as_deref() != Some(native_sha) {
        return false;
    }
    matches!(
        cand.office_extract_status.as_deref(),
        Some(matter_core::office_extract_status::OK)
            | Some(matter_core::office_extract_status::SKIPPED)
    )
}

/// True when we already sniffed this native as not-office (skip without re-read).
fn already_skipped_not_office(cand: &OfficeCandidate, native_sha: &str, force: bool) -> bool {
    !force
        && cand.office_extract_status.as_deref()
            == Some(matter_core::office_extract_status::SKIPPED)
        && cand.office_source_native_sha256.as_deref() == Some(native_sha)
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

    // Idempotent skip after successful extract for this native.
    if already_extracted_ok(cand, native_sha, force) {
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

    // Avoid re-loading natives already sniffed as not-office.
    if already_skipped_not_office(cand, native_sha, force) {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    // Size precheck via CAS metadata — never materialize oversized natives.
    match matter.cas_len(native_sha) {
        Ok(len) => {
            if let Err(e) = reject_oversized_native_len(len) {
                record_error(matter, &cand.id, native_sha, &e)?;
                summary.error_count += 1;
                summary.completed_count += 1;
                return Ok(());
            }
        }
        Err(e) => {
            record_error(
                matter,
                &cand.id,
                native_sha,
                &Error::Other(format!("CAS stat: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    }

    let native_bytes = match matter.get_bytes_capped(native_sha, MAX_NATIVE_INPUT_BYTES) {
        Ok(b) => b,
        Err(e) => {
            // Map size-cap failures from get_bytes_capped to limit code when possible.
            let err = {
                let msg = e.to_string();
                if msg.contains("exceeds cap") {
                    Error::limit(msg)
                } else {
                    Error::Other(format!("CAS read: {e}"))
                }
            };
            record_error(matter, &cand.id, native_sha, &err)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    // Sniff before full extract so CAS-only non-office natives can be marked
    // skipped (with source) instead of error-retrying forever.
    match detect::detect_format(
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
        Some(&native_bytes),
    ) {
        Ok(None) => {
            matter.apply_office_text(ApplyOfficeTextInput {
                item_id: cand.id.clone(),
                force: true, // bookkeeping even if prior error status
                text: None,
                method: None,
                status: Some(matter_core::office_extract_status::SKIPPED.into()),
                error: Some("not_office".into()),
                source_native_sha256: Some(native_sha.into()),
                partial: false,
                file_category: None,
                refine_file_category: false,
            })?;
            summary.skipped_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
        Ok(Some(_)) => {}
        Err(e) => {
            // Legacy / encrypted detection errors — record and continue job.
            record_error(matter, &cand.id, native_sha, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    }

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
            // Does not set office_source_native_sha256 → next non-force run retries.
            record_error(matter, &cand.id, native_sha, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
        }
    }
    Ok(())
}

fn record_error(matter: &Matter, item_id: &str, native_sha: &str, err: &Error) -> Result<()> {
    // Error path: status=error, do not claim successful source (apply leaves
    // office_source_native_sha256 untouched). `source_native_sha256` is passed
    // only for apply's native resolution fallback on skip checks — apply ignores
    // it for error UPDATE of the source column.
    matter.apply_office_text(ApplyOfficeTextInput {
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
    // Propagate item_errors persistence failures so the job can surface them.
    matter
        .record_item_error(matter_core::ItemErrorInput {
            item_id: Some(item_id.into()),
            source_id: None,
            job_id: None,
            stage: OFFICE_EXTRACT_STAGE.into(),
            code: err.code().into(),
            message: err.short_message(),
            detail: None,
        })
        .map_err(|e| Error::Other(format!("record_item_error failed: {e}")))?;
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_oversized_native_len_unit() {
        assert!(reject_oversized_native_len_with_max(10, 10).is_ok());
        let err = reject_oversized_native_len_with_max(11, 10).unwrap_err();
        assert_eq!(err.code(), "office_limit_exceeded");
    }

    #[test]
    fn already_extracted_requires_success_status_not_error() {
        let mut cand = OfficeCandidate {
            id: "i1".into(),
            path: Some("a.docx".into()),
            mime_type: None,
            native_sha256: Some("abc".into()),
            text_sha256: Some("txt".into()),
            office_source_native_sha256: Some("abc".into()),
            office_extract_status: Some("error".into()),
            file_category: None,
        };
        assert!(!already_extracted_ok(&cand, "abc", false));
        cand.office_extract_status = Some("ok".into());
        assert!(already_extracted_ok(&cand, "abc", false));
        cand.office_extract_status = Some("skipped".into());
        assert!(already_extracted_ok(&cand, "abc", false));
        assert!(!already_extracted_ok(&cand, "abc", true));
    }
}
