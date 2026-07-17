//! Blocking PST extract → Normalized Items.

use matter_core::{
    compute_email_logical_hash, item_role, item_status, normalize_body, normalize_message_id,
    AuditEventInput, EmailLogicalInput, Item, ItemErrorInput, ItemInput, ItemUpdate, JobState,
    LogicalAttachment, Matter, FAMILY_KIND_EMAIL_ATTACHMENTS, LOGICAL_HASH_VERSION,
};
use pst_reader::{filetime_to_rfc3339, NodeId};
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
/// **Caller contract:** invoke from a dedicated blocking worker only.
pub fn extract_pst_item(
    matter: &Matter,
    source_id: &str,
    pst_item_id: &str,
    limits: &ExtractLimits,
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

    let job = matter.create_job(JOB_KIND_EXTRACT_PST)?;
    matter.set_job_state(&job.id, JobState::Running, None)?;

    let cursor = ExtractCursor::new(
        source_id,
        &pst_path,
        pst_item_id,
        inv.native_sha256.as_deref(),
        limits.batch_size.max(1),
    );

    run_extract(
        matter, source_id, &inv, &job.id, cursor, limits, cancel, false,
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
        matter, source_id, &inv, job_id, cursor, limits, cancel, true,
    )
}

/// Optional path-based entry: register a minimal inventory row then extract.
///
/// Streams the PST into CAS via [`Matter::put_reader`] (no full-file `Vec`).
/// Open still prefers the filesystem path when present.
pub fn extract_pst_path(
    matter: &Matter,
    source_id: &str,
    pst_fs_path: &str,
    limits: &ExtractLimits,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ExtractSummary> {
    use std::fs;

    let path = camino::Utf8Path::new(pst_fs_path);
    if !path.as_std_path().is_file() {
        return Err(Error::PstOpenFailed(format!("not a file: {pst_fs_path}")));
    }
    let meta = fs::metadata(path.as_std_path())?;
    let size_bytes = meta.len() as i64;
    let mut file = fs::File::open(path.as_std_path())?;
    let digest = matter
        .put_reader(&mut file)
        .map_err(|e| Error::CasPutFailed(e.to_string()))?;
    let logical = path.file_name().unwrap_or(pst_fs_path).to_string();
    let item = matter.insert_item(ItemInput {
        source_id: Some(source_id.to_string()),
        path: Some(logical),
        native_sha256: Some(digest),
        status: item_status::DISCOVERED.to_string(),
        size_bytes: Some(size_bytes),
        file_category: Some("pst".into()),
        ..Default::default()
    })?;
    extract_pst_item(matter, source_id, &item.id, limits, cancel)
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
) -> Result<ExtractSummary> {
    let pst_path = inv.path.clone().unwrap_or_else(|| cursor.pst_path.clone());
    let source = matter.get_source(source_id)?;

    let fs_candidate = candidate_fs_path(&source.path, &pst_path);
    let spec = PstOpenSpec {
        inventory_path: pst_path.clone(),
        native_sha256: inv
            .native_sha256
            .clone()
            .or(cursor.pst_native_sha256.clone()),
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
            // Skip if any prior row exists for (source_id, path): extracted,
            // partial (with or without hash), error, or other. Retry-with-update
            // is deferred — insert_item has no unique path key yet.
            if matter.item_by_source_path(source_id, &msg_path)?.is_some() {
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

    let sent_at = extracted.submit_time.and_then(filetime_to_rfc3339);
    let received_at = extracted.delivery_time.and_then(filetime_to_rfc3339);

    let body_raw = extracted.body_text.clone().unwrap_or_default();
    let body_for_hash = normalize_body(&body_raw);
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

    let html_sha256 = if let Some(ref html) = extracted.body_html {
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

    // Insert parent shell first so family can link.
    let family = matter.insert_family(FAMILY_KIND_EMAIL_ATTACHMENTS)?;
    let parent = matter.insert_item(ItemInput {
        source_id: Some(source_id.to_string()),
        family_id: Some(family.id.clone()),
        path: Some(msg_path.clone()),
        status: item_status::PARTIAL.to_string(),
        role: Some(item_role::PARENT.to_string()),
        mime_type: Some("application/vnd.ms-outlook".into()),
        file_category: Some("email".into()),
        message_id: mid.clone(),
        subject: extracted.subject.clone(),
        from_addr: extracted.sender_email.clone(),
        to_addrs_json: Some(serde_json::to_string(&to)?),
        cc_addrs_json: Some(serde_json::to_string(&cc)?),
        bcc_addrs_json: Some(serde_json::to_string(&bcc)?),
        sent_at: sent_at.clone(),
        received_at: received_at.clone(),
        size_bytes: extracted.message_size.map(|s| s as i64),
        text_sha256: text_sha256.clone(),
        html_sha256: html_sha256.clone(),
        extra_json: Some(
            json!({
                "pst_nid": nid_hex(msg_nid.0),
                "folder": folder_path,
                "extract_tool": "extract-pst",
                "extract_version": env!("CARGO_PKG_VERSION"),
                "native_format": NATIVE_FORMAT_V1,
            })
            .to_string(),
        ),
        ..Default::default()
    })?;

    // Attachments
    let att_list = pst.list_attachments(msg_nid).unwrap_or_default();
    let mut logical_atts: Vec<LogicalAttachment> = Vec::new();
    let mut native_atts: Vec<NativeAttachment> = Vec::new();
    let mut attach_err = 0u64;
    let mut parent_partial = false;

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

    // Logical hash via matter-core (always pass bcc).
    let logical_input = EmailLogicalInput {
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
    };
    let logical_hash = compute_email_logical_hash(&logical_input);

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
