//! Core `fts_index` job: batch index with delete-before-add + checkpoints.

use std::time::Instant;

use chrono::Utc;
use matter_core::{sha256_hex, AuditEventInput, FtsFieldUpdate, Matter};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tantivy::schema::Term;

use crate::error::{Result, SearchError};
use crate::index::{delete_then_add, remove_index_dir, MatterIndex, DEFAULT_WRITER_HEAP_BYTES};
use crate::params::FtsIndexParams;
use crate::schema::FtsSchema;

/// Job kind string for process-runner.
pub const JOB_KIND_FTS_INDEX: &str = "fts_index";
/// Checkpoint stage name.
pub const FTS_STAGE: &str = "fts_index";

/// Summary counts after an FTS index run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FtsSummary {
    pub completed_count: u64,
    pub indexed_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
}

/// Outcome of [`run_fts_index`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FtsOutcome {
    Succeeded(FtsSummary),
    Paused(FtsSummary),
    Failed {
        message: String,
        summary: FtsSummary,
    },
}

/// Alias used in public docs.
pub type FtsIndexOutcome = FtsOutcome;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    #[serde(default = "default_phase")]
    phase: String,
    cursor_index: u64,
    completed_count: u64,
    indexed_count: u64,
    skipped_count: u64,
    error_count: u64,
    params: serde_json::Value,
}

fn default_phase() -> String {
    "index".into()
}

