//! Core produce job: select → withhold gate → control numbers → package → DAT.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use chrono::Utc;
use matter_core::{join_addrs_json, path_basename, AuditEventInput, Item, Matter, EXPORTS_DIR};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::dat::{format_utc_datetime, write_load_csv, write_load_dat, LoadRow};
use crate::error::{ProduceError, Result};
use crate::layout::{
    production_stamp, resolve_output_root, sanitize_filename_part, write_readme, VolumeLayout,
    PRODUCTIONS_DIR,
};
use crate::params::{ProduceParams, SCOPE_ITEM_IDS, SCOPE_REVIEW_CORPUS};
use crate::resolve::{is_email_like, load_body_for_eml, resolve_native, resolve_text};

/// Job kind string for process-runner.
pub const JOB_KIND_PRODUCE: &str = "produce";
/// Accepted alias for job kind registration / docs.
pub const JOB_KIND_PRODUCTION_EXPORT: &str = "production_export";
/// Checkpoint stage name.
pub const PRODUCE_STAGE: &str = "produce";

/// Summary counts after a produce run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProduceSummary {
    pub completed_count: u64,
    pub selected_count: u64,
    pub produced_count: u64,
    pub skipped_withheld: u64,
    pub skipped_other: u64,
    pub error_count: u64,
    pub production_set_id: String,
    pub production_name: String,
    pub output_root: String,
    pub next_seq: u64,
}

/// Outcome of [`run_produce`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProduceOutcome {
    Succeeded(ProduceSummary),
    Paused(ProduceSummary),
    Failed {
        message: String,
        summary: ProduceSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    /// `work` | `finalize` | `done`
    #[serde(default = "default_phase_work")]
    phase: String,
    /// Index into ordered item id list.
    cursor_index: u64,
    completed_count: u64,
    selected_count: u64,
    produced_count: u64,
    skipped_withheld: u64,
    skipped_other: u64,
    error_count: u64,
    next_seq: u64,
    production_set_id: String,
    production_name: String,
    output_root: String,
    params: serde_json::Value,
    /// Frozen selection for stable resume.
    #[serde(default)]
    ordered_ids: Vec<String>,
    /// Item ids already fully processed (ok/skipped/error) — no renumber.
    #[serde(default)]
    done_item_ids: Vec<String>,
}

fn default_phase_work() -> String {
    "work".into()
}

fn summary_from_cursor(c: &CheckpointCursor) -> ProduceSummary {
    ProduceSummary {
        completed_count: c.completed_count,
        selected_count: c.selected_count,
        produced_count: c.produced_count,
        skipped_withheld: c.skipped_withheld,
        skipped_other: c.skipped_other,
        error_count: c.error_count,
        production_set_id: c.production_set_id.clone(),
        production_name: c.production_name.clone(),
        output_root: c.output_root.clone(),
        next_seq: c.next_seq,
    }
}

