//! Core cull job: evaluate rules → family pass → write with checkpoints.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use chrono::Utc;
use matter_core::{item_cull_status, AuditEventInput, CullFieldUpdate, Matter};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::denist::{load_sha256_list, DenistList};
use crate::error::{CullError, Result};
use crate::eval::{evaluate_item, reasons_to_json, ItemCullDecision};
use crate::family::apply_family_policy;
use crate::params::CullParams;
use crate::presets::{builtin_rules, PRESET_UNIQUE_ONLY};
use crate::rules::CullRules;

/// Job kind string for process-runner.
pub const JOB_KIND_CULL: &str = "cull";
/// Checkpoint stage name.
pub const CULL_STAGE: &str = "cull";

/// Summary counts after a cull run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CullSummary {
    pub completed_count: u64,
    pub included: u64,
    pub culled: u64,
    /// Counts keyed by reason code (JSON object in audit).
    #[serde(default)]
    pub by_reason: HashMap<String, u64>,
}

/// Outcome of [`run_cull`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CullOutcome {
    Succeeded(CullSummary),
    Paused(CullSummary),
    Failed {
        message: String,
        summary: CullSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    /// `items` | `family` | `done`
    #[serde(default = "default_phase_items")]
    phase: String,
    cursor_index: u64,
    completed_count: u64,
    included: u64,
    culled: u64,
    #[serde(default)]
    by_reason: HashMap<String, u64>,
    params: serde_json::Value,
    /// Frozen resolved rules for resume.
    #[serde(default)]
    rules: serde_json::Value,
    #[serde(default)]
    preset_name: Option<String>,
    #[serde(default)]
    preset_id: Option<String>,
}

fn default_phase_items() -> String {
    "items".into()
}

