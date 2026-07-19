//! Resumable `ics_extract` job.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use matter_core::{
    compute_non_email_logical_hash, ics_extract_status, item_role, item_status,
    ApplyIcsExtractInput, AuditEventInput, IcsCandidate, IcsExtractApplyResult, Item, ItemInput,
    ItemUpdate, Matter, NonEmailLogicalInput, LOGICAL_HASH_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detect;
use crate::error::{Error, Result};
use crate::extract::{extract_ics_catch_unwind, ParsedIcs, ParsedVEvent};
use crate::limits::{MAX_NATIVE_INPUT_BYTES, MAX_SINGLE_EVENT_NATIVE_BYTES};
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

/// True when a multi-event container has a full set of isolated calendar children.
///
/// A child counts when it is `file_category=calendar`, has a native digest that
/// is not the parent mega-file, and is at a successful terminal for that native.
fn multi_expansion_complete(
    matter: &Matter,
    parent_id: &str,
    parent_native: &str,
    expected_vevents: usize,
) -> Result<bool> {
    if expected_vevents == 0 {
        return Ok(true);
    }
    let children = matter.list_attachments(parent_id)?;
    let mut complete_paths: HashSet<String> = HashSet::new();
    for c in children {
        if c.file_category.as_deref() != Some("calendar") {
            continue;
        }
        let Some(child_native) = c.native_sha256.as_deref() else {
            continue;
        };
        if child_native == parent_native {
            continue;
        }
        let child_ok = matches!(
            c.ics_extract_status.as_deref(),
            Some(ics_extract_status::OK) | Some(ics_extract_status::SKIPPED)
        );
        let source_matches = c.ics_source_native_sha256.as_deref() == Some(child_native);
        if child_ok && source_matches {
            if let Some(path) = c.path {
                complete_paths.insert(path);
            } else {
                // Path-less complete child still contributes to count uniquely by id.
                complete_paths.insert(c.id);
            }
        }
    }
    Ok(complete_paths.len() == expected_vevents)
}

fn vevent_count_from_extra_json(extra_json: Option<&str>) -> Option<usize> {
    let raw = extra_json?;
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    v.get("vevent_count")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
}

