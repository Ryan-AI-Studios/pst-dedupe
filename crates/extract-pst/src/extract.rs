//! Blocking PST extract → Normalized Items.

use matter_core::{
    compute_email_logical_hash, compute_non_email_logical_hash, item_role, item_status,
    normalize_body, normalize_conversation_index_to_hex, normalize_message_id, parse_in_reply_to,
    parse_references_header, references_to_json, AuditEventInput, ConversationIndexInput,
    EmailLogicalInput, Item, ItemErrorInput, ItemInput, ItemUpdate, JobState, LogicalAttachment,
    Matter, NonEmailLogicalInput, FAMILY_KIND_EMAIL_ATTACHMENTS, LOGICAL_HASH_VERSION,
};
use pst_reader::{filetime_to_rfc3339, is_calendar_message_class, ExtractedMessage, NodeId};
use serde_json::json;

use crate::checkpoint::{nid_hex, ExtractCursor};
use crate::error::{Error, Result};
use crate::limits::{ExtractLimits, ExtractSummary, JOB_KIND_EXTRACT_PST, STAGE_PST_EXTRACT};
use crate::native_message::{
    encode_native_message_v1, NativeAttachment, NativeMessageV1, NATIVE_FORMAT_V1,
};
use crate::open::{candidate_fs_path, open_pst, PstOpenSpec};
use crate::recipients::parse_display_list;

/// List inventory items that look like PSTs under a source.
pub fn list_discovered_psts(matter: &Matter, source_id: &str) -> Result<Vec<Item>> {
    let items = matter.list_items_for_source(source_id)?;
    Ok(items
        .into_iter()
        .filter(|i| {
            i.path
                .as_deref()
                .map(|p| p.to_ascii_lowercase().ends_with(".pst"))
                .unwrap_or(false)
        })
        .collect())
}

