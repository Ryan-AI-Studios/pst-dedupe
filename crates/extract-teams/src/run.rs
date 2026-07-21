//! Resumable `teams_extract` job.

use std::collections::HashMap;
use std::time::Instant;

use matter_core::{
    compute_non_email_logical_hash, item_role, item_status, teams_extract_status,
    ApplyTeamsExtractInput, AuditEventInput, Item, ItemErrorInput, ItemInput, ItemUpdate, Matter,
    NonEmailLogicalInput, TeamsCandidate, TeamsExtractApplyResult, LOGICAL_HASH_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detect::{self, export_format};
use crate::error::{Error, Result};
use crate::html_parse::{parse_teams_html, ParsedChatMessage};
use crate::json_parse::parse_teams_json;
use crate::limits::methods;
use crate::params::TeamsExtractParams;
use crate::pst_enrich::{enrich_from_metadata, PstEnrichInput};

/// Job kind string for process-runner.
pub const JOB_KIND_TEAMS_EXTRACT: &str = "teams_extract";
/// Checkpoint stage name.
pub const TEAMS_EXTRACT_STAGE: &str = "teams_extract";

/// Summary counts after a teams extract run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamsExtractSummary {
    pub completed_count: u64,
    pub extracted_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
    pub child_count: u64,
    /// Leaves that entered the HTML adapter path (ok/skip/error).
    #[serde(default)]
    pub html_count: u64,
    /// Leaves that entered the JSON adapter path.
    #[serde(default)]
    pub json_count: u64,
    /// Leaves that entered the PST enrich path.
    #[serde(default)]
    pub pst_count: u64,
}

/// Outcome of [`run_teams_extract`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamsExtractOutcome {
    Succeeded(TeamsExtractSummary),
    Paused(TeamsExtractSummary),
    Failed {
        message: String,
        summary: TeamsExtractSummary,
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
    #[serde(default)]
    html_count: u64,
    #[serde(default)]
    json_count: u64,
    #[serde(default)]
    pst_count: u64,
    params: serde_json::Value,
}