/// Run produce on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between items.
/// Calls `progress(completed_count)` after each item.
pub fn run_produce(
    matter: &Matter,
    job_id: &str,
    params: &ProduceParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<ProduceOutcome> {
    let started = Instant::now();
    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = resolve_params(params, prior.as_ref())?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "produce.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({ "params": params_json }).to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_produce_inner(
        matter,
        job_id,
        &effective,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(ProduceOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "produce.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "selected": s.selected_count,
                    "produced": s.produced_count,
                    "skipped_withheld": s.skipped_withheld,
                    "skipped_other": s.skipped_other,
                    "errors": s.error_count,
                    "production_set_id": s.production_set_id,
                    "output_root": s.output_root,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(ProduceOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(ProduceOutcome::Paused(_)) => {}
        Ok(ProduceOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "produce.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "selected": summary.selected_count,
                    "produced": summary.produced_count,
                    "skipped_withheld": summary.skipped_withheld,
                    "skipped_other": summary.skipped_other,
                    "errors": summary.error_count,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(ProduceError::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            // Best-effort: mark production_set failed and include counts from checkpoint.
            let fail_summary = mark_failed_from_checkpoint(matter, job_id);
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "produce.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": e.to_string(),
                    "selected": fail_summary.selected_count,
                    "produced": fail_summary.produced_count,
                    "skipped_withheld": fail_summary.skipped_withheld,
                    "skipped_other": fail_summary.skipped_other,
                    "errors": fail_summary.error_count,
                    "production_set_id": fail_summary.production_set_id,
                    "output_root": fail_summary.output_root,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(ProduceError::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

/// On hard `Err` from the inner run, mark any known production set as `failed`
/// and return the best-known summary for the audit payload.
fn mark_failed_from_checkpoint(matter: &Matter, job_id: &str) -> ProduceSummary {
    let Ok(Some(cursor)) = load_prior_checkpoint(matter, job_id) else {
        return ProduceSummary::default();
    };
    if !cursor.production_set_id.is_empty() {
        let _ = update_production_set_status(
            matter,
            &cursor.production_set_id,
            "failed",
            cursor.next_seq,
        );
    }
    summary_from_cursor(&cursor)
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, PRODUCE_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(ProduceError::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn resolve_params(call: &ProduceParams, prior: Option<&CheckpointCursor>) -> Result<ProduceParams> {
    if let Some(prior) = prior {
        if let Some(obj) = prior.params.as_object() {
            if !obj.is_empty() {
                return ProduceParams::from_json(&prior.params.to_string())
                    .map_err(|e| ProduceError::InvalidParams(format!("checkpoint params: {e}")));
            }
        }
    }
    let effective = call.clone();
    effective.validate_shape()?;
    Ok(effective)
}

fn run_produce_inner(
    matter: &Matter,
    job_id: &str,
    params: &ProduceParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<ProduceOutcome> {
    let mut cursor = if let Some(mut prior) = prior {
        prior.params = params_json.clone();
        prior
    } else {
        CheckpointCursor {
            phase: "work".into(),
            cursor_index: 0,
            completed_count: 0,
            selected_count: 0,
            produced_count: 0,
            skipped_withheld: 0,
            skipped_other: 0,
            error_count: 0,
            next_seq: 1,
            production_set_id: String::new(),
            production_name: String::new(),
            output_root: String::new(),
            params: params_json.clone(),
            ordered_ids: Vec::new(),
            done_item_ids: Vec::new(),
        }
    };

    // Finalize phase only: rewrite load files + README from production_items.
    if cursor.phase == "done" {
        return Ok(ProduceOutcome::Succeeded(summary_from_cursor(&cursor)));
    }
    if cursor.phase == "finalize" {
        return finalize_volume(matter, job_id, params, &mut cursor);
    }

    // Fresh selection + production set.
    if cursor.ordered_ids.is_empty() {
        let ordered = select_item_ids(matter, params)?;
        if ordered.is_empty() {
            return Ok(ProduceOutcome::Failed {
                message:
                    "empty selection: no items to produce (review corpus empty or item_ids empty)"
                        .into(),
                summary: summary_from_cursor(&cursor),
            });
        }

        // Fail-closed withhold scan before any assignment when requested.
        if params.fail_if_withheld {
            for id in &ordered {
                if matter.item_is_withheld(id)? {
                    return Ok(ProduceOutcome::Failed {
                        message: format!(
                            "fail_if_withheld: item {id} is withheld; aborting production"
                        ),
                        summary: summary_from_cursor(&cursor),
                    });
                }
            }
        }

        let output_root = resolve_output_root(matter, params)?;
        // Safety: default path stays under matter exports/productions.
        if params.output_dir.is_none() {
            let expected_prefix = matter.root().join(EXPORTS_DIR).join(PRODUCTIONS_DIR);
            if !output_root.as_str().starts_with(expected_prefix.as_str()) {
                return Ok(ProduceOutcome::Failed {
                    message: format!(
                        "internal: default output_root {} not under {}",
                        output_root, expected_prefix
                    ),
                    summary: summary_from_cursor(&cursor),
                });
            }
        }

        let name = params
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| production_stamp(params));

        let set_id = create_production_set(
            matter,
            job_id,
            &name,
            params.bates_prefix_clean(),
            params_json,
            output_root.as_str(),
        )?;

        cursor.ordered_ids = ordered;
        cursor.selected_count = cursor.ordered_ids.len() as u64;
        cursor.production_set_id = set_id;
        cursor.production_name = name;
        cursor.output_root = output_root.to_string();
        cursor.next_seq = 1;
        save_checkpoint(matter, job_id, &cursor)?;
    }

    let layout = VolumeLayout::create(camino::Utf8Path::new(&cursor.output_root))?;
    // Index existing rows.jsonl so resume after crash-between-append-and-checkpoint
    // does not re-produce / re-append (idempotent by ITEM_ID).
    let mut jsonl_by_item = index_rows_jsonl_by_item_id(&layout)?;
    let mut done: HashSet<String> = cursor.done_item_ids.iter().cloned().collect();

    // Late-withhold sweep: a hold asserted after produce/pause must purge any
    // prior JSONL row and artifacts before recovery or finalize includes them.
    if let Some(failed) = apply_late_withhold_sweep(
        matter,
        params,
        &layout,
        &mut cursor,
        &mut jsonl_by_item,
        &mut done,
    )? {
        return Ok(failed);
    }

    let total = cursor.ordered_ids.len();
    let mut offset = cursor.cursor_index as usize;
    if offset > total {
        return Err(ProduceError::Other(format!(
            "checkpoint cursor_index {offset} exceeds selection count {total}"
        )));
    }

    while offset < total {
        if cancel.map(|f| f()).unwrap_or(false) {
            cursor.cursor_index = offset as u64;
            cursor.phase = "work".into();
            update_production_set_status(
                matter,
                &cursor.production_set_id,
                "partial",
                cursor.next_seq,
            )?;
            save_checkpoint(matter, job_id, &cursor)?;
            return Ok(ProduceOutcome::Paused(summary_from_cursor(&cursor)));
        }

        let item_id = cursor.ordered_ids[offset].clone();
        if done.contains(&item_id) {
            offset += 1;
            cursor.cursor_index = offset as u64;
            cursor.completed_count = offset as u64;
            continue;
        }

        // Withhold gate first — also covers late withhold after a prior produce of this item.
        // Must run before JSONL recovery so a hold asserted between pause and resume never
        // leaves the item in the final DAT/NATIVES/TEXT.
        if matter.item_is_withheld(&item_id)? {
            if params.fail_if_withheld {
                // Drop any recovered artifacts/JSONL so partial volumes are not left dirty.
                if let Some(existing) = jsonl_by_item.remove(&item_id) {
                    let _ = remove_row_artifacts(&layout, &existing);
                    rewrite_rows_jsonl_excluding(&layout, &item_id)?;
                }
                update_production_set_status(
                    matter,
                    &cursor.production_set_id,
                    "failed",
                    cursor.next_seq,
                )?;
                return Ok(ProduceOutcome::Failed {
                    message: format!(
                        "fail_if_withheld: item {item_id} is withheld; aborting production"
                    ),
                    summary: summary_from_cursor(&cursor),
                });
            }
            // Invalidate any prior JSONL/native/text for this control.
            if let Some(existing) = jsonl_by_item.remove(&item_id) {
                let _ = remove_row_artifacts(&layout, &existing);
                rewrite_rows_jsonl_excluding(&layout, &item_id)?;
            }
            // Count once: late-withhold sweep may already have recorded this item.
            if !production_item_is_skipped_withheld(matter, &cursor.production_set_id, &item_id)? {
                cursor.skipped_withheld += 1;
                record_production_item(
                    matter,
                    &cursor.production_set_id,
                    &item_id,
                    "",
                    None,
                    None,
                    "skipped",
                    Some("withheld"),
                    None,
                )?;
            }
            // Never write natives/text/DAT for withheld.
            cursor.done_item_ids.push(item_id.clone());
            done.insert(item_id);
            offset += 1;
            cursor.cursor_index = offset as u64;
            cursor.completed_count = offset as u64;
            save_checkpoint(matter, job_id, &cursor)?;
            progress(cursor.completed_count);
            continue;
        }

        // Crash window recovery: JSONL already has a complete row for this ITEM_ID
        // (append succeeded, checkpoint did not). Reuse that control only when
        // referenced artifacts still exist under the volume (and SHA matches when set).
        if let Some(existing) = jsonl_by_item.get(&item_id).cloned() {
            if recovered_artifacts_valid(&layout, &existing) {
                recover_done_from_existing_row(
                    &mut cursor,
                    &mut done,
                    params.bates_prefix_clean(),
                    &item_id,
                    &existing.control_number,
                );
                offset += 1;
                cursor.cursor_index = offset as u64;
                cursor.completed_count = offset as u64;
                save_checkpoint(matter, job_id, &cursor)?;
                progress(cursor.completed_count);
                continue;
            }
            // Invalid / missing artifacts: drop the stale JSONL row and re-produce.
            jsonl_by_item.remove(&item_id);
            let _ = remove_row_artifacts(&layout, &existing);
            rewrite_rows_jsonl_excluding(&layout, &item_id)?;
            // Fall through — prior control from production_items / existing row is reused.
        }

        // production_items status=ok without JSONL: re-use control if recorded, then re-produce
        // so the load row is written (crash between DB record and JSONL append).
        let prior_ok_control =
            production_item_ok_control(matter, &cursor.production_set_id, &item_id)?;

        let item = match matter.get_item(&item_id) {
            Ok(i) => i,
            Err(e) => {
                cursor.error_count += 1;
                record_production_item(
                    matter,
                    &cursor.production_set_id,
                    &item_id,
                    "",
                    None,
                    None,
                    "error",
                    None,
                    Some(&format!("get_item: {e}")),
                )?;
                cursor.done_item_ids.push(item_id.clone());
                done.insert(item_id);
                offset += 1;
                cursor.cursor_index = offset as u64;
                cursor.completed_count = offset as u64;
                save_checkpoint(matter, job_id, &cursor)?;
                progress(cursor.completed_count);
                continue;
            }
        };

        // Assign control number from next_seq (monotonic; never renumber done rows).
        // If production_items already recorded ok with a control, reuse that control and
        // do not burn a new sequence value.
        let control_safe = if let Some(prior) = prior_ok_control.as_deref() {
            let safe = sanitize_filename_part(prior);
            advance_next_seq_for_control(&mut cursor, params.bates_prefix_clean(), prior);
            safe
        } else {
            let control = format_control(
                params.bates_prefix_clean(),
                cursor.next_seq,
                params.seq_width,
            );
            let safe = sanitize_filename_part(&control);
            cursor.next_seq += 1;
            safe
        };

        match produce_one_item(matter, params, &item, &layout, &control_safe) {
            Ok(ProducedOne::Ok {
                native_relpath,
                text_relpath,
                row,
            }) => {
                // Order: production_items (ok) → JSONL append → done + checkpoint.
                // Crash after JSONL is recovered via jsonl_by_item on resume.
                record_production_item(
                    matter,
                    &cursor.production_set_id,
                    &item_id,
                    &control_safe,
                    native_relpath.as_deref(),
                    text_relpath.as_deref(),
                    "ok",
                    None,
                    None,
                )?;
                append_row_json(&layout, &row)?;
                cursor.produced_count += 1;
            }
            Ok(ProducedOne::Skipped { reason }) => {
                cursor.skipped_other += 1;
                record_production_item(
                    matter,
                    &cursor.production_set_id,
                    &item_id,
                    &control_safe,
                    None,
                    None,
                    "skipped",
                    Some(&reason),
                    None,
                )?;
            }
            Ok(ProducedOne::Error { message }) => {
                cursor.error_count += 1;
                record_production_item(
                    matter,
                    &cursor.production_set_id,
                    &item_id,
                    &control_safe,
                    None,
                    None,
                    "error",
                    None,
                    Some(&message),
                )?;
            }
            Err(e) => {
                cursor.error_count += 1;
                record_production_item(
                    matter,
                    &cursor.production_set_id,
                    &item_id,
                    &control_safe,
                    None,
                    None,
                    "error",
                    None,
                    Some(&e.to_string()),
                )?;
            }
        }

        update_production_set_status(
            matter,
            &cursor.production_set_id,
            "running",
            cursor.next_seq,
        )?;
        cursor.done_item_ids.push(item_id.clone());
        done.insert(item_id);
        offset += 1;
        cursor.cursor_index = offset as u64;
        cursor.completed_count = offset as u64;
        save_checkpoint(matter, job_id, &cursor)?;
        progress(cursor.completed_count);
    }

    cursor.phase = "finalize".into();
    save_checkpoint(matter, job_id, &cursor)?;
    finalize_volume(matter, job_id, params, &mut cursor)
}

enum ProducedOne {
    Ok {
        native_relpath: Option<String>,
        text_relpath: Option<String>,
        row: Box<LoadRow>,
    },
    Skipped {
        reason: String,
    },
    Error {
        message: String,
    },
}

fn produce_one_item(
    matter: &Matter,
    params: &ProduceParams,
    item: &Item,
    layout: &VolumeLayout,
    control: &str,
) -> Result<ProducedOne> {
    // When redactions exist, resolve text first so a missing redacted artifact
    // never leaves an unregistered native under NATIVES/.
    // For non-redacted items, same order is fine (text may be optional).
    let text_result = resolve_text(matter, item, layout.text.as_std_path(), control)?;

    // Redacted text missing is a hard item error (never original text on disk).
    if let Err(reason) = &text_result {
        if reason == "redacted_text_missing" {
            // Ensure no original text leaked — we never wrote TEXT or native for this path.
            return Ok(ProducedOne::Error {
                message: reason.clone(),
            });
        }
    }

    let text_art = match text_result {
        Ok(v) => v,
        Err(reason) => {
            return Ok(ProducedOne::Error { message: reason });
        }
    };

    // When synthetic EML may be generated, load production body from the correct
    // CAS (redacted when redactions apply) so the .eml matches TEXT/.
    let needs_eml = item
        .native_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none()
        && params.export_eml_if_missing_native
        && is_email_like(item);
    let eml_body = if needs_eml {
        match load_body_for_eml(matter, item)? {
            Ok(b) => Some(b),
            Err(reason) => {
                return Ok(ProducedOne::Error { message: reason });
            }
        }
    } else {
        None
    };

    let native_result = resolve_native(
        matter,
        item,
        params,
        layout.natives.as_std_path(),
        control,
        eml_body,
    )?;

    let (native_art, native_relpath, file_ext, mime, size, sha) = match native_result {
        Ok(art) => {
            let rel = VolumeLayout::native_relpath(control, &art.file_ext);
            let ext = art.file_ext.clone();
            let mime = art.mime_type.clone();
            let size = art.file_size.to_string();
            let sha = art.sha256.clone();
            (Some(art), Some(rel), ext, mime, size, sha)
        }
        Err(reason) => {
            if text_art.is_none() {
                return Ok(ProducedOne::Skipped { reason });
            }
            // Text-only production.
            (
                None,
                None,
                String::new(),
                item.mime_type.clone().unwrap_or_default(),
                String::new(),
                String::new(),
            )
        }
    };

    let text_relpath = if text_art.is_some() {
        Some(VolumeLayout::text_relpath(control))
    } else {
        None
    };

    let has_redacted = text_art.as_ref().map(|t| t.has_redacted).unwrap_or(false);

    let prod_status = match (native_art.is_some(), text_art.is_some()) {
        (true, true) => "NATIVE_AND_TEXT",
        (true, false) => "NATIVE",
        (false, true) => "TEXT_ONLY",
        (false, false) => {
            return Ok(ProducedOne::Skipped {
                reason: "nothing_to_produce".into(),
            });
        }
    };

    let file_name = path_basename(item.path.as_deref());
    let row = LoadRow {
        control_number: control.to_string(),
        item_id: item.id.clone(),
        parent_item_id: item.parent_item_id.clone().unwrap_or_default(),
        family_id: item.family_id.clone().unwrap_or_default(),
        custodian: item.custodian.clone().unwrap_or_default(),
        file_name,
        file_ext,
        file_category: item.file_category.clone().unwrap_or_default(),
        mime_type: mime,
        file_size: size,
        sha256: sha,
        date_sent: format_utc_datetime(item.sent_at.as_deref()),
        date_received: format_utc_datetime(item.received_at.as_deref()),
        date_created: format_utc_datetime(item.created_at.as_deref()),
        from: item.from_addr.clone().unwrap_or_default(),
        to: join_addrs_json(item.to_addrs_json.as_deref()),
        cc: join_addrs_json(item.cc_addrs_json.as_deref()),
        bcc: join_addrs_json(item.bcc_addrs_json.as_deref()),
        subject: item
            .subject
            .clone()
            .or_else(|| item.title.clone())
            .unwrap_or_default(),
        native_path: native_relpath.clone().unwrap_or_default(),
        text_path: text_relpath.clone().unwrap_or_default(),
        has_redacted_text: if has_redacted { "Y" } else { "N" }.into(),
        withheld: "N".into(),
        prod_status: prod_status.into(),
    };

    let _ = native_art; // silence unused when only metadata used
    Ok(ProducedOne::Ok {
        native_relpath,
        text_relpath,
        row: Box::new(row),
    })
}

fn append_row_json(layout: &VolumeLayout, row: &LoadRow) -> Result<()> {
    // Side file for finalize rebuild (JSONL of field maps).
    let path = layout.data.join("rows.jsonl");
    let mut map = serde_json::Map::new();
    for (name, val) in crate::dat::DAT_FIELDS.iter().zip(row.field_values().iter()) {
        map.insert((*name).to_string(), json!(val));
    }
    // BEGBATES/ENDBATES/CONTROL are same — field_values already expands.
    let line = serde_json::to_string(&serde_json::Value::Object(map))?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path.as_std_path())?;
    writeln!(f, "{line}")?;
    Ok(())
}

fn finalize_volume(
    matter: &Matter,
    job_id: &str,
    params: &ProduceParams,
    cursor: &mut CheckpointCursor,
) -> Result<ProduceOutcome> {
    let layout = VolumeLayout::create(camino::Utf8Path::new(&cursor.output_root))?;
    let rows = load_rows_from_jsonl(&layout)?;

    write_load_dat(layout.load_dat.as_std_path(), &rows)?;
    if params.include_csv_twin {
        write_load_csv(layout.load_csv.as_std_path(), &rows)?;
    }

    let counts_line = format!(
        "selected={} produced={} skipped_withheld={} skipped_other={} errors={}",
        cursor.selected_count,
        cursor.produced_count,
        cursor.skipped_withheld,
        cursor.skipped_other,
        cursor.error_count
    );
    write_readme(
        &layout.readme,
        &cursor.production_name,
        params.expand_family,
        &counts_line,
    )?;

    let status = if cursor.error_count > 0 || cursor.skipped_other > 0 {
        "complete_with_errors"
    } else {
        "complete"
    };
    update_production_set_status(matter, &cursor.production_set_id, status, cursor.next_seq)?;
    cursor.phase = "done".into();
    save_checkpoint(matter, job_id, cursor)?;

    // Remove intermediate jsonl only after durable done checkpoint.
    let jsonl = layout.data.join("rows.jsonl");
    let _ = std::fs::remove_file(jsonl.as_std_path());

    Ok(ProduceOutcome::Succeeded(summary_from_cursor(cursor)))
}

fn load_rows_from_jsonl(layout: &VolumeLayout) -> Result<Vec<LoadRow>> {
    let path = layout.data.join("rows.jsonl");
    if !path.as_std_path().exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path.as_std_path())?;
    // Keep last occurrence per ITEM_ID so crash/resume duplicates cannot
    // inflate load.dat / load.csv row counts.
    let mut by_item: HashMap<String, LoadRow> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row = load_row_from_json_value(&serde_json::from_str(line)?)?;
        if row.item_id.is_empty() {
            // Malformed sidecar line — keep under a synthetic key so we do not drop it silently.
            let key = format!("__missing_item_{}", order.len());
            order.push(key.clone());
            by_item.insert(key, row);
            continue;
        }
        if !by_item.contains_key(&row.item_id) {
            order.push(row.item_id.clone());
        }
        by_item.insert(row.item_id.clone(), row);
    }
    let mut rows = Vec::with_capacity(order.len());
    for id in order {
        if let Some(r) = by_item.remove(&id) {
            rows.push(r);
        }
    }
    // Defensive: unique CONTROL_NUMBER among produced rows (last ITEM_ID wins already).
    let mut seen_control = HashSet::new();
    for r in &rows {
        if r.control_number.is_empty() {
            continue;
        }
        if !seen_control.insert(r.control_number.clone()) {
            return Err(ProduceError::Other(format!(
                "duplicate CONTROL_NUMBER {} after ITEM_ID dedupe of rows.jsonl",
                r.control_number
            )));
        }
    }
    Ok(rows)
}

fn load_row_from_json_value(v: &serde_json::Value) -> Result<LoadRow> {
    let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    Ok(LoadRow {
        control_number: g("CONTROL_NUMBER"),
        item_id: g("ITEM_ID"),
        parent_item_id: g("PARENT_ITEM_ID"),
        family_id: g("FAMILY_ID"),
        custodian: g("CUSTODIAN"),
        file_name: g("FILE_NAME"),
        file_ext: g("FILE_EXT"),
        file_category: g("FILE_CATEGORY"),
        mime_type: g("MIME_TYPE"),
        file_size: g("FILE_SIZE"),
        sha256: g("SHA256"),
        date_sent: g("DATE_SENT"),
        date_received: g("DATE_RECEIVED"),
        date_created: g("DATE_CREATED"),
        from: g("FROM"),
        to: g("TO"),
        cc: g("CC"),
        bcc: g("BCC"),
        subject: g("SUBJECT"),
        native_path: g("NATIVE_PATH"),
        text_path: g("TEXT_PATH"),
        has_redacted_text: g("HAS_REDACTED_TEXT"),
        withheld: g("WITHHELD"),
        prod_status: g("PROD_STATUS"),
    })
}

/// Map ITEM_ID → last LoadRow present in `DATA/rows.jsonl` (if any).
fn index_rows_jsonl_by_item_id(layout: &VolumeLayout) -> Result<HashMap<String, LoadRow>> {
    let rows = load_rows_from_jsonl(layout)?;
    let mut map = HashMap::with_capacity(rows.len());
    for r in rows {
        if !r.item_id.is_empty() {
            map.insert(r.item_id.clone(), r);
        }
    }
    Ok(map)
}

/// Mark an item done from a pre-existing JSONL row without re-producing.
///
/// Advances `next_seq` past the assigned control when the crash window left
/// the checkpoint behind the durable row append.
fn recover_done_from_existing_row(
    cursor: &mut CheckpointCursor,
    done: &mut HashSet<String>,
    prefix: &str,
    item_id: &str,
    control_number: &str,
) {
    let seq_before = cursor.next_seq;
    advance_next_seq_for_control(cursor, prefix, control_number);
    // If this control was at/after the checkpoint's next_seq, the append was not
    // checkpointed yet — count it as produced now.
    if let Some(seq) = parse_control_seq(prefix, control_number) {
        if seq >= seq_before {
            cursor.produced_count += 1;
        }
    }
    if !done.contains(item_id) {
        cursor.done_item_ids.push(item_id.to_string());
        done.insert(item_id.to_string());
    }
}

/// Purge produced artifacts/JSONL for any selected item that is now withheld.
///
/// Returns `Some(Failed)` when `fail_if_withheld` trips; otherwise updates cursor
/// counts and records `production_items` as skipped withheld.
fn apply_late_withhold_sweep(
    matter: &Matter,
    params: &ProduceParams,
    layout: &VolumeLayout,
    cursor: &mut CheckpointCursor,
    jsonl_by_item: &mut HashMap<String, LoadRow>,
    done: &mut HashSet<String>,
) -> Result<Option<ProduceOutcome>> {
    let ids: Vec<String> = cursor.ordered_ids.clone();
    for item_id in ids {
        if !matter.item_is_withheld(&item_id)? {
            continue;
        }
        if params.fail_if_withheld {
            if let Some(existing) = jsonl_by_item.remove(&item_id) {
                let _ = remove_row_artifacts(layout, &existing);
                rewrite_rows_jsonl_excluding(layout, &item_id)?;
            }
            update_production_set_status(
                matter,
                &cursor.production_set_id,
                "failed",
                cursor.next_seq,
            )?;
            return Ok(Some(ProduceOutcome::Failed {
                message: format!(
                    "fail_if_withheld: item {item_id} is withheld; aborting production"
                ),
                summary: summary_from_cursor(cursor),
            }));
        }

        let already_skipped_withheld =
            production_item_is_skipped_withheld(matter, &cursor.production_set_id, &item_id)?;
        let had_jsonl = jsonl_by_item.contains_key(&item_id);
        let was_ok =
            production_item_ok_control(matter, &cursor.production_set_id, &item_id)?.is_some();
        if let Some(existing) = jsonl_by_item.remove(&item_id) {
            let _ = remove_row_artifacts(layout, &existing);
            rewrite_rows_jsonl_excluding(layout, &item_id)?;
        }
        // Un-count as produced when we previously packaged this item.
        if (had_jsonl || was_ok) && cursor.produced_count > 0 {
            cursor.produced_count -= 1;
        }
        // Count each withheld item at most once across multi-resume sweeps.
        if !already_skipped_withheld {
            cursor.skipped_withheld += 1;
            record_production_item(
                matter,
                &cursor.production_set_id,
                &item_id,
                "",
                None,
                None,
                "skipped",
                Some("withheld"),
                None,
            )?;
        }
        if !done.contains(&item_id) {
            cursor.done_item_ids.push(item_id.clone());
            done.insert(item_id);
        }
    }
    Ok(None)
}

fn production_item_is_skipped_withheld(
    matter: &Matter,
    set_id: &str,
    item_id: &str,
) -> Result<bool> {
    let mut stmt = matter.connection().prepare(
        "SELECT 1 FROM production_items \
         WHERE production_set_id = ?1 AND item_id = ?2 \
           AND status = 'skipped' AND skip_reason = 'withheld' LIMIT 1",
    )?;
    let mut rows = stmt.query(params![set_id, item_id])?;
    Ok(rows.next()?.is_some())
}

/// Validate that non-empty NATIVE_PATH / TEXT_PATH from a recovered JSONL row
/// resolve under the volume root, and that SHA256 matches the native file when set.
fn recovered_artifacts_valid(layout: &VolumeLayout, row: &LoadRow) -> bool {
    validate_recovered_artifacts(layout, row).is_ok()
}

fn validate_recovered_artifacts(layout: &VolumeLayout, row: &LoadRow) -> Result<()> {
    if !row.native_path.trim().is_empty() {
        let abs = volume_rel_to_abs(layout, &row.native_path)?;
        if !abs.exists() {
            return Err(ProduceError::Other(format!(
                "missing native artifact for recovered row: {}",
                row.native_path
            )));
        }
        if !row.sha256.trim().is_empty() {
            let bytes = std::fs::read(&abs)?;
            let disk_sha = {
                let d = Sha256::digest(&bytes);
                d.iter().map(|b| format!("{b:02x}")).collect::<String>()
            };
            if !disk_sha.eq_ignore_ascii_case(row.sha256.trim()) {
                return Err(ProduceError::Other(format!(
                    "SHA256 mismatch for recovered native {}: dat={} disk={}",
                    row.native_path, row.sha256, disk_sha
                )));
            }
            if !row.file_size.trim().is_empty() {
                if let Ok(expected) = row.file_size.trim().parse::<u64>() {
                    if expected != bytes.len() as u64 {
                        return Err(ProduceError::Other(format!(
                            "size mismatch for recovered native {}: dat={} disk={}",
                            row.native_path,
                            expected,
                            bytes.len()
                        )));
                    }
                }
            }
        }
    }
    if !row.text_path.trim().is_empty() {
        let abs = volume_rel_to_abs(layout, &row.text_path)?;
        if !abs.exists() {
            return Err(ProduceError::Other(format!(
                "missing text artifact for recovered row: {}",
                row.text_path
            )));
        }
    }
    Ok(())
}

/// Convert a DAT-style relative path (`NATIVES\…` / `TEXT\…`) to an absolute path
/// under the volume root.
fn volume_rel_to_abs(layout: &VolumeLayout, rel: &str) -> Result<std::path::PathBuf> {
    let normalized = rel.replace('/', "\\");
    let parts: Vec<&str> = normalized.split('\\').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err(ProduceError::Other("empty relative path".into()));
    }
    // Reject path escape attempts.
    if parts.iter().any(|p| *p == ".." || p.contains(':')) {
        return Err(ProduceError::Other(format!(
            "refusing unsafe relative path: {rel}"
        )));
    }
    let mut abs = layout.root.as_std_path().to_path_buf();
    for p in parts {
        abs.push(p);
    }
    Ok(abs)
}

/// Delete native/text files referenced by a recovered row (best-effort).
fn remove_row_artifacts(layout: &VolumeLayout, row: &LoadRow) -> Result<()> {
    if !row.native_path.trim().is_empty() {
        if let Ok(abs) = volume_rel_to_abs(layout, &row.native_path) {
            let _ = std::fs::remove_file(abs);
        }
    }
    if !row.text_path.trim().is_empty() {
        if let Ok(abs) = volume_rel_to_abs(layout, &row.text_path) {
            let _ = std::fs::remove_file(abs);
        }
    }
    Ok(())
}

/// Rewrite `DATA/rows.jsonl` excluding `item_id` (last-wins map already applied).
fn rewrite_rows_jsonl_excluding(layout: &VolumeLayout, exclude_item_id: &str) -> Result<()> {
    let rows = load_rows_from_jsonl(layout)?;
    let path = layout.data.join("rows.jsonl");
    // Truncate and rewrite remaining rows.
    use std::io::Write;
    let mut f = std::fs::File::create(path.as_std_path())?;
    for row in rows {
        if row.item_id == exclude_item_id {
            continue;
        }
        let mut map = serde_json::Map::new();
        for (name, val) in crate::dat::DAT_FIELDS.iter().zip(row.field_values().iter()) {
            map.insert((*name).to_string(), json!(val));
        }
        let line = serde_json::to_string(&serde_json::Value::Object(map))?;
        writeln!(f, "{line}")?;
    }
    f.flush()?;
    Ok(())
}

fn advance_next_seq_for_control(cursor: &mut CheckpointCursor, prefix: &str, control: &str) {
    if let Some(seq) = parse_control_seq(prefix, control) {
        let next = seq.saturating_add(1);
        if next > cursor.next_seq {
            cursor.next_seq = next;
        }
    }
}

/// Parse trailing decimal sequence from `{prefix}{digits}`.
fn parse_control_seq(prefix: &str, control: &str) -> Option<u64> {
    let rest = control.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// If `production_items` already has status=ok for this item, return its control number.
fn production_item_ok_control(
    matter: &Matter,
    set_id: &str,
    item_id: &str,
) -> Result<Option<String>> {
    let mut stmt = matter.connection().prepare(
        "SELECT control_number FROM production_items \
         WHERE production_set_id = ?1 AND item_id = ?2 AND status = 'ok' LIMIT 1",
    )?;
    let mut rows = stmt.query(params![set_id, item_id])?;
    if let Some(row) = rows.next()? {
        let control: String = row.get(0)?;
        if control.is_empty() || control.starts_with("SKIP_") {
            return Ok(None);
        }
        return Ok(Some(control));
    }
    Ok(None)
}

fn format_control(prefix: &str, seq: u64, width: u32) -> String {
    format!("{prefix}{seq:0width$}", width = width as usize)
}

fn select_item_ids(matter: &Matter, params: &ProduceParams) -> Result<Vec<String>> {
    match params.scope.as_str() {
        SCOPE_REVIEW_CORPUS => {
            let mut ids = list_in_review_ids(matter)?;
            if params.expand_family {
                ids = expand_family_ids(matter, &ids)?;
            }
            Ok(ids)
        }
        SCOPE_ITEM_IDS => {
            let mut ids = params.item_ids.clone();
            // Stable unique order preserving first occurrence.
            let mut seen = HashSet::new();
            ids.retain(|id| seen.insert(id.clone()));
            if params.expand_family {
                ids = expand_family_ids(matter, &ids)?;
            }
            Ok(ids)
        }
        other => Err(ProduceError::InvalidParams(format!(
            "unknown scope '{other}'"
        ))),
    }
}

fn list_in_review_ids(matter: &Matter) -> Result<Vec<String>> {
    let mut stmt = matter.connection().prepare(
        "SELECT id FROM items \
         WHERE matter_id = ?1 AND in_review = 1 \
         ORDER BY COALESCE(review_order, 999999999), id ASC",
    )?;
    let rows = stmt.query_map(params![matter.id()], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Lightweight family expand: include direct children and parents of selected.
fn expand_family_ids(matter: &Matter, base: &[String]) -> Result<Vec<String>> {
    let mut set: HashSet<String> = base.iter().cloned().collect();
    for id in base {
        let item = matter.get_item(id)?;
        if let Some(parent) = item.parent_item_id.as_deref() {
            set.insert(parent.to_string());
        }
        if let Some(fid) = item.family_id.as_deref() {
            let mut stmt = matter
                .connection()
                .prepare("SELECT id FROM items WHERE matter_id = ?1 AND family_id = ?2")?;
            let rows = stmt.query_map(params![matter.id(), fid], |row| row.get::<_, String>(0))?;
            for r in rows {
                set.insert(r?);
            }
        }
        // Direct children by parent_item_id
        let mut stmt = matter
            .connection()
            .prepare("SELECT id FROM items WHERE matter_id = ?1 AND parent_item_id = ?2")?;
        let rows = stmt.query_map(params![matter.id(), id], |row| row.get::<_, String>(0))?;
        for r in rows {
            set.insert(r?);
        }
    }
    let mut out: Vec<String> = set.into_iter().collect();
    out.sort();
    Ok(out)
}

fn create_production_set(
    matter: &Matter,
    job_id: &str,
    name: &str,
    bates_prefix: &str,
    params_json: &serde_json::Value,
    output_root: &str,
) -> Result<String> {
    let id = new_id("prod");
    let now = Utc::now().to_rfc3339();
    let params_s = params_json.to_string();
    matter.connection().execute(
        "INSERT INTO production_sets \
         (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status, params_json, output_root, job_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 'running', ?7, ?8, ?9)",
        params![
            id,
            matter.id(),
            name,
            now,
            now,
            bates_prefix,
            params_s,
            output_root,
            job_id
        ],
    )?;
    Ok(id)
}

fn update_production_set_status(
    matter: &Matter,
    set_id: &str,
    status: &str,
    next_seq: u64,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    matter.connection().execute(
        "UPDATE production_sets SET status = ?1, next_seq = ?2, updated_at = ?3 WHERE id = ?4",
        params![status, next_seq as i64, now, set_id],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn record_production_item(
    matter: &Matter,
    set_id: &str,
    item_id: &str,
    control_number: &str,
    native_relpath: Option<&str>,
    text_relpath: Option<&str>,
    status: &str,
    skip_reason: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    // control_number may be empty for withheld skips — use a unique placeholder for unique index.
    let control = if control_number.is_empty() {
        format!("SKIP_{item_id}")
    } else {
        control_number.to_string()
    };
    matter.connection().execute(
        "INSERT INTO production_items \
         (production_set_id, item_id, control_number, native_relpath, text_relpath, status, skip_reason, error, produced_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(production_set_id, item_id) DO UPDATE SET \
           control_number = excluded.control_number, \
           native_relpath = excluded.native_relpath, \
           text_relpath = excluded.text_relpath, \
           status = excluded.status, \
           skip_reason = excluded.skip_reason, \
           error = excluded.error, \
           produced_at = excluded.produced_at",
        params![
            set_id,
            item_id,
            control,
            native_relpath,
            text_relpath,
            status,
            skip_reason,
            error,
            now
        ],
    )?;
    Ok(())
}

fn save_checkpoint(matter: &Matter, job_id: &str, cursor: &CheckpointCursor) -> Result<()> {
    let cursor_json = serde_json::to_string(cursor)
        .map_err(|e| ProduceError::Other(format!("checkpoint serialize: {e}")))?;
    matter.put_checkpoint(
        job_id,
        PRODUCE_STAGE,
        &cursor_json,
        cursor.completed_count as i64,
    )?;
    Ok(())
}

fn new_id(prefix: &str) -> String {
    let ts = Utc::now().timestamp_millis();
    let mut hasher = Sha256::new();
    hasher.update(ts.to_le_bytes());
    hasher.update(prefix.as_bytes());
    // Mix in a bit of process randomness via address of a local.
    let marker = std::time::Instant::now();
    hasher.update(format!("{marker:?}").as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}_{}", &hex[..16])
}
