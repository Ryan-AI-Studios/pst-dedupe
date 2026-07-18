//! Core promote job: policy → expand → order → write with checkpoints.

use std::collections::HashSet;
use std::time::Instant;

use chrono::Utc;
use matter_core::{AuditEventInput, Matter, PromoteFieldUpdate, DEFAULT_REVIEW_SET_NAME};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{PromoteError, Result};
use crate::family::expand_families_bidirectional;
use crate::order::ordered_membership;
use crate::params::PromoteParams;
use crate::policy::{
    policy_implies_expand, resolve_policy, select_base_ids_from_candidates, POLICY_AUTO,
};

/// Job kind string for process-runner.
pub const JOB_KIND_PROMOTE: &str = "promote";
/// Checkpoint stage name.
pub const PROMOTE_STAGE: &str = "promote";

/// Summary counts after a promote run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromoteSummary {
    pub completed_count: u64,
    pub promoted_count: u64,
    pub resolved_policy: String,
    pub review_set_id: String,
    pub review_set_name: String,
}

/// Outcome of [`run_promote`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromoteOutcome {
    Succeeded(PromoteSummary),
    Paused(PromoteSummary),
    Failed {
        message: String,
        summary: PromoteSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    /// `write` | `done`
    #[serde(default = "default_phase_write")]
    phase: String,
    /// Index into the ordered membership stream (0-based).
    cursor_index: u64,
    completed_count: u64,
    promoted_count: u64,
    resolved_policy: String,
    review_set_id: String,
    review_set_name: String,
    params: serde_json::Value,
    /// Frozen ordered id list for stable resume (thin).
    #[serde(default)]
    ordered_ids: Vec<String>,
}

fn default_phase_write() -> String {
    "write".into()
}