/// Run FTS indexing on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between batches.
/// Calls `progress(completed_count)` after each committed batch.
///
/// **reset:true:** drops any open handles held by this function, removes
/// `index/`, clears `fts_*`, then full rebuild. Callers (Desk) must drop their
/// own `MatterIndex` / readers before starting a reset job.
pub fn run_fts_index(
    matter: &Matter,
    job_id: &str,
    params: &FtsIndexParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<FtsOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(SearchError::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "fts_index.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({ "params": params_json }).to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_fts_inner(
        matter,
        job_id,
        &effective,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(FtsOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "fts_index.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "indexed_count": s.indexed_count,
                    "skipped_count": s.skipped_count,
                    "error_count": s.error_count,
                    "completed_count": s.completed_count,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(FtsOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(FtsOutcome::Paused(_)) => {}
        Ok(FtsOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "fts_index.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "indexed_count": summary.indexed_count,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(SearchError::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "fts_index.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(SearchError::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, FTS_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(SearchError::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &FtsIndexParams,
    prior: Option<&CheckpointCursor>,
) -> Result<FtsIndexParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<FtsIndexParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(SearchError::Other(format!(
                        "checkpoint params unreadable: {e}"
                    )));
                }
            }
        }
    }
    Ok(call_site.clone())
}

fn run_fts_inner(
    matter: &Matter,
    job_id: &str,
    params: &FtsIndexParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<FtsOutcome> {
    let mut summary = FtsSummary::default();
    let mut cursor_index: u64 = 0;

    if let Some(ref p) = prior {
        summary.completed_count = p.completed_count;
        summary.indexed_count = p.indexed_count;
        summary.skipped_count = p.skipped_count;
        summary.error_count = p.error_count;
        cursor_index = p.cursor_index;
    }

    // reset: only when starting fresh (cursor_index == 0 and no prior progress)
    let do_reset = params.reset && prior.is_none();
    if do_reset {
        // No live MatterIndex handle held yet — safe to remove_dir_all.
        remove_index_dir(matter.root())?;
        matter.clear_fts_fields()?;
        cursor_index = 0;
        summary = FtsSummary::default();
    }

    let index = MatterIndex::open_or_create(matter.root())?;
    let fts_schema = index.fts_schema().clone();
    let heap = if params.writer_heap_bytes == 0 {
        DEFAULT_WRITER_HEAP_BYTES
    } else {
        params.writer_heap_bytes
    };
    let mut writer = index.writer(heap)?;

    let batch_size = params.batch_size.max(1);

    // Phase 0: remove Tantivy docs for items that lost text / eligibility.
    // cursor_index is reused across orphan-delete then index phases; on resume
    // after partial index, orphans may already be gone (idempotent delete).
    if !purge_orphans(
        matter,
        job_id,
        &mut writer,
        &fts_schema,
        params_json,
        cancel,
        progress,
        &mut summary,
    )? {
        drop(writer);
        index.shutdown();
        return Ok(FtsOutcome::Paused(summary));
    }

    // Page through candidates; skip already-current when incremental.
    // cursor_index is the absolute offset into list_fts_candidates.
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            drop(writer);
            index.shutdown();
            return Ok(FtsOutcome::Paused(summary));
        }

        let page = matter.list_fts_candidates(cursor_index, batch_size as u64)?;
        if page.is_empty() {
            break;
        }

        let parent_ids: Vec<String> = page.iter().map(|c| c.id.clone()).collect();
        let attach_map = matter.list_attachment_names_for_parents(&parent_ids)?;

        let mut updates: Vec<FtsFieldUpdate> = Vec::new();
        let now = Utc::now().to_rfc3339();

        for cand in &page {
            cursor_index = cursor_index.saturating_add(1);

            let content_sha = cand
                .text_sha256
                .clone()
                .or_else(|| cand.html_sha256.clone());
            let Some(content_sha) = content_sha else {
                summary.skipped_count = summary.skipped_count.saturating_add(1);
                summary.completed_count = summary.completed_count.saturating_add(1);
                continue;
            };

            let subject = cand
                .subject
                .clone()
                .or_else(|| cand.title.clone())
                .unwrap_or_default();
            let path = cand.path.clone().unwrap_or_default();
            let attach_names = attach_map
                .get(&cand.id)
                .map(|v| v.join(" "))
                .unwrap_or_default();

            // Payload digest covers body digest + searchable metadata so subject/
            // path/attach_names changes re-index even when body digest is unchanged.
            let payload = indexed_payload_digest(&content_sha, &subject, &path, &attach_names);

            // Incremental: skip when bookkeeping matches full indexed payload.
            if !params.reset && cand.fts_text_sha256.as_deref() == Some(payload.as_str()) {
                summary.skipped_count = summary.skipped_count.saturating_add(1);
                summary.completed_count = summary.completed_count.saturating_add(1);
                continue;
            }

            let body_result = load_body_text(matter, cand);
            match body_result {
                Ok(body) => {
                    if let Err(e) = delete_then_add(
                        &mut writer,
                        &fts_schema,
                        &cand.id,
                        &subject,
                        &body,
                        &path,
                        &attach_names,
                    ) {
                        summary.error_count = summary.error_count.saturating_add(1);
                        summary.completed_count = summary.completed_count.saturating_add(1);
                        updates.push(FtsFieldUpdate {
                            item_id: cand.id.clone(),
                            fts_text_sha256: None,
                            fts_indexed_at: Some(now.clone()),
                            fts_error: Some(e.to_string()),
                        });
                        continue;
                    }
                    summary.indexed_count = summary.indexed_count.saturating_add(1);
                    summary.completed_count = summary.completed_count.saturating_add(1);
                    updates.push(FtsFieldUpdate {
                        item_id: cand.id.clone(),
                        fts_text_sha256: Some(payload),
                        fts_indexed_at: Some(now.clone()),
                        fts_error: None,
                    });
                }
                Err(e) => {
                    summary.error_count = summary.error_count.saturating_add(1);
                    summary.completed_count = summary.completed_count.saturating_add(1);
                    updates.push(FtsFieldUpdate {
                        item_id: cand.id.clone(),
                        fts_text_sha256: None,
                        fts_indexed_at: Some(now.clone()),
                        fts_error: Some(e.to_string()),
                    });
                }
            }
        }

        // Commit Tantivy first, then SQLite mark + checkpoint in one txn.
        if let Err(e) = writer.commit() {
            drop(writer);
            index.shutdown();
            return Ok(FtsOutcome::Failed {
                message: format!("tantivy commit failed: {e}"),
                summary,
            });
        }

        let cursor = CheckpointCursor {
            phase: "index".into(),
            cursor_index,
            completed_count: summary.completed_count,
            indexed_count: summary.indexed_count,
            skipped_count: summary.skipped_count,
            error_count: summary.error_count,
            params: params_json.clone(),
        };
        let cursor_json = serde_json::to_string(&cursor).unwrap_or_else(|_| "{}".into());
        if let Err(e) = matter.apply_fts_batch_with_checkpoint(
            job_id,
            FTS_STAGE,
            &updates,
            &cursor_json,
            summary.completed_count as i64,
        ) {
            drop(writer);
            index.shutdown();
            return Ok(FtsOutcome::Failed {
                message: format!("sqlite fts batch failed: {e}"),
                summary,
            });
        }

        progress(summary.completed_count);

        if page.len() < batch_size {
            break;
        }
    }

    drop(writer);
    index.shutdown();
    Ok(FtsOutcome::Succeeded(summary))
}

