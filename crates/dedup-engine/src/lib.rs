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

pub mod body_cloud_links;
pub mod eml_pack;
pub mod exporter;
pub mod grouping;
pub mod hasher;
pub mod index;
pub mod integrity;
pub mod keepset;
pub mod report;
pub mod util;

pub use body_cloud_links::{
    scan_body_cloud_links, BodyCloudLinkHit, BodyCloudScan, BodyCloudUrlSource,
    MAX_BODY_SCAN_CHARS, MAX_LINKS_PER_MESSAGE, MAX_URL_LEN,
};

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
pub use grouping::{
    mid_join_compatible, normalize_recipient_identity_keys, normalize_recipients,
    recipient_has_x500, BoundBy, CanonicalRecipient, CanonicalRecipientType, DedupeScope,
    GroupingContext, GroupingStats, IdentityLevel, Tier1Verify,
};
pub use hasher::{
    compute_content_hash, compute_dedup_keys, compute_dedup_keys_ex, count_weak_fields,
    hash_full_body, normalize_message_id, normalize_subject, tier2_eligibility, AttachmentInfo,
    DedupKeys, StrongHashInput, Tier2IneligibleReason,
};
pub use index::{DedupIndex, DedupResult, DedupTier, IndexItem, MessageRef};
pub use integrity::{
    attach_reason_from_pst_error, compute_preflight, reason_from_pst_error, AttachProbePreflight,
    FileScanStatus, IntegrityCsvWriter, IntegrityReason, IntegrityThresholds, PreflightInputs,
    PreflightRecommendation, PreflightReport, ScanMode, SkipRecord, SCAN_INTEGRITY_SCHEMA,
};
pub use keepset::{
    build_keep_set, build_keep_set_materialized, build_keep_set_with_ctx, classify_folder,
    decided_by_rung, duplicate_source_aggregate, edrm_mih_hex, fidelity_rank,
    fidelity_rank_with_mode, finalize_with_materialize, finalize_with_materialize_opts,
    folder_class_and_rank, format_date_filetime_utc, group_candidates, group_candidates_ctx,
    group_candidates_with_stats, is_attach_incomplete, rank_key, reason_fidelity_tier,
    recoverable_items_hint, resolve_groups, resolve_groups_with_ctx, resolve_groups_with_grouping,
    resolve_item_date, segment_glob_match, sort_input_paths, source_rank_of, write_keep_set_json,
    CanonicalAttachment, CanonicalMessage, DateSource, DecisionCsvWriter, DecisionRecord,
    DecisionRole, FamilyPolicy, FidelityMode, FolderClass, FolderRankMode, GroupingOutcome,
    KeepEntry, KeepPolicy, KeepSet, KeepSetError, KeepSetProvenance, KeepSetStats,
    MaterializeBuildOpts, MaterializeError, MaterializeFinalizeOpts, MessageLocus,
    MessageMaterializer, PromoteReason, RankContext, RankKey, RecoverableScanItem, ResolvedKeepSet,
    SoftSkipAttachRecord, DECISION_CSV_HEADER, DECISION_CSV_HEADER_V1, DUPLICATE_SOURCES_CAP,
    KEEP_SET_SCHEMA,
};
pub use report::{write_csv_report, StreamingCsvReportWriter};
pub use util::{filetime_to_unix, format_bytes, truncate_utf8};