/// Run promote on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between batches.
/// Calls `progress(completed_count)` after each committed write batch.
pub fn run_promote(
    matter: &Matter,
    job_id: &str,
    params: &PromoteParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<PromoteOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let (effective_params, resolved_policy) = resolve_params(matter, params, prior.as_ref())?;
    let params_json = serde_json::to_value(&effective_params).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "promote.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "resolved_policy": resolved_policy,
            "requested_policy": effective_params.policy,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_promote_inner(
        matter,
        job_id,
        &effective_params,
        &resolved_policy,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(PromoteOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "promote.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "promoted_count": s.promoted_count,
                    "completed_count": s.completed_count,
                    "resolved_policy": s.resolved_policy,
                    "review_set_id": s.review_set_id,
                    "review_set_name": s.review_set_name,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(PromoteOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(PromoteOutcome::Paused(_)) => {}
        Ok(PromoteOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "promote.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "promoted_count": summary.promoted_count,
                    "resolved_policy": summary.resolved_policy,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(PromoteError::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "promote.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(PromoteError::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, PROMOTE_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(PromoteError::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn resolve_params(
    matter: &Matter,
    call: &PromoteParams,
    prior: Option<&CheckpointCursor>,
) -> Result<(PromoteParams, String)> {
    if let Some(prior) = prior {
        if let Some(obj) = prior.params.as_object() {
            if !obj.is_empty() {
                let effective = PromoteParams::from_json(&prior.params.to_string())
                    .map_err(|e| PromoteError::InvalidParams(format!("checkpoint params: {e}")))?;
                let resolved = if !prior.resolved_policy.is_empty() {
                    prior.resolved_policy.clone()
                } else {
                    resolve_policy(matter, &effective.policy)?
                };
                return Ok((effective, resolved));
            }
        }
    }
    let effective = call.clone();
    effective.validate_shape()?;
    let resolved = resolve_policy(matter, &effective.policy)?;
    Ok((effective, resolved))
}

#[allow(clippy::too_many_arguments)]
fn run_promote_inner(
    matter: &Matter,
    job_id: &str,
    params: &PromoteParams,
    resolved_policy: &str,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<PromoteOutcome> {
    let batch_size = params.batch_size.max(1);

    let set_name = if params.review_set_name.trim().is_empty() {
        DEFAULT_REVIEW_SET_NAME
    } else {
        params.review_set_name.trim()
    };
    let review_set = matter.ensure_default_review_set(set_name)?;

    let mut cursor = prior.unwrap_or(CheckpointCursor {
        phase: "write".into(),
        cursor_index: 0,
        completed_count: 0,
        promoted_count: 0,
        resolved_policy: resolved_policy.to_string(),
        review_set_id: review_set.id.clone(),
        review_set_name: review_set.name.clone(),
        params: params_json.clone(),
        ordered_ids: Vec::new(),
    });
    cursor.params = params_json.clone();
    cursor.resolved_policy = resolved_policy.to_string();
    cursor.review_set_id = review_set.id.clone();
    cursor.review_set_name = review_set.name.clone();

    let is_fresh = cursor.cursor_index == 0
        && cursor.completed_count == 0
        && cursor.ordered_ids.is_empty()
        && cursor.phase == "write";

    if params.reset && is_fresh {
        matter.clear_review_membership_for_set(&review_set.id)?;
    }

    // Build or restore ordered membership.
    if cursor.ordered_ids.is_empty() {
        let candidates = matter.list_promote_candidates()?;
        let mut base = select_base_ids_from_candidates(
            matter,
            &candidates,
            resolved_policy,
            params.require_dedupe,
        )?;

        let do_expand = params.expand_families || policy_implies_expand(resolved_policy);
        if do_expand {
            base = expand_families_bidirectional(matter, &base)?;
        }

        // Unique ids, then single-pass order.
        let unique: HashSet<String> = base.into_iter().collect();
        let unique_vec: Vec<String> = unique.into_iter().collect();
        let ordered = ordered_membership(matter, &unique_vec)?;
        cursor.ordered_ids = ordered.into_iter().map(|r| r.id).collect();
        cursor.promoted_count = cursor.ordered_ids.len() as u64;
    }

    if cursor.phase == "done" {
        return Ok(PromoteOutcome::Succeeded(summary_from_cursor(&cursor)));
    }

    if params.fail_if_empty && cursor.ordered_ids.is_empty() {
        return Ok(PromoteOutcome::Failed {
            message: "fail_if_empty: review set membership is empty".into(),
            summary: summary_from_cursor(&cursor),
        });
    }

    let now = Utc::now().to_rfc3339();
    let total = cursor.ordered_ids.len();
    let mut offset = cursor.cursor_index as usize;
    if offset > total {
        return Err(PromoteError::Other(format!(
            "checkpoint cursor_index {offset} exceeds membership count {total}"
        )));
    }

    while offset < total {
        if cancel.map(|f| f()).unwrap_or(false) {
            cursor.cursor_index = offset as u64;
            cursor.phase = "write".into();
            let cursor_json = serde_json::to_string(&cursor)
                .map_err(|e| PromoteError::Other(format!("checkpoint serialize: {e}")))?;
            matter.apply_promote_batch_with_checkpoint(
                job_id,
                PROMOTE_STAGE,
                &[],
                &cursor_json,
                cursor.completed_count as i64,
            )?;
            return Ok(PromoteOutcome::Paused(summary_from_cursor(&cursor)));
        }

        let end = (offset + batch_size as usize).min(total);
        let mut updates = Vec::with_capacity(end - offset);
        for (i, id) in cursor.ordered_ids[offset..end].iter().enumerate() {
            let order = (offset + i + 1) as i64; // dense 1..N
            updates.push(PromoteFieldUpdate {
                item_id: id.clone(),
                in_review: Some(1),
                review_set_id: Some(review_set.id.clone()),
                review_order: Some(order),
                promoted_at: Some(now.clone()),
                promote_job_id: Some(job_id.to_string()),
                promote_policy: Some(resolved_policy.to_string()),
            });
        }

        cursor.cursor_index = end as u64;
        cursor.completed_count = end as u64;
        if end == total {
            cursor.phase = "done".into();
        }

        let cursor_json = serde_json::to_string(&cursor)
            .map_err(|e| PromoteError::Other(format!("checkpoint serialize: {e}")))?;
        matter.apply_promote_batch_with_checkpoint(
            job_id,
            PROMOTE_STAGE,
            &updates,
            &cursor_json,
            cursor.completed_count as i64,
        )?;
        progress(cursor.completed_count);
        offset = end;
    }

    // Snapshot review set meta after full write.
    let policy_json = params_json.to_string();
    matter.update_review_set_snapshot(
        &review_set.id,
        resolved_policy,
        Some(&policy_json),
        cursor.promoted_count as i64,
    )?;

    // Audit warning field for empty set (still Succeeded).
    if cursor.promoted_count == 0 {
        let _ = matter.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "promote.warning".into(),
            entity: format!("job:{job_id}"),
            params_json: json!({
                "warning": "empty_review_set",
                "resolved_policy": resolved_policy,
                "requested_policy": params.policy,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        });
    }

    let _ = POLICY_AUTO; // silence if unused in some builds
    Ok(PromoteOutcome::Succeeded(summary_from_cursor(&cursor)))
}

fn summary_from_cursor(cursor: &CheckpointCursor) -> PromoteSummary {
    PromoteSummary {
        completed_count: cursor.completed_count,
        promoted_count: cursor.promoted_count,
        resolved_policy: cursor.resolved_policy.clone(),
        review_set_id: cursor.review_set_id.clone(),
        review_set_name: cursor.review_set_name.clone(),
    }
}
