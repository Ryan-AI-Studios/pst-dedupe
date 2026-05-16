//! # dedup-engine
//!
//! Email deduplication engine with tiered hashing strategy.
//!
//! ## Strategy
//!
//! **Tier 1 — Message-ID:** Emails with the same RFC 2822 Message-ID header are
//! definitively the same message (including copies to different recipients).
//!
//! **Tier 2 — Content Hash:** For emails missing a Message-ID, we compute a SHA-256
//! hash of: normalized subject + submit time + sender + body preview + attachment metadata.

pub mod hasher;
pub mod index;
pub mod report;
pub mod exporter;

pub use index::{DedupIndex, DedupResult, DedupTier, MessageRef};
pub use hasher::compute_dedup_keys;
pub use report::write_csv_report;
pub use exporter::export_eml;