/// Whether a terminal parent might be an incomplete multi-event expansion that
/// must be resumed (legacy bug: parent marked ok before children finished).
fn should_resume_incomplete_container(
    matter: &Matter,
    cand: &IcsCandidate,
    native_sha: &str,
) -> Result<bool> {
    // Only archive containers are multi-event parents in this model.
    if cand.file_category.as_deref() != Some("archive") {
        return Ok(false);
    }
    let item = matter.get_item(&cand.id)?;
    let expected = vevent_count_from_extra_json(item.extra_json.as_deref());
    match expected {
        Some(n) => Ok(!multi_expansion_complete(matter, &cand.id, native_sha, n)?),
        // Missing vevent_count — re-parse / re-expand to be safe.
        None => Ok(true),
    }
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
        // Incomplete multi-event expansions must resume even if a prior run
        // incorrectly marked the parent terminal before all children existed.
        if should_resume_incomplete_container(matter, cand, native_sha)? {
            // Fall through to CAS load + re-expand (upsert missing children).
        } else {
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
        expand_multi_event_container(
            matter,
            cand,
            native_sha,
            &parent_path,
            &parsed,
            force,
            summary,
        )?;
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
    if ev.fields.tz_ambiguous {
        extra["cal_tz_ambiguous"] = json!(1);
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

/// Expand a multi-VEVENT ICS into archive parent + one isolated child per event.
///
/// **Terminal parent status (`ok`) is applied only after all children are fully
/// created** (native CAS + item + ics bookkeeping). Resume creates missing
/// children by stable path; force upserts by path and detaches extras so
/// re-runs do not duplicate.
fn expand_multi_event_container(
    matter: &Matter,
    cand: &IcsCandidate,
    native_sha: &str,
    parent_path: &str,
    parsed: &ParsedIcs,
    force: bool,
    summary: &mut IcsExtractSummary,
) -> Result<()> {
    let vevent_count = parsed.events.len();
    let parent_extra = json!({
        "ics_container": true,
        "vevent_count": vevent_count,
        "extract_tool": "extract-calendar",
        "extract_version": env!("CARGO_PKG_VERSION"),
    })
    .to_string();

    // Intermediate parent bookkeeping only — do **not** set successful terminal
    // until every child is committed (crash/resume safety).
    matter.update_item(
        &cand.id,
        ItemUpdate {
            file_category: Some(Some("archive".into())),
            message_class: Some(Some("VCALENDAR".into())),
            extra_json: Some(Some(parent_extra.clone())),
            ..Default::default()
        },
    )?;

    // Ensure parent has a family so children can link (cohesion rule).
    let parent_item = matter.get_item(&cand.id)?;
    let family_id = if let Some(fid) = parent_item.family_id.clone() {
        fid
    } else {
        let fam = matter.insert_family("ics-events")?;
        matter.update_item(
            &cand.id,
            ItemUpdate {
                family_id: Some(Some(fam.id.clone())),
                role: Some(Some(item_role::PARENT.into())),
                ..Default::default()
            },
        )?;
        fam.id
    };

    // Index existing children by stable path; collect path-duplicate extras.
    let existing = matter.list_attachments(&cand.id)?;
    let mut by_path: HashMap<String, Item> = HashMap::new();
    let mut orphan_ids: Vec<String> = Vec::new();
    for child in existing {
        match child.path.clone() {
            Some(path) => match by_path.entry(path) {
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(child);
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    // Duplicate path from a prior force bug — detach later.
                    orphan_ids.push(child.id);
                }
            },
            None => orphan_ids.push(child.id),
        }
    }

    let mut expected_paths: HashSet<String> = HashSet::new();
    // Track leafs reserved by this expansion so sanitize collisions get unique paths.
    let mut reserved_leafs: HashSet<String> = HashSet::new();

    for ev in &parsed.events {
        let child_path = unique_child_path(parent_path, ev, &mut reserved_leafs);
        expected_paths.insert(child_path.clone());

        // Resume: keep complete children unless force rewrites them.
        if !force {
            if let Some(existing_child) = by_path.get(&child_path) {
                if child_is_complete(existing_child, native_sha) {
                    continue;
                }
            }
        }

        if let Err(e) = upsert_container_child(
            matter,
            cand,
            native_sha,
            &family_id,
            parsed,
            ev,
            &child_path,
            by_path.get(&child_path),
        ) {
            // Item error is retryable (status=error, source not terminal-locked).
            record_error(matter, &cand.id, native_sha, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
        summary.child_count += 1;
    }

    // Force (or path-duplicate cleanup): detach children not part of this expansion.
    // Always detach path duplicates; on force also detach unexpected paths.
    for (path, child) in &by_path {
        if force && !expected_paths.contains(path) {
            orphan_ids.push(child.id.clone());
        }
    }
    for id in orphan_ids {
        matter.update_item(
            &id,
            ItemUpdate {
                parent_item_id: Some(None),
                role: Some(Some(item_role::STANDALONE.into())),
                ..Default::default()
            },
        )?;
    }

    // Verify full expansion before claiming parent terminal success.
    if !multi_expansion_complete(matter, &cand.id, native_sha, vevent_count)? {
        // Should not happen after upsert loop; surface as error for retry.
        record_error(
            matter,
            &cand.id,
            native_sha,
            &Error::Other(format!(
                "multi-event expansion incomplete: expected {vevent_count} children"
            )),
        )?;
        summary.error_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    // Terminal parent ok only after all children exist.
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
        extra_json: Some(parent_extra),
        ..Default::default()
    })?;

    summary.extracted_count += 1;
    summary.completed_count += 1;
    Ok(())
}

fn child_is_complete(child: &Item, parent_native: &str) -> bool {
    if child.file_category.as_deref() != Some("calendar") {
        return false;
    }
    let Some(child_native) = child.native_sha256.as_deref() else {
        return false;
    };
    if child_native == parent_native {
        return false;
    }
    let status_ok = matches!(
        child.ics_extract_status.as_deref(),
        Some(ics_extract_status::OK) | Some(ics_extract_status::SKIPPED)
    );
    status_ok && child.ics_source_native_sha256.as_deref() == Some(child_native)
}

#[allow(clippy::too_many_arguments)]
fn upsert_container_child(
    matter: &Matter,
    cand: &IcsCandidate,
    native_sha: &str,
    family_id: &str,
    parsed: &ParsedIcs,
    ev: &ParsedVEvent,
    child_path: &str,
    existing: Option<&Item>,
) -> Result<()> {
    reject_oversized_single_event_native(ev.single_event_ics.len())?;
    let child_native = matter.put_bytes(&ev.single_event_ics)?;
    // Produce safety: child native ≠ parent mega hash.
    debug_assert_ne!(child_native, native_sha);

    let (text, partial) = synthesize_calendar_review_text(&ev.fields);
    let text_sha = if text.is_empty() {
        None
    } else {
        Some(matter.put_bytes(text.as_bytes())?)
    };

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
    if ev.fields.tz_ambiguous {
        extra["cal_tz_ambiguous"] = json!(1);
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

    let child_id = if let Some(existing_child) = existing {
        matter.update_item(
            &existing_child.id,
            ItemUpdate {
                path: Some(Some(child_path.into())),
                native_sha256: Some(Some(child_native.clone())),
                status: Some(item_status::EXTRACTED.into()),
                role: Some(Some(item_role::ATTACHMENT.into())),
                parent_item_id: Some(Some(cand.id.clone())),
                family_id: Some(Some(family_id.into())),
                mime_type: Some(Some("text/calendar".into())),
                file_category: Some(Some("calendar".into())),
                subject: Some(ev.fields.subject.clone()),
                from_addr: Some(ev.fields.cal_organizer.clone()),
                to_addrs_json: Some(to_json.clone()),
                sent_at: Some(sent_at.clone()),
                size_bytes: Some(Some(ev.single_event_ics.len() as i64)),
                text_sha256: Some(text_sha),
                logical_hash: Some(Some(logical)),
                logical_hash_version: Some(LOGICAL_HASH_VERSION),
                message_class: Some(ev.fields.message_class.clone()),
                cal_start_at: Some(ev.fields.cal_start_at.clone()),
                cal_end_at: Some(ev.fields.cal_end_at.clone()),
                cal_all_day: Some(ev.fields.cal_all_day),
                cal_location: Some(ev.fields.cal_location.clone()),
                cal_organizer: Some(ev.fields.cal_organizer.clone()),
                cal_attendees_json: Some(ev.fields.cal_attendees_json.clone()),
                cal_busy_status: Some(ev.fields.cal_busy_status.clone()),
                cal_is_recurring: Some(ev.fields.cal_is_recurring),
                cal_recurrence_id: Some(ev.fields.cal_recurrence_id.clone()),
                cal_uid: Some(ev.fields.cal_uid.clone()),
                cal_extract_method: Some(ev.fields.cal_extract_method.clone()),
                extra_json: Some(Some(extra.to_string())),
                ..Default::default()
            },
        )?;
        existing_child.id.clone()
    } else {
        let child = matter.insert_item(ItemInput {
            path: Some(child_path.into()),
            native_sha256: Some(child_native.clone()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            parent_item_id: Some(cand.id.clone()),
            family_id: Some(family_id.into()),
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
        child.id
    };

    matter.apply_ics_extract(ApplyIcsExtractInput {
        item_id: child_id,
        force: true,
        text: None,
        method: Some(parsed.method.clone()),
        status: Some(ics_extract_status::OK.into()),
        source_native_sha256: Some(child_native),
        refine_file_category: false,
        ..Default::default()
    })?;
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

/// Deterministic unique child path per VEVENT.
///
/// Sanitization is non-injective (`a/b` and `a:b` both become `a_b`). When the
/// sanitized leaf collides with one already reserved in this expansion, append
/// a short digest of the raw UID (or the event index) so each VEVENT stays unique
/// and resume-stable.
fn unique_child_path(
    parent_path: &str,
    ev: &ParsedVEvent,
    reserved_leafs: &mut HashSet<String>,
) -> String {
    let raw_uid = ev.fields.cal_uid.as_deref().filter(|u| !u.is_empty());
    let base = raw_uid
        .map(sanitize_path_component)
        .unwrap_or_else(|| format!("vevent-{}", ev.index));

    let mut leaf = base.clone();
    if reserved_leafs.contains(&leaf) {
        let disambig = match raw_uid {
            Some(uid) => short_digest_hex(uid.as_bytes()),
            None => format!("{}", ev.index),
        };
        leaf = format!("{base}-{disambig}");
        if reserved_leafs.contains(&leaf) {
            leaf = format!("{base}-{disambig}-{}", ev.index);
        }
    }
    reserved_leafs.insert(leaf.clone());
    format!("{parent_path}!/{leaf}.ics")
}

fn short_digest_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let dig = Sha256::digest(bytes);
    // First 8 hex chars of sha256 — enough for path disambiguation.
    dig.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// Reject oversized single-event native before CAS put.
pub fn reject_oversized_single_event_native(len: usize) -> Result<()> {
    reject_oversized_single_event_native_with_max(len, MAX_SINGLE_EVENT_NATIVE_BYTES)
}

/// Injectable max for tests.
pub fn reject_oversized_single_event_native_with_max(len: usize, max: usize) -> Result<()> {
    if len > max {
        return Err(Error::limit(format!(
            "single-event native size {len} exceeds max {max}"
        )));
    }
    Ok(())
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

    #[test]
    fn reject_oversized_single_event_native_unit() {
        assert!(reject_oversized_single_event_native_with_max(5, 5).is_ok());
        let err = reject_oversized_single_event_native_with_max(6, 5).unwrap_err();
        assert_eq!(err.code(), "ics_limit_exceeded");
        assert!(err.short_message().contains("single-event"));
    }

    #[test]
    fn sanitize_collision_yields_distinct_leafs() {
        // a/b and a:b both sanitize to a_b — must disambiguate.
        let mut reserved = HashSet::new();
        let ev0 = ParsedVEvent {
            index: 0,
            fields: crate::extract::CalendarEventFields {
                cal_uid: Some("a/b".into()),
                ..Default::default()
            },
            single_event_ics: Vec::new(),
        };
        let ev1 = ParsedVEvent {
            index: 1,
            fields: crate::extract::CalendarEventFields {
                cal_uid: Some("a:b".into()),
                ..Default::default()
            },
            single_event_ics: Vec::new(),
        };
        let p0 = unique_child_path("export.ics", &ev0, &mut reserved);
        let p1 = unique_child_path("export.ics", &ev1, &mut reserved);
        assert_ne!(p0, p1, "colliding sanitize must produce unique paths");
        assert!(p0.ends_with(".ics") && p1.ends_with(".ics"));
        assert!(p0.starts_with("export.ics!/"));
        assert!(p1.starts_with("export.ics!/"));
        // Second path should carry disambiguation digest.
        assert!(
            p1.contains('-') || p0.contains('-'),
            "expected hash suffix on collision: {p0} / {p1}"
        );
    }

    #[test]
    fn unique_child_path_is_stable_for_same_uid() {
        let mut r1 = HashSet::new();
        let mut r2 = HashSet::new();
        let ev = ParsedVEvent {
            index: 0,
            fields: crate::extract::CalendarEventFields {
                cal_uid: Some("stable-uid@ex.com".into()),
                ..Default::default()
            },
            single_event_ics: Vec::new(),
        };
        let a = unique_child_path("p.ics", &ev, &mut r1);
        let b = unique_child_path("p.ics", &ev, &mut r2);
        assert_eq!(a, b);
    }
}