/// Blocking extract of one inventory PST item.
///
/// Creates a new `extract_pst` job, then extracts on that job. Prefer
/// [`extract_pst_item_on_job`] when a process runner already owns job creation.
///
/// **Caller contract:** invoke from a dedicated blocking worker only.
pub fn extract_pst_item(
    matter: &Matter,
    source_id: &str,
    pst_item_id: &str,
    limits: &ExtractLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ExtractSummary> {
    // Validate before creating a job so bad input does not leave orphan rows.
    let _ = matter
        .get_source(source_id)
        .map_err(|_| Error::SourceNotFound(source_id.to_string()))?;
    let inv = matter
        .get_item(pst_item_id)
        .map_err(|_| Error::InventoryItemNotFound(pst_item_id.to_string()))?;
    let pst_path = inv
        .path
        .clone()
        .ok_or_else(|| Error::NotAPstItem(format!("{pst_item_id}: missing path")))?;
    if !pst_path.to_ascii_lowercase().ends_with(".pst") {
        return Err(Error::NotAPstItem(pst_path));
    }

    let job = matter.create_job(JOB_KIND_EXTRACT_PST)?;
    matter.set_job_state(&job.id, JobState::Running, None)?;
    extract_pst_item_on_job(matter, source_id, pst_item_id, limits, &job.id, cancel)
}

/// Extract one inventory PST item on a **pre-created** job id (Option C).
///
/// Does **not** call `create_job`. Callers (e.g. `process-runner`) must create
/// the job before invoking this.
///
/// **Caller contract:** same blocking-thread rules as [`extract_pst_item`].
pub fn extract_pst_item_on_job(
    matter: &Matter,
    source_id: &str,
    pst_item_id: &str,
    limits: &ExtractLimits,
    job_id: &str,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ExtractSummary> {
    let _ = matter
        .get_source(source_id)
        .map_err(|_| Error::SourceNotFound(source_id.to_string()))?;
    let inv = matter
        .get_item(pst_item_id)
        .map_err(|_| Error::InventoryItemNotFound(pst_item_id.to_string()))?;
    let pst_path = inv
        .path
        .clone()
        .ok_or_else(|| Error::NotAPstItem(format!("{pst_item_id}: missing path")))?;
    if !pst_path.to_ascii_lowercase().ends_with(".pst") {
        return Err(Error::NotAPstItem(pst_path));
    }

    ensure_extract_job_running(matter, job_id)?;

    let cursor = ExtractCursor::new(
        source_id,
        &pst_path,
        pst_item_id,
        inv.native_sha256.as_deref(),
        limits.batch_size.max(1),
    );

    run_extract(
        matter, source_id, &inv, job_id, cursor, limits, cancel, false, None,
    )
}

/// Resume a paused/failed `extract_pst` job from its checkpoint.
pub fn resume_extract(
    matter: &Matter,
    source_id: &str,
    job_id: &str,
    limits: &ExtractLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ExtractSummary> {
    let job = matter.get_job(job_id)?;
    if job.kind != JOB_KIND_EXTRACT_PST {
        return Err(Error::InvalidJob(format!(
            "{job_id}: kind {} != {JOB_KIND_EXTRACT_PST}",
            job.kind
        )));
    }
    let cp = matter
        .get_checkpoint(job_id, STAGE_PST_EXTRACT)?
        .ok_or_else(|| Error::InvalidJob(format!("{job_id}: no checkpoint")))?;
    let cursor = ExtractCursor::from_json(&cp.cursor_json)?;
    if cursor.source_id != source_id {
        return Err(Error::InvalidJob(format!("{job_id}: source_id mismatch")));
    }
    let inv = matter
        .get_item(&cursor.pst_item_id)
        .map_err(|_| Error::InventoryItemNotFound(cursor.pst_item_id.clone()))?;

    // Fail closed if inventory path/digest no longer matches the checkpoint PST.
    verify_resume_pst_identity(&cursor, &inv)?;

    match job.state {
        JobState::Paused | JobState::Failed | JobState::Pending => {
            matter.set_job_state(job_id, JobState::Running, None)?;
        }
        JobState::Running => {}
        other => {
            return Err(Error::InvalidJob(format!(
                "{job_id}: cannot resume from state {other}"
            )));
        }
    }

    run_extract(
        matter, source_id, &inv, job_id, cursor, limits, cancel, true, None,
    )
}

/// Optional path-based entry: register a minimal inventory row then extract.
///
/// Creates a new `extract_pst` job, then extracts on that job. Prefer
/// [`extract_pst_path_on_job`] when a process runner already owns job creation.
///
/// Streams the PST into CAS via [`Matter::put_reader`] (no full-file `Vec`).
/// Open uses the **exact** caller-supplied filesystem path for this run (CAS
/// put is only for inventory digest); inventory `path` stays the stable leaf
/// name so message path keys remain stable.
pub fn extract_pst_path(
    matter: &Matter,
    source_id: &str,
    pst_fs_path: &str,
    limits: &ExtractLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ExtractSummary> {
    let _ = matter
        .get_source(source_id)
        .map_err(|_| Error::SourceNotFound(source_id.to_string()))?;
    let path = camino::Utf8Path::new(pst_fs_path);
    if !path.as_std_path().is_file() {
        return Err(Error::PstOpenFailed(format!("not a file: {pst_fs_path}")));
    }

    let job = matter.create_job(JOB_KIND_EXTRACT_PST)?;
    matter.set_job_state(&job.id, JobState::Running, None)?;
    extract_pst_path_on_job(matter, source_id, pst_fs_path, limits, &job.id, cancel)
}

/// Path-based extract on a **pre-created** job id (Option C).
///
/// Does **not** call `create_job`. Still registers inventory and CAS-puts the
/// PST. Callers must create the job before invoking this.
///
/// **Caller contract:** same blocking-thread rules as [`extract_pst_path`].
pub fn extract_pst_path_on_job(
    matter: &Matter,
    source_id: &str,
    pst_fs_path: &str,
    limits: &ExtractLimits,
    job_id: &str,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ExtractSummary> {
    use std::fs;

    let _ = matter
        .get_source(source_id)
        .map_err(|_| Error::SourceNotFound(source_id.to_string()))?;

    let path = camino::Utf8Path::new(pst_fs_path);
    if !path.as_std_path().is_file() {
        return Err(Error::PstOpenFailed(format!("not a file: {pst_fs_path}")));
    }

    ensure_extract_job_running(matter, job_id)?;

    // Canonicalize when possible so open targets the same bytes just hashed.
    let exact_fs = path
        .canonicalize_utf8()
        .unwrap_or_else(|_| path.to_path_buf());
    let meta = fs::metadata(exact_fs.as_std_path())?;
    let size_bytes = meta.len() as i64;
    let mut file = fs::File::open(exact_fs.as_std_path())?;
    let digest = matter
        .put_reader(&mut file)
        .map_err(|e| Error::CasPutFailed(e.to_string()))?;
    let logical = path.file_name().unwrap_or(pst_fs_path).to_string();
    let item = matter.insert_item(ItemInput {
        source_id: Some(source_id.to_string()),
        path: Some(logical.clone()),
        native_sha256: Some(digest.clone()),
        status: item_status::DISCOVERED.to_string(),
        size_bytes: Some(size_bytes),
        file_category: Some("pst".into()),
        ..Default::default()
    })?;

    let mut cursor = ExtractCursor::new(
        source_id,
        &logical,
        &item.id,
        Some(digest.as_str()),
        limits.batch_size.max(1),
    );
    // Persist exact FS path so resume does not re-derive a different PST.
    cursor.open_fs_path = Some(exact_fs.to_string());

    run_extract(
        matter,
        source_id,
        &item,
        job_id,
        cursor,
        limits,
        cancel,
        false,
        Some(exact_fs),
    )
}

/// Ensure `job_id` exists, is kind `extract_pst`, and is (or becomes) Running.
fn ensure_extract_job_running(matter: &Matter, job_id: &str) -> Result<()> {
    let job = matter.get_job(job_id)?;
    if job.kind != JOB_KIND_EXTRACT_PST {
        return Err(Error::InvalidJob(format!(
            "{job_id}: kind {} != {JOB_KIND_EXTRACT_PST}",
            job.kind
        )));
    }
    match job.state {
        JobState::Running => Ok(()),
        JobState::Pending | JobState::Paused | JobState::Failed => {
            matter.set_job_state(job_id, JobState::Running, None)?;
            Ok(())
        }
        JobState::Cancelled => {
            // Cancelled → Pending → Running is the allowed retry path.
            matter.set_job_state(job_id, JobState::Pending, None)?;
            matter.set_job_state(job_id, JobState::Running, None)?;
            Ok(())
        }
        JobState::Succeeded => Err(Error::InvalidJob(format!(
            "{job_id}: cannot run extract on succeeded job"
        ))),
    }
}

/// Ensure resume continues against the same PST identity the checkpoint recorded.
fn verify_resume_pst_identity(cursor: &ExtractCursor, inv: &Item) -> Result<()> {
    let inv_path = inv.path.as_deref().unwrap_or("");
    if cursor.pst_path != inv_path {
        return Err(Error::ResumePstMismatch(format!(
            "checkpoint pst_path {:?} != inventory path {:?}",
            cursor.pst_path, inv.path
        )));
    }

    match (
        cursor.pst_native_sha256.as_deref(),
        inv.native_sha256.as_deref(),
    ) {
        (Some(c), Some(i)) if c != i => Err(Error::ResumePstMismatch(format!(
            "checkpoint pst_native_sha256 {c} != inventory {i}"
        ))),
        (Some(c), None) => Err(Error::ResumePstMismatch(format!(
            "checkpoint has digest {c} but inventory native_sha256 is missing"
        ))),
        // Checkpoint lacked digest (legacy) — only path was checked above.
        (None, _) => Ok(()),
        (Some(_), Some(_)) => Ok(()),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_extract(
    matter: &Matter,
    source_id: &str,
    inv: &Item,
    job_id: &str,
    mut cursor: ExtractCursor,
    limits: &ExtractLimits,
    cancel: Option<&dyn Fn() -> bool>,
    is_resume: bool,
    // When set (path entry), open this exact filesystem path first.
    open_fs_override: Option<camino::Utf8PathBuf>,
) -> Result<ExtractSummary> {
    let pst_path = inv.path.clone().unwrap_or_else(|| cursor.pst_path.clone());
    let source = matter.get_source(source_id)?;

    // Open path resolution (custody-safe):
    // 1) Explicit override for this call (extract_pst_path first run)
    // 2) Checkpoint-persisted exact path (resume after extract_pst_path)
    // 3) When a whole-file digest is known: prefer CAS materialize rather than
    //    fuzzy source+leaf FS guesses that could open a different PST.
    // 4) Else derive candidate under package root (relative names never use CWD).
    let digest = inv
        .native_sha256
        .clone()
        .or(cursor.pst_native_sha256.clone());

    if open_fs_override.is_some() && cursor.open_fs_path.is_none() {
        if let Some(ref p) = open_fs_override {
            cursor.open_fs_path = Some(p.to_string());
        }
    }

    let fs_candidate = open_fs_override
        .or_else(|| {
            cursor.open_fs_path.as_ref().and_then(|p| {
                let pb = camino::Utf8PathBuf::from(p.as_str());
                if pb.as_std_path().is_file() {
                    Some(pb)
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            if digest.is_some() {
                // Digest known: open via CAS unless we already have an exact path.
                None
            } else {
                candidate_fs_path(&source.path, &pst_path)
            }
        });

    let spec = PstOpenSpec {
        inventory_path: pst_path.clone(),
        native_sha256: digest,
        filesystem_path: fs_candidate,
    };

    if !is_resume {
        let _ = matter.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "extract.start".into(),
            entity: format!("source:{source_id}"),
            params_json: json!({
                "job_id": job_id,
                "pst_item_id": inv.id,
                "pst_path": &pst_path,
                "pst_native_sha256": inv.native_sha256,
                "limits": {
                    "batch_size": limits.batch_size,
                    "max_messages": limits.max_messages,
                    "max_attachment_bytes": limits.max_attachment_bytes,
                    "max_in_memory_put_bytes": limits.max_in_memory_put_bytes,
                },
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
    }

    let mut opened = match open_pst(matter, job_id, &spec) {
        Ok(o) => o,
        Err(e) => {
            let code = e.code();
            let msg = e.to_string();
            let _ = matter.record_item_error(ItemErrorInput {
                item_id: Some(inv.id.clone()),
                source_id: Some(source_id.to_string()),
                job_id: Some(job_id.to_string()),
                stage: STAGE_PST_EXTRACT.into(),
                code: code.into(),
                message: msg.clone(),
                detail: None,
            });
            let _ = matter.set_job_state(job_id, JobState::Failed, Some(&msg));
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "extract.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": msg, "code": code }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            })?;
            return Err(e);
        }
    };

    let folders = match opened.pst.folders() {
        Ok(f) => f,
        Err(e) => {
            let msg = format!("folder walk failed: {e}");
            fail_job(matter, job_id, "pst_open_failed", &msg)?;
            return Err(Error::PstOpenFailed(msg));
        }
    };

    let batch_size = limits.batch_size.max(1);
    let mut since_batch: u64 = 0;
    let mut run_messages: u64 = 0;
    let mut cancelled = false;
    // Safety cap hit mid-walk: job is incomplete and resumable (not Succeeded).
    let mut hit_max_messages = false;

    // Resume position: find starting folder/message index.
    let mut start_folder_idx = 0usize;
    let mut start_msg_idx = 0usize;
    if let Some(ref last_folder) = cursor.last_folder_path {
        if let Some(fi) = folders.iter().position(|f| &f.path == last_folder) {
            start_folder_idx = fi;
            if let Some(idx) = cursor.folder_message_index {
                // Resume after the last completed index.
                start_msg_idx = (idx as usize).saturating_add(1);
            }
        }
    }

    'folders: for (fi, folder) in folders.iter().enumerate() {
        if fi < start_folder_idx {
            continue;
        }
        let msg_start = if fi == start_folder_idx {
            start_msg_idx
        } else {
            0
        };

        for (mi, &msg_nid) in folder.message_nids.iter().enumerate() {
            if mi < msg_start {
                continue;
            }

            if cancel.map(|c| c()).unwrap_or(false) {
                cancelled = true;
                break 'folders;
            }

            if let Some(max) = limits.max_messages {
                if run_messages >= max {
                    // More messages remain in the walk; do not claim full success.
                    hit_max_messages = true;
                    break 'folders;
                }
            }

            let msg_path = message_path(&pst_path, &folder.path, msg_nid.0);

            // Never double-insert the same message path on re-extract / resume.
            // Message paths contain "!/"; inventory PST rows do not.
            // If a prior row exists for (source_id, path): refresh threading
            // header columns only (no re-CAS, no second insert). Full field
            // retry-with-update is deferred until unique path upsert exists.
            if let Some(existing) = matter.item_by_source_path(source_id, &msg_path)? {
                // Best-effort header refresh; failures leave prior columns as-is.
                if let Ok(extracted) = opened.pst.read_message_extract(msg_nid) {
                    let _ = refresh_thread_headers(matter, &existing.id, &extracted);
                }
                cursor.last_folder_path = Some(folder.path.clone());
                cursor.last_message_nid = Some(nid_hex(msg_nid.0));
                cursor.folder_message_index = Some(mi as i64);
                cursor.completed_count = cursor.completed_count.saturating_add(1);
                continue;
            }

            match extract_one_message(
                matter,
                source_id,
                job_id,
                &pst_path,
                &folder.path,
                msg_nid,
                &mut opened.pst,
                limits,
                &mut cursor,
            ) {
                Ok(()) => {
                    cursor.messages_ok = cursor.messages_ok.saturating_add(1);
                }
                Err(e) => {
                    cursor.messages_err = cursor.messages_err.saturating_add(1);
                    let _ = matter.record_item_error(ItemErrorInput {
                        item_id: None,
                        source_id: Some(source_id.to_string()),
                        job_id: Some(job_id.to_string()),
                        stage: STAGE_PST_EXTRACT.into(),
                        code: e.code().into(),
                        message: format!("{msg_path}: {e}"),
                        detail: Some(msg_path.clone()),
                    });
                }
            }

            cursor.last_folder_path = Some(folder.path.clone());
            cursor.last_message_nid = Some(nid_hex(msg_nid.0));
            cursor.folder_message_index = Some(mi as i64);
            cursor.completed_count = cursor.completed_count.saturating_add(1);
            since_batch += 1;
            run_messages += 1;

            if since_batch >= batch_size {
                write_checkpoint(matter, job_id, &cursor)?;
                since_batch = 0;
            }
        }
        // Folder complete: clear mid-folder index advance is already last message.
        write_checkpoint(matter, job_id, &cursor)?;
        since_batch = 0;
    }

    if since_batch > 0 || cancelled || hit_max_messages {
        write_checkpoint(matter, job_id, &cursor)?;
    }

    let completed = !cancelled && !hit_max_messages;
    let summary = ExtractSummary {
        source_id: source_id.to_string(),
        job_id: job_id.to_string(),
        messages_ok: cursor.messages_ok,
        messages_err: cursor.messages_err,
        attachments_ok: cursor.attachments_ok,
        attachments_err: cursor.attachments_err,
        completed,
        cancelled,
    };

    if cancelled {
        matter.set_job_state(job_id, JobState::Paused, Some("cancelled"))?;
        // No extract.complete on cancel; resume continues.
    } else if hit_max_messages {
        // max_messages is a safety cap: mid-PST stop is incomplete + resumable.
        matter.set_job_state(job_id, JobState::Paused, Some("max_messages"))?;
        let _ = matter.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "extract.paused".into(),
            entity: format!("job:{job_id}"),
            params_json: json!({
                "source_id": source_id,
                "reason": "max_messages",
                "max_messages": limits.max_messages,
                "messages_ok": summary.messages_ok,
                "messages_err": summary.messages_err,
                "attachments_ok": summary.attachments_ok,
                "attachments_err": summary.attachments_err,
                "completed_count": cursor.completed_count,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
    } else {
        matter.set_job_state(job_id, JobState::Succeeded, None)?;
        let _ = matter.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "extract.complete".into(),
            entity: format!("job:{job_id}"),
            params_json: json!({
                "source_id": source_id,
                "messages_ok": summary.messages_ok,
                "messages_err": summary.messages_err,
                "attachments_ok": summary.attachments_ok,
                "attachments_err": summary.attachments_err,
                "completed_count": cursor.completed_count,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
    }

    Ok(summary)
}

fn write_checkpoint(matter: &Matter, job_id: &str, cursor: &ExtractCursor) -> Result<()> {
    let json = cursor.to_json()?;
    matter.put_checkpoint(
        job_id,
        STAGE_PST_EXTRACT,
        &json,
        cursor.completed_count as i64,
    )?;
    Ok(())
}

fn fail_job(matter: &Matter, job_id: &str, code: &str, msg: &str) -> Result<()> {
    let _ = matter.set_job_state(job_id, JobState::Failed, Some(msg));
    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "extract.fail".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({ "error": msg, "code": code }).to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;
    Ok(())
}

fn message_path(pst_inventory_path: &str, folder_path: &str, message_nid: u64) -> String {
    format!(
        "{pst_inventory_path}!/{folder_path}/{}",
        nid_hex(message_nid)
    )
}

fn attach_path(msg_path: &str, index: usize, filename: &str) -> String {
    format!("{msg_path}/attach/{index}_{}", safe_filename(filename))
}

fn safe_filename(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if s.is_empty() {
        "attachment.bin".into()
    } else {
        s
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_one_message(
    matter: &Matter,
    source_id: &str,
    job_id: &str,
    pst_path: &str,
    folder_path: &str,
    msg_nid: NodeId,
    pst: &mut pst_reader::PstFile,
    limits: &ExtractLimits,
    cursor: &mut ExtractCursor,
) -> Result<()> {
    let msg_path = message_path(pst_path, folder_path, msg_nid.0);

    let extracted = pst
        .read_message_extract(msg_nid)
        .map_err(|e| Error::MessagePropsFailed(format!("nid={:x}: {e}", msg_nid.0)))?;

    let to = parse_display_list(extracted.display_to.as_deref());
    let cc = parse_display_list(extracted.display_cc.as_deref());
    let bcc = parse_display_list(extracted.display_bcc.as_deref());

    let cat = map_pst_message_category(extracted.message_class.as_deref());
    let is_calendar = cat.is_calendar;

    let cal_start_at = extracted.start_date.and_then(filetime_to_rfc3339);
    let cal_end_at = extracted.end_date.and_then(filetime_to_rfc3339);

    let mut sent_at = extracted.submit_time.and_then(filetime_to_rfc3339);
    let received_at = extracted.delivery_time.and_then(filetime_to_rfc3339);
    // Prefer cal_start_at for sort when email sent time missing (calendar path).
    if is_calendar && sent_at.is_none() {
        sent_at = cal_start_at.clone();
    }

    let body_raw = extracted.body_text.clone().unwrap_or_default();
    // Calendar review text: structured summary + description (spec §3.6).
    let display_body = if is_calendar {
        synthesize_calendar_review_text(
            &extracted,
            &to,
            cal_start_at.as_deref(),
            cal_end_at.as_deref(),
        )
    } else {
        body_raw.clone()
    };
    let body_for_hash = normalize_body(&display_body);
    let body_bytes = body_for_hash.as_bytes();

    let text_sha256 = if body_bytes.is_empty() {
        None
    } else if (body_bytes.len() as u64) <= limits.max_in_memory_put_bytes {
        Some(
            matter
                .put_bytes(body_bytes)
                .map_err(|e| Error::CasPutFailed(e.to_string()))?,
        )
    } else {
        let mut cur = std::io::Cursor::new(body_bytes);
        Some(
            matter
                .put_reader(&mut cur)
                .map_err(|e| Error::CasPutFailed(e.to_string()))?,
        )
    };

    let html_sha256 = if is_calendar {
        // Calendar path stores synthesized plain text only.
        None
    } else if let Some(ref html) = extracted.body_html {
        if html.is_empty() {
            None
        } else if (html.len() as u64) <= limits.max_in_memory_put_bytes {
            Some(
                matter
                    .put_bytes(html)
                    .map_err(|e| Error::CasPutFailed(e.to_string()))?,
            )
        } else {
            let mut cur = std::io::Cursor::new(html.as_slice());
            Some(
                matter
                    .put_reader(&mut cur)
                    .map_err(|e| Error::CasPutFailed(e.to_string()))?,
            )
        }
    } else {
        None
    };

    let mid = extracted
        .message_id
        .as_deref()
        .map(normalize_message_id)
        .filter(|s| !s.is_empty());

    let (in_reply_to, references_json, conversation_topic, conversation_index_hex) =
        thread_header_fields(&extracted);

    let file_category = cat.file_category;
    let from_addr = if is_calendar {
        // Organizer not available via standard tags in P0; fall back to sender.
        extracted.sender_email.clone()
    } else {
        extracted.sender_email.clone()
    };

    // Insert parent shell first so family can link.
    let family = matter.insert_family(FAMILY_KIND_EMAIL_ATTACHMENTS)?;
    let parent = matter.insert_item(ItemInput {
        source_id: Some(source_id.to_string()),
        family_id: Some(family.id.clone()),
        path: Some(msg_path.clone()),
        status: item_status::PARTIAL.to_string(),
        role: Some(item_role::PARENT.to_string()),
        mime_type: Some("application/vnd.ms-outlook".into()),
        file_category: Some(file_category.into()),
        message_id: mid.clone(),
        subject: extracted.subject.clone(),
        from_addr: from_addr.clone(),
        to_addrs_json: Some(serde_json::to_string(&to)?),
        cc_addrs_json: Some(serde_json::to_string(&cc)?),
        bcc_addrs_json: Some(serde_json::to_string(&bcc)?),
        sent_at: sent_at.clone(),
        received_at: received_at.clone(),
        size_bytes: extracted.message_size.map(|s| s as i64),
        text_sha256: text_sha256.clone(),
        html_sha256: html_sha256.clone(),
        in_reply_to,
        references_json,
        conversation_topic,
        conversation_index_hex,
        message_class: extracted.message_class.clone(),
        cal_start_at: if is_calendar {
            cal_start_at.clone()
        } else {
            None
        },
        cal_end_at: if is_calendar {
            cal_end_at.clone()
        } else {
            None
        },
        cal_location: if is_calendar {
            extracted.location.clone()
        } else {
            None
        },
        cal_organizer: if is_calendar { from_addr.clone() } else { None },
        cal_extract_method: cat.cal_extract_method.map(|s| s.into()),
        extra_json: Some(
            json!({
                "pst_nid": nid_hex(msg_nid.0),
                "folder": folder_path,
                "extract_tool": "extract-pst",
                "extract_version": env!("CARGO_PKG_VERSION"),
                "native_format": NATIVE_FORMAT_V1,
                "message_class": extracted.message_class,
            })
            .to_string(),
        ),
        ..Default::default()
    })?;

    // Attachments — never swallow enumeration failures as "zero attachments".
    let mut logical_atts: Vec<LogicalAttachment> = Vec::new();
    let mut native_atts: Vec<NativeAttachment> = Vec::new();
    let mut attach_err = 0u64;
    let mut parent_partial = false;

    let att_list = match pst.list_attachments(msg_nid) {
        Ok(list) => list,
        Err(e) => {
            // Still extract email fields with empty attach list, but mark partial.
            parent_partial = true;
            attach_err += 1;
            cursor.attachments_err = cursor.attachments_err.saturating_add(1);
            let msg = format!("list_attachments nid={:x}: {e}", msg_nid.0);
            let _ = matter.record_item_error(ItemErrorInput {
                item_id: Some(parent.id.clone()),
                source_id: Some(source_id.to_string()),
                job_id: Some(job_id.to_string()),
                stage: STAGE_PST_EXTRACT.into(),
                code: "attach_list_failed".into(),
                message: msg,
                detail: Some(msg_path.clone()),
            });
            Vec::new()
        }
    };

    for (idx, att) in att_list.iter().enumerate() {
        let a_path = attach_path(&msg_path, idx, &att.filename);
        let size_u64 = att.size as u64;

        if let Some(cap) = limits.max_attachment_bytes {
            if size_u64 > cap {
                attach_err += 1;
                parent_partial = true;
                cursor.attachments_err = cursor.attachments_err.saturating_add(1);
                let child = matter.insert_item(ItemInput {
                    source_id: Some(source_id.to_string()),
                    family_id: Some(family.id.clone()),
                    path: Some(a_path),
                    status: item_status::ERROR.to_string(),
                    role: Some(item_role::ATTACHMENT.to_string()),
                    parent_item_id: Some(parent.id.clone()),
                    file_category: Some("attachment".into()),
                    title: Some(att.filename.clone()),
                    size_bytes: Some(att.size as i64),
                    mime_type: att.mime_tag.clone(),
                    ..Default::default()
                })?;
                let _ = matter.set_item_family_role(
                    &child.id,
                    Some(&family.id),
                    item_role::ATTACHMENT,
                    Some(&parent.id),
                );
                let _ = matter.record_item_error(ItemErrorInput {
                    item_id: Some(child.id),
                    source_id: Some(source_id.to_string()),
                    job_id: Some(job_id.to_string()),
                    stage: STAGE_PST_EXTRACT.into(),
                    code: "attach_too_large".into(),
                    message: format!("attachment {} exceeds cap {cap}", att.filename),
                    detail: None,
                });
                continue;
            }
        }

        let digest = match pst.open_attachment_data(msg_nid, att.nid) {
            Ok(mut reader) => match matter.put_reader(&mut reader) {
                Ok(d) => d,
                Err(e) => {
                    attach_err += 1;
                    parent_partial = true;
                    cursor.attachments_err = cursor.attachments_err.saturating_add(1);
                    let child = matter.insert_item(ItemInput {
                        source_id: Some(source_id.to_string()),
                        family_id: Some(family.id.clone()),
                        path: Some(a_path),
                        status: item_status::ERROR.to_string(),
                        role: Some(item_role::ATTACHMENT.to_string()),
                        parent_item_id: Some(parent.id.clone()),
                        file_category: Some("attachment".into()),
                        title: Some(att.filename.clone()),
                        size_bytes: Some(att.size as i64),
                        ..Default::default()
                    })?;
                    let _ = matter.set_item_family_role(
                        &child.id,
                        Some(&family.id),
                        item_role::ATTACHMENT,
                        Some(&parent.id),
                    );
                    let _ = matter.record_item_error(ItemErrorInput {
                        item_id: Some(child.id),
                        source_id: Some(source_id.to_string()),
                        job_id: Some(job_id.to_string()),
                        stage: STAGE_PST_EXTRACT.into(),
                        code: "cas_put_failed".into(),
                        message: e.to_string(),
                        detail: None,
                    });
                    continue;
                }
            },
            Err(e) => {
                attach_err += 1;
                parent_partial = true;
                cursor.attachments_err = cursor.attachments_err.saturating_add(1);
                let child = matter.insert_item(ItemInput {
                    source_id: Some(source_id.to_string()),
                    family_id: Some(family.id.clone()),
                    path: Some(a_path),
                    status: item_status::ERROR.to_string(),
                    role: Some(item_role::ATTACHMENT.to_string()),
                    parent_item_id: Some(parent.id.clone()),
                    file_category: Some("attachment".into()),
                    title: Some(att.filename.clone()),
                    size_bytes: Some(att.size as i64),
                    ..Default::default()
                })?;
                let _ = matter.set_item_family_role(
                    &child.id,
                    Some(&family.id),
                    item_role::ATTACHMENT,
                    Some(&parent.id),
                );
                let _ = matter.record_item_error(ItemErrorInput {
                    item_id: Some(child.id),
                    source_id: Some(source_id.to_string()),
                    job_id: Some(job_id.to_string()),
                    stage: STAGE_PST_EXTRACT.into(),
                    code: "attach_data_missing".into(),
                    message: e.to_string(),
                    detail: None,
                });
                continue;
            }
        };

        let child = matter.insert_item(ItemInput {
            source_id: Some(source_id.to_string()),
            family_id: Some(family.id.clone()),
            path: Some(a_path),
            native_sha256: Some(digest.clone()),
            status: item_status::EXTRACTED.to_string(),
            role: Some(item_role::ATTACHMENT.to_string()),
            parent_item_id: Some(parent.id.clone()),
            file_category: Some("attachment".into()),
            title: Some(att.filename.clone()),
            size_bytes: Some(att.size as i64),
            mime_type: att.mime_tag.clone(),
            ..Default::default()
        })?;
        let _ = matter.set_item_family_role(
            &child.id,
            Some(&family.id),
            item_role::ATTACHMENT,
            Some(&parent.id),
        );

        logical_atts.push(LogicalAttachment {
            filename: att.filename.clone(),
            size: size_u64,
            native_sha256: digest.clone(),
        });
        native_atts.push(NativeAttachment {
            filename: att.filename.clone(),
            size: size_u64,
            native_sha256: digest,
        });
        cursor.attachments_ok = cursor.attachments_ok.saturating_add(1);
    }

    // Native message blob
    let native = NativeMessageV1 {
        message_nid: msg_nid.0,
        message_id: extracted.message_id.clone().unwrap_or_default(),
        subject: extracted.subject.clone().unwrap_or_default(),
        from: extracted.sender_email.clone().unwrap_or_default(),
        to: to.join("; "),
        cc: cc.join("; "),
        bcc: bcc.join("; "),
        sent: sent_at.clone().unwrap_or_default(),
        received: received_at.clone().unwrap_or_default(),
        body: body_bytes.to_vec(),
        attachments: native_atts,
    };
    let native_bytes = encode_native_message_v1(&native);
    let native_sha256 = matter
        .put_bytes(&native_bytes)
        .map_err(|e| Error::CasPutFailed(e.to_string()))?;

    // Logical hash: email MID path when Message-ID present; pure calendar without
    // Message-ID uses non-email preimage (spec §3.7). Meeting requests that also
    // carry Message-ID keep the email path for exact dups of the message.
    let logical_hash = if is_calendar && mid.is_none() {
        let child_digests: Vec<String> = logical_atts
            .iter()
            .map(|a| a.native_sha256.clone())
            .collect();
        compute_non_email_logical_hash(&NonEmailLogicalInput {
            category: Some("calendar".into()),
            title: extracted.subject.clone(),
            author: from_addr.clone(),
            created: cal_start_at.clone().or(sent_at.clone()),
            text: Some(body_for_hash.clone()),
            children_native_sha256: child_digests,
        })
    } else {
        // Email path (IPM.Note etc.) and calendar-with-MID: always pass bcc.
        compute_email_logical_hash(&EmailLogicalInput {
            message_id: extracted.message_id.clone(),
            subject: extracted.subject.clone(),
            from: extracted.sender_email.clone(),
            to: to.clone(),
            cc: cc.clone(),
            bcc: bcc.clone(),
            sent: sent_at.clone(),
            received: received_at.clone(),
            body: Some(body_for_hash.clone()),
            attachments: logical_atts,
        })
    };

    let status = if parent_partial || attach_err > 0 {
        item_status::PARTIAL
    } else {
        item_status::EXTRACTED
    };

    matter.update_item(
        &parent.id,
        ItemUpdate {
            native_sha256: Some(Some(native_sha256)),
            logical_hash: Some(Some(logical_hash)),
            logical_hash_version: Some(LOGICAL_HASH_VERSION),
            status: Some(status.to_string()),
            text_sha256: Some(text_sha256),
            html_sha256: Some(html_sha256),
            attachment_count: Some(Some(att_list.len() as i64)),
            ..Default::default()
        },
    )?;
    let _ = matter.set_item_family_role(&parent.id, Some(&family.id), item_role::PARENT, None);

    let _ = job_id; // used in error paths
    Ok(())
}

/// Extract-path category mapping from `PidTagMessageClass` (spec §3.2 / §3.9).
///
/// Isolated so unit tests can assert `file_category` / `cal_extract_method`
/// without opening a full PST.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PstMessageCategoryMapping {
    file_category: &'static str,
    cal_extract_method: Option<&'static str>,
    is_calendar: bool,
}

fn map_pst_message_category(message_class: Option<&str>) -> PstMessageCategoryMapping {
    let is_calendar = message_class.is_some_and(is_calendar_message_class);
    if is_calendar {
        PstMessageCategoryMapping {
            file_category: "calendar",
            cal_extract_method: Some("pst_oxocal_v1"),
            is_calendar: true,
        }
    } else {
        PstMessageCategoryMapping {
            file_category: "email",
            cal_extract_method: None,
            is_calendar: false,
        }
    }
}

/// Synthesize review plain-text for calendar items (spec §3.6).
fn synthesize_calendar_review_text(
    extracted: &ExtractedMessage,
    attendees: &[String],
    start: Option<&str>,
    end: Option<&str>,
) -> String {
    let subject = extracted.subject.as_deref().unwrap_or("");
    let when = match (start, end) {
        (Some(s), Some(e)) => format!("{s} – {e}"),
        (Some(s), None) => s.to_string(),
        (None, Some(e)) => format!("– {e}"),
        (None, None) => String::new(),
    };
    let where_ = extracted.location.as_deref().unwrap_or("");
    let organizer = extracted.sender_email.as_deref().unwrap_or("");
    let att_line = attendees.join("; ");
    let class = extracted.message_class.as_deref().unwrap_or("");
    let description = extracted.body_text.as_deref().unwrap_or("");
    format!(
        "Subject: {subject}\n\
         When: {when}\n\
         Where: {where_}\n\
         Organizer: {organizer}\n\
         Attendees: {att_line}\n\
         Busy: \n\
         Class: {class}\n\
         ---\n\
         {description}"
    )
}

/// Normalize reply-chain / conversation header fields for parent insert.
///
/// Missing props → `None` (never fabricate). Matters extracted before track 0022
/// lack these columns until re-extract (which refreshes headers on the existing
/// path skip).
fn thread_header_fields(
    extracted: &ExtractedMessage,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let in_reply_to = extracted.in_reply_to.as_deref().and_then(parse_in_reply_to);

    let references_json = extracted.references.as_deref().and_then(|raw| {
        let refs = parse_references_header(raw);
        if refs.is_empty() {
            None
        } else {
            Some(references_to_json(&refs))
        }
    });

    let conversation_topic = extracted
        .conversation_topic
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let conversation_index_hex = if let Some(ref bytes) = extracted.conversation_index_bytes {
        normalize_conversation_index_to_hex(ConversationIndexInput::Bytes(bytes))
    } else if let Some(ref s) = extracted.conversation_index_string {
        normalize_conversation_index_to_hex(ConversationIndexInput::Base64(s))
    } else {
        None
    };

    (
        in_reply_to,
        references_json,
        conversation_topic,
        conversation_index_hex,
    )
}

/// Update only the four threading header columns on an existing parent item.
///
/// Used on re-extract when `(source_id, path)` already exists: refresh headers
/// without double-insert or re-CAS of body/attachments. Nested `Some(...)`
/// always writes (inner `None` → SQL NULL when the prop is absent).
fn refresh_thread_headers(
    matter: &Matter,
    item_id: &str,
    extracted: &ExtractedMessage,
) -> Result<()> {
    let (in_reply_to, references_json, conversation_topic, conversation_index_hex) =
        thread_header_fields(extracted);
    matter.update_item(
        item_id,
        ItemUpdate {
            in_reply_to: Some(in_reply_to),
            references_json: Some(references_json),
            conversation_topic: Some(conversation_topic),
            conversation_index_hex: Some(conversation_index_hex),
            ..Default::default()
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_header_fields_normalize_references_and_index() {
        let mut msg = ExtractedMessage {
            nid: NodeId(1),
            message_id: Some("<a@ex.com>".into()),
            subject: None,
            sender_email: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            submit_time: None,
            delivery_time: None,
            body_text: None,
            body_html: None,
            message_size: None,
            has_attachments: None,
            in_reply_to: Some("<Parent@Ex.COM>".into()),
            references: Some("<one@ex.com>\r\n\t<two@ex.com>".into()),
            conversation_topic: Some("  Topic  ".into()),
            conversation_index_bytes: Some(vec![0x01, 0x02, 0x03]),
            conversation_index_string: None,
            message_class: None,
            start_date: None,
            end_date: None,
            location: None,
        };
        let (irt, refs, topic, ci) = thread_header_fields(&msg);
        assert_eq!(irt.as_deref(), Some("parent@ex.com"));
        let refs_v: Vec<String> = serde_json::from_str(refs.as_deref().unwrap()).unwrap();
        assert_eq!(refs_v, vec!["one@ex.com", "two@ex.com"]);
        assert_eq!(topic.as_deref(), Some("Topic"));
        assert_eq!(ci.as_deref(), Some("010203"));

        // Base64 path when binary absent
        msg.conversation_index_bytes = None;
        // "AQID" is base64 of [1,2,3]
        msg.conversation_index_string = Some("AQID".into());
        let (_, _, _, ci2) = thread_header_fields(&msg);
        assert_eq!(ci2.as_deref(), Some("010203"));
    }

    #[test]
    fn thread_header_fields_missing_are_none() {
        let msg = ExtractedMessage {
            nid: NodeId(1),
            message_id: None,
            subject: None,
            sender_email: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            submit_time: None,
            delivery_time: None,
            body_text: None,
            body_html: None,
            message_size: None,
            has_attachments: None,
            in_reply_to: None,
            references: None,
            conversation_topic: None,
            conversation_index_bytes: None,
            conversation_index_string: None,
            message_class: None,
            start_date: None,
            end_date: None,
            location: None,
        };
        let (a, b, c, d) = thread_header_fields(&msg);
        assert!(a.is_none() && b.is_none() && c.is_none() && d.is_none());
    }

    #[test]
    fn calendar_review_text_contains_markers() {
        let msg = ExtractedMessage {
            nid: NodeId(1),
            message_id: None,
            subject: Some("Standup".into()),
            sender_email: Some("org@ex.com".into()),
            display_to: Some("a@ex.com".into()),
            display_cc: None,
            display_bcc: None,
            submit_time: None,
            delivery_time: None,
            body_text: Some("Bring notes".into()),
            body_html: None,
            message_size: None,
            has_attachments: None,
            in_reply_to: None,
            references: None,
            conversation_topic: None,
            conversation_index_bytes: None,
            conversation_index_string: None,
            message_class: Some("IPM.Appointment".into()),
            start_date: None,
            end_date: None,
            location: Some("Room 1".into()),
        };
        let text = synthesize_calendar_review_text(
            &msg,
            &["a@ex.com".into()],
            Some("2026-07-18T14:00:00Z"),
            Some("2026-07-18T14:30:00Z"),
        );
        assert!(text.contains("Subject: Standup"));
        assert!(text.contains("Where: Room 1"));
        assert!(text.contains("Class: IPM.Appointment"));
        assert!(text.contains("Bring notes"));
        assert!(is_calendar_message_class("IPM.Appointment"));
        assert!(!is_calendar_message_class("IPM.Note"));
    }

    /// Spec §3.9 cases 7–8: calendar MessageClass → calendar category; Note stays email.
    #[test]
    fn pst_message_category_mapping_calendar_and_note() {
        let appt = map_pst_message_category(Some("IPM.Appointment"));
        assert_eq!(appt.file_category, "calendar");
        assert_eq!(appt.cal_extract_method, Some("pst_oxocal_v1"));
        assert!(appt.is_calendar);
        assert!(is_calendar_message_class("IPM.Appointment"));

        let meeting = map_pst_message_category(Some("IPM.Schedule.Meeting.Request"));
        assert_eq!(meeting.file_category, "calendar");
        assert_eq!(meeting.cal_extract_method, Some("pst_oxocal_v1"));
        assert!(meeting.is_calendar);

        let note = map_pst_message_category(Some("IPM.Note"));
        assert_eq!(note.file_category, "email");
        assert_eq!(note.cal_extract_method, None);
        assert!(!note.is_calendar);
        assert!(!is_calendar_message_class("IPM.Note"));

        let missing = map_pst_message_category(None);
        assert_eq!(missing.file_category, "email");
        assert_eq!(missing.cal_extract_method, None);
        assert!(!missing.is_calendar);
    }

    #[test]
    fn reextract_refresh_populates_headers_on_existing_parent() {
        // Simulate: parent inserted without headers → refresh path → headers set.
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
        let root = root.join("reextract-headers");
        let matter = Matter::create(&root, "ReextractHeaders").expect("create");
        let parent = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::PARENT.into()),
                file_category: Some("email".into()),
                path: Some("mail.pst!/Inbox/abc".into()),
                message_id: Some("child@ex.com".into()),
                // No threading headers on first extract (pre-0022 shape).
                in_reply_to: None,
                references_json: None,
                conversation_topic: None,
                conversation_index_hex: None,
                ..Default::default()
            })
            .expect("insert");
        assert!(parent.in_reply_to.is_none());
        assert!(parent.references_json.is_none());

        let extracted = ExtractedMessage {
            nid: NodeId(1),
            message_id: Some("<child@ex.com>".into()),
            subject: Some("Re: Topic".into()),
            sender_email: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            submit_time: None,
            delivery_time: None,
            body_text: None,
            body_html: None,
            message_size: None,
            has_attachments: None,
            in_reply_to: Some("<Parent@Ex.COM>".into()),
            references: Some("<root@ex.com> <Parent@Ex.COM>".into()),
            conversation_topic: Some("Topic".into()),
            conversation_index_bytes: Some(vec![0xaa, 0xbb]),
            conversation_index_string: None,
            message_class: None,
            start_date: None,
            end_date: None,
            location: None,
        };
        refresh_thread_headers(&matter, &parent.id, &extracted).expect("refresh");

        let updated = matter.get_item(&parent.id).expect("get");
        assert_eq!(updated.in_reply_to.as_deref(), Some("parent@ex.com"));
        let refs: Vec<String> =
            serde_json::from_str(updated.references_json.as_deref().unwrap()).unwrap();
        assert_eq!(refs, vec!["root@ex.com", "parent@ex.com"]);
        assert_eq!(updated.conversation_topic.as_deref(), Some("Topic"));
        assert_eq!(updated.conversation_index_hex.as_deref(), Some("aabb"));
        // Other fields unchanged
        assert_eq!(updated.message_id.as_deref(), Some("child@ex.com"));
        assert_eq!(updated.status, item_status::EXTRACTED);
    }
}
