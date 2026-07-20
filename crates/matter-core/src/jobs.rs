//! Jobs and checkpoints for resumable work.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Job lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    Running,
    Paused,
    Failed,
    Cancelled,
    Succeeded,
}

impl JobState {
    /// Wire/DB representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Succeeded => "succeeded",
        }
    }

    /// Parse from DB/wire form.
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "succeeded" => Ok(Self::Succeeded),
            other => Err(Error::InvalidJobState(other.to_string())),
        }
    }

    /// Whether `self -> to` is an allowed transition.
    pub fn can_transition_to(self, to: Self) -> bool {
        use JobState::*;
        matches!(
            (self, to),
            (Pending, Running)
                | (Pending, Cancelled)
                | (Running, Paused)
                | (Running, Failed)
                | (Running, Cancelled)
                | (Running, Succeeded)
                | (Paused, Running)
                | (Paused, Cancelled)
                | (Paused, Failed)
                // Allow re-open from failed for retry workflows
                | (Failed, Pending)
                | (Failed, Running)
                | (Cancelled, Pending)
        )
    }
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A processing job row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub matter_id: String,
    pub kind: String,
    pub state: JobState,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Parent orchestration job (`workflow_run` / `profile_run`), if any.
    pub parent_job_id: Option<String>,
}

/// Opaque checkpoint cursor owned by the calling stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobCheckpoint {
    pub job_id: String,
    pub stage: String,
    pub cursor_json: String,
    pub completed_count: i64,
    pub updated_at: String,
}

const JOB_SELECT_COLS: &str = "id, matter_id, kind, state, started_at, finished_at, \
     error_summary, created_at, updated_at, parent_job_id";

fn map_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Job, String)> {
    let state_str: String = row.get(3)?;
    Ok((
        Job {
            id: row.get(0)?,
            matter_id: row.get(1)?,
            kind: row.get(2)?,
            // placeholder; set by caller after parse
            state: JobState::Pending,
            started_at: row.get(4)?,
            finished_at: row.get(5)?,
            error_summary: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            parent_job_id: row.get(9)?,
        },
        state_str,
    ))
}

fn finalize_job(mut job: Job, state_str: String) -> Result<Job> {
    job.state = JobState::parse(&state_str)?;
    Ok(job)
}

pub(crate) fn create_job(
    conn: &Connection,
    id: &str,
    matter_id: &str,
    kind: &str,
    now: &str,
    parent_job_id: Option<&str>,
) -> Result<Job> {
    conn.execute(
        "INSERT INTO jobs (id, matter_id, kind, state, started_at, finished_at, error_summary, \
         created_at, updated_at, parent_job_id) \
         VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, ?5, ?5, ?6)",
        params![
            id,
            matter_id,
            kind,
            JobState::Pending.as_str(),
            now,
            parent_job_id
        ],
    )?;
    get_job(conn, id)
}

pub(crate) fn get_job(conn: &Connection, job_id: &str) -> Result<Job> {
    conn.query_row(
        &format!("SELECT {JOB_SELECT_COLS} FROM jobs WHERE id = ?1"),
        params![job_id],
        map_job_row,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Error::JobNotFound(job_id.to_string()),
        other => Error::Sqlite(other),
    })
    .and_then(|(job, state_str)| finalize_job(job, state_str))
}

/// List all jobs for a matter, newest first by `created_at`.
pub(crate) fn list_jobs(conn: &Connection, matter_id: &str) -> Result<Vec<Job>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOB_SELECT_COLS} FROM jobs WHERE matter_id = ?1 ORDER BY created_at DESC"
    ))?;
    let rows = stmt.query_map(params![matter_id], map_job_row)?;
    let mut out = Vec::new();
    for row in rows {
        let (job, state_str) = row?;
        out.push(finalize_job(job, state_str)?);
    }
    Ok(out)
}

/// List direct children of a parent job (oldest first for stable orchestration order).
pub(crate) fn list_child_jobs(conn: &Connection, parent_job_id: &str) -> Result<Vec<Job>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {JOB_SELECT_COLS} FROM jobs WHERE parent_job_id = ?1 \
         ORDER BY created_at ASC, id ASC"
    ))?;
    let rows = stmt.query_map(params![parent_job_id], map_job_row)?;
    let mut out = Vec::new();
    for row in rows {
        let (job, state_str) = row?;
        out.push(finalize_job(job, state_str)?);
    }
    Ok(out)
}

pub(crate) fn set_job_state(
    conn: &Connection,
    job_id: &str,
    to: JobState,
    now: &str,
    error_summary: Option<&str>,
) -> Result<Job> {
    let job = get_job(conn, job_id)?;
    if job.state == to {
        return Ok(job);
    }
    if !job.state.can_transition_to(to) {
        return Err(Error::InvalidJobTransition {
            from: job.state.to_string(),
            to: to.to_string(),
        });
    }

    let started_at = match (job.state, to) {
        (JobState::Pending, JobState::Running) | (JobState::Failed, JobState::Running) => {
            Some(now.to_string())
        }
        _ => job.started_at.clone(),
    };

    let finished_at = match to {
        JobState::Succeeded | JobState::Failed | JobState::Cancelled => Some(now.to_string()),
        JobState::Running | JobState::Pending | JobState::Paused => None,
    };

    let summary = match to {
        JobState::Failed => error_summary
            .map(|s| s.to_string())
            .or(job.error_summary.clone()),
        JobState::Succeeded | JobState::Pending | JobState::Running => None,
        _ => job.error_summary.clone(),
    };

    conn.execute(
        "UPDATE jobs SET state = ?1, started_at = ?2, finished_at = ?3, error_summary = ?4, updated_at = ?5 \
         WHERE id = ?6",
        params![
            to.as_str(),
            started_at,
            finished_at,
            summary,
            now,
            job_id
        ],
    )?;
    get_job(conn, job_id)
}

/// Upsert the latest checkpoint for `(job_id, stage)`.
pub(crate) fn put_checkpoint(
    conn: &Connection,
    job_id: &str,
    stage: &str,
    cursor_json: &str,
    completed_count: i64,
    now: &str,
) -> Result<JobCheckpoint> {
    // Ensure job exists.
    let _ = get_job(conn, job_id)?;
    conn.execute(
        "INSERT INTO job_checkpoints (job_id, stage, cursor_json, completed_count, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(job_id, stage) DO UPDATE SET \
            cursor_json = excluded.cursor_json, \
            completed_count = excluded.completed_count, \
            updated_at = excluded.updated_at",
        params![job_id, stage, cursor_json, completed_count, now],
    )?;
    get_checkpoint(conn, job_id, stage)?
        .ok_or_else(|| Error::Other("checkpoint missing after upsert".into()))
}

pub(crate) fn get_checkpoint(
    conn: &Connection,
    job_id: &str,
    stage: &str,
) -> Result<Option<JobCheckpoint>> {
    conn.query_row(
        "SELECT job_id, stage, cursor_json, completed_count, updated_at \
         FROM job_checkpoints WHERE job_id = ?1 AND stage = ?2",
        params![job_id, stage],
        |row| {
            Ok(JobCheckpoint {
                job_id: row.get(0)?,
                stage: row.get(1)?,
                cursor_json: row.get(2)?,
                completed_count: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Error::from)
}
