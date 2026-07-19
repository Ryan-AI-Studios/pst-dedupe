//! Resumable `ics_extract` job.

use std::time::Instant;

use matter_core::{
    compute_non_email_logical_hash, ics_extract_status, item_role, item_status,
    ApplyIcsExtractInput, AuditEventInput, IcsCandidate, IcsExtractApplyResult, ItemInput, Matter,
    NonEmailLogicalInput, LOGICAL_HASH_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detect;
use crate::error::{Error, Result};
use crate::extract::extract_ics_catch_unwind;
use crate::limits::MAX_NATIVE_INPUT_BYTES;
use crate::params::IcsExtractParams;
use crate::text::synthesize_calendar_review_text;

/// Job kind string for process-runner.
pub const JOB_KIND_ICS_EXTRACT: &str = "ics_extract";
/// Checkpoint stage name.
pub const ICS_EXTRACT_STAGE: &str = "ics_extract";

/// Summary counts after an ICS extract run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IcsExtractSummary {
    pub completed_count: u64,
    pub extracted_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
    pub child_count: u64,
}

/// Outcome of [`run_ics_extract`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IcsExtractOutcome {
    Succeeded(IcsExtractSummary),
    Paused(IcsExtractSummary),
    Failed {
        message: String,
        summary: IcsExtractSummary,
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
    #[serde(default)]
    child_count: u64,
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

/// Run ICS extract on `matter` for the runner-created `job_id`.
pub fn run_ics_extract(
    matter: &Matter,
    job_id: &str,
    params: &IcsExtractParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<IcsExtractOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(Error::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ics_extract.start".into(),
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
        Ok(IcsExtractOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ics_extract.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "extracted_count": s.extracted_count,
                    "skipped_count": s.skipped_count,
                    "error_count": s.error_count,
                    "child_count": s.child_count,
                    "completed_count": s.completed_count,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(IcsExtractOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(IcsExtractOutcome::Paused(_)) => {}
        Ok(IcsExtractOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ics_extract.fail".into(),
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
                action: "ics_extract.fail".into(),
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
    let Some(cp) = matter.get_checkpoint(job_id, ICS_EXTRACT_STAGE)? else {
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
    call_site: &IcsExtractParams,
    prior: Option<&CheckpointCursor>,
) -> Result<IcsExtractParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<IcsExtractParams>(p.params.clone()) {
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
    params: &IcsExtractParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<IcsExtractOutcome> {
    let mut summary = IcsExtractSummary::default();
    let mut cursor_index = 0u64;
    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        summary.completed_count = p.completed_count;
        summary.extracted_count = p.extracted_count;
        summary.skipped_count = p.skipped_count;
        summary.error_count = p.error_count;
        summary.child_count = p.child_count;
    }

    let batch = params.batch_size.max(1);
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            write_checkpoint(matter, job_id, cursor_index, &summary, params_json, None)?;
            progress(summary.completed_count);
            return Ok(IcsExtractOutcome::Paused(summary));
        }

        let candidates = matter.list_ics_candidates(cursor_index, batch as u64, params.force)?;
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
                return Ok(IcsExtractOutcome::Paused(summary));
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

    Ok(IcsExtractOutcome::Succeeded(summary))
}

fn already_extracted_ok(cand: &IcsCandidate, native_sha: &str, force: bool) -> bool {
    if force {
        return false;
    }
    if cand.ics_source_native_sha256.as_deref() != Some(native_sha) {
        return false;
    }
    matches!(
        cand.ics_extract_status.as_deref(),
        Some(ics_extract_status::OK) | Some(ics_extract_status::SKIPPED)
    )
}

fn process_one(
    matter: &Matter,
    cand: &IcsCandidate,
    force: bool,
    summary: &mut IcsExtractSummary,
) -> Result<()> {
    let Some(native_sha) = cand.native_sha256.as_deref() else {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    // Skip calendar children produced by a prior expansion (they are not containers).
    if cand.parent_item_id.is_some()
        && cand.file_category.as_deref() == Some("calendar")
        && already_extracted_ok(cand, native_sha, force)
    {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    if already_extracted_ok(cand, native_sha, force) {
        matter.apply_ics_extract(ApplyIcsExtractInput {
            item_id: cand.id.clone(),
            force,
            text: None,
            method: None,
            status: Some(ics_extract_status::SKIPPED.into()),
            error: None,
            source_native_sha256: Some(native_sha.into()),
            ..Default::default()
        })?;
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

    if !detect::looks_like_ics(&native_bytes)
        && !detect::detect_ics(
            cand.path.as_deref(),
            cand.mime_type.as_deref(),
            Some(&native_bytes),
        )
    {
        matter.apply_ics_extract(ApplyIcsExtractInput {
            item_id: cand.id.clone(),
            force: true,
            text: None,
            method: None,
            status: Some(ics_extract_status::SKIPPED.into()),
            error: Some("ics_not_ics".into()),
            source_native_sha256: Some(native_sha.into()),
            ..Default::default()
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    if !detect::looks_like_ics(&native_bytes) {
        matter.apply_ics_extract(ApplyIcsExtractInput {
            item_id: cand.id.clone(),
            force: true,
            text: None,
            method: None,
            status: Some(ics_extract_status::SKIPPED.into()),
            error: Some("ics_not_ics".into()),
            source_native_sha256: Some(native_sha.into()),
            ..Default::default()
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    let parsed = match extract_ics_catch_unwind(&native_bytes) {
        Ok(p) => p,
        Err(e) => {
            record_error(matter, &cand.id, native_sha, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    let parent_path = cand.path.clone().unwrap_or_else(|| "calendar.ics".into());

    if parsed.is_container {
        // Parent → archive container; children get isolated natives.
        matter.apply_ics_extract(ApplyIcsExtractInput {
            item_id: cand.id.clone(),
            force: true,
            text: None,
            method: Some(parsed.method.clone()),
            status: Some(ics_extract_status::OK.into()),
            error: None,
            source_native_sha256: Some(native_sha.into()),
            file_category: Some("archive".into()),
            refine_file_category: true,
            message_class: Some("VCALENDAR".into()),
            extra_json: Some(
                json!({
                    "ics_container": true,
                    "vevent_count": parsed.events.len(),
                    "extract_tool": "extract-calendar",
                    "extract_version": env!("CARGO_PKG_VERSION"),
                })
                .to_string(),
            ),
            ..Default::default()
        })?;

        // Ensure parent has a family so children can link (cohesion rule).
        let parent_item = matter.get_item(&cand.id)?;
        let family_id = if let Some(fid) = parent_item.family_id.clone() {
            fid
        } else {
            let fam = matter.insert_family("ics-events")?;
            matter.update_item(
                &cand.id,
                matter_core::ItemUpdate {
                    family_id: Some(Some(fam.id.clone())),
                    role: Some(Some(item_role::PARENT.into())),
                    ..Default::default()
                },
            )?;
            fam.id
        };

        for ev in &parsed.events {
            let child_native = matter.put_bytes(&ev.single_event_ics)?;
            // Produce safety: child native ≠ parent mega hash.
            debug_assert_ne!(child_native, native_sha);

            let (text, partial) = synthesize_calendar_review_text(&ev.fields);
            let text_sha = if text.is_empty() {
                None
            } else {
                Some(matter.put_bytes(text.as_bytes())?)
            };

            let leaf = ev
                .fields
                .cal_uid
                .as_deref()
                .filter(|u| !u.is_empty())
                .map(sanitize_path_component)
                .unwrap_or_else(|| format!("vevent-{}", ev.index));
            let child_path = format!("{parent_path}!/{leaf}.ics");

            let mut extra = json!({
                "extract_tool": "extract-calendar",
                "extract_version": env!("CARGO_PKG_VERSION"),
                "parent_native_sha256": native_sha,
                "vevent_index": ev.index,
            });
            if ev.fields.tz_unresolved {
                extra["cal_tz_unresolved"] = json!(1);
                if let Some(ref t) = ev.fields.unresolved_tzid {
                    extra["unresolved_tzid"] = json!(t);
                }
            }
            if let Some(ref r) = ev.fields.rrule_text {
                extra["rrule"] = json!(r);
            }
            if partial {
                extra["text_truncated"] = json!(true);
            }

            let logical = compute_non_email_logical_hash(&NonEmailLogicalInput {
                category: Some("calendar".into()),
                title: ev.fields.subject.clone(),
                author: ev.fields.cal_organizer.clone(),
                created: ev.fields.cal_start_at.clone(),
                text: Some(text.clone()),
                children_native_sha256: vec![],
            });

            let sent_at = ev.fields.cal_start_at.clone();
            let to_json = if ev.fields.attendee_addrs.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&ev.fields.attendee_addrs).unwrap_or_default())
            };

            let child = matter.insert_item(ItemInput {
                path: Some(child_path),
                native_sha256: Some(child_native.clone()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::ATTACHMENT.into()),
                parent_item_id: Some(cand.id.clone()),
                family_id: Some(family_id.clone()),
                mime_type: Some("text/calendar".into()),
                file_category: Some("calendar".into()),
                subject: ev.fields.subject.clone(),
                from_addr: ev.fields.cal_organizer.clone(),
                to_addrs_json: to_json.clone(),
                sent_at: sent_at.clone(),
                size_bytes: Some(ev.single_event_ics.len() as i64),
                text_sha256: text_sha,
                logical_hash: Some(logical),
                logical_hash_version: Some(LOGICAL_HASH_VERSION),
                message_class: ev.fields.message_class.clone(),
                cal_start_at: ev.fields.cal_start_at.clone(),
                cal_end_at: ev.fields.cal_end_at.clone(),
                cal_all_day: ev.fields.cal_all_day,
                cal_location: ev.fields.cal_location.clone(),
                cal_organizer: ev.fields.cal_organizer.clone(),
                cal_attendees_json: ev.fields.cal_attendees_json.clone(),
                cal_busy_status: ev.fields.cal_busy_status.clone(),
                cal_is_recurring: ev.fields.cal_is_recurring,
                cal_recurrence_id: ev.fields.cal_recurrence_id.clone(),
                cal_uid: ev.fields.cal_uid.clone(),
                cal_extract_method: ev.fields.cal_extract_method.clone(),
                extra_json: Some(extra.to_string()),
                ..Default::default()
            })?;

            // Mark child as successfully extracted for this native.
            matter.apply_ics_extract(ApplyIcsExtractInput {
                item_id: child.id,
                force: true,
                text: None,
                method: Some(parsed.method.clone()),
                status: Some(ics_extract_status::OK.into()),
                source_native_sha256: Some(child_native),
                refine_file_category: false,
                ..Default::default()
            })?;
            summary.child_count += 1;
        }
        summary.extracted_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    // Single-event: apply fields onto the leaf item; may keep original native.
    let ev = &parsed.events[0];
    let (text, partial) = synthesize_calendar_review_text(&ev.fields);
    let mut extra = json!({
        "extract_tool": "extract-calendar",
        "extract_version": env!("CARGO_PKG_VERSION"),
    });
    if ev.fields.tz_unresolved {
        extra["cal_tz_unresolved"] = json!(1);
        if let Some(ref t) = ev.fields.unresolved_tzid {
            extra["unresolved_tzid"] = json!(t);
        }
    }
    if let Some(ref r) = ev.fields.rrule_text {
        extra["rrule"] = json!(r);
    }
    if partial {
        extra["text_truncated"] = json!(true);
    }

    let to_json = if ev.fields.attendee_addrs.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&ev.fields.attendee_addrs).unwrap_or_default())
    };
    let sent = ev.fields.cal_start_at.clone();

    let apply = matter.apply_ics_extract(ApplyIcsExtractInput {
        item_id: cand.id.clone(),
        force,
        text: if text.is_empty() { None } else { Some(text) },
        method: Some(parsed.method.clone()),
        status: Some(ics_extract_status::OK.into()),
        error: if partial {
            Some("truncated".into())
        } else {
            None
        },
        source_native_sha256: Some(native_sha.into()),
        file_category: Some("calendar".into()),
        refine_file_category: true,
        message_class: ev.fields.message_class.clone(),
        cal_start_at: ev.fields.cal_start_at.clone(),
        cal_end_at: ev.fields.cal_end_at.clone(),
        cal_all_day: ev.fields.cal_all_day,
        cal_location: ev.fields.cal_location.clone(),
        cal_organizer: ev.fields.cal_organizer.clone(),
        cal_attendees_json: ev.fields.cal_attendees_json.clone(),
        cal_busy_status: ev.fields.cal_busy_status.clone(),
        cal_is_recurring: ev.fields.cal_is_recurring,
        cal_recurrence_id: ev.fields.cal_recurrence_id.clone(),
        cal_uid: ev.fields.cal_uid.clone(),
        cal_extract_method: ev.fields.cal_extract_method.clone(),
        subject: ev.fields.subject.clone(),
        from_addr: ev.fields.cal_organizer.clone(),
        to_addrs_json: to_json,
        sent_at: sent,
        extra_json: Some(extra.to_string()),
    })?;

    match apply {
        IcsExtractApplyResult::Skipped => summary.skipped_count += 1,
        IcsExtractApplyResult::Applied { .. } => summary.extracted_count += 1,
        IcsExtractApplyResult::Error { .. } => summary.error_count += 1,
    }
    summary.completed_count += 1;
    Ok(())
}

fn sanitize_path_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "event".into()
    } else {
        // Cap length for path sanity
        out.chars().take(80).collect()
    }
}

fn record_error(matter: &Matter, item_id: &str, native_sha: &str, err: &Error) -> Result<()> {
    matter.apply_ics_extract(ApplyIcsExtractInput {
        item_id: item_id.into(),
        force: true,
        text: None,
        method: None,
        status: Some(ics_extract_status::ERROR.into()),
        error: Some(format!("{}: {}", err.code(), err.short_message())),
        source_native_sha256: Some(native_sha.into()),
        ..Default::default()
    })?;
    matter
        .record_item_error(matter_core::ItemErrorInput {
            item_id: Some(item_id.into()),
            source_id: None,
            job_id: None,
            stage: ICS_EXTRACT_STAGE.into(),
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
    summary: &IcsExtractSummary,
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
        child_count: summary.child_count,
        params: params_json.clone(),
    };
    let cursor_json = serde_json::to_string(&cursor).unwrap_or_else(|_| "{}".into());
    matter.put_checkpoint(
        job_id,
        ICS_EXTRACT_STAGE,
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
        assert_eq!(err.code(), "ics_limit_exceeded");
    }
}
