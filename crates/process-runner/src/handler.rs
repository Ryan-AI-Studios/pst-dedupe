//! Pluggable job handler trait and context.

use matter_core::Matter;

use crate::cancel::CancelToken;
use crate::error::RunnerError;
use crate::progress::ProgressSink;

/// Outcome of a handler run (maps to durable job state when still Running).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    /// Work finished successfully.
    Succeeded {
        message: Option<String>,
        completed_count: u64,
    },
    /// Cooperative cancel or intentional pause — job should be **Paused**.
    Paused {
        message: Option<String>,
        completed_count: u64,
    },
    /// Hard failure — job should be **Failed**.
    Failed { message: String },
}

/// Context passed to [`JobHandler::run`] on the matter worker thread.
pub struct JobContext<'a> {
    /// Matter handle owned exclusively by the worker thread for this job.
    pub matter: &'a Matter,
    /// Runner-created job id — handlers must not create another job.
    pub job_id: &'a str,
    /// Optional source id (resume / extract).
    pub source_id: Option<&'a str>,
    /// Opaque JSON params for the handler kind.
    pub params_json: &'a str,
    pub cancel: &'a CancelToken,
    pub progress: ProgressSink,
    pub is_resume: bool,
}

/// Pluggable stage handler registered with the runner.
///
/// Implementations **must** honor cancel and may block. They receive a
/// pre-created `job_id` and must not call `create_job`.
pub trait JobHandler: Send + Sync {
    /// Stable kind string (e.g. `"ingest"`, `"extract_pst"`).
    fn kind(&self) -> &'static str;

    /// Run or resume work on the matter worker thread.
    fn run(&self, ctx: &JobContext<'_>) -> Result<JobOutcome, RunnerError>;
}

/// JSON params for start (opaque string for the handler).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobParams {
    pub json: String,
}

impl JobParams {
    pub fn new(json: impl Into<String>) -> Self {
        Self { json: json.into() }
    }

    pub fn empty() -> Self {
        Self { json: "{}".into() }
    }
}

impl From<String> for JobParams {
    fn from(json: String) -> Self {
        Self { json }
    }
}

impl From<&str> for JobParams {
    fn from(json: &str) -> Self {
        Self {
            json: json.to_string(),
        }
    }
}
