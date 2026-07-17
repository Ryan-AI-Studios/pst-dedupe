//! # ingest-purview
//!
//! Detect Microsoft Purview-style export layouts (and simpler PST/ZIP/folder
//! dumps), register them as matter `sources`, and **safely expand** ZIP
//! containers into the matter CAS with inventory + resumable leaf checkpoints.
//!
//! ## Blocking-thread caller contract
//!
//! [`ingest_path`] and [`resume_ingest`] are **CPU- and IO-bound** and block for
//! the duration of expand. Callers **must** run them on a dedicated blocking
//! worker (`std::thread`, rayon, or `tokio::task::spawn_blocking` in 0019+).
//! Calling them on the GUI thread or a Tokio async worker will freeze the Desk.
//!
//! This crate does not enforce that contract.
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
pub use ingest::{ingest_path, resume_ingest, IngestSummary};
pub use limits::ExpandLimits;
pub use path_safety::sanitize_logical_path;
