//! [`JobBackend`] abstraction for local process-runner and future remote workers.
//!
//! # Remote worker physics (LOCKED — track 0061 §3.6.1)
//!
//! Matter **SQLite is host-local only**. A Kubernetes or remote worker **must not**
//! open `matter.db` over the network (NFS, SMB, or remote rusqlite).
//!
//! | Path | Allowed |
//! |---|---|
//! | Local | [`LocalProcessRunnerBackend`] / [`ProcessRunner`] on the same host as SQLite |
//! | Remote (residual) | **HTTP client to matter-service only** — claim/complete jobs, write items, CAS via service or BlobStore creds as designed |
//!
//! Trait methods are transport-agnostic so a future HTTP residual can implement
//! the same surface without SQL. Do **not** add a "remote rusqlite" backend.

use camino::Utf8Path;
use matter_core::{Job, JobState, Matter};

use crate::error::{Result, RunnerError};
use crate::handler::JobParams;
use crate::runner::ProcessRunner;

/// A job claimed by a worker (local or remote residual).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedJob {
    pub job_id: String,
    pub kind: String,
    pub matter_id: String,
    pub params_json: String,
}

/// Abstract job queue / worker control plane.
///
/// **Local P0:** [`LocalProcessRunnerBackend`] adapts today's
/// [`ProcessRunner`] (inline single worker; `claim` may no-op).
///
/// **Remote residual:** implementors **must** use HTTP to matter-service —
/// never remote SQL against the matter DB.
pub trait JobBackend: Send + Sync {
    /// Enqueue work; returns durable job id.
    fn enqueue(&self, matter_root: &Utf8Path, kind: &str, params: JobParams) -> Result<String>;

    /// Claim next available job for `worker_id`, if any.
    ///
    /// Local backends may return `None` and run jobs inline via `enqueue`/`start`.
    fn claim(&self, worker_id: &str) -> Result<Option<ClaimedJob>>;

    /// Heartbeat / lease renewal while running.
    fn heartbeat(&self, job_id: &str, worker_id: &str) -> Result<()>;

    /// Mark job succeeded.
    fn complete(&self, job_id: &str, message: Option<&str>) -> Result<()>;

    /// Mark job failed with message.
    fn fail(&self, job_id: &str, message: &str) -> Result<()>;

    /// Best-effort cancel.
    fn cancel(&self, job_id: &str) -> Result<()>;
}

/// Local adapter: documents ProcessRunner as the production local path.
///
/// `enqueue` creates a pending job row via `Matter` when a root is provided
/// through the runner; full orchestration remains on [`ProcessRunner::start`].
///
/// Remote workers must **not** clone this pattern over NFS SQLite — use HTTP.
pub struct LocalProcessRunnerBackend {
    runner: ProcessRunner,
}

impl LocalProcessRunnerBackend {
    pub fn new(runner: ProcessRunner) -> Self {
        Self { runner }
    }

    /// Access the underlying process runner (production local path).
    pub fn runner(&self) -> &ProcessRunner {
        &self.runner
    }

    /// Mutable access for register/start.
    pub fn runner_mut(&mut self) -> &mut ProcessRunner {
        &mut self.runner
    }
}

impl JobBackend for LocalProcessRunnerBackend {
    fn enqueue(&self, matter_root: &Utf8Path, kind: &str, params: JobParams) -> Result<String> {
        // Local path: start runs on the single worker (creates job row).
        self.runner.start(matter_root, kind, params)
    }

    fn claim(&self, _worker_id: &str) -> Result<Option<ClaimedJob>> {
        // Local single-worker: jobs are accepted via start/enqueue, not pulled.
        Ok(None)
    }

    fn heartbeat(&self, _job_id: &str, _worker_id: &str) -> Result<()> {
        // Local: progress is via watch channel; no lease table in P0.
        Ok(())
    }

    fn complete(&self, job_id: &str, message: Option<&str>) -> Result<()> {
        // Local path: ProcessRunner sets terminal state from handler outcomes.
        // External complete (HTTP residual workers) must use
        // [`set_job_terminal_local`] on the **host-local** Matter via matter-service —
        // never a silent no-op success here.
        let _ = message;
        Err(RunnerError::InvalidJob(format!(
            "LocalProcessRunnerBackend::complete({job_id}): local path uses ProcessRunner \
             handler outcomes; external complete requires set_job_terminal_local on the \
             host-local Matter (HTTP residual via matter-service)"
        )))
    }

    fn fail(&self, job_id: &str, message: &str) -> Result<()> {
        let _ = message;
        Err(RunnerError::InvalidJob(format!(
            "LocalProcessRunnerBackend::fail({job_id}): local path uses ProcessRunner \
             handler outcomes; external fail requires set_job_terminal_local on the \
             host-local Matter (HTTP residual via matter-service)"
        )))
    }

    fn cancel(&self, job_id: &str) -> Result<()> {
        self.runner.cancel(job_id)
    }
}

/// Helper: mark a job terminal on a host-local matter (service host only).
///
/// Remote workers call this **via matter-service HTTP**, never by opening this
/// path over the network themselves.
pub fn set_job_terminal_local(
    matter: &Matter,
    job_id: &str,
    state: JobState,
    error_summary: Option<&str>,
) -> Result<Job> {
    matter
        .set_job_state(job_id, state, error_summary)
        .map_err(RunnerError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RunnerConfig;

    #[test]
    fn local_claim_is_none() {
        let runner = ProcessRunner::new(RunnerConfig::default());
        let backend = LocalProcessRunnerBackend::new(runner);
        assert!(backend.claim("w1").expect("claim").is_none());
        assert!(backend.heartbeat("j1", "w1").is_ok());
    }

    #[test]
    fn complete_and_fail_return_error_not_silent_ok() {
        let runner = ProcessRunner::new(RunnerConfig::default());
        let backend = LocalProcessRunnerBackend::new(runner);
        let err = backend
            .complete("job-1", Some("done"))
            .expect_err("complete must not silent-ok");
        match err {
            RunnerError::InvalidJob(m) => {
                assert!(m.contains("set_job_terminal_local") || m.contains("ProcessRunner"));
            }
            e => panic!("unexpected: {e}"),
        }
        let err = backend
            .fail("job-1", "boom")
            .expect_err("fail must not silent-ok");
        match err {
            RunnerError::InvalidJob(m) => {
                assert!(m.contains("set_job_terminal_local") || m.contains("ProcessRunner"));
            }
            e => panic!("unexpected: {e}"),
        }
    }
}