/// Run teams extract on `matter` for the runner-created `job_id`.
pub fn run_teams_extract(
    matter: &Matter,
    job_id: &str,
    params: &TeamsExtractParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<TeamsExtractOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(Error::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "teams_extract.start".into(),
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
        Ok(TeamsExtractOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "teams_extract.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "extracted_count": s.extracted_count,
                    "skipped_count": s.skipped_count,
                    "error_count": s.error_count,
                    "child_count": s.child_count,
                    "completed_count": s.completed_count,
                    "html_count": s.html_count,
                    "json_count": s.json_count,
                    "pst_count": s.pst_count,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(TeamsExtractOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(TeamsExtractOutcome::Paused(_)) => {}
        Ok(TeamsExtractOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "teams_extract.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "extracted_count": summary.extracted_count,
                    "skipped_count": summary.skipped_count,
                    "error_count": summary.error_count,
                    "child_count": summary.child_count,
                    "html_count": summary.html_count,
                    "json_count": summary.json_count,
                    "pst_count": summary.pst_count,
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
                action: "teams_extract.fail".into(),
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
    let Some(cp) = matter.get_checkpoint(job_id, TEAMS_EXTRACT_STAGE)? else {
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
    call_site: &TeamsExtractParams,
    prior: Option<&CheckpointCursor>,
) -> Result<TeamsExtractParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<TeamsExtractParams>(p.params.clone()) {
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
    params: &TeamsExtractParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<TeamsExtractOutcome> {
    let mut summary = TeamsExtractSummary::default();
    let mut cursor_index = 0u64;
    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        summary.completed_count = p.completed_count;
        summary.extracted_count = p.extracted_count;
        summary.skipped_count = p.skipped_count;
        summary.error_count = p.error_count;
        summary.child_count = p.child_count;
        summary.html_count = p.html_count;
        summary.json_count = p.json_count;
        summary.pst_count = p.pst_count;
    }

    let batch = params.batch_size.max(1);
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            write_checkpoint(matter, job_id, cursor_index, &summary, params_json, None)?;
            progress(summary.completed_count);
            return Ok(TeamsExtractOutcome::Paused(summary));
        }

        let candidates = matter.list_teams_candidates(
            cursor_index,
            batch as u64,
            params.source_id.as_deref(),
        )?;
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
                return Ok(TeamsExtractOutcome::Paused(summary));
            }

            process_one(matter, &cand, params, job_id, &mut summary)?;
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

    Ok(TeamsExtractOutcome::Succeeded(summary))
}

fn already_ok(cand: &TeamsCandidate, reprocess: bool) -> bool {
    if reprocess {
        return false;
    }
    matches!(
        cand.teams_extract_status.as_deref(),
        Some(teams_extract_status::OK) | Some(teams_extract_status::SKIPPED)
    )
}

fn process_one(
    matter: &Matter,
    cand: &TeamsCandidate,
    params: &TeamsExtractParams,
    job_id: &str,
    summary: &mut TeamsExtractSummary,
) -> Result<()> {
    if already_ok(cand, params.reprocess()) {
        matter.apply_teams_extract(ApplyTeamsExtractInput {
            item_id: cand.id.clone(),
            force: false,
            status: Some(teams_extract_status::SKIPPED.into()),
            ..Default::default()
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    let format = detect::detect_format(
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
        cand.message_class.as_deref(),
    );

    let Some(format) = format else {
        matter.apply_teams_extract(ApplyTeamsExtractInput {
            item_id: cand.id.clone(),
            force: true,
            status: Some(teams_extract_status::SKIPPED.into()),
            error: Some("teams_not_teams".into()),
            method: None,
            ..Default::default()
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    if !params.allows_format(format) {
        matter.apply_teams_extract(ApplyTeamsExtractInput {
            item_id: cand.id.clone(),
            force: true,
            status: Some(teams_extract_status::SKIPPED.into()),
            error: Some(format!("format_{format}_disabled")),
            ..Default::default()
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    match format {
        export_format::HTML => process_html(matter, cand, params, job_id, summary),
        export_format::JSON => process_json(matter, cand, params, job_id, summary),
        export_format::PST => process_pst(matter, cand, params, job_id, summary),
        other => {
            record_error(
                matter,
                &cand.id,
                job_id,
                &Error::UnsupportedFormat(other.into()),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            Ok(())
        }
    }
}

fn process_html(
    matter: &Matter,
    cand: &TeamsCandidate,
    params: &TeamsExtractParams,
    job_id: &str,
    summary: &mut TeamsExtractSummary,
) -> Result<()> {
    summary.html_count += 1;
    let Some(native_sha) = cand.native_sha256.as_deref() else {
        record_error(
            matter,
            &cand.id,
            job_id,
            &Error::Other("missing native_sha256".into()),
        )?;
        summary.error_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    match matter.cas_len(native_sha) {
        Ok(len) if len > params.max_html_bytes => {
            record_error(
                matter,
                &cand.id,
                job_id,
                &Error::limit(format!(
                    "html size {len} exceeds max {}",
                    params.max_html_bytes
                )),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
        Ok(_) => {}
        Err(e) => {
            record_error(
                matter,
                &cand.id,
                job_id,
                &Error::Other(format!("CAS stat: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    }

    let bytes = match matter.get_bytes_capped(native_sha, params.max_html_bytes) {
        Ok(b) => b,
        Err(e) => {
            record_error(
                matter,
                &cand.id,
                job_id,
                &Error::Other(format!("CAS read: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    // Broad .html candidates often are not Teams exports — skip quietly.
    if !detect::looks_like_teams_html(&bytes) {
        record_skip(
            matter,
            &cand.id,
            summary,
            Some("not_teams_html"),
            Some("not_teams_html"),
        )?;
        return Ok(());
    }

    let html = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };

    // Teams-shaped but unparseable / empty → hard error (not skip).
    let parsed = match parse_teams_html(&html, params.max_messages_per_file) {
        Ok(p) => p,
        Err(e) => {
            record_error(matter, &cand.id, job_id, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    expand_messages(
        matter,
        cand,
        &parsed.messages,
        export_format::HTML,
        methods::HTML_FIXTURE_V1,
        params.reprocess(),
        summary,
    )?;

    matter.apply_teams_extract(ApplyTeamsExtractInput {
        item_id: cand.id.clone(),
        force: true,
        status: Some(teams_extract_status::OK.into()),
        method: Some(methods::HTML_FIXTURE_V1.into()),
        team_name: parsed.team_name,
        channel_name: parsed.channel_name,
        chat_type: Some(parsed.chat_type),
        chat_export_format: Some(export_format::HTML.into()),
        role: Some(item_role::PARENT.into()),
        file_category: Some(file_category::Category::Archive.as_str().into()),
        refine_file_category: true,
        extra_json: Some(
            json!({
                "extract_tool": "extract-teams",
                "extract_version": env!("CARGO_PKG_VERSION"),
                "message_count": parsed.messages.len(),
            })
            .to_string(),
        ),
        ..Default::default()
    })?;

    summary.extracted_count += 1;
    summary.completed_count += 1;
    Ok(())
}

fn process_json(
    matter: &Matter,
    cand: &TeamsCandidate,
    params: &TeamsExtractParams,
    job_id: &str,
    summary: &mut TeamsExtractSummary,
) -> Result<()> {
    summary.json_count += 1;
    let Some(native_sha) = cand.native_sha256.as_deref() else {
        record_error(
            matter,
            &cand.id,
            job_id,
            &Error::Other("missing native_sha256".into()),
        )?;
        summary.error_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    match matter.cas_len(native_sha) {
        Ok(len) if len > params.max_html_bytes => {
            record_error(
                matter,
                &cand.id,
                job_id,
                &Error::limit(format!(
                    "json size {len} exceeds max {}",
                    params.max_html_bytes
                )),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
        Ok(_) => {}
        Err(e) => {
            record_error(
                matter,
                &cand.id,
                job_id,
                &Error::Other(format!("CAS stat: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    }

    let bytes = match matter.get_bytes_capped(native_sha, params.max_html_bytes) {
        Ok(b) => b,
        Err(e) => {
            record_error(
                matter,
                &cand.id,
                job_id,
                &Error::Other(format!("CAS read: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    // Random JSON configs are candidates by extension — skip when not Teams-shaped.
    if !detect::looks_like_teams_json(&bytes) {
        record_skip(
            matter,
            &cand.id,
            summary,
            Some("not_teams_json"),
            Some("not_teams_json"),
        )?;
        return Ok(());
    }

    // Schema-shaped but unusable / corrupt JSON → hard error.
    let messages = match parse_teams_json(&bytes, params.max_messages_per_file) {
        Ok(m) => m,
        Err(e) => {
            record_error(matter, &cand.id, job_id, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    expand_messages(
        matter,
        cand,
        &messages,
        export_format::JSON,
        methods::JSON_BEST_EFFORT_V1,
        params.reprocess(),
        summary,
    )?;

    matter.apply_teams_extract(ApplyTeamsExtractInput {
        item_id: cand.id.clone(),
        force: true,
        status: Some(teams_extract_status::OK.into()),
        method: Some(methods::JSON_BEST_EFFORT_V1.into()),
        chat_export_format: Some(export_format::JSON.into()),
        role: Some(item_role::PARENT.into()),
        file_category: Some(file_category::Category::Archive.as_str().into()),
        refine_file_category: true,
        extra_json: Some(
            json!({
                "extract_tool": "extract-teams",
                "extract_version": env!("CARGO_PKG_VERSION"),
                "message_count": messages.len(),
            })
            .to_string(),
        ),
        ..Default::default()
    })?;

    summary.extracted_count += 1;
    summary.completed_count += 1;
    Ok(())
}

fn process_pst(
    matter: &Matter,
    cand: &TeamsCandidate,
    params: &TeamsExtractParams,
    job_id: &str,
    summary: &mut TeamsExtractSummary,
) -> Result<()> {
    summary.pst_count += 1;

    // Honest CAS handling: declared text_sha256 that fails to load/decode is an error,
    // not a silent subject-only success. Missing text_sha256 allows subject fallback.
    let existing_text = if let Some(ref sha) = cand.text_sha256 {
        match matter.get_bytes(sha) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => Some(s),
                Err(e) => {
                    record_error(
                        matter,
                        &cand.id,
                        job_id,
                        &Error::utf8(format!("text_sha256 utf-8 decode failed: {e}")),
                    )?;
                    summary.error_count += 1;
                    summary.completed_count += 1;
                    return Ok(());
                }
            },
            Err(e) => {
                record_error(
                    matter,
                    &cand.id,
                    job_id,
                    &Error::cas(format!("text_sha256 CAS read failed: {e}")),
                )?;
                summary.error_count += 1;
                summary.completed_count += 1;
                return Ok(());
            }
        }
    } else {
        None
    };

    let attachments = match load_pst_attachment_names(matter, &cand.id) {
        Ok(a) => a,
        Err(e) => {
            record_error(
                matter,
                &cand.id,
                job_id,
                &Error::Other(format!("list attachments: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    let Some(msg) = enrich_from_metadata(&PstEnrichInput {
        message_class: cand.message_class.as_deref(),
        path: cand.path.as_deref(),
        from_addr: cand.from_addr.as_deref(),
        sent_at: cand.sent_at.as_deref(),
        subject: cand.subject.as_deref(),
        existing_text: existing_text.as_deref(),
        team_hint: None,
        channel_hint: None,
        attachments: &attachments,
    }) else {
        matter.apply_teams_extract(ApplyTeamsExtractInput {
            item_id: cand.id.clone(),
            force: true,
            status: Some(teams_extract_status::SKIPPED.into()),
            error: Some("teams_not_teams".into()),
            method: Some(methods::PST_ENRICH_V1.into()),
            ..Default::default()
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    // Prefer not to destroy good plain text: only replace when empty or HTML-ish,
    // or when attachment lines need injection into the review body.
    let text_to_write = match existing_text.as_deref() {
        Some(t)
            if !t.trim().is_empty()
                && !t.trim_start().starts_with('<')
                && !t.contains("<script")
                && t == msg.plain_text =>
        {
            if params.reprocess() {
                Some(msg.plain_text.clone())
            } else {
                None
            }
        }
        _ => Some(msg.plain_text.clone()),
    };

    let apply = matter.apply_teams_extract(ApplyTeamsExtractInput {
        item_id: cand.id.clone(),
        force: params.reprocess(),
        text: text_to_write,
        method: Some(methods::PST_ENRICH_V1.into()),
        status: Some(teams_extract_status::OK.into()),
        conversation_id: Some(msg.conversation_id),
        conversation_bucket_date: Some(msg.conversation_bucket_date),
        chat_type: Some(msg.chat_type),
        team_name: msg.team_name,
        channel_name: msg.channel_name,
        chat_export_format: Some(export_format::PST.into()),
        file_category: Some(file_category::Category::Chat.as_str().into()),
        refine_file_category: true,
        role: Some(item_role::CHAT_MESSAGE.into()),
        from_addr: msg.from_addr,
        sent_at: msg.sent_at,
        subject: cand.subject.clone(),
        message_class: cand.message_class.clone(),
        extra_json: Some(
            json!({
                "extract_tool": "extract-teams",
                "extract_version": env!("CARGO_PKG_VERSION"),
            })
            .to_string(),
        ),
        ..Default::default()
    })?;

    match apply {
        TeamsExtractApplyResult::Skipped => summary.skipped_count += 1,
        TeamsExtractApplyResult::Applied { .. } => summary.extracted_count += 1,
        TeamsExtractApplyResult::Error { .. } => {
            let _ = job_id;
            summary.error_count += 1;
        }
    }
    summary.completed_count += 1;
    Ok(())
}

/// Collect attachment filenames for a PST message from matter child items.
///
/// Uses children with `role=attachment` (or any child with a title when attachment_count
/// is set). Filenames come from `title`, then `subject`, then path leaf.
fn load_pst_attachment_names(
    matter: &Matter,
    parent_id: &str,
) -> Result<Vec<(Option<String>, Option<String>)>> {
    let children = matter.list_attachments(parent_id)?;
    let mut out = Vec::new();
    for child in children {
        let is_attachment = child.role.as_deref() == Some(item_role::ATTACHMENT);
        if !is_attachment {
            continue;
        }
        let name = child
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| child.subject.clone().filter(|s| !s.trim().is_empty()))
            .or_else(|| {
                child.path.as_ref().and_then(|p| {
                    p.rsplit(['/', '\\', '!'])
                        .next()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
            });
        if name.is_some() {
            out.push((name, None));
        }
    }
    Ok(out)
}

fn expand_messages(
    matter: &Matter,
    parent: &TeamsCandidate,
    messages: &[ParsedChatMessage],
    export_format: &str,
    method: &str,
    force: bool,
    summary: &mut TeamsExtractSummary,
) -> Result<()> {
    let parent_path = parent.path.clone().unwrap_or_else(|| "chat_export".into());

    // Ensure parent has a family for children.
    let parent_item = matter.get_item(&parent.id)?;
    let family_id = if let Some(fid) = parent_item.family_id.clone() {
        fid
    } else {
        let fam = matter.insert_family("teams-chat")?;
        matter.update_item(
            &parent.id,
            ItemUpdate {
                family_id: Some(Some(fam.id.clone())),
                role: Some(Some(item_role::PARENT.into())),
                ..Default::default()
            },
        )?;
        fam.id
    };

    let existing = matter.list_attachments(&parent.id)?;
    let mut by_path: HashMap<String, Item> = HashMap::new();
    for child in existing {
        if let Some(path) = child.path.clone() {
            by_path.entry(path).or_insert(child);
        }
    }

    for (i, msg) in messages.iter().enumerate() {
        let leaf = msg
            .export_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(sanitize_path_component)
            .unwrap_or_else(|| format!("msg-{i}"));
        let child_path = format!("{parent_path}!/{leaf}");

        if !force {
            if let Some(existing_child) = by_path.get(&child_path) {
                if existing_child.conversation_id.as_deref() == Some(msg.conversation_id.as_str())
                    && existing_child.text_sha256.is_some()
                {
                    continue;
                }
            }
        }

        let text_sha = if msg.plain_text.is_empty() {
            None
        } else {
            Some(matter.put_bytes(msg.plain_text.as_bytes())?)
        };

        let logical = compute_non_email_logical_hash(&NonEmailLogicalInput {
            category: Some(file_category::Category::Chat.as_str().into()),
            title: msg.from_name.clone().or_else(|| msg.from_addr.clone()),
            author: msg.from_addr.clone(),
            created: msg.sent_at.clone(),
            text: Some(msg.plain_text.clone()),
            children_native_sha256: vec![],
        });

        let subject = msg
            .from_name
            .clone()
            .or_else(|| msg.from_addr.clone())
            .or_else(|| msg.export_id.clone());

        if let Some(existing_child) = by_path.get(&child_path) {
            matter.update_item(
                &existing_child.id,
                ItemUpdate {
                    path: Some(Some(child_path.clone())),
                    status: Some(item_status::EXTRACTED.into()),
                    role: Some(Some(item_role::CHAT_MESSAGE.into())),
                    parent_item_id: Some(Some(parent.id.clone())),
                    family_id: Some(Some(family_id.clone())),
                    file_category: Some(Some(file_category::Category::Chat.as_str().into())),
                    subject: Some(subject.clone()),
                    from_addr: Some(msg.from_addr.clone()),
                    sent_at: Some(msg.sent_at.clone()),
                    message_id: Some(msg.export_id.clone()),
                    text_sha256: Some(text_sha.clone()),
                    logical_hash: Some(Some(logical.clone())),
                    logical_hash_version: Some(LOGICAL_HASH_VERSION),
                    conversation_id: Some(Some(msg.conversation_id.clone())),
                    chat_type: Some(Some(msg.chat_type.clone())),
                    team_name: Some(msg.team_name.clone()),
                    channel_name: Some(msg.channel_name.clone()),
                    chat_export_format: Some(Some(export_format.into())),
                    conversation_bucket_date: Some(Some(msg.conversation_bucket_date.clone())),
                    extra_json: Some(Some(
                        json!({
                            "extract_tool": "extract-teams",
                            "extract_version": env!("CARGO_PKG_VERSION"),
                            "teams_method": method,
                        })
                        .to_string(),
                    )),
                    ..Default::default()
                },
            )?;
            matter.apply_teams_extract(ApplyTeamsExtractInput {
                item_id: existing_child.id.clone(),
                force: true,
                status: Some(teams_extract_status::OK.into()),
                method: Some(method.into()),
                conversation_id: Some(msg.conversation_id.clone()),
                conversation_bucket_date: Some(msg.conversation_bucket_date.clone()),
                chat_type: Some(msg.chat_type.clone()),
                team_name: msg.team_name.clone(),
                channel_name: msg.channel_name.clone(),
                chat_export_format: Some(export_format.into()),
                ..Default::default()
            })?;
        } else {
            let child = matter.insert_item(ItemInput {
                path: Some(child_path),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::CHAT_MESSAGE.into()),
                parent_item_id: Some(parent.id.clone()),
                family_id: Some(family_id.clone()),
                file_category: Some(file_category::Category::Chat.as_str().into()),
                subject,
                from_addr: msg.from_addr.clone(),
                sent_at: msg.sent_at.clone(),
                message_id: msg.export_id.clone(),
                text_sha256: text_sha,
                logical_hash: Some(logical),
                logical_hash_version: Some(LOGICAL_HASH_VERSION),
                conversation_id: Some(msg.conversation_id.clone()),
                chat_type: Some(msg.chat_type.clone()),
                team_name: msg.team_name.clone(),
                channel_name: msg.channel_name.clone(),
                chat_export_format: Some(export_format.into()),
                conversation_bucket_date: Some(msg.conversation_bucket_date.clone()),
                extra_json: Some(
                    json!({
                        "extract_tool": "extract-teams",
                        "extract_version": env!("CARGO_PKG_VERSION"),
                        "teams_method": method,
                    })
                    .to_string(),
                ),
                ..Default::default()
            })?;
            matter.apply_teams_extract(ApplyTeamsExtractInput {
                item_id: child.id,
                force: true,
                status: Some(teams_extract_status::OK.into()),
                method: Some(method.into()),
                conversation_id: Some(msg.conversation_id.clone()),
                conversation_bucket_date: Some(msg.conversation_bucket_date.clone()),
                chat_type: Some(msg.chat_type.clone()),
                team_name: msg.team_name.clone(),
                channel_name: msg.channel_name.clone(),
                chat_export_format: Some(export_format.into()),
                ..Default::default()
            })?;
        }
        summary.child_count += 1;
    }
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
        "msg".into()
    } else {
        out.chars().take(80).collect()
    }
}

/// Mark leaf skipped without recording an item_error (noisy false-positive candidates).
fn record_skip(
    matter: &Matter,
    item_id: &str,
    summary: &mut TeamsExtractSummary,
    method: Option<&str>,
    note: Option<&str>,
) -> Result<()> {
    matter.apply_teams_extract(ApplyTeamsExtractInput {
        item_id: item_id.into(),
        force: true,
        text: None,
        method: method.map(|s| s.into()),
        status: Some(teams_extract_status::SKIPPED.into()),
        error: note.map(|s| s.into()),
        ..Default::default()
    })?;
    summary.skipped_count += 1;
    summary.completed_count += 1;
    Ok(())
}

fn record_error(matter: &Matter, item_id: &str, job_id: &str, err: &Error) -> Result<()> {
    matter.apply_teams_extract(ApplyTeamsExtractInput {
        item_id: item_id.into(),
        force: true,
        text: None,
        method: None,
        status: Some(teams_extract_status::ERROR.into()),
        error: Some(format!("{}: {}", err.code(), err.short_message())),
        ..Default::default()
    })?;
    matter
        .record_item_error(ItemErrorInput {
            item_id: Some(item_id.into()),
            source_id: None,
            job_id: Some(job_id.into()),
            stage: TEAMS_EXTRACT_STAGE.into(),
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
    summary: &TeamsExtractSummary,
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
        html_count: summary.html_count,
        json_count: summary.json_count,
        pst_count: summary.pst_count,
        params: params_json.clone(),
    };
    let cursor_json = serde_json::to_string(&cursor)
        .map_err(|e| Error::Other(format!("checkpoint json: {e}")))?;
    matter.put_checkpoint(
        job_id,
        TEAMS_EXTRACT_STAGE,
        &cursor_json,
        summary.completed_count as i64,
    )?;
    Ok(())
}
