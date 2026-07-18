//! Core threading algorithm: header graph + subject + ConversationIndex + family.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use chrono::Utc;
use matter_core::{
    item_thread_method, normalize_message_id, parse_references_json, AuditEventInput, Matter,
    ThreadCandidate, ThreadFieldUpdate,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Result, ThreadError};
use crate::keys::{
    conversation_index_key, message_id_key, sha256_hex, subject_key_hash, CompactKey,
};
use crate::normalize::normalize_subject_thread;
use crate::params::ThreadParams;
use crate::unionfind::UnionFind;

/// Job kind string for process-runner.
pub const JOB_KIND_THREAD: &str = "thread";
/// Checkpoint stage name.
pub const THREAD_STAGE: &str = "thread";

/// Opaque ConversationIndex prefix length in hex characters (22 bytes).
pub const CONVERSATION_INDEX_PREFIX_HEX_LEN: usize = 44;

/// Summary counts after a thread run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub completed_count: u64,
    pub thread_count: u64,
    pub header_linked: u64,
    pub subject_linked: u64,
    pub index_linked: u64,
    pub singleton: u64,
}

/// Outcome of [`run_thread`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadOutcome {
    Succeeded(ThreadSummary),
    Paused(ThreadSummary),
    Failed {
        message: String,
        summary: ThreadSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    completed_count: u64,
    thread_count: u64,
    header_linked: u64,
    subject_linked: u64,
    index_linked: u64,
    singleton: u64,
    /// `assign` while writing parent assignments; `family` during family inherit.
    #[serde(default = "default_phase_assign")]
    phase: String,
    /// Index into parents that have non-null thread_id (family pass only).
    #[serde(default)]
    family_cursor: u64,
    params: serde_json::Value,
}

fn default_phase_assign() -> String {
    "assign".into()
}

/// Per-parent assignment produced by the algorithm (in stable order).
#[derive(Debug, Clone)]
struct Assignment {
    item_id: String,
    thread_id: String,
    thread_root_item_id: String,
    thread_method: String,
}

