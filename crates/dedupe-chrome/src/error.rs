//! Structured command errors for the chrome host.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    pub kind: String,
    pub message: String,
}

impl CommandError {
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            kind: "not_found".into(),
            message: message.into(),
        }
    }

    pub fn encrypted(message: impl Into<String>) -> Self {
        Self {
            kind: "encrypted".into(),
            message: message.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            kind: "failed".into(),
            message: message.into(),
        }
    }

    pub fn fts_unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: "fts_unavailable".into(),
            message: message.into(),
        }
    }

    pub fn busy(job_id: impl Into<String>) -> Self {
        let job_id = job_id.into();
        Self {
            kind: "busy".into(),
            message: format!("matter is busy: a job is already running ({job_id})"),
        }
    }
}

pub(crate) fn map_runner(err: process_runner::RunnerError) -> CommandError {
    match err {
        process_runner::RunnerError::Busy { job_id } => CommandError::busy(job_id),
        other => CommandError::failed(other.to_string()),
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for CommandError {}

pub(crate) fn map_core(err: matter_core::Error) -> CommandError {
    match err {
        matter_core::Error::ItemNotFound(id) => {
            CommandError::not_found(format!("item not found: {id}"))
        }
        other => CommandError::failed(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_display_starts_with_busy_colon() {
        let e = CommandError::busy("job_x");
        assert_eq!(e.kind, "busy");
        assert!(
            format!("{e}").starts_with("busy:"),
            "tauri_invoke String must match ui is_busy_invoke_err: got {e}"
        );
    }
}
