//! CLI error type.

use std::path::PathBuf;

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
}

pub type Result<T> = std::result::Result<T, CliError>;
