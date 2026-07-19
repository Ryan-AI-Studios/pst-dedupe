//! # file-category
//!
//! Stable **`taxonomy_v1`** vocabulary and pure-Rust classifier for Dedupe Desk
//! (track **0037**).
//!
//! ## Pipeline priority
//!
//! 1. Structural / `message_class`
//! 2. Extractor refine (optional; non-legacy closed set)
//! 3. Magic bytes (≤64 KiB) — specific magic beats extension; **ZIP/OLE** use §3.4.1
//! 4. MIME (stored or `mime_guess`)
//! 5. Extension table (includes **`.msg` → email**)
//! 6. Fallback: `unrecognized` / `other`
//!
//! ## Role ≠ category
//!
//! `role=attachment` stays on **role**. Category describes **content type**.
//! Bare `attachment` is **not** a valid category.
//!
//! ## Job
//!
//! Resumable [`run_classify`] (`kind = "classify"`) — **blocking**; call only on
//! the matter worker thread.
//!
//! ## Pins
//!
//! - `mime_guess` 2.0.x
//! - `infer` 0.19.x (pure Rust; no libmagic)

#![forbid(unsafe_code)]

pub mod category;
pub mod classify;
pub mod error;
pub mod extension;
pub mod magic;
pub mod mime_map;
pub mod params;
pub mod run;

pub use category::{Category, CategoryMethod, Classification, Confidence, ALL, TAXONOMY_V1};
pub use classify::{classify, classify_path_mime, classify_with_head, ClassifyInput};
pub use error::{Error, Result};
pub use params::ClassifyParams;
pub use run::{run_classify, ClassifyOutcome, ClassifySummary, CLASSIFY_STAGE, JOB_KIND_CLASSIFY};
