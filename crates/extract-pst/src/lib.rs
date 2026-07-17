//! # extract-pst
//!
//! Blocking library that opens **Unicode PST** evidence (filesystem and/or
//! matter CAS), walks folders/messages/attachments via **`pst-reader`**, and
//! writes **Normalized Items** into `matter-core`.
//!
//! ## ⚠️ BLOCKING THREAD WARNING
//!
//! [`extract_pst_item`], [`extract_pst_item_on_job`], [`extract_pst_path`],
//! [`extract_pst_path_on_job`], and [`resume_extract`] are **CPU- and IO-bound**
//! and block for the duration of the walk. Callers **must** run them on a
//! dedicated blocking worker (`std::thread` or the **0019** `process-runner`
//! matter worker). Calling them on the GUI thread or a Tokio async worker will
//! freeze the Desk.
//!
//! ## Job-id authority (Option C)
//!
//! Orchestrated runs use [`extract_pst_item_on_job`] /
//! [`extract_pst_path_on_job`] with a job id created by `process-runner`. The
//! public wrappers create a job then call the on-job path. Resume already takes
//! an existing `job_id`.
//!
//! ## Native identity
//!
//! Parent email `native_sha256` is the CAS digest of a deterministic
//! **`pst-native-message-v1`** blob — **not** synthetic EML. EML export is
//! deferred to track 0040.
//!
//! Attachments stream into CAS via `put_reader` (no multi-GB full `Vec` on the
//! production path).
//!
//! ## Temp hygiene
//!
//! CAS-only PSTs are materialised under `<matter>/workspace/temp/` only — never
//! `std::env::temp_dir()`. Leftover temp is cleaned on `Matter::open` /
//! `Matter::create`.
//!
//! ## Checkpoints
//!
//! Mid-folder checkpoints every `batch_size` messages (default 500) with
//! `last_folder_path`, `last_message_nid`, and `folder_message_index`.
//!
//! ## Out of scope
//!
//! - EML as native identity
//! - Mutating source PST evidence
//! - Matter-wide dedupe (0021)

#![forbid(unsafe_code)]

pub mod checkpoint;
pub mod error;
pub mod extract;
pub mod limits;
pub mod native_message;
pub mod open;
pub mod recipients;

pub use checkpoint::ExtractCursor;
pub use error::{Error, Result};
pub use extract::{
    extract_pst_item, extract_pst_item_on_job, extract_pst_path, extract_pst_path_on_job,
    list_discovered_psts, resume_extract,
};
pub use limits::{ExtractLimits, ExtractSummary, JOB_KIND_EXTRACT_PST, STAGE_PST_EXTRACT};
pub use native_message::{
    encode_native_message_v1, native_message_v1_digest, NativeAttachment, NativeMessageV1,
    NATIVE_FORMAT_V1, NATIVE_MAGIC, NATIVE_VERSION,
};
pub use open::{candidate_fs_path, open_pst, OpenedPst, PstOpenSpec};
pub use recipients::{bcc_for_logical, parse_display_list};
