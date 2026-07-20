//! `job run|resume|cancel|status|list` commands.

use matter_core::{JobState, Matter};
use serde_json::json;

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};
use crate::matter_cmd::{open_matter, open_matter_read, resolve_matter_root};
use crate::paths::load_params_json;
use crate::runner_util::{resume_job_wait, run_job_wait};

pub fn job_run(
    path: &std::path::Path,
    kind: &str,
    params_json: Option<&str>,
    json: bool,
) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let params = load_params_json(params_json)?;
    let params_str = serde_json::to_string(&params)?;
    let _job = run_job_wait(&root, kind, &params_str, json)?;
    Ok(())
}

pub fn job_resume(path: &std::path::Path, job_id: &str, json: bool) -> Result<()> {
    if job_id.trim().is_empty() {
        return Err(CliError::Usage("job-id must not be empty".into()));
    }
    let root = resolve_matter_root(path)?;
    let _job = resume_job_wait(&root, job_id, json)?;
    Ok(())
}

pub fn job_cancel(path: &std::path::Path, job_id: &str, json: bool) -> Result<()> {
    if job_id.trim().is_empty() {
        return Err(CliError::Usage("job-id must not be empty".into()));
    }
    let root = resolve_matter_root(path)?;
    let matter = open_matter(&root)?;
    let job = matter.get_job(job_id).map_err(CliError::from)?;
    match job.state {
        JobState::Succeeded | JobState::Cancelled | JobState::Failed => {
            if json {
                emit_json(
                    true,
                    &ok_envelope(json!({
                        "job_id": job.id,
                        "state": job.state.as_str(),
                        "message": "job already terminal; no cancel needed",
                    })),
                )?;
            } else {
                println!(
                    "job {} already terminal ({}); nothing to cancel",
                    job.id, job.state
                );
            }
            Ok(())
        }
        JobState::Pending | JobState::Running | JobState::Paused => {
            let updated = matter
                .set_job_state(job_id, JobState::Cancelled, Some("cancelled via CLI"))
                .map_err(CliError::from)?;
            if json {
                emit_json(
                    true,
                    &ok_envelope(json!({
                        "job_id": updated.id,
                        "state": updated.state.as_str(),
                        "kind": updated.kind,
                        "message": "cancelled",
                    })),
                )?;
            } else {
                println!("cancelled job {} ({})", updated.id, updated.kind);
            }
            Ok(())
        }
    }
}

pub fn job_status(path: &std::path::Path, job_id: &str, json: bool) -> Result<()> {
    if job_id.trim().is_empty() {
        return Err(CliError::Usage("job-id must not be empty".into()));
    }
    let root = resolve_matter_root(path)?;
    let matter = open_matter_read(&root)?;
    let job = matter.get_job(job_id).map_err(CliError::from)?;
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "job_id": job.id,
                "kind": job.kind,
                "state": job.state.as_str(),
                "message": job.error_summary,
                "parent_job_id": job.parent_job_id,
                "started_at": job.started_at,
                "finished_at": job.finished_at,
                "created_at": job.created_at,
                "updated_at": job.updated_at,
            })),
        )?;
    } else {
        println!("job {}", job.id);
        println!("  kind:    {}", job.kind);
        println!("  state:   {}", job.state);
        println!("  parent:  {:?}", job.parent_job_id);
        println!("  started: {:?}", job.started_at);
        println!("  finished:{:?}", job.finished_at);
        if let Some(ref m) = job.error_summary {
            println!("  error:   {m}");
        }
    }
    Ok(())
}

pub fn job_list(
    path: &std::path::Path,
    parent: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let matter = open_matter_read(&root)?;
    let mut jobs = if let Some(pid) = parent {
        matter.list_child_jobs(pid).map_err(CliError::from)?
    } else {
        matter.list_jobs().map_err(CliError::from)?
    };
    // Newest first for overview.
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    if limit > 0 && jobs.len() > limit {
        jobs.truncate(limit);
    }

    if json {
        let rows: Vec<_> = jobs
            .iter()
            .map(|j| {
                json!({
                    "job_id": j.id,
                    "kind": j.kind,
                    "state": j.state.as_str(),
                    "parent_job_id": j.parent_job_id,
                    "created_at": j.created_at,
                    "finished_at": j.finished_at,
                    "error_summary": j.error_summary,
                })
            })
            .collect();
        emit_json(
            true,
            &ok_envelope(json!({
                "jobs": rows,
                "count": rows.len(),
            })),
        )?;
    } else {
        if jobs.is_empty() {
            println!("(no jobs)");
            return Ok(());
        }
        for j in &jobs {
            println!(
                "{}  {:16}  {:10}  parent={:?}",
                j.id,
                j.kind,
                j.state.as_str(),
                j.parent_job_id
            );
        }
    }
    Ok(())
}

/// Mark job cancelled via durable state (used by tests without runner).
#[allow(dead_code)]
pub fn cancel_job_state(matter: &Matter, job_id: &str) -> Result<()> {
    matter
        .set_job_state(job_id, JobState::Cancelled, Some("cancelled via CLI"))
        .map_err(CliError::from)?;
    Ok(())
}
