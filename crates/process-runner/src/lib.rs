//! # process-runner
//!
//! In-process **matter job runner** for Dedupe Desk:
//!
//! - **Single matter worker thread** owns `Matter` for the active job
//! - **Cancel** via [`CancelToken`] (`Arc<AtomicBool>`, cooperative)
//! - **Progress** via `tokio::sync::watch` (latest snapshot) + optional broadcast
//! - **Option C:** runner is sole creator of job rows for orchestrated runs
//!
//! ## Never call extract/ingest on the UI thread
//!
//! Handlers run only on the matter worker. GUI/CLI code should call
//! [`ProcessRunner::start`] / [`cancel`] / [`watch_progress`] only.
//!
//! ## Drop / shutdown
//!
//! [`ProcessRunner::shutdown`] and [`Drop`] set cancel and **join** the worker
//! so in-flight SQLite batches can finish or cleanly pause.

#![forbid(unsafe_code)]

pub mod cancel;
pub mod config;
pub mod error;
pub mod handler;
pub mod handlers;
pub mod progress;
pub mod register;
pub mod runner;

pub use cancel::CancelToken;
pub use config::RunnerConfig;
pub use error::{Result, RunnerError};
pub use handler::{JobContext, JobHandler, JobOutcome, JobParams};
pub use progress::{JobProgressSnapshot, ProgressEvent, ProgressSink};
pub use register::{default_handler_kinds, register_default_handlers};
pub use runner::{JobSnapshot, ProcessRunner};

#[cfg(feature = "ingest")]
pub use handlers::IngestHandler;

#[cfg(feature = "extract_pst")]
pub use handlers::ExtractPstHandler;

#[cfg(feature = "dedupe")]
pub use handlers::MatterDedupeHandler;

#[cfg(feature = "thread")]
pub use handlers::MatterThreadHandler;

#[cfg(feature = "neardup")]
pub use handlers::MatterNearDupHandler;

#[cfg(feature = "cull")]
pub use handlers::MatterCullHandler;

#[cfg(feature = "promote")]
pub use handlers::MatterPromoteHandler;

#[cfg(feature = "produce")]
pub use handlers::{MatterProduceHandler, MatterProductionExportHandler};

#[cfg(feature = "qc")]
pub use handlers::MatterQcHandler;

#[cfg(feature = "gap")]
pub use handlers::MatterGapHandler;

#[cfg(feature = "fts")]
pub use handlers::MatterFtsIndexHandler;

#[cfg(feature = "office")]
pub use handlers::MatterOfficeExtractHandler;

#[cfg(feature = "pdf")]
pub use handlers::MatterPdfExtractHandler;

#[cfg(feature = "calendar")]
pub use handlers::MatterIcsExtractHandler;

#[cfg(feature = "ocr")]
pub use handlers::MatterOcrHandler;

#[cfg(feature = "classify")]
pub use handlers::MatterClassifyHandler;

#[cfg(feature = "entity")]
pub use handlers::MatterEntityScanHandler;

pub use handlers::MatterProfileRunHandler;
pub use handlers::MatterWorkflowRunHandler;
