//! # ingest-purview
//!
//! Detect Microsoft Purview-style export layouts (and simpler PST/ZIP/folder
//! dumps), register them as matter `sources`, and **safely expand** ZIP
//! containers into the matter CAS with inventory + resumable leaf checkpoints.
//!
//! ## Blocking-thread caller contract
//!
//! [`ingest_path`], [`ingest_path_on_job`], and [`resume_ingest`] are **CPU- and
//! IO-bound** and block for the duration of expand. Callers **must** run them
//! on a dedicated blocking worker (`std::thread` or the **0019** `process-runner`
//! matter worker). Calling them on the GUI thread or a Tokio async worker will
//! freeze the Desk.
//!
//! ## Job-id authority (Option C)
//!
//! Orchestrated runs use [`ingest_path_on_job`] with a job id created by
//! `process-runner`. [`ingest_path`] remains a thin wrapper that creates a job
//! then calls the on-job path. This crate does not enforce the blocking contract.
//!
//! ## Out of scope
//!
//! - PST message extraction (`pst-reader` / track 0018)
//! - Full Normalized Item model (0017)
//! - 7z expand
//! - Mutating source package files

#![forbid(unsafe_code)]

pub mod detect;
pub mod encoding;
pub mod error;
pub mod expand;
pub mod ingest;
pub mod limits;
pub mod path_safety;

pub use detect::{detect, DetectResult, PackageKind};
pub use error::{Error, Result};
pub use expand::ExpandCursor;
pub use ingest::{ingest_path, ingest_path_on_job, resume_ingest, IngestSummary};
pub use limits::ExpandLimits;
pub use path_safety::sanitize_logical_path;