/// Run email threading on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between batches.
/// Calls `progress(completed_count)` after each committed batch.
///
/// **Resume:** when a prior checkpoint exists with a non-empty `params` object,
/// those frozen params are the source of truth. Corrupt non-empty checkpoint
/// JSON is an error.
///
/// **`thread_id` scheme (full 64-char SHA-256 hex):**
/// - Headers: `SHA-256("thread:v1\n" || min matter MID in component)`
/// - Subject: `SHA-256("thread-subj:v1\n" || subject_key)`
/// - ConversationIndex: `SHA-256("thread-ci:v1\n" || prefix44)`
/// - Singleton with no MID: own item id as `thread_id`
pub fn run_thread(
    matter: &Matter,
    job_id: &str,
    params: &ThreadParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<ThreadOutcome> {
    let started = Instant::now();

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "thread.start".into(),
        entity: format!("job:{job_id}"),
        params_json: params_json.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_thread_inner(
        matter,
        job_id,
        &effective,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(ThreadOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "thread.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "completed_count": s.completed_count,
                    "thread_count": s.thread_count,
                    "header_linked": s.header_linked,
                    "subject_linked": s.subject_linked,
                    "index_linked": s.index_linked,
                    "singleton": s.singleton,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(ThreadOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(ThreadOutcome::Paused(_)) => {}
        Ok(ThreadOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "thread.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "thread_count": summary.thread_count,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(ThreadError::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "thread.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(ThreadError::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, THREAD_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(ThreadError::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(call: &ThreadParams, prior: Option<&CheckpointCursor>) -> Result<ThreadParams> {
    let Some(prior) = prior else {
        return Ok(call.clone());
    };
    let Some(obj) = prior.params.as_object() else {
        return Ok(call.clone());
    };
    if obj.is_empty() {
        return Ok(call.clone());
    }
    ThreadParams::from_json(&prior.params.to_string())
        .map_err(|e| ThreadError::InvalidParams(format!("checkpoint params: {e}")))
}

fn run_thread_inner(
    matter: &Matter,
    job_id: &str,
    params: &ThreadParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<ThreadOutcome> {
    let batch_size = params.batch_size.max(1);

    let mut cursor = prior.unwrap_or(CheckpointCursor {
        cursor_index: 0,
        completed_count: 0,
        thread_count: 0,
        header_linked: 0,
        subject_linked: 0,
        index_linked: 0,
        singleton: 0,
        phase: "assign".into(),
        family_cursor: 0,
        params: params_json.clone(),
    });
    cursor.params = params_json.clone();

    let is_fresh = cursor.cursor_index == 0
        && cursor.completed_count == 0
        && cursor.phase == "assign"
        && cursor.family_cursor == 0;
    if params.reset && is_fresh {
        matter.clear_thread_fields(params.family_inherit)?;
    }

    // Load thin candidates (identity + headers only — no bodies).
    let candidates = matter.list_email_parents_for_thread()?;
    let assignments = compute_assignments(&candidates, params);

    // Count methods for summary (on full plan).
    let mut plan_header = 0u64;
    let mut plan_subject = 0u64;
    let mut plan_index = 0u64;
    let mut plan_singleton = 0u64;
    let mut distinct_threads: HashSet<String> = HashSet::new();
    for a in &assignments {
        distinct_threads.insert(a.thread_id.clone());
        match a.thread_method.as_str() {
            item_thread_method::HEADERS => plan_header += 1,
            item_thread_method::SUBJECT => plan_subject += 1,
            item_thread_method::CONVERSATION_INDEX => plan_index += 1,
            _ => plan_singleton += 1,
        }
    }

    if cursor.phase == "assign" {
        let now = now_rfc3339();
        let mut staged: Vec<ThreadFieldUpdate> = Vec::new();
        let mut i = cursor.cursor_index as usize;

        while i < assignments.len() {
            if cancel.map(|f| f()).unwrap_or(false) {
                if !staged.is_empty() {
                    commit_batch(matter, job_id, &staged, &cursor, params_json, progress)?;
                    staged.clear();
                }
                return Ok(ThreadOutcome::Paused(summary_from_cursor(&cursor)));
            }

            let a = &assignments[i];
            // Incremental: skip items that already have thread_id when !reset.
            if !params.reset {
                if let Some(cand) = candidates.get(i) {
                    if cand.thread_id.is_some() {
                        i += 1;
                        cursor.cursor_index = i as u64;
                        cursor.completed_count = i as u64;
                        continue;
                    }
                }
            }

            staged.push(ThreadFieldUpdate {
                item_id: a.item_id.clone(),
                thread_id: Some(a.thread_id.clone()),
                thread_root_item_id: Some(a.thread_root_item_id.clone()),
                thread_method: Some(a.thread_method.clone()),
                threaded_at: Some(now.clone()),
                thread_job_id: Some(job_id.to_string()),
            });
            i += 1;
            cursor.cursor_index = i as u64;
            cursor.completed_count = i as u64;
            // Progress counts: recompute from plan prefix for checkpoint stability.
            cursor.header_linked = plan_header.min(i as u64);
            cursor.subject_linked = plan_subject.min(i as u64);
            cursor.index_linked = plan_index.min(i as u64);
            cursor.singleton = plan_singleton.min(i as u64);
            cursor.thread_count = distinct_threads.len() as u64;

            if staged.len() as u64 >= batch_size {
                // Accurate counts from committed portion
                recount_prefix(&assignments, i, &mut cursor);
                commit_batch(matter, job_id, &staged, &cursor, params_json, progress)?;
                staged.clear();
            }
        }

        if !staged.is_empty() {
            recount_prefix(&assignments, assignments.len(), &mut cursor);
            commit_batch(matter, job_id, &staged, &cursor, params_json, progress)?;
            staged.clear();
        }

        recount_prefix(&assignments, assignments.len(), &mut cursor);
        cursor.phase = "family".into();
        cursor.family_cursor = 0;
        commit_batch(matter, job_id, &[], &cursor, params_json, progress)?;
    }

    if params.family_inherit {
        if let Some(outcome) = run_family_pass(
            matter,
            job_id,
            cancel,
            progress,
            &mut cursor,
            params_json,
            batch_size,
            &candidates,
            &assignments,
        )? {
            return Ok(outcome);
        }
    }

    recount_prefix(&assignments, assignments.len(), &mut cursor);
    Ok(ThreadOutcome::Succeeded(summary_from_cursor(&cursor)))
}

fn recount_prefix(assignments: &[Assignment], through: usize, cursor: &mut CheckpointCursor) {
    let mut header = 0u64;
    let mut subject = 0u64;
    let mut index = 0u64;
    let mut singleton = 0u64;
    let mut threads = HashSet::new();
    for a in assignments.iter().take(through) {
        threads.insert(a.thread_id.clone());
        match a.thread_method.as_str() {
            item_thread_method::HEADERS => header += 1,
            item_thread_method::SUBJECT => subject += 1,
            item_thread_method::CONVERSATION_INDEX => index += 1,
            _ => singleton += 1,
        }
    }
    cursor.header_linked = header;
    cursor.subject_linked = subject;
    cursor.index_linked = index;
    cursor.singleton = singleton;
    cursor.thread_count = threads.len() as u64;
}

/// Compute all parent assignments in memory from thin rows.
fn compute_assignments(candidates: &[ThreadCandidate], params: &ThreadParams) -> Vec<Assignment> {
    let n = candidates.len();
    // index → assignment (filled progressively)
    let mut method: Vec<Option<String>> = vec![None; n];
    let mut thread_id: Vec<Option<String>> = vec![None; n];
    let mut root_id: Vec<Option<String>> = vec![None; n];

    // --- Phase A: header graph ---
    if params.use_headers {
        let mut uf = UnionFind::new();
        // compact key → normalized MID string (only when we know the string)
        let mut key_to_mid: HashMap<CompactKey, String> = HashMap::new();
        // items that have a header edge (In-Reply-To or References non-empty linking)
        let mut has_header_edge: Vec<bool> = vec![false; n];

        for (i, c) in candidates.iter().enumerate() {
            if let Some(ref mid) = c.message_id {
                if let Some(k) = message_id_key(mid) {
                    let norm = normalize_message_id(mid);
                    key_to_mid.insert(k, norm);
                    uf.make_set(k);
                }
            }
            if let Some(ref irt) = c.in_reply_to {
                if let Some(k) = message_id_key(irt) {
                    let norm = normalize_message_id(irt);
                    key_to_mid.entry(k).or_insert(norm);
                    uf.make_set(k);
                    has_header_edge[i] = true;
                    if let Some(ref mid) = c.message_id {
                        if let Some(mk) = message_id_key(mid) {
                            uf.union(mk, k);
                        } else {
                            // Item has In-Reply-To but no own MID — still join via phantom
                            // by tracking a synthetic per-item key? Spec: union over MIDs.
                            // Without own MID, we only have the referenced MID. Collect later.
                        }
                    }
                }
            }
            let refs = parse_references_json(c.references_json.as_deref());
            for r in &refs {
                if let Some(k) = message_id_key(r) {
                    key_to_mid
                        .entry(k)
                        .or_insert_with(|| normalize_message_id(r));
                    uf.make_set(k);
                    has_header_edge[i] = true;
                    if let Some(ref mid) = c.message_id {
                        if let Some(mk) = message_id_key(mid) {
                            uf.union(mk, k);
                        }
                    }
                }
            }
            // Phantom-only links: if item has no own MID but has IRT/refs, union those
            // referenced MIDs together so children sharing a phantom parent join.
            if c.message_id.is_none()
                || message_id_key(c.message_id.as_deref().unwrap_or("")).is_none()
            {
                let mut ref_keys: Vec<CompactKey> = Vec::new();
                if let Some(ref irt) = c.in_reply_to {
                    if let Some(k) = message_id_key(irt) {
                        ref_keys.push(k);
                    }
                }
                for r in &refs {
                    if let Some(k) = message_id_key(r) {
                        ref_keys.push(k);
                    }
                }
                for w in ref_keys.windows(2) {
                    uf.union(w[0], w[1]);
                }
            }
        }

        // Map each item to component root key.
        // Items with own MID: component of that MID.
        // Items without own MID but with IRT/refs: component of first linked MID.
        let mut item_component: Vec<Option<CompactKey>> = vec![None; n];
        for (i, c) in candidates.iter().enumerate() {
            if let Some(ref mid) = c.message_id {
                if let Some(mk) = message_id_key(mid) {
                    item_component[i] = Some(uf.find(mk));
                    continue;
                }
            }
            if let Some(ref irt) = c.in_reply_to {
                if let Some(k) = message_id_key(irt) {
                    item_component[i] = Some(uf.find(k));
                    continue;
                }
            }
            let refs = parse_references_json(c.references_json.as_deref());
            if let Some(r) = refs.first() {
                if let Some(k) = message_id_key(r) {
                    item_component[i] = Some(uf.find(k));
                }
            }
        }

        // Group item indices by component.
        let mut components: HashMap<CompactKey, Vec<usize>> = HashMap::new();
        for (i, comp) in item_component.iter().enumerate() {
            if let Some(ck) = comp {
                components.entry(*ck).or_default().push(i);
            }
        }

        for (_root, members) in components {
            if members.is_empty() {
                continue;
            }
            // Collect matter MIDs in this component for thread_id.
            let mut matter_mids: Vec<String> = Vec::new();
            for &i in &members {
                if let Some(ref mid) = candidates[i].message_id {
                    let norm = normalize_message_id(mid);
                    if !norm.is_empty() {
                        matter_mids.push(norm);
                    }
                }
            }
            matter_mids.sort();
            matter_mids.dedup();

            // Root = earliest by stable order (candidates already sorted).
            let Some(&root_idx) = members.iter().min() else {
                continue;
            };
            let root_item = candidates[root_idx].id.clone();

            let tid = if let Some(min_mid) = matter_mids.first() {
                sha256_hex(&format!("thread:v1\n{min_mid}"))
            } else {
                // No matter MID — use root item id.
                root_item.clone()
            };

            // Multi-member or any with header edge → headers; pure singleton MID only → singleton
            let multi = members.len() >= 2;
            for &i in &members {
                if multi || has_header_edge[i] {
                    // Pure singleton with only own MID and no refs → singleton later
                    if multi || has_header_edge[i] {
                        method[i] = Some(item_thread_method::HEADERS.into());
                        thread_id[i] = Some(tid.clone());
                        root_id[i] = Some(root_item.clone());
                    }
                }
            }
            // Spec: Pure singleton with only own MID and no refs → singleton
            if members.len() == 1 {
                let i = members[0];
                if !has_header_edge[i] {
                    // Leave for singleton assignment
                    method[i] = None;
                    thread_id[i] = None;
                    root_id[i] = None;
                }
            }
        }
    }

    // --- Phase B: subject fallback among remaining unassigned ---
    if params.use_subject_fallback {
        let mut by_subject: HashMap<CompactKey, Vec<usize>> = HashMap::new();
        let mut subject_str: HashMap<CompactKey, String> = HashMap::new();
        for (i, c) in candidates.iter().enumerate() {
            if method[i].is_some() {
                continue;
            }
            // Only remaining singletons / unthreaded
            let key = c
                .subject
                .as_deref()
                .map(normalize_subject_thread)
                .filter(|s| !s.is_empty());
            let Some(sk) = key else {
                continue;
            };
            let hk = subject_key_hash(&sk);
            subject_str.entry(hk).or_insert(sk);
            by_subject.entry(hk).or_default().push(i);
        }
        for (hk, members) in by_subject {
            if members.len() < 2 {
                continue;
            }
            let sk = subject_str.get(&hk).map(|s| s.as_str()).unwrap_or("");
            let tid = sha256_hex(&format!("thread-subj:v1\n{sk}"));
            let Some(&root_idx) = members.iter().min() else {
                continue;
            };
            let root_item = candidates[root_idx].id.clone();
            for &i in &members {
                method[i] = Some(item_thread_method::SUBJECT.into());
                thread_id[i] = Some(tid.clone());
                root_id[i] = Some(root_item.clone());
            }
        }
    }

    // --- Phase C: ConversationIndex among remaining ---
    if params.use_conversation_index {
        let mut by_ci: HashMap<CompactKey, Vec<usize>> = HashMap::new();
        let mut ci_str: HashMap<CompactKey, String> = HashMap::new();
        for (i, c) in candidates.iter().enumerate() {
            if method[i].is_some() {
                continue;
            }
            let Some(ref hex) = c.conversation_index_hex else {
                continue;
            };
            let hex = hex.trim();
            if hex.len() < CONVERSATION_INDEX_PREFIX_HEX_LEN {
                continue;
            }
            let prefix = hex[..CONVERSATION_INDEX_PREFIX_HEX_LEN].to_ascii_lowercase();
            if !prefix.chars().all(|ch| ch.is_ascii_hexdigit()) {
                continue;
            }
            let hk = conversation_index_key(&prefix);
            ci_str.entry(hk).or_insert(prefix);
            by_ci.entry(hk).or_default().push(i);
        }
        for (hk, members) in by_ci {
            if members.len() < 2 {
                continue;
            }
            let prefix = ci_str.get(&hk).map(|s| s.as_str()).unwrap_or("");
            let tid = sha256_hex(&format!("thread-ci:v1\n{prefix}"));
            let Some(&root_idx) = members.iter().min() else {
                continue;
            };
            let root_item = candidates[root_idx].id.clone();
            for &i in &members {
                method[i] = Some(item_thread_method::CONVERSATION_INDEX.into());
                thread_id[i] = Some(tid.clone());
                root_id[i] = Some(root_item.clone());
            }
        }
    }

    // --- Remaining: singletons ---
    let mut out = Vec::with_capacity(n);
    for (i, c) in candidates.iter().enumerate() {
        if let (Some(m), Some(tid), Some(root)) =
            (method[i].clone(), thread_id[i].clone(), root_id[i].clone())
        {
            out.push(Assignment {
                item_id: c.id.clone(),
                thread_id: tid,
                thread_root_item_id: root,
                thread_method: m,
            });
            continue;
        }
        // Singleton: unique thread_id
        let tid = if let Some(ref mid) = c.message_id {
            let norm = normalize_message_id(mid);
            if !norm.is_empty() {
                sha256_hex(&format!("thread:v1\n{norm}"))
            } else {
                c.id.clone()
            }
        } else {
            c.id.clone()
        };
        out.push(Assignment {
            item_id: c.id.clone(),
            thread_id: tid,
            thread_root_item_id: c.id.clone(),
            thread_method: item_thread_method::SINGLETON.into(),
        });
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn run_family_pass(
    matter: &Matter,
    job_id: &str,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    cursor: &mut CheckpointCursor,
    params_json: &serde_json::Value,
    batch_size: u64,
    candidates: &[ThreadCandidate],
    assignments: &[Assignment],
) -> Result<Option<ThreadOutcome>> {
    // Parents with thread assignments (all after assign phase).
    let now = now_rfc3339();
    let mut staged: Vec<ThreadFieldUpdate> = Vec::new();
    let mut i = cursor.family_cursor as usize;

    while i < candidates.len() {
        if cancel.map(|f| f()).unwrap_or(false) {
            if !staged.is_empty() {
                commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
                staged.clear();
            }
            return Ok(Some(ThreadOutcome::Paused(summary_from_cursor(cursor))));
        }

        let parent_id = &candidates[i].id;
        let assign = &assignments[i];
        let children = matter.list_attachments(parent_id)?;
        for child in children {
            staged.push(ThreadFieldUpdate {
                item_id: child.id,
                thread_id: Some(assign.thread_id.clone()),
                thread_root_item_id: Some(assign.thread_root_item_id.clone()),
                thread_method: Some(assign.thread_method.clone()),
                threaded_at: Some(now.clone()),
                thread_job_id: Some(job_id.to_string()),
            });
            if staged.len() as u64 >= batch_size {
                cursor.family_cursor = i as u64;
                commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
                staged.clear();
            }
        }
        i += 1;
        cursor.family_cursor = i as u64;
    }

    if !staged.is_empty() {
        commit_batch(matter, job_id, &staged, cursor, params_json, progress)?;
    }
    Ok(None)
}

fn commit_batch(
    matter: &Matter,
    job_id: &str,
    updates: &[ThreadFieldUpdate],
    cursor: &CheckpointCursor,
    params_json: &serde_json::Value,
    progress: &impl Fn(u64),
) -> Result<()> {
    let mut cursor = cursor.clone();
    cursor.params = params_json.clone();
    let cursor_json = serde_json::to_string(&cursor)?;
    matter.apply_thread_batch_with_checkpoint(
        job_id,
        THREAD_STAGE,
        updates,
        &cursor_json,
        cursor.completed_count as i64,
    )?;
    progress(cursor.completed_count);
    Ok(())
}

fn summary_from_cursor(cursor: &CheckpointCursor) -> ThreadSummary {
    ThreadSummary {
        completed_count: cursor.completed_count,
        thread_count: cursor.thread_count,
        header_linked: cursor.header_linked,
        subject_linked: cursor.subject_linked,
        index_linked: cursor.index_linked,
        singleton: cursor.singleton,
    }
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
