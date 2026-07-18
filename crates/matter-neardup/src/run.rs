//! Core near-duplicate job: sketch → LSH cluster → write with checkpoints.

use std::io::Read;
use std::time::Instant;

use chrono::Utc;
use matter_core::{
    item_dedup_role, item_near_dup_role, AuditEventInput, Matter, NearDupFieldUpdate,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cluster::{cluster_and_score, ItemMeta};
use crate::error::{NearDupError, Result};
use crate::lsh::lsh_candidate_pairs;
use crate::minhash::minhash_signature;
use crate::params::{NearDupParams, NEAR_DUP_METHOD};
use crate::tokenize::text_to_shingles;

/// Job kind string for process-runner.
pub const JOB_KIND_NEARDUP: &str = "neardup";
/// Checkpoint stage name.
pub const NEARDUP_STAGE: &str = "neardup";

/// Summary counts after a near-dup run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NearDupSummary {
    pub completed_count: u64,
    pub group_count: u64,
    pub member_count: u64,
    pub unique_count: u64,
    pub skipped_count: u64,
}

/// Outcome of [`run_neardup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NearDupOutcome {
    Succeeded(NearDupSummary),
    Paused(NearDupSummary),
    Failed {
        message: String,
        summary: NearDupSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    /// `sketch` | `cluster` | `write`
    #[serde(default = "default_phase_sketch")]
    phase: String,
    cursor_index: u64,
    completed_count: u64,
    group_count: u64,
    member_count: u64,
    unique_count: u64,
    skipped_count: u64,
    params: serde_json::Value,
}

fn default_phase_sketch() -> String {
    "sketch".into()
}

/// Pending result row ready for batch write.
#[derive(Debug, Clone)]
struct PendingResult {
    item_id: String,
    role: String,
    group_id: Option<String>,
    pivot_item_id: Option<String>,
    similarity: Option<f64>,
}

