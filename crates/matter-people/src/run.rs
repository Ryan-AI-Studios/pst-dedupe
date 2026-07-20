//! Two-pass `people_graph` job: resumable Pass 1 + atomic Pass 2.

use std::time::Instant;

use matter_core::{people_graph_pass, sha256_hex, AuditEventInput, Matter};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{PeopleError, Result};
use crate::params::PeopleGraphParams;
use crate::pass1::process_pass1_item;
use crate::pass2::run_pass2;

/// Job kind string for process-runner.
pub const JOB_KIND_PEOPLE_GRAPH: &str = "people_graph";
/// Checkpoint stage name (Pass 1 cursor).
pub const PEOPLE_GRAPH_STAGE: &str = "people_graph";
/// Engine version token embedded in rebuild fingerprint.
pub const PEOPLE_GRAPH_ENGINE_VERSION: &str = "people_graph_v1";

/// Summary after build (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeopleGraphSummary {
    pub completed_count: u64,
    pub items_processed: u64,
    pub participants_written: u64,
    pub overflow_count: u64,
    pub people_count: u64,
    pub edge_count: u64,
    pub pass1_done: bool,
    pub pass2_done: bool,
}

/// Full success payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeopleGraphReport {
    pub items_processed: u64,
    pub participants_written: u64,
    pub overflow_count: u64,
    pub people_count: u64,
    pub edge_count: u64,
    pub fingerprint: String,
    pub built_at: String,
}

/// Outcome of [`run_people_graph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeopleGraphOutcome {
    Succeeded(PeopleGraphReport),
    Paused(PeopleGraphSummary),
    Failed {
        message: String,
        summary: PeopleGraphSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    items_processed: u64,
    participants_written: u64,
    overflow_count: u64,
    #[serde(default)]
    reset_done: bool,
    #[serde(default)]
    pass1_done: bool,
    params: serde_json::Value,
}

/// Build fingerprint: hash of engine version + params JSON.
///
/// Soft-stale note: when `reset:false` and this fingerprint matches a complete
/// graph, Pass 1/2 are skipped. Inventory changes (new/edited items) are **not**
/// part of the fingerprint — residual; prefer `reset:true` after inventory churn.
pub fn people_graph_fingerprint(params: &PeopleGraphParams) -> Result<String> {
    let params_json = serde_json::to_string(params)
        .map_err(|e| PeopleError::other(format!("serialize params: {e}")))?;
    let pre = format!("{PEOPLE_GRAPH_ENGINE_VERSION}|{params_json}");
    Ok(sha256_hex(pre.as_bytes()))
}

/// Run two-pass people graph build for the runner-created `job_id`.
///
/// Does **not** call `create_job`. Honors `cancel` between Pass-1 items.
/// `built_at` is set only after Pass 2 completes.
pub fn run_people_graph(
    matter: &Matter,
    job_id: &str,
    params: &PeopleGraphParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<PeopleGraphOutcome> {
    let started = Instant::now();
    let result = run_body(matter, job_id, params, cancel, &progress);

    match &result {
        Ok(PeopleGraphOutcome::Succeeded(r)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "people_graph.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "items_processed": r.items_processed,
                    "participants_written": r.participants_written,
                    "overflow_count": r.overflow_count,
                    "people_count": r.people_count,
                    "edge_count": r.edge_count,
                    "fingerprint": r.fingerprint,
                    "built_at": r.built_at,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                let message = format!("audit complete failed: {e}");
                let summary = summary_from_report(r);
                let _ = matter.append_audit(AuditEventInput {
                    actor: "system".into(),
                    action: "people_graph.fail".into(),
                    entity: format!("job:{job_id}"),
                    params_json: fail_audit_params(&message, &summary).to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                });
                return Ok(PeopleGraphOutcome::Failed { message, summary });
            }
        }
        Ok(PeopleGraphOutcome::Paused(_)) => {}
        Ok(PeopleGraphOutcome::Failed { message, summary }) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "people_graph.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(message, summary).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
        Err(e) => {
            let empty = PeopleGraphSummary::default();
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "people_graph.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(&e.to_string(), &empty).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
    }

    result
}

fn summary_from_report(r: &PeopleGraphReport) -> PeopleGraphSummary {
    PeopleGraphSummary {
        completed_count: r.items_processed,
        items_processed: r.items_processed,
        participants_written: r.participants_written,
        overflow_count: r.overflow_count,
        people_count: r.people_count,
        edge_count: r.edge_count,
        pass1_done: true,
        pass2_done: true,
    }
}

fn fail_audit_params(message: &str, summary: &PeopleGraphSummary) -> serde_json::Value {
    json!({
        "error": message,
        "items_processed": summary.items_processed,
        "participants_written": summary.participants_written,
        "people_count": summary.people_count,
        "edge_count": summary.edge_count,
    })
}

fn run_body(
    matter: &Matter,
    job_id: &str,
    params: &PeopleGraphParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<PeopleGraphOutcome> {
    params.validate()?;

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate()?;
    let params_json = serde_json::to_value(&effective)
        .map_err(|e| PeopleError::other(format!("serialize people_graph params: {e}")))?;

    let resuming = prior
        .as_ref()
        .is_some_and(|p| p.completed_count > 0 || p.pass1_done);
    let fingerprint = people_graph_fingerprint(&effective)?;
    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "people_graph.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "resume": resuming,
            "reset": effective.reset,
            "fingerprint": fingerprint,
            "engine_version": PEOPLE_GRAPH_ENGINE_VERSION,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    run_inner(
        matter,
        job_id,
        &effective,
        cancel,
        progress,
        &params_json,
        prior,
    )
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, PEOPLE_GRAPH_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(PeopleError::other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &PeopleGraphParams,
    prior: Option<&CheckpointCursor>,
) -> Result<PeopleGraphParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<PeopleGraphParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(PeopleError::other(format!(
                        "checkpoint params unreadable: {e}"
                    )));
                }
            }
        }
    }
    Ok(call_site.clone())
}

fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &PeopleGraphParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<PeopleGraphOutcome> {
    let mut summary = PeopleGraphSummary::default();
    let mut cursor_index = 0u64;
    let mut last_item_id: Option<String> = None;
    let mut reset_done = false;
    let mut pass1_done = false;

    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        last_item_id = p.last_item_id;
        summary.completed_count = p.completed_count;
        summary.items_processed = p.items_processed;
        summary.participants_written = p.participants_written;
        summary.overflow_count = p.overflow_count;
        reset_done = p.reset_done;
        pass1_done = p.pass1_done;
        summary.pass1_done = pass1_done;
    }

    let fail = |summary: PeopleGraphSummary, e: PeopleError| -> Result<PeopleGraphOutcome> {
        Ok(PeopleGraphOutcome::Failed {
            message: e.to_string(),
            summary,
        })
    };

    // reset:true → clear tables once (not again on resume).
    if params.reset && !reset_done {
        if let Err(e) = matter.clear_people_graph_tables() {
            return fail(summary, e.into());
        }
        reset_done = true;
        if let Err(e) = matter.set_people_graph_pass(Some(people_graph_pass::PASS1), Some(job_id)) {
            return fail(summary, e.into());
        }
        if let Err(e) = write_checkpoint(
            matter,
            job_id,
            &CkArgs {
                cursor_index,
                summary: &summary,
                params_json,
                last_item_id: last_item_id.as_deref(),
                reset_done,
                pass1_done,
            },
        ) {
            return fail(summary, e);
        }
    } else if !pass1_done {
        if let Err(e) = matter.set_people_graph_pass(Some(people_graph_pass::PASS1), Some(job_id)) {
            return fail(summary, e.into());
        }
    }

    // Soft skip when fingerprint fresh and not reset (P0: still full rebuild if stale).
    if !params.reset && !pass1_done {
        if let Ok(st) = matter.people_graph_status() {
            if st.is_complete {
                if let Ok(fp) = people_graph_fingerprint(params) {
                    if st.fingerprint.as_deref() == Some(fp.as_str()) {
                        return Ok(PeopleGraphOutcome::Succeeded(PeopleGraphReport {
                            items_processed: 0,
                            participants_written: 0,
                            overflow_count: 0,
                            people_count: st.people_count as u64,
                            edge_count: st.edge_count as u64,
                            fingerprint: fp,
                            built_at: st.built_at.unwrap_or_default(),
                        }));
                    }
                }
            }
        }
        // Stale/incomplete without reset: full rebuild (clear then pass1).
        if let Err(e) = matter.clear_people_graph_tables() {
            return fail(summary, e.into());
        }
        reset_done = true;
        if let Err(e) = matter.set_people_graph_pass(Some(people_graph_pass::PASS1), Some(job_id)) {
            return fail(summary, e.into());
        }
    }

    // ---- Pass 1 ----
    if !pass1_done {
        let batch = params.batch_size.max(1) as u64;
        loop {
            if cancel.map(|c| c()).unwrap_or(false) {
                if let Err(e) = write_checkpoint(
                    matter,
                    job_id,
                    &CkArgs {
                        cursor_index,
                        summary: &summary,
                        params_json,
                        last_item_id: last_item_id.as_deref(),
                        reset_done,
                        pass1_done,
                    },
                ) {
                    return fail(summary, e);
                }
                progress(summary.completed_count);
                return Ok(PeopleGraphOutcome::Paused(summary));
            }

            let candidates =
                match matter.list_people_pass1_candidates(last_item_id.as_deref(), batch) {
                    Ok(c) => c,
                    Err(e) => return fail(summary, e.into()),
                };
            if candidates.is_empty() {
                break;
            }

            for cand in candidates {
                if cancel.map(|c| c()).unwrap_or(false) {
                    if let Err(e) = write_checkpoint(
                        matter,
                        job_id,
                        &CkArgs {
                            cursor_index,
                            summary: &summary,
                            params_json,
                            last_item_id: last_item_id.as_deref(),
                            reset_done,
                            pass1_done,
                        },
                    ) {
                        return fail(summary, e);
                    }
                    progress(summary.completed_count);
                    return Ok(PeopleGraphOutcome::Paused(summary));
                }

                match process_pass1_item(matter, &cand, params) {
                    Ok((written, overflow)) => {
                        summary.participants_written =
                            summary.participants_written.saturating_add(written);
                        summary.overflow_count = summary.overflow_count.saturating_add(overflow);
                        summary.items_processed += 1;
                        summary.completed_count += 1;
                    }
                    Err(e) => return fail(summary, e),
                }
                cursor_index += 1;
                last_item_id = Some(cand.id.clone());
                progress(summary.completed_count);
                if let Err(e) = write_checkpoint(
                    matter,
                    job_id,
                    &CkArgs {
                        cursor_index,
                        summary: &summary,
                        params_json,
                        last_item_id: last_item_id.as_deref(),
                        reset_done,
                        pass1_done,
                    },
                ) {
                    return fail(summary, e);
                }
            }
        }
        pass1_done = true;
        summary.pass1_done = true;
        if let Err(e) = write_checkpoint(
            matter,
            job_id,
            &CkArgs {
                cursor_index,
                summary: &summary,
                params_json,
                last_item_id: last_item_id.as_deref(),
                reset_done,
                pass1_done,
            },
        ) {
            return fail(summary, e);
        }
    }

    // Cancel after Pass 1 but before Pass 2 → still pause (Pass 2 not started).
    if cancel.map(|c| c()).unwrap_or(false) {
        progress(summary.completed_count);
        return Ok(PeopleGraphOutcome::Paused(summary));
    }

    // ---- Pass 2 (atomic; re-run from scratch if interrupted) ----
    if let Err(e) = run_pass2(matter, params) {
        return fail(summary, e);
    }

    let fingerprint = match people_graph_fingerprint(params) {
        Ok(f) => f,
        Err(e) => return fail(summary, e),
    };
    let built_at = match matter.set_people_graph_complete(&fingerprint, Some(job_id)) {
        Ok(t) => t,
        Err(e) => return fail(summary, e.into()),
    };

    let st = match matter.people_graph_status() {
        Ok(s) => s,
        Err(e) => return fail(summary, e.into()),
    };
    summary.people_count = st.people_count as u64;
    summary.edge_count = st.edge_count as u64;
    summary.pass2_done = true;

    Ok(PeopleGraphOutcome::Succeeded(PeopleGraphReport {
        items_processed: summary.items_processed,
        participants_written: summary.participants_written,
        overflow_count: summary.overflow_count,
        people_count: summary.people_count,
        edge_count: summary.edge_count,
        fingerprint,
        built_at,
    }))
}

struct CkArgs<'a> {
    cursor_index: u64,
    summary: &'a PeopleGraphSummary,
    params_json: &'a serde_json::Value,
    last_item_id: Option<&'a str>,
    reset_done: bool,
    pass1_done: bool,
}

fn write_checkpoint(matter: &Matter, job_id: &str, args: &CkArgs<'_>) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index: args.cursor_index,
        last_item_id: args.last_item_id.map(|s| s.to_string()),
        completed_count: args.summary.completed_count,
        items_processed: args.summary.items_processed,
        participants_written: args.summary.participants_written,
        overflow_count: args.summary.overflow_count,
        reset_done: args.reset_done,
        pass1_done: args.pass1_done,
        params: args.params_json.clone(),
    };
    let json = serde_json::to_string(&cursor).map_err(|e| PeopleError::other(e.to_string()))?;
    matter.put_checkpoint(
        job_id,
        PEOPLE_GRAPH_STAGE,
        &json,
        args.summary.completed_count as i64,
    )?;
    Ok(())
}
