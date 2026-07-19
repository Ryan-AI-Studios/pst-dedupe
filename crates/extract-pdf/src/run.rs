//! Resumable `pdf_extract` job.

use std::time::Instant;

use matter_core::{
    ApplyPdfTextInput, AuditEventInput, Matter, PdfCandidate, PdfExtractApplyResult,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detect;
use crate::error::{Error, Result};
use crate::extract::extract_pdf_catch_unwind;
use crate::limits::MAX_NATIVE_INPUT_BYTES;
use crate::params::PdfExtractParams;

/// Job kind string for process-runner.
pub const JOB_KIND_PDF_EXTRACT: &str = "pdf_extract";
/// Checkpoint stage name.
pub const PDF_EXTRACT_STAGE: &str = "pdf_extract";

/// Summary counts after a pdf extract run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfExtractSummary {
    pub completed_count: u64,
    pub extracted_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
}

/// Outcome of [`run_pdf_extract`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfExtractOutcome {
    Succeeded(PdfExtractSummary),
    Paused(PdfExtractSummary),
    Failed {
        message: String,
        summary: PdfExtractSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    extracted_count: u64,
    skipped_count: u64,
    error_count: u64,
    params: serde_json::Value,
}

/// Reject oversized native length before any full CAS load / extract.
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

