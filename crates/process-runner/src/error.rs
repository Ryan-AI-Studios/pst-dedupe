//! Typed errors for the in-process job runner.

use thiserror::Error;

/// Result alias for process-runner operations.
pub type Result<T> = std::result::Result<T, RunnerError>;

/// Errors returned by [`crate::ProcessRunner`] and handlers.
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("matter is busy: a job is already running ({job_id})")]
    Busy { job_id: String },

    #[error("unknown job kind: {0}")]
    UnknownKind(String),

    #[error("handler failed: {0}")]
    HandlerFailed(String),

    #[error("failed to open matter at {path}: {message}")]
    MatterOpen { path: String, message: String },

    #[error("job not found: {0}")]
    JobNotFound(String),

    #[error("invalid job: {0}")]
    InvalidJob(String),

    #[error("invalid params: {0}")]
    InvalidParams(String),

    #[error("cancel failed: {0}")]
    CancelFailed(String),

    #[error("runner is shut down")]
    ShutDown,

    #[error("worker channel closed")]
    WorkerGone,

    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("{0}")]
    Other(String),
}
