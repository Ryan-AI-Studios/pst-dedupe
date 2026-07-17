//! Extract limits and summary types.

/// Limits for a PST extract run.
#[derive(Debug, Clone)]
pub struct ExtractLimits {
    /// Commit + checkpoint every N messages (mid-folder). Default 500.
    pub batch_size: u64,
    /// Optional safety cap on messages processed **this run**.
    ///
    /// When the cap is hit before the folder walk finishes, the job is
    /// **Paused** (resumable) with `ExtractSummary.completed = false` — never
    /// `Succeeded`. Raise the cap or call [`crate::resume_extract`] to continue.
    pub max_messages: Option<u64>,
    /// Fail closed when a single attachment exceeds this size.
    pub max_attachment_bytes: Option<u64>,
    /// Below this size, `put_bytes` is allowed; above → stream via `put_reader`.
    pub max_in_memory_put_bytes: u64,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            batch_size: 500,
            max_messages: None,
            max_attachment_bytes: None,
            max_in_memory_put_bytes: 16 * 1024 * 1024,
        }
    }
}

impl ExtractLimits {
    /// Test-friendly limits: batch_size=1, small in-memory threshold.
    pub fn for_tests() -> Self {
        Self {
            batch_size: 1,
            max_messages: None,
            max_attachment_bytes: None,
            max_in_memory_put_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Summary returned by extract / resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractSummary {
    pub source_id: String,
    pub job_id: String,
    pub messages_ok: u64,
    pub messages_err: u64,
    pub attachments_ok: u64,
    pub attachments_err: u64,
    /// True only when the folder walk finished fully (job `Succeeded`).
    /// False on cancel or when `max_messages` stopped the run mid-PST.
    pub completed: bool,
    /// True when cancelled mid-run (job Paused; resume-capable).
    /// Distinct from a `max_messages` pause (`cancelled == false`, `completed == false`).
    pub cancelled: bool,
}

/// Job kind string for PST extract.
pub const JOB_KIND_EXTRACT_PST: &str = "extract_pst";

/// Checkpoint stage name.
pub const STAGE_PST_EXTRACT: &str = "pst_extract";