/// SHA-256 hex of body digest + searchable metadata fields.
///
/// Stored in `fts_text_sha256` so incremental skip stays honest when subject,
/// path, or attachment names change without a body rewrite.
pub fn indexed_payload_digest(
    body_sha: &str,
    subject: &str,
    path: &str,
    attach_names: &str,
) -> String {
    let mut buf =
        Vec::with_capacity(body_sha.len() + subject.len() + path.len() + attach_names.len() + 3);
    buf.extend_from_slice(body_sha.as_bytes());
    buf.push(0);
    buf.extend_from_slice(subject.as_bytes());
    buf.push(0);
    buf.extend_from_slice(path.as_bytes());
    buf.push(0);
    buf.extend_from_slice(attach_names.as_bytes());
    sha256_hex(&buf)
}

/// Delete Tantivy docs + clear fts_* for ineligible items that still look indexed.
///
/// Returns `false` if cancelled mid-pass (caller should return Paused).
#[allow(clippy::too_many_arguments)]
fn purge_orphans(
    matter: &Matter,
    job_id: &str,
    writer: &mut tantivy::IndexWriter,
    fts_schema: &FtsSchema,
    params_json: &serde_json::Value,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    summary: &mut FtsSummary,
) -> Result<bool> {
    let mut orphan_offset = 0u64;
    const BATCH: u64 = 200;
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            return Ok(false);
        }
        let orphans = matter.list_fts_orphans(orphan_offset, BATCH)?;
        if orphans.is_empty() {
            break;
        }
        let mut updates = Vec::new();
        let now = Utc::now().to_rfc3339();
        for id in &orphans {
            let term = Term::from_field_text(fts_schema.item_id, id);
            writer.delete_term(term);
            updates.push(FtsFieldUpdate {
                item_id: id.clone(),
                fts_text_sha256: None,
                fts_indexed_at: Some(now.clone()),
                fts_error: None,
            });
            summary.skipped_count = summary.skipped_count.saturating_add(1);
            summary.completed_count = summary.completed_count.saturating_add(1);
        }
        writer
            .commit()
            .map_err(|e| SearchError::Index(format!("orphan delete commit: {e}")))?;
        let cursor = CheckpointCursor {
            phase: "purge".into(),
            cursor_index: orphan_offset,
            completed_count: summary.completed_count,
            indexed_count: summary.indexed_count,
            skipped_count: summary.skipped_count,
            error_count: summary.error_count,
            params: params_json.clone(),
        };
        let cursor_json = serde_json::to_string(&cursor).unwrap_or_else(|_| "{}".into());
        matter.apply_fts_batch_with_checkpoint(
            job_id,
            FTS_STAGE,
            &updates,
            &cursor_json,
            summary.completed_count as i64,
        )?;
        progress(summary.completed_count);
        if (orphans.len() as u64) < BATCH {
            break;
        }
        // After clearing fts_*, orphans drop out of the list; keep offset 0.
        orphan_offset = 0;
    }
    Ok(true)
}

fn load_body_text(matter: &Matter, cand: &matter_core::FtsCandidate) -> Result<String> {
    if let Some(ref sha) = cand.text_sha256 {
        let bytes = matter.get_bytes(sha)?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    if let Some(ref sha) = cand.html_sha256 {
        let bytes = matter.get_bytes(sha)?;
        let html = String::from_utf8_lossy(&bytes);
        return Ok(strip_html_tags_minimal(&html));
    }
    Ok(String::new())
}

/// Minimal HTML tag strip (aligned with matter-core logical_hash helper).
fn strip_html_tags_minimal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}