/// Run PDF extract on `matter` for the runner-created `job_id`.
pub fn run_pdf_extract(
    matter: &Matter,
    job_id: &str,
    params: &PdfExtractParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<PdfExtractOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(Error::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "pdf_extract.start".into(),
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
        Ok(PdfExtractOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "pdf_extract.complete".into(),
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
                return Ok(PdfExtractOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(PdfExtractOutcome::Paused(_)) => {}
        Ok(PdfExtractOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "pdf_extract.fail".into(),
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
                action: "pdf_extract.fail".into(),
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
    let Some(cp) = matter.get_checkpoint(job_id, PDF_EXTRACT_STAGE)? else {
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
    call_site: &PdfExtractParams,
    prior: Option<&CheckpointCursor>,
) -> Result<PdfExtractParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<PdfExtractParams>(p.params.clone()) {
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
    params: &PdfExtractParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<PdfExtractOutcome> {
    let mut summary = PdfExtractSummary::default();
    let mut cursor_index = 0u64;
    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        summary.completed_count = p.completed_count;
        summary.extracted_count = p.extracted_count;
        summary.skipped_count = p.skipped_count;
        summary.error_count = p.error_count;
    }

    // Stable PDF-eligible list with OFFSET = cursor_index (never shrink pending).
    let batch = params.batch_size.max(1);
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            write_checkpoint(matter, job_id, cursor_index, &summary, params_json, None)?;
            progress(summary.completed_count);
            return Ok(PdfExtractOutcome::Paused(summary));
        }

        let candidates = matter.list_pdf_candidates(cursor_index, batch as u64, params.force)?;
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
                return Ok(PdfExtractOutcome::Paused(summary));
            }

            process_one(matter, &cand, params.force, &mut summary)?;
            cursor_index += 1;
            progress(summary.completed_count);

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

    Ok(PdfExtractOutcome::Succeeded(summary))
}

/// True when a successful terminal extract already covers this native.
fn already_extracted_ok(cand: &PdfCandidate, native_sha: &str, force: bool) -> bool {
    if force {
        return false;
    }
    if cand.pdf_source_native_sha256.as_deref() != Some(native_sha) {
        return false;
    }
    matches!(
        cand.pdf_extract_status.as_deref(),
        Some(matter_core::pdf_extract_status::OK)
            | Some(matter_core::pdf_extract_status::SKIPPED)
            | Some(matter_core::pdf_extract_status::LOW_TEXT)
            | Some(matter_core::pdf_extract_status::EMPTY)
    )
}

/// True when we already sniffed this native as not-pdf (skip without re-read).
fn already_skipped_not_pdf(cand: &PdfCandidate, native_sha: &str, force: bool) -> bool {
    !force
        && cand.pdf_extract_status.as_deref() == Some(matter_core::pdf_extract_status::SKIPPED)
        && cand.pdf_source_native_sha256.as_deref() == Some(native_sha)
}

fn process_one(
    matter: &Matter,
    cand: &PdfCandidate,
    force: bool,
    summary: &mut PdfExtractSummary,
) -> Result<()> {
    let Some(native_sha) = cand.native_sha256.as_deref() else {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    if already_extracted_ok(cand, native_sha, force) {
        let _ = matter.apply_pdf_text(ApplyPdfTextInput {
            item_id: cand.id.clone(),
            force,
            text: None,
            method: None,
            status: Some(matter_core::pdf_extract_status::SKIPPED.into()),
            error: None,
            source_native_sha256: Some(native_sha.into()),
            partial: false,
            page_count: None,
            needs_ocr: None,
            file_category: None,
            refine_file_category: false,
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    if already_skipped_not_pdf(cand, native_sha, force) {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

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

    // Sniff: meta or magic. Non-PDF → skipped with source so we don't re-read forever.
    let is_pdf = detect::detect_pdf(
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
        Some(&native_bytes),
    );
    if !is_pdf || !detect::looks_like_pdf(&native_bytes) {
        // Path said pdf but bad magic, or CAS-only non-pdf: skip with source.
        if !detect::looks_like_pdf(&native_bytes) {
            matter.apply_pdf_text(ApplyPdfTextInput {
                item_id: cand.id.clone(),
                force: true,
                text: None,
                method: None,
                status: Some(matter_core::pdf_extract_status::SKIPPED.into()),
                error: Some("pdf_not_pdf".into()),
                source_native_sha256: Some(native_sha.into()),
                partial: false,
                page_count: None,
                needs_ocr: Some(0),
                file_category: None,
                refine_file_category: false,
            })?;
            summary.skipped_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    }

    let extract_result = extract_pdf_catch_unwind(
        &native_bytes,
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
    );

    match extract_result {
        Ok(extracted) => {
            let needs_ocr = if extracted.class.needs_ocr() { 1 } else { 0 };
            let text_opt = if extracted.class == crate::extract::TextClass::Empty
                || extracted.text.is_empty()
            {
                None
            } else {
                Some(extracted.text)
            };
            let apply = matter.apply_pdf_text(ApplyPdfTextInput {
                item_id: cand.id.clone(),
                force,
                text: text_opt,
                method: Some(extracted.method),
                status: Some(extracted.class.as_status().into()),
                error: if extracted.partial {
                    Some("truncated".into())
                } else if extracted.class == crate::extract::TextClass::Empty {
                    Some("pdf_empty_text".into())
                } else {
                    None
                },
                source_native_sha256: Some(native_sha.into()),
                partial: extracted.partial,
                page_count: Some(extracted.page_count as i64),
                needs_ocr: Some(needs_ocr),
                file_category: Some("pdf".into()),
                refine_file_category: true,
            })?;
            match apply {
                PdfExtractApplyResult::Skipped => summary.skipped_count += 1,
                PdfExtractApplyResult::Applied { .. }
                | PdfExtractApplyResult::Empty { .. }
                | PdfExtractApplyResult::LowText { .. } => summary.extracted_count += 1,
                PdfExtractApplyResult::Error { .. } => summary.error_count += 1,
            }
            summary.completed_count += 1;
        }
        Err(e) => {
            record_error(matter, &cand.id, native_sha, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
        }
    }
    Ok(())
}

fn record_error(matter: &Matter, item_id: &str, native_sha: &str, err: &Error) -> Result<()> {
    matter.apply_pdf_text(ApplyPdfTextInput {
        item_id: item_id.into(),
        force: true,
        text: None,
        method: None,
        status: Some(matter_core::pdf_extract_status::ERROR.into()),
        error: Some(format!("{}: {}", err.code(), err.short_message())),
        source_native_sha256: Some(native_sha.into()),
        partial: false,
        page_count: None,
        needs_ocr: Some(0),
        file_category: None,
        refine_file_category: false,
    })?;
    matter
        .record_item_error(matter_core::ItemErrorInput {
            item_id: Some(item_id.into()),
            source_id: None,
            job_id: None,
            stage: PDF_EXTRACT_STAGE.into(),
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
    summary: &PdfExtractSummary,
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
        PDF_EXTRACT_STAGE,
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
        assert_eq!(err.code(), "pdf_limit_exceeded");
    }

    #[test]
    fn already_extracted_covers_terminal_statuses() {
        let mut cand = PdfCandidate {
            id: "i1".into(),
            path: Some("a.pdf".into()),
            mime_type: None,
            native_sha256: Some("abc".into()),
            text_sha256: Some("txt".into()),
            pdf_source_native_sha256: Some("abc".into()),
            pdf_extract_status: Some("error".into()),
            pdf_needs_ocr: 0,
            file_category: None,
        };
        assert!(!already_extracted_ok(&cand, "abc", false));
        cand.pdf_extract_status = Some("ok".into());
        assert!(already_extracted_ok(&cand, "abc", false));
        cand.pdf_extract_status = Some("low_text".into());
        assert!(already_extracted_ok(&cand, "abc", false));
        cand.pdf_extract_status = Some("empty".into());
        cand.text_sha256 = None;
        assert!(already_extracted_ok(&cand, "abc", false));
        cand.pdf_extract_status = Some("skipped".into());
        assert!(already_extracted_ok(&cand, "abc", false));
        assert!(!already_extracted_ok(&cand, "abc", true));
    }
}
