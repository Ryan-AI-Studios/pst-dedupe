//! Typed errors for PST extract.

use thiserror::Error;

/// Result alias for extract-pst.
pub type Result<T> = std::result::Result<T, Error>;

/// Structured extract errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("matter error: {0}")]
    Matter(#[from] matter_core::Error),

    #[error("PST error: {0}")]
    Pst(#[from] pst_reader::PstError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("pst_open_failed: {0}")]
    PstOpenFailed(String),

    #[error("pst_ansi_rejected: {0}")]
    PstAnsiRejected(String),

    #[error("message_props_failed: {0}")]
    MessagePropsFailed(String),

    #[error("attach_data_missing: {0}")]
    AttachDataMissing(String),

    #[error("attach_too_large: size {size} exceeds cap {cap}")]
    AttachTooLarge { size: u64, cap: u64 },

    #[error("cas_put_failed: {0}")]
    CasPutFailed(String),

    #[error("cancelled")]
    Cancelled,

    #[error("inventory item not found: {0}")]
    InventoryItemNotFound(String),

    #[error("not a PST inventory item: {0}")]
    NotAPstItem(String),

    #[error("job not found or wrong kind: {0}")]
    InvalidJob(String),

    #[error("source not found: {0}")]
    SourceNotFound(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Stable machine code for item_errors / job summaries.
    pub fn code(&self) -> &'static str {
        match self {
            Self::PstOpenFailed(_) => "pst_open_failed",
            Self::PstAnsiRejected(_) => "pst_ansi_rejected",
            Self::MessagePropsFailed(_) => "message_props_failed",
            Self::AttachDataMissing(_) => "attach_data_missing",
            Self::AttachTooLarge { .. } => "attach_too_large",
            Self::CasPutFailed(_) => "cas_put_failed",
            Self::Cancelled => "cancelled",
            Self::Pst(pst_reader::PstError::AnsiPstNotSupported(_)) => "pst_ansi_rejected",
            Self::Pst(_) => "pst_error",
            Self::Matter(_) => "matter_error",
            Self::Io(_) => "io_error",
            Self::Json(_) => "json_error",
            Self::InventoryItemNotFound(_) => "inventory_item_not_found",
            Self::NotAPstItem(_) => "not_a_pst_item",
            Self::InvalidJob(_) => "invalid_job",
            Self::SourceNotFound(_) => "source_not_found",
            Self::Other(_) => "other",
        }
    }
}