/// Run cull on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between batches.
/// Calls `progress(completed_count)` after each committed write batch.
pub fn run_cull(
    matter: &Matter,
    job_id: &str,
    params: &CullParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<CullOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let (effective_params, rules, preset_name, preset_id) =
        resolve_params_and_rules(matter, params, prior.as_ref())?;
    let params_json = serde_json::to_value(&effective_params).unwrap_or_else(|_| json!({}));
    let rules_json = serde_json::to_value(&rules).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "cull.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "preset_name": preset_name,
            "preset_id": preset_id,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_cull_inner(
        matter,
        job_id,
        &effective_params,
        &rules,
        preset_name.as_deref(),
        preset_id.as_deref(),
        cancel,
        &progress,
        &params_json,
        &rules_json,
        prior,
    );

    match &result {
        Ok(CullOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "cull.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "included": s.included,
                    "culled": s.culled,
                    "completed_count": s.completed_count,
                    "by_reason": s.by_reason,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(CullOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(CullOutcome::Paused(_)) => {}
        Ok(CullOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "cull.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "included": summary.included,
                    "culled": summary.culled,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(CullError::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "cull.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(CullError::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, CULL_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(CullError::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn resolve_params_and_rules(
    matter: &Matter,
    call: &CullParams,
    prior: Option<&CheckpointCursor>,
) -> Result<(CullParams, CullRules, Option<String>, Option<String>)> {
    // Resume: freeze params + rules from checkpoint when present.
    if let Some(prior) = prior {
        if let Some(obj) = prior.params.as_object() {
            if !obj.is_empty() {
                let effective = CullParams::from_json(&prior.params.to_string())
                    .map_err(|e| CullError::InvalidParams(format!("checkpoint params: {e}")))?;
                let rules = if !prior.rules.is_null()
                    && prior
                        .rules
                        .as_object()
                        .map(|o| !o.is_empty())
                        .unwrap_or(false)
                {
                    CullRules::from_json(&prior.rules.to_string())?
                } else {
                    resolve_rules(matter, &effective)?.0
                };
                return Ok((
                    effective,
                    rules,
                    prior.preset_name.clone(),
                    prior.preset_id.clone(),
                ));
            }
        }
    }

    let effective = call.clone();
    effective.validate_shape()?;
    let (rules, name, id) = resolve_rules(matter, &effective)?;
    Ok((effective, rules, name, id))
}

fn resolve_rules(
    matter: &Matter,
    params: &CullParams,
) -> Result<(CullRules, Option<String>, Option<String>)> {
    if let Some(local) = params.try_resolve_local()? {
        return Ok(local);
    }

    if let Some(ref id) = params.preset_id {
        let preset = matter
            .get_cull_preset(id)
            .map_err(|e| CullError::InvalidParams(format!("preset_id '{id}': {e}")))?;
        let rules = CullRules::from_json(&preset.rules_json)?;
        return Ok((rules, Some(preset.name), Some(preset.id)));
    }

    let name = params.preset_name.as_deref().unwrap_or(PRESET_UNIQUE_ONLY);

    if let Some(rules) = builtin_rules(name) {
        rules.validate()?;
        return Ok((rules, Some(name.to_string()), None));
    }

    // Look up user preset by name.
    let presets = matter.list_cull_presets()?;
    if let Some(p) = presets.into_iter().find(|p| p.name == name) {
        let rules = CullRules::from_json(&p.rules_json)?;
        return Ok((rules, Some(p.name), Some(p.id)));
    }

    Err(CullError::InvalidParams(format!(
        "unknown cull preset '{name}' (not a built-in and not in cull_presets)"
    )))
}

#[allow(clippy::too_many_arguments)]
fn run_cull_inner(
    matter: &Matter,
    job_id: &str,
    params: &CullParams,
    rules: &CullRules,
    preset_name: Option<&str>,
    preset_id: Option<&str>,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    rules_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<CullOutcome> {
    let batch_size = params.batch_size.max(1);

    // Load DeNIST list once if enabled (fail closed).
    let denist: Option<DenistList> = if rules.denist.enabled {
        let path = rules
            .denist
            .hash_list_path
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .ok_or_else(|| {
                CullError::Denist(
                    "denist.enabled but hash_list_path is missing; provide a local SHA-256 list path"
                        .into(),
                )
            })?;
        Some(load_sha256_list(Path::new(path))?)
    } else {
        None
    };

    let mut cursor = prior.unwrap_or(CheckpointCursor {
        phase: "items".into(),
        cursor_index: 0,
        completed_count: 0,
        included: 0,
        culled: 0,
        by_reason: HashMap::new(),
        params: params_json.clone(),
        rules: rules_json.clone(),
        preset_name: preset_name.map(|s| s.to_string()),
        preset_id: preset_id.map(|s| s.to_string()),
    });
    cursor.params = params_json.clone();
    cursor.rules = rules_json.clone();
    cursor.preset_name = preset_name.map(|s| s.to_string());
    cursor.preset_id = preset_id.map(|s| s.to_string());

    let is_fresh =
        cursor.cursor_index == 0 && cursor.completed_count == 0 && cursor.phase == "items";
    let process_attachments = rules.roles.process_attachments;
    if params.reset && is_fresh {
        // Clear only the eligible set (same filter as list_cull_candidates).
        matter.clear_cull_fields(process_attachments)?;
    }

    let candidates = matter.list_cull_candidates(process_attachments)?;

    // --- Phase: items — evaluate + write in batches ---
    if cursor.phase == "items" {
        let start = cursor.cursor_index as usize;
        if start > candidates.len() {
            return Err(CullError::Other(format!(
                "checkpoint cursor_index {start} exceeds candidate count {}",
                candidates.len()
            )));
        }

        let mut batch: Vec<(String, ItemCullDecision)> = Vec::new();
        let now = Utc::now().to_rfc3339();

        for (i, cand) in candidates.iter().enumerate().skip(start) {
            if cancel.map(|f| f()).unwrap_or(false) {
                // Flush pending batch first? Prefer checkpoint at last committed.
                // Pending uncommitted batch is dropped (safe).
                let summary = summary_from_cursor(&cursor);
                return Ok(CullOutcome::Paused(summary));
            }

            let decision = evaluate_item(cand, rules, denist.as_ref());
            batch.push((cand.id.clone(), decision));

            if batch.len() as u64 >= batch_size || i + 1 == candidates.len() {
                let updates =
                    build_updates(&batch, job_id, preset_id, preset_name, &now, &mut cursor);
                let cursor_json = serde_json::to_string(&cursor)
                    .map_err(|e| CullError::Other(format!("checkpoint serialize: {e}")))?;
                matter.apply_cull_batch_with_checkpoint(
                    job_id,
                    CULL_STAGE,
                    &updates,
                    &cursor_json,
                    cursor.completed_count as i64,
                )?;
                progress(cursor.completed_count);
                batch.clear();
            }
        }

        // Move to family phase.
        cursor.phase = "family".into();
        cursor.cursor_index = 0;
        let cursor_json = serde_json::to_string(&cursor)
            .map_err(|e| CullError::Other(format!("checkpoint serialize: {e}")))?;
        matter.apply_cull_batch_with_checkpoint(
            job_id,
            CULL_STAGE,
            &[],
            &cursor_json,
            cursor.completed_count as i64,
        )?;
    }

    // --- Phase: family pass ---
    if cursor.phase == "family" {
        // Re-read candidates + current decisions from DB for family pass.
        // On resume mid-family we recompute decisions and continue writes from
        // cursor.cursor_index (cancel-aware every batch).
        let candidates = matter.list_cull_candidates(process_attachments)?;
        let mut decisions: HashMap<String, ItemCullDecision> = HashMap::new();
        for c in &candidates {
            let item = matter.get_item(&c.id)?;
            let reasons: Vec<String> = item
                .cull_reasons_json
                .as_deref()
                .and_then(|j| serde_json::from_str(j).ok())
                .unwrap_or_default();
            let status = item
                .cull_status
                .unwrap_or_else(|| item_cull_status::INCLUDED.into());
            decisions.insert(c.id.clone(), ItemCullDecision { status, reasons });
        }

        apply_family_policy(&candidates, &mut decisions, rules.family_policy);

        // Write final family decisions. Recompute counts from final decisions.
        let mut included = 0u64;
        let mut culled = 0u64;
        let mut by_reason: HashMap<String, u64> = HashMap::new();
        let now = Utc::now().to_rfc3339();
        let mut updates: Vec<CullFieldUpdate> = Vec::new();

        for c in &candidates {
            let d = decisions
                .get(&c.id)
                .cloned()
                .unwrap_or_else(ItemCullDecision::included);
            if d.is_culled() {
                culled += 1;
                for r in &d.reasons {
                    *by_reason.entry(r.clone()).or_insert(0) += 1;
                }
            } else {
                included += 1;
            }
            updates.push(CullFieldUpdate {
                item_id: c.id.clone(),
                cull_status: Some(d.status),
                cull_reasons_json: Some(reasons_to_json(&d.reasons)),
                cull_preset_id: preset_id.map(|s| s.to_string()),
                cull_preset_name: preset_name.map(|s| s.to_string()),
                culled_at: Some(now.clone()),
                cull_job_id: Some(job_id.to_string()),
            });
        }

        cursor.included = included;
        cursor.culled = culled;
        cursor.by_reason = by_reason.clone();

        // Resume family writes from checkpoint cursor (0 on first family entry).
        let mut offset = cursor.cursor_index as usize;
        if offset > updates.len() {
            return Err(CullError::Other(format!(
                "family checkpoint cursor_index {offset} exceeds update count {}",
                updates.len()
            )));
        }

        while offset < updates.len() {
            // Honor cancel on every family batch (not only the first).
            if cancel.map(|f| f()).unwrap_or(false) {
                cursor.phase = "family".into();
                cursor.cursor_index = offset as u64;
                cursor.included = included;
                cursor.culled = culled;
                cursor.by_reason = by_reason.clone();
                let cursor_json = serde_json::to_string(&cursor)
                    .map_err(|e| CullError::Other(format!("checkpoint serialize: {e}")))?;
                // Persist pause checkpoint (empty write batch) so resume is exact.
                matter.apply_cull_batch_with_checkpoint(
                    job_id,
                    CULL_STAGE,
                    &[],
                    &cursor_json,
                    cursor.completed_count as i64,
                )?;
                return Ok(CullOutcome::Paused(summary_from_cursor(&cursor)));
            }
            let end = (offset + batch_size as usize).min(updates.len());
            let slice = &updates[offset..end];
            cursor.cursor_index = end as u64;
            cursor.included = included;
            cursor.culled = culled;
            cursor.by_reason = by_reason.clone();
            if end == updates.len() {
                cursor.phase = "done".into();
            } else {
                cursor.phase = "family".into();
            }
            let cursor_json = serde_json::to_string(&cursor)
                .map_err(|e| CullError::Other(format!("checkpoint serialize: {e}")))?;
            matter.apply_cull_batch_with_checkpoint(
                job_id,
                CULL_STAGE,
                slice,
                &cursor_json,
                cursor.completed_count as i64,
            )?;
            progress(cursor.completed_count);
            offset = end;
        }
    }

    Ok(CullOutcome::Succeeded(summary_from_cursor(&cursor)))
}

fn build_updates(
    batch: &[(String, ItemCullDecision)],
    job_id: &str,
    preset_id: Option<&str>,
    preset_name: Option<&str>,
    now: &str,
    cursor: &mut CheckpointCursor,
) -> Vec<CullFieldUpdate> {
    let mut updates = Vec::with_capacity(batch.len());
    for (id, d) in batch {
        if d.is_culled() {
            cursor.culled += 1;
            for r in &d.reasons {
                *cursor.by_reason.entry(r.clone()).or_insert(0) += 1;
            }
        } else {
            cursor.included += 1;
        }
        cursor.completed_count += 1;
        cursor.cursor_index = cursor.completed_count;
        updates.push(CullFieldUpdate {
            item_id: id.clone(),
            cull_status: Some(d.status.clone()),
            cull_reasons_json: Some(reasons_to_json(&d.reasons)),
            cull_preset_id: preset_id.map(|s| s.to_string()),
            cull_preset_name: preset_name.map(|s| s.to_string()),
            culled_at: Some(now.to_string()),
            cull_job_id: Some(job_id.to_string()),
        });
    }
    updates
}

fn summary_from_cursor(cursor: &CheckpointCursor) -> CullSummary {
    CullSummary {
        completed_count: cursor.completed_count,
        included: cursor.included,
        culled: cursor.culled,
        by_reason: cursor.by_reason.clone(),
    }
}
