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

pub mod eml_pack;
pub mod exporter;
pub mod hasher;
pub mod index;
pub mod integrity;
pub mod keepset;
pub mod report;
pub mod util;

pub use eml_pack::{
    clamp_files_per_volume, format_date_utc_filetime, format_date_utc_unix, make_eml_pack_filename,
    merge_pack_degraded, normalize_body_crlf_bytes, normalize_text_body_crlf,
    sanitize_header_value, validate_volume_prefix, volume_dirname, write_canonical_eml,
    write_crlf_line, write_eml_pack_manifest, AttachStreamSource, EmlPackManifest,
    EmlPackMessageRow, EmlPackStats, EmlWriteError, EmlWriteOpts, EmlWriteResult,
    NullAttachStreamSource, VolumePackWriter, ABS_PATH_BUDGET, ATTACH_EMBEDDED_MSG,
    DEFAULT_FILES_PER_VOLUME, EML_PACK_SCHEMA, REASON_ATTACH_PART_FAILED,
};
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
