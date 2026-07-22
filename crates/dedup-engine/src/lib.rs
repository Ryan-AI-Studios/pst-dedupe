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

pub mod exporter;
pub mod hasher;
pub mod index;
pub mod integrity;
pub mod keepset;
pub mod report;
pub mod util;

pub use exporter::export_eml;
pub use hasher::{compute_dedup_keys, normalize_message_id};
pub use index::{DedupIndex, DedupResult, DedupTier, MessageRef};
pub use integrity::{
    compute_preflight, reason_from_pst_error, FileScanStatus, IntegrityCsvWriter, IntegrityReason,
    IntegrityThresholds, PreflightRecommendation, PreflightReport, ScanMode, SkipRecord,
    SCAN_INTEGRITY_SCHEMA,
};
pub use keepset::{
    build_keep_set, build_keep_set_materialized, edrm_mih_hex, finalize_with_materialize,
    group_candidates, sort_input_paths, write_keep_set_json, CanonicalAttachment, CanonicalMessage,
    DecisionCsvWriter, DecisionRecord, DecisionRole, FamilyPolicy, KeepEntry, KeepPolicy, KeepSet,
    KeepSetError, KeepSetProvenance, KeepSetStats, MaterializeBuildOpts, MaterializeError,
    MessageLocus, MessageMaterializer, RecoverableScanItem, ResolvedKeepSet, KEEP_SET_SCHEMA,
};
pub use report::{write_csv_report, StreamingCsvReportWriter};
pub use util::{filetime_to_unix, format_bytes, truncate_utf8};
