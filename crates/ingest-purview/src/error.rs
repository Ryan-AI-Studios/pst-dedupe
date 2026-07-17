//! Typed errors for package detect / safe ZIP expand / ingest.

use thiserror::Error;

/// Result alias for ingest-purview operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by detect, expand, and ingest APIs.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("matter store error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("path rejected ({code}): {message}")]
    PathRejected { code: &'static str, message: String },

    #[error("zip bomb limit ({code}): {message}")]
    ZipBomb { code: &'static str, message: String },

    #[error("ZIP nest depth exceeded (max {max})")]
    ZipDepth { max: u32 },

    #[error("unsupported container: {0}")]
    UnsupportedContainer(String),

    #[error("package path not found or unreadable: {0}")]
    PackageNotFound(String),

    #[error("unsupported package kind for ingest: {0}")]
    UnsupportedPackage(String),

    #[error("ingest cancelled by caller")]
    Cancelled,

    #[error("source not found: {0}")]
    SourceNotFound(String),

    #[error("job not found: {0}")]
    JobNotFound(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Stable error code for audit / item_errors when applicable.
    pub fn code(&self) -> &'static str {
        match self {
            Self::PathRejected { code, .. } => code,
            Self::ZipBomb { code, .. } => code,
            Self::ZipDepth { .. } => "zip_depth",
            Self::UnsupportedContainer(_) => "unsupported_7z",
            Self::Cancelled => "cancelled",
            Self::Zip(_) => "zip_corrupt",
            Self::Io(_) => "io_error",
            Self::PackageNotFound(_) => "package_not_found",
            Self::UnsupportedPackage(_) => "unsupported_package",
            Self::SourceNotFound(_) => "source_not_found",
            Self::JobNotFound(_) => "job_not_found",
            Self::Json(_) => "json_error",
            Self::Matter(_) => "matter_error",
            Self::Other(_) => "other",
        }
    }

    /// Whether this error is a per-entry soft failure (record + continue).
    ///
    /// Zip-bomb limits (`ZipBomb`, including ratio/size/entries) are **not**
    /// entry-level: they fail the whole job (fail closed).
    pub fn is_entry_level(&self) -> bool {
        matches!(
            self,
            Self::PathRejected { .. } | Self::UnsupportedContainer(_)
        )
    }
}

/// Stable codes for item_errors / audit.
pub mod codes {
    pub const ZIP_PATH_TRAVERSAL: &str = "zip_path_traversal";
    pub const ZIP_ABSOLUTE_PATH: &str = "zip_absolute_path";
    pub const ZIP_EMPTY_PATH: &str = "zip_empty_path";
    pub const ZIP_UNSAFE_PATH: &str = "zip_unsafe_path";
    pub const ZIP_BOMB_SIZE: &str = "zip_bomb_size";
    pub const ZIP_BOMB_RATIO: &str = "zip_bomb_ratio";
    pub const ZIP_BOMB_ENTRIES: &str = "zip_bomb_entries";
    pub const ZIP_DEPTH: &str = "zip_depth";
    pub const ZIP_CORRUPT: &str = "zip_corrupt";
    pub const UNSUPPORTED_7Z: &str = "unsupported_7z";
    pub const CANCELLED: &str = "cancelled";
    pub const IO_ERROR: &str = "io_error";
}
