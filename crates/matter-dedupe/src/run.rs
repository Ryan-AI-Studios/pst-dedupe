//! Core dedupe algorithm: ordered parent pass + family attach linking.

use std::collections::HashMap;
use std::time::Instant;

use chrono::Utc;
use matter_core::{
    item_dedup_role, item_dedup_tier, AuditEventInput, DedupRoleUpdate, DedupeCandidate, Item,
    Matter,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{DedupeError, Result};
use crate::keys::{logical_hash_key, message_id_key, CompactKey};
use crate::params::DedupeParams;
use crate::policy::FamilyPolicy;

/// Job kind string for process-runner.
pub const JOB_KIND_DEDUPE: &str = "dedupe";
/// Checkpoint stage name.
pub const DEDUPE_STAGE: &str = "dedupe";

/// Summary counts after a dedupe run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupeSummary {
    pub unique: u64,
    pub duplicate: u64,
    pub skipped: u64,
    pub mid_logical_conflicts: u64,
    /// Parents processed in this run (including resumed progress).
    pub completed_count: u64,
}

/// Outcome of [`run_dedupe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupeOutcome {
    Succeeded(DedupeSummary),
    Paused(DedupeSummary),
    Failed {
        message: String,
        summary: DedupeSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct CheckpointCursor {
    cursor_index: u64,
    completed_count: u64,
    unique: u64,
    duplicate: u64,
    skipped: u64,
    mid_logical_conflicts: u64,
    /// `parents` while scoring parents; `family` during family attach pass.
    #[serde(default = "default_phase_parents")]
    phase: String,
    /// Index into parents that were marked duplicate (family pass only).
    #[serde(default)]
    family_cursor: u64,
    params: serde_json::Value,
}

fn default_phase_parents() -> String {
    "parents".into()
}

/// Run tiered dedupe on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between batches.
/// Calls `progress(completed_count)` after each committed batch.
///
/// **Resume:** when a prior checkpoint exists with a non-empty `params` object,
/// those frozen params are the source of truth for the run (call-site params are
/// ignored for scoring/policy). Corrupt non-empty checkpoint JSON is an error.
pub fn run_dedupe(
    matter: &Matter,
    job_id: &str,
    params: &DedupeParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<DedupeOutcome> {
    let started = Instant::now();

    // Resolve checkpoint + effective params before start audit so corrupt
    // checkpoints fail loudly and audit records the params actually used.
    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "dedupe.start".into(),
        entity: format!("job:{job_id}"),
        params_json: params_json.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_dedupe_inner(
        matter,
        job_id,
        &effective,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(DedupeOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "dedupe.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "unique": s.unique,
                    "duplicate": s.duplicate,
                    "skipped": s.skipped,
                    "mid_logical_conflicts": s.mid_logical_conflicts,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(DedupeOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(DedupeOutcome::Paused(_)) => {}
        Ok(DedupeOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "dedupe.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "unique": summary.unique,
                    "duplicate": summary.duplicate,
                    "skipped": summary.skipped,
                    "mid_logical_conflicts": summary.mid_logical_conflicts,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(DedupeError::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "dedupe.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(DedupeError::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

/// Load and parse the dedupe checkpoint. Missing/empty cursor → fresh start.
/// Non-empty but invalid JSON → hard error (not treated as absent).
fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, DEDUPE_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(DedupeError::Other(format!("corrupt checkpoint: {e}"))),
    }
}

/// On resume, checkpoint `params` (when a non-empty object) freeze the run.
fn effective_params(call: &DedupeParams, prior: Option<&CheckpointCursor>) -> Result<DedupeParams> {
    let Some(prior) = prior else {
        return Ok(call.clone());
    };
    let Some(obj) = prior.params.as_object() else {
        return Ok(call.clone());
    };
    if obj.is_empty() {
        return Ok(call.clone());
    }
    DedupeParams::from_json(&prior.params.to_string())
        .map_err(|e| DedupeError::InvalidParams(format!("checkpoint params: {e}")))
}

fn run_dedupe_inner(
    matter: &Matter,
    job_id: &str,
    params: &DedupeParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<DedupeOutcome> {
    let batch_size = params.batch_size.max(1);

    let mut cursor = prior.unwrap_or(CheckpointCursor {
        cursor_index: 0,
        completed_count: 0,
        unique: 0,
        duplicate: 0,
        skipped: 0,
        mid_logical_conflicts: 0,
        phase: "parents".into(),
        family_cursor: 0,
        params: params_json.clone(),
    });
    // Keep cursor.params aligned with effective (frozen) params for this run.
    cursor.params = params_json.clone();

    // Reset only on a fresh start (no checkpoint or cursor still at 0 in parents
    // phase with reset requested). On resume after committed batches, skip reset.
    let is_fresh = cursor.cursor_index == 0
        && cursor.completed_count == 0
        && cursor.phase == "parents"
        && cursor.family_cursor == 0;
    if params.reset && is_fresh {
        let include_att = matches!(
            params.family_policy,
            FamilyPolicy::SuppressChildrenWithParent
        );
        matter.clear_dedupe_fields(include_att)?;
    }

    // Rebuild compact maps from already-committed unique parents when resuming.
    let mut mid_map: HashMap<CompactKey, String> = HashMap::new();
    let mut logical_map: HashMap<CompactKey, String> = HashMap::new();
    // logical_hash of canonical by mid key — for conflict detection on resume.
    let mut mid_logical: HashMap<CompactKey, Option<String>> = HashMap::new();

    if cursor.cursor_index > 0 || cursor.phase != "parents" {
        rebuild_maps_from_committed(
            matter,
            cursor.cursor_index,
            params,
            &mut mid_map,
            &mut logical_map,
            &mut mid_logical,
        )?;
    }

    if cursor.phase == "parents" {
        let parent_pass = run_parent_pass(
            matter,
            job_id,
            params,
            cancel,
            progress,
            &mut cursor,
            params_json,
            batch_size,
            &mut mid_map,
            &mut logical_map,
            &mut mid_logical,
        )?;
        if let Some(outcome) = parent_pass {
            return Ok(outcome);
        }
        cursor.phase = "family".into();
        cursor.family_cursor = 0;
        // Persist phase transition with empty batch.
        commit_batch(matter, job_id, &[], &cursor, params_json, progress)?;
    }

    if matches!(
        params.family_policy,
        FamilyPolicy::SuppressChildrenWithParent
    ) {
        let family_pass = run_family_pass(
            matter,
            job_id,
            cancel,
            progress,
            &mut cursor,
            params_json,
            batch_size,
        )?;
        if let Some(outcome) = family_pass {
            return Ok(outcome);
        }
    }

    let summary = summary_from_cursor(&cursor);
    Ok(DedupeOutcome::Succeeded(summary))
}

fn rebuild_maps_from_committed(
    matter: &Matter,
    through_index: u64,
    params: &DedupeParams,
    mid_map: &mut HashMap<CompactKey, String>,
    logical_map: &mut HashMap<CompactKey, String>,
    mid_logical: &mut HashMap<CompactKey, Option<String>>,
) -> Result<()> {
    if through_index == 0 {
        return Ok(());
    }
    // Page already-processed uniques to rebuild maps.
    let page = matter.list_email_parents_for_dedupe_range(0, through_index)?;
    for c in page {
        if c.dedup_role.as_deref() != Some(item_dedup_role::UNIQUE) {
            continue;
        }
        if params.use_message_id {
            if let Some(ref mid) = c.message_id {
                if let Some(k) = message_id_key(mid) {
                    mid_map.entry(k).or_insert_with(|| c.id.clone());
                    mid_logical
                        .entry(k)
                        .or_insert_with(|| c.logical_hash.clone());
                }
            }
        }
        if params.use_logical_hash {
            if let Some(ref lh) = c.logical_hash {
                if let Some(k) = logical_hash_key(lh) {
                    logical_map.entry(k).or_insert_with(|| c.id.clone());
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_parent_pass(
    matter: &Matter,
    job_id: &str,
    params: &DedupeParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    cursor: &mut CheckpointCursor,
    params_json: &serde_json::Value,
    batch_size: u64,
    mid_map: &mut HashMap<CompactKey, String>,
    logical_map: &mut HashMap<CompactKey, String>,
    mid_logical: &mut HashMap<CompactKey, Option<String>>,
) -> Result<Option<DedupeOutcome>> {
    let total = matter.count_email_parents_for_dedupe()?;
    let mut staged: Vec<DedupRoleUpdate> = Vec::new();
    let now = now_rfc3339();

    let mut i = cursor.cursor_index;
    // Page size for loading candidates (independent of commit batch).
    const PAGE: u64 = 500;

    while i < total {
        if cancel.map(|f| f()).unwrap_or(false) {
            if !staged.is_empty() {
                // Do not advance cursor for uncommitted staged — flush empty;
                // commit only if we want partial batch. Spec: cancel between
                // batches commits current batch if any.
                commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
                staged.clear();
            }
            return Ok(Some(DedupeOutcome::Paused(summary_from_cursor(cursor))));
        }

        let page_limit = (total - i).min(PAGE);
        let page = matter.list_email_parents_for_dedupe_range(i, page_limit)?;
        if page.is_empty() {
            break;
        }

        for cand in page {
            if cancel.map(|f| f()).unwrap_or(false) {
                if !staged.is_empty() {
                    commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
                    staged.clear();
                }
                return Ok(Some(DedupeOutcome::Paused(summary_from_cursor(cursor))));
            }

            // Incremental mode: skip already-assigned roles.
            if !params.reset && cand.dedup_role.is_some() {
                // Still seed maps from existing uniques for subsequent items.
                seed_maps_from_existing(&cand, params, mid_map, logical_map, mid_logical);
                i += 1;
                cursor.cursor_index = i;
                cursor.completed_count = i;
                continue;
            }

            let update = resolve_parent(
                &cand,
                job_id,
                params,
                &now,
                mid_map,
                logical_map,
                mid_logical,
                &mut cursor.unique,
                &mut cursor.duplicate,
                &mut cursor.skipped,
                &mut cursor.mid_logical_conflicts,
            );
            staged.push(update);
            i += 1;
            cursor.cursor_index = i;
            cursor.completed_count = i;

            if staged.len() as u64 >= batch_size {
                commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
                staged.clear();
            }
        }
    }

    if !staged.is_empty() {
        commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
        staged.clear();
    }

    let _ = job_id;
    Ok(None)
}

fn seed_maps_from_existing(
    cand: &DedupeCandidate,
    params: &DedupeParams,
    mid_map: &mut HashMap<CompactKey, String>,
    logical_map: &mut HashMap<CompactKey, String>,
    mid_logical: &mut HashMap<CompactKey, Option<String>>,
) {
    if cand.dedup_role.as_deref() != Some(item_dedup_role::UNIQUE) {
        return;
    }
    if params.use_message_id {
        if let Some(ref mid) = cand.message_id {
            if let Some(k) = message_id_key(mid) {
                mid_map.entry(k).or_insert_with(|| cand.id.clone());
                mid_logical
                    .entry(k)
                    .or_insert_with(|| cand.logical_hash.clone());
            }
        }
    }
    if params.use_logical_hash {
        if let Some(ref lh) = cand.logical_hash {
            if let Some(k) = logical_hash_key(lh) {
                logical_map.entry(k).or_insert_with(|| cand.id.clone());
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_parent(
    cand: &DedupeCandidate,
    job_id: &str,
    params: &DedupeParams,
    now: &str,
    mid_map: &mut HashMap<CompactKey, String>,
    logical_map: &mut HashMap<CompactKey, String>,
    mid_logical: &mut HashMap<CompactKey, Option<String>>,
    unique: &mut u64,
    duplicate: &mut u64,
    _skipped: &mut u64,
    mid_logical_conflicts: &mut u64,
) -> DedupRoleUpdate {
    // Tier 1: Message-ID
    if params.use_message_id {
        if let Some(ref mid) = cand.message_id {
            if let Some(k) = message_id_key(mid) {
                if let Some(canon) = mid_map.get(&k) {
                    // Policy A: MID wins even if logical differs.
                    if let Some(canon_lh) = mid_logical.get(&k) {
                        if canon_lh.as_deref() != cand.logical_hash.as_deref() {
                            // Only count when both sides have a logical value and they differ,
                            // or one has and the other doesn't after both present?
                            // Spec: "MID matches with differing logical_hash"
                            if canon_lh.is_some()
                                && cand.logical_hash.is_some()
                                && canon_lh != &cand.logical_hash
                            {
                                *mid_logical_conflicts += 1;
                            } else if canon_lh != &cand.logical_hash {
                                // one Some one None also "differ"
                                *mid_logical_conflicts += 1;
                            }
                        }
                    }
                    *duplicate += 1;
                    return DedupRoleUpdate {
                        item_id: cand.id.clone(),
                        dedup_role: Some(item_dedup_role::DUPLICATE.into()),
                        duplicate_of_item_id: Some(canon.clone()),
                        dedup_tier: Some(item_dedup_tier::MESSAGE_ID.into()),
                        dedup_group_id: Some(canon.clone()),
                        deduped_at: Some(now.to_string()),
                        dedup_job_id: Some(job_id.to_string()),
                        extra_json: None,
                    };
                }
                // First seen for this MID → unique
                mid_map.insert(k, cand.id.clone());
                mid_logical.insert(k, cand.logical_hash.clone());
                // Also seed logical map so later empty-MID peers can find us? No —
                // tier order: if this item had MID it used MID. Peers with same
                // logical but no MID use logical map; if we want them to collapse
                // with this unique we should seed logical map for uniques.
                if params.use_logical_hash {
                    if let Some(ref lh) = cand.logical_hash {
                        if let Some(lk) = logical_hash_key(lh) {
                            logical_map.entry(lk).or_insert_with(|| cand.id.clone());
                        }
                    }
                }
                *unique += 1;
                return DedupRoleUpdate {
                    item_id: cand.id.clone(),
                    dedup_role: Some(item_dedup_role::UNIQUE.into()),
                    duplicate_of_item_id: None,
                    dedup_tier: Some(item_dedup_tier::MESSAGE_ID.into()),
                    dedup_group_id: Some(cand.id.clone()),
                    deduped_at: Some(now.to_string()),
                    dedup_job_id: Some(job_id.to_string()),
                    extra_json: None,
                };
            }
        }
    }

    // Tier 2: logical_hash
    if params.use_logical_hash {
        if let Some(ref lh) = cand.logical_hash {
            if let Some(k) = logical_hash_key(lh) {
                if let Some(canon) = logical_map.get(&k) {
                    *duplicate += 1;
                    return DedupRoleUpdate {
                        item_id: cand.id.clone(),
                        dedup_role: Some(item_dedup_role::DUPLICATE.into()),
                        duplicate_of_item_id: Some(canon.clone()),
                        dedup_tier: Some(item_dedup_tier::LOGICAL_HASH.into()),
                        dedup_group_id: Some(canon.clone()),
                        deduped_at: Some(now.to_string()),
                        dedup_job_id: Some(job_id.to_string()),
                        extra_json: None,
                    };
                }
                logical_map.insert(k, cand.id.clone());
                *unique += 1;
                return DedupRoleUpdate {
                    item_id: cand.id.clone(),
                    dedup_role: Some(item_dedup_role::UNIQUE.into()),
                    duplicate_of_item_id: None,
                    dedup_tier: Some(item_dedup_tier::LOGICAL_HASH.into()),
                    dedup_group_id: Some(cand.id.clone()),
                    deduped_at: Some(now.to_string()),
                    dedup_job_id: Some(job_id.to_string()),
                    extra_json: None,
                };
            }
        }
    }

    // No usable key → unique / none
    *unique += 1;
    DedupRoleUpdate {
        item_id: cand.id.clone(),
        dedup_role: Some(item_dedup_role::UNIQUE.into()),
        duplicate_of_item_id: None,
        dedup_tier: Some(item_dedup_tier::NONE.into()),
        dedup_group_id: Some(cand.id.clone()),
        deduped_at: Some(now.to_string()),
        dedup_job_id: Some(job_id.to_string()),
        extra_json: None,
    }
}

fn run_family_pass(
    matter: &Matter,
    job_id: &str,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    cursor: &mut CheckpointCursor,
    params_json: &serde_json::Value,
    batch_size: u64,
) -> Result<Option<DedupeOutcome>> {
    // Collect parent ids that are duplicates (thin query via list + filter).
    // For large matters this is still only ids/roles, not bodies.
    let parents = matter.list_email_parents_for_dedupe()?;
    let dup_parents: Vec<DedupeCandidate> = parents
        .into_iter()
        .filter(|p| p.dedup_role.as_deref() == Some(item_dedup_role::DUPLICATE))
        .collect();

    let mut staged: Vec<DedupRoleUpdate> = Vec::new();
    let now = now_rfc3339();
    let mut idx = cursor.family_cursor;

    while (idx as usize) < dup_parents.len() {
        if cancel.map(|f| f()).unwrap_or(false) {
            if !staged.is_empty() {
                commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
                staged.clear();
            }
            return Ok(Some(DedupeOutcome::Paused(summary_from_cursor(cursor))));
        }

        let dup_parent = &dup_parents[idx as usize];
        let canon_parent_id = matter
            .get_item(&dup_parent.id)?
            .duplicate_of_item_id
            .clone();

        let mut canon_children: Vec<Item> = match canon_parent_id.as_deref() {
            Some(cid) => matter.list_attachments(cid)?,
            None => Vec::new(),
        };
        // Stable order for multi-match: path then id.
        canon_children.sort_by(|a, b| {
            let pa = a.path.as_deref().unwrap_or("");
            let pb = b.path.as_deref().unwrap_or("");
            pa.cmp(pb).then_with(|| a.id.cmp(&b.id))
        });

        let mut dup_children = matter.list_attachments(&dup_parent.id)?;
        dup_children.sort_by(|a, b| {
            let pa = a.path.as_deref().unwrap_or("");
            let pb = b.path.as_deref().unwrap_or("");
            pa.cmp(pb).then_with(|| a.id.cmp(&b.id))
        });

        for child in dup_children {
            let (dup_of, unmatched) = match_attach(&child, &canon_children);
            // Never set duplicate_of to parent email id.
            if let Some(ref d) = dup_of {
                if canon_parent_id.as_deref() == Some(d.as_str()) {
                    return Err(DedupeError::Other(
                        "internal: attach duplicate_of resolved to parent email".into(),
                    ));
                }
            }

            // Resume re-processes attaches for the current parent after mid-parent
            // commits. Role UPDATEs are idempotent; only count a child once so
            // summary/audit `duplicate` does not inflate.
            let already_family = child.dedup_role.as_deref() == Some(item_dedup_role::DUPLICATE)
                && child.dedup_tier.as_deref() == Some(item_dedup_tier::FAMILY);

            let extra = merge_family_extra_json(child.extra_json.as_deref(), unmatched);

            staged.push(DedupRoleUpdate {
                item_id: child.id,
                dedup_role: Some(item_dedup_role::DUPLICATE.into()),
                duplicate_of_item_id: dup_of,
                dedup_tier: Some(item_dedup_tier::FAMILY.into()),
                dedup_group_id: canon_parent_id.clone(),
                deduped_at: Some(now.clone()),
                dedup_job_id: Some(job_id.to_string()),
                extra_json: extra,
            });
            if !already_family {
                cursor.duplicate += 1;
            }

            // Commit mid-parent when batch is full; family_cursor advances only
            // after the whole parent is processed so resume re-processes attaches
            // (idempotent UPDATE; counts skip already_family above).
            if staged.len() as u64 >= batch_size {
                commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
                staged.clear();
                // Poll cancel after every committed batch so mid-parent cancels
                // are observed promptly (not only at the next parent boundary).
                if cancel.map(|f| f()).unwrap_or(false) {
                    return Ok(Some(DedupeOutcome::Paused(summary_from_cursor(cursor))));
                }
            }
        }

        idx += 1;
        cursor.family_cursor = idx;

        if staged.len() as u64 >= batch_size {
            commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
            staged.clear();
            if cancel.map(|f| f()).unwrap_or(false) {
                return Ok(Some(DedupeOutcome::Paused(summary_from_cursor(cursor))));
            }
        }
    }

    if !staged.is_empty() {
        commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
    }

    Ok(None)
}

/// Merge or clear `family_attach_unmatched` in `extra_json`.
///
/// - `unmatched == true`: set flag true (preserve other keys).
/// - `unmatched == false`: strip the flag if present so a later match after a
///   prior unmatched run stays honest; leave column unchanged when nothing to strip.
fn merge_family_extra_json(existing: Option<&str>, unmatched: bool) -> Option<Option<String>> {
    if unmatched {
        let mut v = serde_json::Map::new();
        if let Some(raw) = existing {
            if let Ok(serde_json::Value::Object(m)) = serde_json::from_str::<serde_json::Value>(raw)
            {
                v = m;
            }
        }
        v.insert("family_attach_unmatched".into(), json!(true));
        return Some(Some(serde_json::Value::Object(v).to_string()));
    }

    // Matched: strip stale unmatched flag when present.
    let raw = existing?;
    let Ok(serde_json::Value::Object(mut m)) = serde_json::from_str::<serde_json::Value>(raw)
    else {
        return None;
    };
    m.remove("family_attach_unmatched")?;
    Some(Some(serde_json::Value::Object(m).to_string()))
}

/// Resolve attach twin on canonical parent: native_sha256 → name+size → unmatched.
///
/// Returns `(duplicate_of_item_id, unmatched)`.
fn match_attach(child: &Item, canon_children: &[Item]) -> (Option<String>, bool) {
    // 1. Same native_sha256
    if let Some(ref digest) = child.native_sha256 {
        if !digest.is_empty() {
            let mut matches: Vec<&Item> = canon_children
                .iter()
                .filter(|c| c.native_sha256.as_deref() == Some(digest.as_str()))
                .collect();
            if !matches.is_empty() {
                matches.sort_by(|a, b| {
                    let pa = a.path.as_deref().unwrap_or("");
                    let pb = b.path.as_deref().unwrap_or("");
                    pa.cmp(pb).then_with(|| a.id.cmp(&b.id))
                });
                return (Some(matches[0].id.clone()), false);
            }
        }
    }

    // 2. Same case-folded filename + size_bytes
    let child_name = filename_lower(child);
    let child_size = child.size_bytes;
    if !child_name.is_empty() {
        let mut matches: Vec<&Item> = canon_children
            .iter()
            .filter(|c| {
                filename_lower(c) == child_name
                    && c.size_bytes == child_size
                    && child_size.is_some()
            })
            .collect();
        if !matches.is_empty() {
            matches.sort_by(|a, b| {
                let pa = a.path.as_deref().unwrap_or("");
                let pb = b.path.as_deref().unwrap_or("");
                pa.cmp(pb).then_with(|| a.id.cmp(&b.id))
            });
            return (Some(matches[0].id.clone()), false);
        }
    }

    // 3. Unmatched — duplicate with NULL duplicate_of (never parent email).
    (None, true)
}

fn filename_lower(item: &Item) -> String {
    let path = item.path.as_deref().unwrap_or("");
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.to_lowercase()
}

fn commit_batch(
    matter: &Matter,
    job_id: &str,
    updates: &[DedupRoleUpdate],
    cursor: &CheckpointCursor,
    params_json: &serde_json::Value,
    progress: &impl Fn(u64),
) -> Result<()> {
    let mut cursor = cursor.clone();
    cursor.params = params_json.clone();
    let cursor_json = serde_json::to_string(&cursor)?;
    matter.apply_dedup_batch_with_checkpoint(
        job_id,
        DEDUPE_STAGE,
        updates,
        &cursor_json,
        cursor.completed_count as i64,
    )?;
    progress(cursor.completed_count);
    Ok(())
}

fn summary_from_cursor(cursor: &CheckpointCursor) -> DedupeSummary {
    DedupeSummary {
        unique: cursor.unique,
        duplicate: cursor.duplicate,
        skipped: cursor.skipped,
        mid_logical_conflicts: cursor.mid_logical_conflicts,
        completed_count: cursor.completed_count,
    }
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