/// Run near-duplicate detection on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between batches.
/// Calls `progress(completed_count)` after each committed write batch.
///
/// **Memory (P0):** all eligible signatures are held in memory after sketch
/// (`item_id` + `token_count` + `H × u64` slots). Body text is streamed from
/// CAS and dropped after sketching. Multi-million spill is deferred (D-0023).
///
/// **Resume:** when a prior checkpoint exists with a non-empty `params` object,
/// those frozen params are the source of truth. Sketch/cluster state is not
/// durable — resume re-sketches then continues write from `cursor_index` when
/// `phase == "write"`.
pub fn run_neardup(
    matter: &Matter,
    job_id: &str,
    params: &NearDupParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<NearDupOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(NearDupError::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "neardup.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "method": NEAR_DUP_METHOD,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_neardup_inner(
        matter,
        job_id,
        &effective,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(NearDupOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "neardup.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "group_count": s.group_count,
                    "member_count": s.member_count,
                    "unique_count": s.unique_count,
                    "skipped_count": s.skipped_count,
                    "completed_count": s.completed_count,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(NearDupOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(NearDupOutcome::Paused(_)) => {}
        Ok(NearDupOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "neardup.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "group_count": summary.group_count,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(NearDupError::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "neardup.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(NearDupError::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, NEARDUP_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(NearDupError::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &NearDupParams,
    prior: Option<&CheckpointCursor>,
) -> Result<NearDupParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().map(|o| !o.is_empty()).unwrap_or(false) {
            return Ok(serde_json::from_value(p.params.clone())?);
        }
    }
    Ok(call_site.clone())
}

fn cancelled(cancel: Option<&dyn Fn() -> bool>) -> bool {
    cancel.map(|f| f()).unwrap_or(false)
}

fn summary_from_cursor(c: &CheckpointCursor) -> NearDupSummary {
    NearDupSummary {
        completed_count: c.completed_count,
        group_count: c.group_count,
        member_count: c.member_count,
        unique_count: c.unique_count,
        skipped_count: c.skipped_count,
    }
}

fn run_neardup_inner(
    matter: &Matter,
    job_id: &str,
    params: &NearDupParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<NearDupOutcome> {
    let write_resume_from = if prior.as_ref().map(|p| p.phase == "write").unwrap_or(false) {
        prior.as_ref().map(|p| p.cursor_index).unwrap_or(0)
    } else {
        0
    };

    // Fresh reset only when not resuming a write phase mid-batch.
    let do_reset = params.reset
        && (prior.is_none()
            || prior
                .as_ref()
                .map(|p| p.phase != "write" && p.completed_count == 0)
                .unwrap_or(true));
    if do_reset {
        matter.clear_near_dup_fields()?;
    }

    if cancelled(cancel) {
        let s = prior.as_ref().map(summary_from_cursor).unwrap_or_default();
        return Ok(NearDupOutcome::Paused(s));
    }

    // --- Sketch phase: stream CAS → signature; drop text ---
    let candidates = matter.list_neardup_candidates(params.include_attachments)?;
    let mut sketched: Vec<ItemMeta> = Vec::new();
    let mut skipped_results: Vec<PendingResult> = Vec::new();

    for (idx, cand) in candidates.iter().enumerate() {
        if cancelled(cancel) {
            // Checkpoint sketch progress (signatures not durable; resume re-sketches).
            let cursor = CheckpointCursor {
                phase: "sketch".into(),
                cursor_index: idx as u64,
                completed_count: (skipped_results.len() + sketched.len()) as u64,
                group_count: 0,
                member_count: 0,
                unique_count: 0,
                skipped_count: skipped_results.len() as u64,
                params: params_json.clone(),
            };
            let json = serde_json::to_string(&cursor)?;
            matter.apply_near_dup_batch_with_checkpoint(
                job_id,
                NEARDUP_STAGE,
                &[],
                &json,
                cursor.completed_count as i64,
            )?;
            progress(cursor.completed_count);
            return Ok(NearDupOutcome::Paused(NearDupSummary {
                completed_count: cursor.completed_count,
                skipped_count: cursor.skipped_count,
                ..Default::default()
            }));
        }

        // Exact-dup skip
        if params.skip_exact_duplicates
            && cand.dedup_role.as_deref() == Some(item_dedup_role::DUPLICATE)
        {
            skipped_results.push(PendingResult {
                item_id: cand.id.clone(),
                role: item_near_dup_role::SKIPPED.into(),
                group_id: None,
                pivot_item_id: None,
                similarity: None,
            });
            continue;
        }

        let Some(ref digest) = cand.text_sha256 else {
            skipped_results.push(PendingResult {
                item_id: cand.id.clone(),
                role: item_near_dup_role::SKIPPED.into(),
                group_id: None,
                pivot_item_id: None,
                similarity: None,
            });
            continue;
        };
        if digest.is_empty() {
            skipped_results.push(PendingResult {
                item_id: cand.id.clone(),
                role: item_near_dup_role::SKIPPED.into(),
                group_id: None,
                pivot_item_id: None,
                similarity: None,
            });
            continue;
        }

        let text = match read_cas_text(matter, digest) {
            Ok(t) => t,
            Err(_) => {
                skipped_results.push(PendingResult {
                    item_id: cand.id.clone(),
                    role: item_near_dup_role::SKIPPED.into(),
                    group_id: None,
                    pivot_item_id: None,
                    similarity: None,
                });
                continue;
            }
        };

        let (prepared, shingles, token_count) = text_to_shingles(
            &text,
            params.shingle_k,
            params.cjk_char_n,
            params.ignore_numbers,
        );
        // Drop body text
        drop(text);

        if prepared.chars().count() < params.min_chars {
            skipped_results.push(PendingResult {
                item_id: cand.id.clone(),
                role: item_near_dup_role::SKIPPED.into(),
                group_id: None,
                pivot_item_id: None,
                similarity: None,
            });
            continue;
        }
        if shingles.is_empty() {
            skipped_results.push(PendingResult {
                item_id: cand.id.clone(),
                role: item_near_dup_role::SKIPPED.into(),
                group_id: None,
                pivot_item_id: None,
                similarity: None,
            });
            continue;
        }

        let sig = minhash_signature(&shingles, params.hash_seed, params.num_hashes);
        sketched.push(ItemMeta {
            item_id: cand.id.clone(),
            token_count,
            imported_at: cand.imported_at.clone(),
            path: cand.path.clone().unwrap_or_default(),
            sig,
        });
    }

    if cancelled(cancel) {
        let cursor = CheckpointCursor {
            phase: "cluster".into(),
            cursor_index: 0,
            completed_count: (skipped_results.len() + sketched.len()) as u64,
            group_count: 0,
            member_count: 0,
            unique_count: 0,
            skipped_count: skipped_results.len() as u64,
            params: params_json.clone(),
        };
        let json = serde_json::to_string(&cursor)?;
        matter.apply_near_dup_batch_with_checkpoint(
            job_id,
            NEARDUP_STAGE,
            &[],
            &json,
            cursor.completed_count as i64,
        )?;
        return Ok(NearDupOutcome::Paused(NearDupSummary {
            completed_count: cursor.completed_count,
            skipped_count: cursor.skipped_count,
            ..Default::default()
        }));
    }

    // --- Cluster phase ---
    let sig_refs: Vec<(usize, &_)> = sketched
        .iter()
        .enumerate()
        .map(|(i, m)| (i, &m.sig))
        .collect();
    let cand_pairs = lsh_candidate_pairs(&sig_refs, params.num_bands, params.rows_per_band);
    let pair_list: Vec<(usize, usize)> = cand_pairs.into_iter().collect();
    let assignments = cluster_and_score(&sketched, &pair_list, params.threshold);

    let mut pending: Vec<PendingResult> =
        Vec::with_capacity(assignments.len() + skipped_results.len());
    pending.extend(skipped_results);

    let mut group_count = 0u64;
    let mut member_count = 0u64;
    let mut unique_count = 0u64;
    let mut skipped_count = 0u64;

    for r in &pending {
        if r.role == item_near_dup_role::SKIPPED {
            skipped_count += 1;
        }
    }

    for a in assignments {
        match a.role.as_str() {
            "pivot" => {
                group_count += 1;
                member_count += 1; // pivot counts as group member for summary
            }
            "member" => {
                member_count += 1;
            }
            "unique" => {
                unique_count += 1;
            }
            _ => {}
        }
        pending.push(PendingResult {
            item_id: a.item_id,
            role: a.role,
            group_id: a.group_id,
            pivot_item_id: a.pivot_item_id,
            similarity: a.similarity,
        });
    }

    // Stable write order by item_id for deterministic cursor resume
    pending.sort_by(|a, b| a.item_id.cmp(&b.item_id));

    // --- Write phase ---
    let batch_size = params.batch_size.max(1) as usize;
    let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let total = pending.len();
    let mut cursor_index = write_resume_from as usize;
    if cursor_index > total {
        cursor_index = total;
    }

    while cursor_index < total {
        if cancelled(cancel) {
            let completed = cursor_index as u64;
            let cursor = CheckpointCursor {
                phase: "write".into(),
                cursor_index: cursor_index as u64,
                completed_count: completed,
                group_count,
                member_count,
                unique_count,
                skipped_count,
                params: params_json.clone(),
            };
            let json = serde_json::to_string(&cursor)?;
            matter.apply_near_dup_batch_with_checkpoint(
                job_id,
                NEARDUP_STAGE,
                &[],
                &json,
                completed as i64,
            )?;
            progress(completed);
            return Ok(NearDupOutcome::Paused(NearDupSummary {
                completed_count: completed,
                group_count,
                member_count,
                unique_count,
                skipped_count,
            }));
        }

        let end = (cursor_index + batch_size).min(total);
        let slice = &pending[cursor_index..end];
        let updates: Vec<NearDupFieldUpdate> = slice
            .iter()
            .map(|r| NearDupFieldUpdate {
                item_id: r.item_id.clone(),
                near_dup_group_id: r.group_id.clone(),
                near_dup_role: Some(r.role.clone()),
                near_dup_similarity: r.similarity,
                near_dup_pivot_item_id: r.pivot_item_id.clone(),
                near_dup_method: Some(NEAR_DUP_METHOD.into()),
                near_duped_at: Some(now.clone()),
                near_dup_job_id: Some(job_id.into()),
            })
            .collect();

        cursor_index = end;
        let completed = cursor_index as u64;
        let cursor = CheckpointCursor {
            phase: "write".into(),
            cursor_index: cursor_index as u64,
            completed_count: completed,
            group_count,
            member_count,
            unique_count,
            skipped_count,
            params: params_json.clone(),
        };
        let json = serde_json::to_string(&cursor)?;
        matter.apply_near_dup_batch_with_checkpoint(
            job_id,
            NEARDUP_STAGE,
            &updates,
            &json,
            completed as i64,
        )?;
        progress(completed);
    }

    Ok(NearDupOutcome::Succeeded(NearDupSummary {
        completed_count: total as u64,
        group_count,
        member_count,
        unique_count,
        skipped_count,
    }))
}

fn read_cas_text(matter: &Matter, digest_hex: &str) -> Result<String> {
    let mut file = matter.cas().open_read(digest_hex)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    // Lossy UTF-8; empty after lossy still returns Ok (caller may skip on prep).
    Ok(String::from_utf8_lossy(&buf).into_owned())
}
