//! CLI error type and stable exit codes (track 0045 §3.4).

use std::path::PathBuf;
use std::process::ExitCode;

/// Stable process exit codes for automation scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CliExit {
    /// Success.
    Success = 0,
    /// Generic / unexpected error.
    Generic = 1,
    /// Usage / validation (bad args, bad JSON, unknown kind, relative path in params).
    Usage = 2,
    /// Matter busy / runner Busy.
    Busy = 3,
    /// Job finished failed or cancelled.
    JobFailed = 4,
    /// Matter open/create/IO error.
    MatterIo = 5,
}

impl From<CliExit> for ExitCode {
    fn from(c: CliExit) -> Self {
        ExitCode::from(c as u8)
    }
}

/// Errors surfaced by the CLI.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("not a .pst file: {0}")]
    NotPst(PathBuf),

    #[error("PST open failed ({path}): {source}")]
    PstOpen {
        path: PathBuf,
        source: pst_reader::PstError,
    },

    #[error("folder traversal failed ({path}): {source}")]
    Folders {
        path: PathBuf,
        source: pst_reader::PstError,
    },

    #[error("CSV report write failed ({path}): {source}")]
    CsvWrite {
        path: PathBuf,
        source: Box<dyn std::error::Error>,
    },

    #[error("JSON output failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Msg(String),

    /// Usage / validation failure (exit 2).
    #[error("{0}")]
    Usage(String),

    /// Runner busy (exit 3).
    #[error("{0}")]
    Busy(String),

    /// Job finished failed or cancelled (exit 4).
    #[error("{message}")]
    JobFailed {
        message: String,
        job_id: Option<String>,
        state: Option<String>,
    },

    /// Matter open/create/IO (exit 5).
    #[error("{0}")]
    MatterIo(String),

    /// Error envelope (or human text) already written; main must not re-emit.
    #[error("{message}")]
    AlreadyEmitted { message: String, exit: CliExit },
}

impl CliError {
    /// Map to a stable exit code.
    pub fn exit_code(&self) -> CliExit {
        match self {
            Self::Usage(_) | Self::PathNotFound(_) | Self::NotPst(_) => CliExit::Usage,
            Self::Busy(_) => CliExit::Busy,
            Self::JobFailed { .. } => CliExit::JobFailed,
            Self::MatterIo(_) => CliExit::MatterIo,
            Self::AlreadyEmitted { exit, .. } => *exit,
            Self::PstOpen { .. }
            | Self::Folders { .. }
            | Self::CsvWrite { .. }
            | Self::Json(_)
            | Self::Io(_)
            | Self::Msg(_) => CliExit::Generic,
        }
    }

    /// Machine-readable error code string for JSON envelopes.
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Usage(_) | Self::PathNotFound(_) | Self::NotPst(_) => "usage",
            Self::Busy(_) => "busy",
            Self::JobFailed { .. } => "job_failed",
            Self::MatterIo(_) => "matter_io",
            Self::Json(_) => "json",
            Self::Io(_) => "io",
            Self::AlreadyEmitted { .. } => "error",
            _ => "error",
        }
    }

    /// Whether stdout/stderr already carries the operator-facing payload.
    pub fn already_emitted(&self) -> bool {
        matches!(self, Self::AlreadyEmitted { .. } | Self::JobFailed { .. })
    }
}

impl From<matter_core::Error> for CliError {
    fn from(e: matter_core::Error) -> Self {
        use matter_core::Error;
        match &e {
            // Validation / not-found style → usage exit 2 (import validation, bad ids).
            Error::Other(_)
            | Error::JobNotFound(_)
            | Error::ItemNotFound(_)
            | Error::SourceNotFound(_)
            | Error::FamilyNotFound(_)
            | Error::ParentItemNotFound(_)
            | Error::InvalidJobState(_)
            | Error::InvalidJobTransition { .. }
            | Error::InvalidDigest(_)
            | Error::Json(_) => CliError::Usage(e.to_string()),
            // Open/create/layout/IO/schema → matter IO exit 5.
            Error::MatterNotFound(_)
            | Error::MatterAlreadyExists(_)
            | Error::DatabaseMissing(_)
            | Error::SchemaVersionMismatch { .. }
            | Error::UnknownSchemaVersion(_)
            | Error::MatterRowMissing
            | Error::Io(_)
            | Error::Sqlite(_)
            | Error::CasCollision { .. }
            | Error::BlobNotFound(_)
            | Error::AuditChainBroken { .. }
            | Error::CrossMatterFamily(_)
            | Error::FamilyCohesion(_)
            | Error::PassphraseRequired(_)
            | Error::WrongPassphrase
            | Error::Crypto(_)
            | Error::CryptoHeaderMissing(_) => CliError::MatterIo(e.to_string()),
        }
    }
}

impl From<process_runner::RunnerError> for CliError {
    fn from(e: process_runner::RunnerError) -> Self {
        use process_runner::RunnerError;
        match e {
            RunnerError::Busy { job_id } => {
                CliError::Busy(format!("matter is busy: job {job_id} is already running"))
            }
            RunnerError::UnknownKind(k) => CliError::Usage(format!("unknown job kind: {k}")),
            RunnerError::InvalidParams(m) | RunnerError::InvalidJob(m) => CliError::Usage(m),
            RunnerError::MatterOpen { path, message } => {
                CliError::MatterIo(format!("failed to open matter at {path}: {message}"))
            }
            RunnerError::JobNotFound(id) => CliError::Usage(format!("job not found: {id}")),
            // Matter errors during create_job / set_job_state → same mapping as MatterIo/Usage.
            RunnerError::Matter(me) => CliError::from(me),
            other => CliError::Msg(other.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, CliError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_matter_db_maps_to_exit_5() {
        let e = process_runner::RunnerError::Matter(matter_core::Error::DatabaseMissing(
            "broken".into(),
        ));
        let cli = CliError::from(e);
        assert_eq!(cli.exit_code(), CliExit::MatterIo);
    }

    #[test]
    fn runner_matter_open_maps_to_exit_5() {
        let e = process_runner::RunnerError::MatterOpen {
            path: "x".into(),
            message: "lock".into(),
        };
        let cli = CliError::from(e);
        assert_eq!(cli.exit_code(), CliExit::MatterIo);
    }

    #[test]
    fn runner_invalid_params_maps_to_exit_2() {
        let e = process_runner::RunnerError::InvalidParams("bad".into());
        let cli = CliError::from(e);
        assert_eq!(cli.exit_code(), CliExit::Usage);
    }
}
