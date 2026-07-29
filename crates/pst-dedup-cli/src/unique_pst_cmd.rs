//! `pst-dedup unique-pst` — keep_set_v1 → streaming unique PST volume(s) + report pack (track 0071).
//!
//! Pipeline (no re-dedupe):
//! integrity scan → resolve_groups → finalize_with_materialize → write_unicode_pst_streaming
//! (multi-volume optional) → report pack → verify completed volumes.
//!
//! Locks: source PSTs read-only; incomplete current volume deleted on fatal write fail;
//! completed volumes retained; export_messages.csv mandatory; default verify is open+count+sample
//! (full rehash only with `--verify-hash`).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::keep_set_cmd::rank_context_from_cli;
use clap::Args;
use dedup_engine::integrity::{
    compute_preflight, IntegrityReason, IntegrityThresholds, PreflightInputs, ScanMode,
    SCAN_INTEGRITY_SCHEMA,
};
use dedup_engine::keepset::{
    finalize_with_materialize_opts, recoverable_items_hint, resolve_groups_with_grouping,
    sort_input_paths, write_keep_set_json, DecisionCsvWriter, FamilyPolicy, KeepPolicy, KeepSet,
    KeepSetProvenance, KeepSetStats, MaterializeFinalizeOpts, KEEP_SET_SCHEMA,
};
use pst_reader::PstFile;
use pst_writer::{
    from_canonical_message_owned, temp_sibling_path, write_unicode_pst_streaming, AttachRead,
    AttachStreamSource, FolderLayoutPolicy, WriteMessage, WriteProgress, WriteProgressSink,
    WritePstOpts, WriteStage, WriterError,
};
use sha2::{Digest, Sha256};

use crate::error::{CliError, Result};
use crate::paths::{
    is_same_or_under, is_same_or_under_resolved, paths_equal, paths_equal_resolved,
    resolve_cli_path_maybe_missing,
};
use crate::pst_materializer::{PstAttachStreamSource, PstMaterializer};
use crate::pst_materializer::{PstHandleCache, DEFAULT_MAX_OPEN_PSTS};
use crate::scan::{
    apply_strict_probe_skips_to_file_stats, evaluate_exit_policy, rebuild_dedup_results_with_ctx,
    recompute_file_status_counts, recompute_per_file_degraded_from_candidates,
    recompute_per_file_dup_from_results, resolve_pst_paths, run_scan, ScanOptions,
};
use crate::unique_export_report::{
    default_report_dir, volume_path_for, write_body_cloud_links_csv, write_export_messages_csv,
    write_summary_json, write_volumes_csv, AttachLedgerMode, AttachLedgerSink, BodyCloudLinkRow,
    ExportMessageRow, ExportSection, LedgerPathMode, PhaseTimings, SummaryError,
    UniqueExportSummary, VerificationReport, VolumeAttachBuffer, VolumeReportRow,
    VolumeVerification, DEFAULT_ATTACH_LEDGER_MAX_ROWS, EXPORT_BODY_CLOUD_LINKS_CSV_NAME,
    PREPARED_BYTES_PEAK_WARN_THRESHOLD, REASON_BODY_CLOUD_LINK, UNIQUE_EXPORT_REPORT_SCHEMA,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Max volume index considered for stale-sibling cleanup and collision guards.
const MAX_VOLUME_SIBLING_INDEX: u32 = 999;

/// Map a writer failure to a summary `error.code` for [`crate::export_outcome::summary_is_retryable`].
///
/// - [`WriterError::Io`] → `write_io` (transient; automation may retry)
/// - [`WriterError::Cancelled`] → `cancelled`
/// - layout / capacity / policy refusals → `export` (permanent)
pub(crate) fn writer_error_summary_code(err: &WriterError) -> &'static str {
    match err {
        WriterError::Io(_) => "write_io",
        WriterError::Cancelled => "cancelled",
        WriterError::Layout(_)
        | WriterError::BodyTooLarge(_)
        | WriterError::AllocationFailed(_)
        | WriterError::Refused(_)
        | WriterError::RefusedSourceOverwrite(_)
        | WriterError::EmlParse(_) => "export",
    }
}

/// Clap surface for `unique-pst` (tuple-variant keeps `Commands` smaller on stack).
#[derive(Debug, Args)]
pub struct UniquePstClapArgs {
    /// PST path(s) as positional arguments (same style as `scan` / `unique-eml`).
    #[arg(required = false)]
    pub paths: Vec<PathBuf>,
    /// PST path(s) via repeated `--input` (merge with positionals).
    #[arg(long = "input", action = clap::ArgAction::Append)]
    pub input: Vec<PathBuf>,
    /// Primary output PST path (volume 1). Multi-volume: `{stem}_vol002.pst`, …
    #[arg(long)]
    pub out: PathBuf,
    /// Report pack directory (default: sibling of `--out` stem + `_report`).
    #[arg(long)]
    pub report_dir: Option<PathBuf>,
    /// Winner policy after fidelity: first_seen (default), keep_largest, prefer_path, earliest_date.
    /// Note: first_seen = sorted input-path order, not chronological send time.
    #[arg(long, default_value = "first_seen", value_parser = parse_keep_policy_arg)]
    pub policy: KeepPolicy,
    /// Parent+attach family: keep_attachments_with_parent (default) or parents_only.
    #[arg(long, default_value = "keep_attachments_with_parent", value_parser = parse_family_policy_arg)]
    pub family_policy: FamilyPolicy,
    /// Path/folder substring preferred under prefer_path (repeatable).
    #[arg(long = "prefer-path-contains")]
    pub prefer_path_contains: Vec<String>,
    /// Prefer BCC-bearing copy (sender-copy completeness; opt-in).
    #[arg(long = "prefer-bcc-copy")]
    pub prefer_bcc_copy: bool,
    /// Enable built-in folder-class ladder.
    #[arg(long = "prefer-folder-class")]
    pub prefer_folder_class: bool,
    /// Custom folder-rank pattern (repeatable, worst-last; replaces built-in).
    #[arg(long = "folder-rank", action = clap::ArgAction::Append)]
    pub folder_rank: Vec<String>,
    /// Ordered source preference (repeatable, best-first).
    #[arg(long = "source-rank", action = clap::ArgAction::Append)]
    pub source_rank: Vec<String>,
    /// Swap source_rank and folder_class rungs.
    #[arg(long = "rank-folder-class-first")]
    pub rank_folder_class_first: bool,
    /// Fidelity ranking: binary (default) or graded.
    #[arg(long = "fidelity-rank", default_value = "binary", value_parser = parse_fidelity_rank_arg)]
    pub fidelity_rank: String,
    /// Streaming decision CSV (default: `{report-dir}/decisions.csv`).
    #[arg(long)]
    pub decision_csv: Option<PathBuf>,
    /// Keep-set JSON (default: `{report-dir}/keepset.json`).
    #[arg(long)]
    pub keep_set_json: Option<PathBuf>,
    /// Folder layout: `preserve` (default) or `flat`.
    #[arg(long, default_value = "preserve", value_parser = parse_folder_layout_arg)]
    pub folder_layout: FolderLayoutArg,
    /// Soft max physical size per volume (bytes). Off = single volume.
    /// Oversized single family may exceed this limit (never severed).
    #[arg(long)]
    pub max_volume_bytes: Option<u64>,
    /// Allow replacing existing `--out` / report-dir contents.
    #[arg(long)]
    pub overwrite: bool,
    /// Full-file rehash of completed volumes vs report digests (default off).
    #[arg(long)]
    pub verify_hash: bool,
    /// Optional co-export unique-eml pack directory (soft residual; may be ignored).
    #[arg(long)]
    pub also_eml: Option<PathBuf>,
    #[arg(long)]
    pub no_tier2: bool,
    #[arg(long)]
    pub no_attachments: bool,
    #[arg(long)]
    pub json: bool,
    #[arg(long, default_value = "best-effort", value_parser = parse_scan_mode_arg)]
    pub mode: ScanMode,
    #[arg(long, default_value_t = 0.05, value_parser = parse_rate_threshold_arg)]
    pub max_skip_rate: f64,
    #[arg(long, default_value_t = 0.01, value_parser = parse_rate_threshold_arg)]
    pub max_crc_skip_rate: f64,
    #[arg(long, default_value_t = 0.0, value_parser = parse_rate_threshold_arg)]
    pub max_failed_file_rate: f64,
    #[arg(long)]
    pub allow_failed_files: bool,
    #[arg(long)]
    pub integrity_csv: Option<PathBuf>,
    #[arg(long, default_value_t = 10_000)]
    pub skip_limit: usize,
    /// Attachment failure ledger: `full` (default CSV+histogram), `summary-only`, or `off`.
    #[arg(long = "attach-ledger", default_value = "full", value_parser = parse_attach_ledger_mode_arg)]
    pub attach_ledger: AttachLedgerMode,
    /// Max rows written to `export_attachments.csv` (default 500000). Histogram is never truncated.
    #[arg(long = "attach-ledger-max-rows", default_value_t = DEFAULT_ATTACH_LEDGER_MAX_ROWS)]
    pub attach_ledger_max_rows: u64,
    /// How `source_path` columns are written in export CSVs: `full` (default) or `basename` (0081).
    /// Basename is presentation-only for handoff; join origin via `source_id` + Matter Archive.
    #[arg(long = "ledger-path-mode", default_value = "full", value_parser = parse_ledger_path_mode_arg)]
    pub ledger_path_mode: LedgerPathMode,
    /// Opt-in budgeted deep attach stream preflight before keep-set resolve (0074). Default off.
    #[arg(long = "deep-attach-preflight")]
    pub deep_attach_preflight: bool,
    /// Deep probe level: `head` (L2, default) or `full` (L3).
    #[arg(long = "deep-attach-level", default_value = "head", value_parser = parse_deep_attach_level_arg)]
    pub deep_attach_level: String,
    #[arg(long = "deep-attach-max-attaches", default_value_t = 50_000)]
    pub deep_attach_max_attaches: u64,
    #[arg(long = "deep-attach-max-probe-bytes", default_value_t = 268_435_456)]
    pub deep_attach_max_probe_bytes: u64,
    #[arg(long = "deep-attach-per-attach-max-bytes", default_value_t = 1_048_576)]
    pub deep_attach_per_attach_max_bytes: u64,
    #[arg(long = "deep-attach-max-probe-time-ms", default_value_t = 2000)]
    pub deep_attach_max_probe_time_ms: u64,
    #[arg(long = "deep-attach-max-open-psts", default_value_t = 32)]
    pub deep_attach_max_open_psts: usize,
    #[arg(long = "deep-attach-max-peer-probes", default_value_t = 3)]
    pub deep_attach_max_peer_probes: u64,
    /// Max attach-stream probe fail rate before preflight recommends re-export (default 0.05).
    #[arg(long = "max-attach-fail-rate", default_value_t = 0.05, value_parser = parse_rate_threshold_arg)]
    pub max_attach_fail_rate: f64,
    /// Strong content identity: off|body|body-recip|body-recip-attach (0086).
    #[arg(long = "strong-content-hash", default_value = "off", value_parser = parse_strong_content_hash_arg)]
    pub strong_content_hash: String,
    /// Max attaches full-stream digested under body-recip-attach (0086; default 50000).
    #[arg(long = "strong-hash-attach-max-attaches", default_value_t = 50_000)]
    pub strong_hash_attach_max_attaches: u64,
    /// Max digest bytes per run under body-recip-attach (0086; default 1 GiB).
    #[arg(long = "strong-hash-attach-max-bytes", default_value_t = 1_073_741_824)]
    pub strong_hash_attach_max_bytes: u64,
    /// Per-attach max digest bytes under body-recip-attach (0086; default 512 MiB).
    #[arg(
        long = "strong-hash-attach-per-attach-max-bytes",
        default_value_t = 536_870_912
    )]
    pub strong_hash_attach_per_attach_max_bytes: u64,
    /// Dedupe partition: global|per-source (0076).
    #[arg(long = "dedupe-scope", default_value = "global", value_parser = parse_dedupe_scope_arg)]
    pub dedupe_scope: String,
    /// Subdivide MID groups: off|content|body (0076).
    #[arg(long = "tier1-verify", default_value = "off", value_parser = parse_tier1_verify_arg)]
    pub tier1_verify: String,
    #[arg(long = "tier1-backfill")]
    pub tier1_backfill: bool,
    #[arg(long = "identity-ignore-inline-attachments")]
    pub identity_ignore_inline_attachments: bool,
    #[arg(long = "allow-cross-mid-tier2")]
    pub allow_cross_mid_tier2: bool,
    #[arg(long = "allow-degenerate-tier2")]
    pub allow_degenerate_tier2: bool,
    /// Allow Tier-2 bind for CRC_SUSPECT items (restores pre-0077; default off) (0077).
    #[arg(long = "allow-crc-suspect-tier2")]
    pub allow_crc_suspect_tier2: bool,
    /// First-N detail CRC warn lines per category before aggregation (0077).
    #[arg(long = "crc-log-limit", default_value_t = 10)]
    pub crc_log_limit: u64,
    /// Seconds between aggregate CRC summary lines after first-N (0077).
    #[arg(long = "crc-log-interval-secs", default_value_t = 30)]
    pub crc_log_interval_secs: u64,
    /// Fail (exit 64) when fidelity is partial. Default **on** when neither
    /// fidelity flag is supplied; mutually exclusive with `--allow-partial-fidelity` (0078).
    #[arg(long = "fail-on-partial-fidelity", action = clap::ArgAction::SetTrue)]
    pub fail_on_partial_fidelity: bool,
    /// Allow partial fidelity to exit 0 (JSON still reports `partial`; 0078).
    #[arg(long = "allow-partial-fidelity", action = clap::ArgAction::SetTrue)]
    pub allow_partial_fidelity: bool,
    /// Opt-in: exit 65 when `export_risk` rank ≥ level (default off; 0078).
    #[arg(long = "fail-on-export-risk", value_parser = parse_fail_on_export_risk_arg)]
    pub fail_on_export_risk: Option<String>,
    /// Max sticky source PST handles for materialize + attach stream (0079; default 32).
    #[arg(long = "max-open-psts", default_value_t = DEFAULT_MAX_OPEN_PSTS)]
    pub max_open_psts: usize,
    /// QC depth: `off|structure|sample|full` (0080). Default **sample**.
    #[arg(long = "qc-level", default_value = "sample", value_parser = parse_qc_level_arg)]
    pub qc_level: String,
    /// Risk-weighted sample cap when `--qc-level sample` (default 64).
    #[arg(long = "qc-sample-max", default_value_t = crate::unique_pst_qc::DEFAULT_QC_SAMPLE_MAX)]
    pub qc_sample_max: usize,
    /// Optional BYOB path to `pffinfo` / `readpst` for counts-only cross-check (0080).
    #[arg(long = "qc-external-reader")]
    pub qc_external_reader: Option<PathBuf>,
    /// Attempt scanpst `-no repair` on a local temp copy when discoverable (0080).
    #[arg(long = "qc-scanpst", action = clap::ArgAction::SetTrue)]
    pub qc_scanpst: bool,
    /// Write Bcc recipient rows and PidTagDisplayBcc into the unique-PST (0082).
    /// Default OFF: consolidating custodians can over-disclose BCC relative to a
    /// single custodian's outward view. Identity hashing still includes BCC when
    /// the source table is present.
    #[arg(long = "include-bcc-recipients", action = clap::ArgAction::SetTrue)]
    pub include_bcc_recipients: bool,
    /// Mode A pre-write promote when keep-set winner materializes with incomplete
    /// attachments and a ranked peer is complete (0083). Default **off** (Mode C
    /// ledger-only). Write-time mid-message promote (Mode B) is not supported.
    /// Under default global dedupe scope this may select another custodian's complete
    /// copy (cross-custodian de-duplication); see the eDiscovery runbook.
    #[arg(long = "promote-on-attach-fail", action = clap::ArgAction::SetTrue)]
    pub promote_on_attach_fail: bool,
}

/// Runtime options for `unique-pst` orchestration.
#[derive(Debug, Clone)]
pub struct UniquePstCliArgs {
    pub paths: Vec<PathBuf>,
    pub out: PathBuf,
    pub report_dir: Option<PathBuf>,
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path_contains: Vec<String>,
    pub prefer_bcc_copy: bool,
    pub prefer_folder_class: bool,
    pub folder_rank: Vec<String>,
    pub source_rank: Vec<String>,
    pub rank_folder_class_first: bool,
    pub fidelity_rank: String,
    pub decision_csv: Option<PathBuf>,
    pub keep_set_json: Option<PathBuf>,
    pub folder_layout: FolderLayoutArg,
    pub max_volume_bytes: Option<u64>,
    pub overwrite: bool,
    pub verify_hash: bool,
    /// Soft: optional co-export unique-eml pack (residual if unused).
    pub also_eml: Option<PathBuf>,
    pub no_tier2: bool,
    pub no_attachments: bool,
    pub json: bool,
    pub mode: ScanMode,
    pub max_skip_rate: f64,
    pub max_crc_skip_rate: f64,
    pub max_failed_file_rate: f64,
    pub allow_failed_files: bool,
    pub integrity_csv: Option<PathBuf>,
    pub skip_limit: usize,
    /// Attachment failure ledger mode (0073). Default `full`.
    pub attach_ledger: AttachLedgerMode,
    /// Max CSV rows for attach ledger (0073). Default 500_000.
    pub attach_ledger_max_rows: u64,
    /// Path column mode for export CSVs (0081). Default `full`.
    pub ledger_path_mode: LedgerPathMode,
    /// Opt-in deep attach preflight (0074). Default off.
    pub deep_attach_preflight: bool,
    pub deep_attach_level: String,
    pub deep_attach_max_attaches: u64,
    pub deep_attach_max_probe_bytes: u64,
    pub deep_attach_per_attach_max_bytes: u64,
    pub deep_attach_max_probe_time_ms: u64,
    pub deep_attach_max_open_psts: usize,
    pub deep_attach_max_peer_probes: u64,
    pub max_attach_fail_rate: f64,
    pub strong_content_hash: String,
    pub strong_hash_attach_max_attaches: u64,
    pub strong_hash_attach_max_bytes: u64,
    pub strong_hash_attach_per_attach_max_bytes: u64,
    pub dedupe_scope: String,
    pub tier1_verify: String,
    pub tier1_backfill: bool,
    pub identity_ignore_inline_attachments: bool,
    pub allow_cross_mid_tier2: bool,
    pub allow_degenerate_tier2: bool,
    pub allow_crc_suspect_tier2: bool,
    pub crc_log_limit: u64,
    pub crc_log_interval_secs: u64,
    /// Fail (exit 64) on partial fidelity (default true; 0078).
    pub fail_on_partial_fidelity: bool,
    /// Allow partial → exit 0 (default false; 0078).
    pub allow_partial_fidelity: bool,
    /// Opt-in risk gate level string or None (0078).
    pub fail_on_export_risk: Option<String>,
    /// Max sticky source PST handles for materialize + attach stream (0079).
    pub max_open_psts: usize,
    /// QC level (0080): off|structure|sample|full.
    pub qc_level: crate::unique_pst_qc::QcLevel,
    /// Sample cap for risk-weighted QC (0080).
    pub qc_sample_max: usize,
    /// Optional independent reader path (0080 BYOB).
    pub qc_external_reader: Option<PathBuf>,
    /// Attempt scanpst when true (0080).
    pub qc_scanpst: bool,
    /// Write Bcc rows + PidTagDisplayBcc (0082). Default false.
    pub include_bcc_recipients: bool,
    /// Mode A pre-write promote-on-attach-fail (0083). Default false.
    pub promote_on_attach_fail: bool,
}

/// Run options / hooks for GUI and library callers (0072).
///
/// CLI default: `stderr_progress: true`, no cancel / progress / log callbacks.
pub struct UniquePstRunOptions {
    pub cancel: Option<Arc<AtomicBool>>,
    /// When true (CLI default), mirror stage lines to stderr.
    pub stderr_progress: bool,
    /// Optional progress observer (GUI).
    pub on_progress: Option<Box<dyn FnMut(UniquePstProgress) + Send>>,
    /// Optional log/warning lines (GUI Details panel).
    pub on_log: Option<Box<dyn FnMut(String) + Send>>,
}

impl Default for UniquePstRunOptions {
    fn default() -> Self {
        Self {
            cancel: None,
            stderr_progress: true,
            on_progress: None,
            on_log: None,
        }
    }
}

/// Structured progress tick for GUI / automation.
#[derive(Debug, Clone)]
pub struct UniquePstProgress {
    /// Stage label: `"scan"` | `"resolve"` | `"materialize"` | `"write"` | `"report"` | `"verify"` | …
    pub stage: String,
    pub volume_index: u32,
    /// Messages written on the **current** volume (resets each volume).
    pub messages_written: u64,
    /// Cumulative messages written across completed volumes + current volume.
    pub messages_written_cumulative: u64,
    pub physical_bytes: u64,
    pub winners_total: Option<u64>,
}

/// Per-volume digest surfaced on [`UniquePstOutcome`] (Done UI / library).
#[derive(Debug, Clone)]
pub struct UniqueVolumeDigest {
    pub volume_index: u32,
    pub path: String,
    pub bytes: u64,
    pub messages_written: u64,
    pub sha256_hex: String,
    /// MD5 when available from the writer report (may be empty).
    pub md5_hex: String,
}

impl From<&VolumeReportRow> for UniqueVolumeDigest {
    fn from(v: &VolumeReportRow) -> Self {
        Self {
            volume_index: v.volume_index,
            path: v.path.clone(),
            bytes: v.bytes,
            messages_written: v.messages_written,
            sha256_hex: v.sha256_hex.clone(),
            md5_hex: v.md5_hex.clone(),
        }
    }
}

/// Structured outcome for library / GUI (also used after soft cancel / partial).
#[derive(Debug, Clone)]
pub struct UniquePstOutcome {
    pub ok: bool,
    pub cancelled: bool,
    pub report_dir: PathBuf,
    pub summary_path: PathBuf,
    pub out: PathBuf,
    pub messages_written_total: u64,
    pub unique: u64,
    pub volume_count: usize,
    /// Completed volumes with digests (empty on pre-write cancel).
    pub volumes: Vec<UniqueVolumeDigest>,
    pub error_message: Option<String>,
    /// Post-export risk level (0077); Desk wizard qualifies success banner.
    pub export_risk: dedup_engine::integrity::PreflightRecommendation,
    /// Classified process exit (0078).
    pub exit: crate::error::CliExit,
    /// Terminal fidelity (0078).
    pub fidelity: crate::export_outcome::ExportFidelity,
    /// Closed-vocabulary exit reasons (0078).
    pub exit_reasons: Vec<&'static str>,
    /// Artifact disposition (0078).
    pub artifact_state: crate::export_outcome::ArtifactState,
}

impl UniquePstClapArgs {
    /// Merge positionals + `--input` into orchestration args.
    pub fn into_cli_args(self) -> std::result::Result<UniquePstCliArgs, CliError> {
        let mut paths = self.paths;
        paths.extend(self.input);
        if paths.is_empty() {
            return Err(CliError::Usage(
                "unique-pst requires at least one PST path (positional or --input)".into(),
            ));
        }
        // Fidelity flags: default fail-on is ON; both explicit → usage (0078).
        if self.fail_on_partial_fidelity && self.allow_partial_fidelity {
            return Err(CliError::Usage(
                "--fail-on-partial-fidelity and --allow-partial-fidelity are mutually exclusive"
                    .into(),
            ));
        }
        let fail_on_partial_fidelity = if self.allow_partial_fidelity {
            false
        } else {
            // Default on when neither flag, or when --fail-on-partial-fidelity alone.
            true
        };
        Ok(UniquePstCliArgs {
            paths,
            out: self.out,
            report_dir: self.report_dir,
            policy: self.policy,
            family_policy: self.family_policy,
            prefer_path_contains: self.prefer_path_contains,
            prefer_bcc_copy: self.prefer_bcc_copy,
            prefer_folder_class: self.prefer_folder_class,
            folder_rank: self.folder_rank,
            source_rank: self.source_rank,
            rank_folder_class_first: self.rank_folder_class_first,
            fidelity_rank: self.fidelity_rank,
            decision_csv: self.decision_csv,
            keep_set_json: self.keep_set_json,
            folder_layout: self.folder_layout,
            max_volume_bytes: self.max_volume_bytes,
            overwrite: self.overwrite,
            verify_hash: self.verify_hash,
            also_eml: self.also_eml,
            no_tier2: self.no_tier2,
            no_attachments: self.no_attachments,
            json: self.json,
            mode: self.mode,
            max_skip_rate: self.max_skip_rate,
            max_crc_skip_rate: self.max_crc_skip_rate,
            max_failed_file_rate: self.max_failed_file_rate,
            allow_failed_files: self.allow_failed_files,
            integrity_csv: self.integrity_csv,
            skip_limit: self.skip_limit,
            attach_ledger: self.attach_ledger,
            attach_ledger_max_rows: self.attach_ledger_max_rows,
            ledger_path_mode: self.ledger_path_mode,
            deep_attach_preflight: self.deep_attach_preflight,
            deep_attach_level: self.deep_attach_level,
            deep_attach_max_attaches: self.deep_attach_max_attaches,
            deep_attach_max_probe_bytes: self.deep_attach_max_probe_bytes,
            deep_attach_per_attach_max_bytes: self.deep_attach_per_attach_max_bytes,
            deep_attach_max_probe_time_ms: self.deep_attach_max_probe_time_ms,
            deep_attach_max_open_psts: self.deep_attach_max_open_psts,
            deep_attach_max_peer_probes: self.deep_attach_max_peer_probes,
            max_attach_fail_rate: self.max_attach_fail_rate,
            strong_content_hash: self.strong_content_hash,
            strong_hash_attach_max_attaches: self.strong_hash_attach_max_attaches,
            strong_hash_attach_max_bytes: self.strong_hash_attach_max_bytes,
            strong_hash_attach_per_attach_max_bytes: self.strong_hash_attach_per_attach_max_bytes,
            dedupe_scope: self.dedupe_scope,
            tier1_verify: self.tier1_verify,
            tier1_backfill: self.tier1_backfill,
            identity_ignore_inline_attachments: self.identity_ignore_inline_attachments,
            allow_cross_mid_tier2: self.allow_cross_mid_tier2,
            allow_degenerate_tier2: self.allow_degenerate_tier2,
            allow_crc_suspect_tier2: self.allow_crc_suspect_tier2,
            crc_log_limit: self.crc_log_limit,
            crc_log_interval_secs: self.crc_log_interval_secs,
            fail_on_partial_fidelity,
            allow_partial_fidelity: self.allow_partial_fidelity,
            fail_on_export_risk: self.fail_on_export_risk,
            max_open_psts: self.max_open_psts,
            qc_level: crate::unique_pst_qc::QcLevel::parse(&self.qc_level)
                .map_err(CliError::Usage)?,
            qc_sample_max: self.qc_sample_max.max(1),
            qc_external_reader: self.qc_external_reader,
            qc_scanpst: self.qc_scanpst,
            include_bcc_recipients: self.include_bcc_recipients,
            promote_on_attach_fail: self.promote_on_attach_fail,
        })
    }
}

/// Folder layout CLI choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderLayoutArg {
    Preserve,
    Flat,
}

impl FolderLayoutArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Flat => "flat",
        }
    }
}

fn parse_folder_layout_arg(s: &str) -> std::result::Result<FolderLayoutArg, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "preserve" => Ok(FolderLayoutArg::Preserve),
        "flat" => Ok(FolderLayoutArg::Flat),
        other => Err(format!(
            "invalid folder-layout '{other}': expected preserve or flat"
        )),
    }
}

fn parse_keep_policy_arg(s: &str) -> std::result::Result<KeepPolicy, String> {
    KeepPolicy::parse(s).ok_or_else(|| {
        format!(
            "invalid policy '{s}': expected first_seen, keep_largest, prefer_path, or earliest_date"
        )
    })
}

fn parse_fidelity_rank_arg(s: &str) -> std::result::Result<String, String> {
    match s {
        "binary" | "graded" => Ok(s.to_string()),
        _ => Err(format!(
            "invalid fidelity-rank '{s}': expected binary or graded"
        )),
    }
}

fn parse_family_policy_arg(s: &str) -> std::result::Result<FamilyPolicy, String> {
    FamilyPolicy::parse(s).ok_or_else(|| {
        format!(
            "invalid family-policy '{s}': expected keep_attachments_with_parent or parents_only"
        )
    })
}

fn parse_scan_mode_arg(s: &str) -> std::result::Result<ScanMode, String> {
    ScanMode::parse(s).ok_or_else(|| format!("invalid mode '{s}': expected best-effort or strict"))
}

fn parse_deep_attach_level_arg(s: &str) -> std::result::Result<String, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "head" | "full" => Ok(s.trim().to_ascii_lowercase()),
        other => Err(format!(
            "invalid deep-attach-level '{other}': expected head or full"
        )),
    }
}

fn parse_attach_ledger_mode_arg(s: &str) -> std::result::Result<AttachLedgerMode, String> {
    AttachLedgerMode::parse(s)
        .ok_or_else(|| format!("invalid attach-ledger '{s}': expected full, summary-only, or off"))
}

fn parse_ledger_path_mode_arg(s: &str) -> std::result::Result<LedgerPathMode, String> {
    LedgerPathMode::parse(s)
        .ok_or_else(|| format!("invalid ledger-path-mode '{s}': expected full or basename"))
}

fn parse_strong_content_hash_arg(s: &str) -> std::result::Result<String, String> {
    crate::grouping_cli::parse_identity_level(s)?;
    Ok(s.to_string())
}

fn parse_qc_level_arg(s: &str) -> std::result::Result<String, String> {
    crate::unique_pst_qc::QcLevel::parse(s)?;
    Ok(s.to_string())
}

fn parse_dedupe_scope_arg(s: &str) -> std::result::Result<String, String> {
    crate::grouping_cli::parse_dedupe_scope(s)?;
    Ok(s.to_string())
}

fn parse_tier1_verify_arg(s: &str) -> std::result::Result<String, String> {
    crate::grouping_cli::parse_tier1_verify(s)?;
    Ok(s.to_string())
}

fn parse_fail_on_export_risk_arg(s: &str) -> std::result::Result<String, String> {
    crate::export_outcome::RiskGate::parse(s)
        .filter(|g| *g != crate::export_outcome::RiskGate::Off)
        .map(|g| g.as_str().to_string())
        .ok_or_else(|| {
            format!(
                "invalid --fail-on-export-risk '{s}': expected ok, re_export_recommended, or not_export_ready"
            )
        })
}

fn parse_rate_threshold_arg(s: &str) -> std::result::Result<f64, String> {
    let v: f64 = s
        .parse()
        .map_err(|_| format!("invalid rate threshold '{s}'"))?;
    if !(0.0..=1.0).contains(&v) {
        return Err(format!("rate threshold must be in [0,1], got {v}"));
    }
    Ok(v)
}

/// Winner prepared for streaming write (meta retained for export_messages).
struct PreparedWinner {
    source_path: String,
    folder_path: String,
    nid: u64,
    message_id_norm: String,
    edrm_mih: String,
    content_hash_hex: String,
    subject: String,
    write_msg: WriteMessage,
    /// Source-side BCC retained for QC known_gap accounting (not written to PST).
    /// Populated from `CanonicalMessage.display_bcc` / adapter `dropped` (0080 DoD-15).
    display_bcc: String,
    /// Source had Bcc (table row and/or non-empty display_bcc) — for `bcc_suppressed` (0082).
    source_has_bcc: bool,
    /// Empty recipient table + flags present + not UNSENT (0082 rule 8 anomaly).
    sent_message_with_no_recipients: bool,
    /// 0085: body-inline document-shaped cloud hits (scanned at prepare; bodies may be moved at write).
    body_cloud_hits: Vec<(String, String)>, // (url, url_source)
    body_cloud_truncated: bool,
}

/// Adapter: `PstAttachStreamSource` → `pst_writer::AttachStreamSource`.
struct WriterAttachAdapter<'a> {
    inner: &'a mut PstAttachStreamSource,
}

/// Forwards `Read` and ORs [`pst_reader::AttachmentDataReader::crc_suspect`] into a
/// shared flag so the production writer can emit `ATTACH_STREAM_CRC` after a
/// successful stream (0077 DoD-19 — warning-only CRC must not be type-erased away).
struct CrcFlaggingAttachReader {
    inner: pst_reader::AttachmentDataReader,
    flag: Arc<std::sync::atomic::AtomicBool>,
}

impl Read for CrcFlaggingAttachReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if self.inner.crc_suspect() {
            self.flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(n)
    }
}

impl AttachStreamSource for WriterAttachAdapter<'_> {
    fn open_attach(
        &mut self,
        source_path: Option<&str>,
        parent_nid: Option<u64>,
        attach_nid: Option<u64>,
        _filename: &str,
    ) -> std::result::Result<Option<Vec<u8>>, String> {
        // Prefer stream path; this full-Vec fallback only for trait completeness.
        match self.open_attach_stream(source_path, parent_nid, attach_nid, _filename)? {
            Some(mut reader) => {
                let mut buf = Vec::new();
                reader
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("read attach: {e}"))?;
                Ok(Some(buf))
            }
            None => Ok(None),
        }
    }

    fn open_attach_stream(
        &mut self,
        source_path: Option<&str>,
        parent_nid: Option<u64>,
        attach_nid: Option<u64>,
        _filename: &str,
    ) -> std::result::Result<Option<AttachRead>, String> {
        let source = source_path.ok_or_else(|| "attach stream missing source_path".to_string())?;
        let parent = parent_nid.ok_or_else(|| "attach stream missing parent_nid".to_string())?;
        let attach = attach_nid.ok_or_else(|| "attach stream missing attach_nid".to_string())?;
        let locus = dedup_engine::keepset::MessageLocus {
            source_path: source.to_string(),
            source_pst: Path::new(source)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            folder_path: String::new(),
            nid: parent,
            is_orphaned: false,
        };
        // Open concrete AttachmentDataReader so late CRC taint survives type erasure.
        let reader = self
            .inner
            .open_attachment_data_reader(&locus, attach)
            .map_err(|e| e.to_string())?;
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(reader.crc_suspect()));
        let wrapped = CrcFlaggingAttachReader {
            inner: reader,
            flag: Arc::clone(&flag),
        };
        Ok(Some(AttachRead::from_reader_with_crc(
            Box::new(wrapped),
            flag,
        )))
    }
}

type ProgressCb = Arc<Mutex<Box<dyn FnMut(UniquePstProgress) + Send>>>;
type LogCb = Arc<Mutex<Box<dyn FnMut(String) + Send>>>;

/// Log write-stage progress at most this often (messages) or every
/// [`WRITE_PROGRESS_LOG_INTERVAL`] (whichever comes first after the first tick).
const WRITE_PROGRESS_LOG_EVERY_N: u64 = 50;
const WRITE_PROGRESS_LOG_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Progress + soft max-volume stop + cooperative cancel (between messages only).
struct VolumeProgressSink {
    max_volume_bytes: Option<u64>,
    volume_index: u32,
    /// Messages already written on prior completed volumes (for cumulative progress).
    messages_written_prior: u64,
    stderr: bool,
    cancel: Option<Arc<AtomicBool>>,
    winners_total: Option<u64>,
    on_progress: Option<ProgressCb>,
    on_log: Option<LogCb>,
    last_log_at: Instant,
    last_logged_messages: u64,
    /// True until the first WritingMessages tick (always log first).
    first_write_log: bool,
}

impl WriteProgressSink for VolumeProgressSink {
    fn on_progress(&mut self, p: &WriteProgress) {
        if p.stage == WriteStage::WritingMessages {
            // Progress bar: every tick (GUI needs smooth updates).
            if let Some(cb) = &self.on_progress {
                if let Ok(mut g) = cb.lock() {
                    g(UniquePstProgress {
                        stage: "write".into(),
                        volume_index: self.volume_index,
                        messages_written: p.messages_written,
                        messages_written_cumulative: self
                            .messages_written_prior
                            .saturating_add(p.messages_written),
                        physical_bytes: p.current_physical_size,
                        winners_total: self.winners_total,
                    });
                }
            }
            // Log lines: throttle to avoid Details/stderr spam on large volumes.
            let elapsed = self.last_log_at.elapsed() >= WRITE_PROGRESS_LOG_INTERVAL;
            let every_n = p.messages_written.saturating_sub(self.last_logged_messages)
                >= WRITE_PROGRESS_LOG_EVERY_N;
            if self.first_write_log || elapsed || every_n {
                self.first_write_log = false;
                self.last_log_at = Instant::now();
                self.last_logged_messages = p.messages_written;
                let line = format!(
                    "unique-pst: volume {} stage={:?} messages={} cumulative={} physical_bytes={}",
                    self.volume_index,
                    p.stage,
                    p.messages_written,
                    self.messages_written_prior
                        .saturating_add(p.messages_written),
                    p.current_physical_size
                );
                if self.stderr {
                    let _ = writeln!(std::io::stderr(), "{line}");
                }
                if let Some(log) = &self.on_log {
                    if let Ok(mut g) = log.lock() {
                        g(line);
                    }
                }
            }
        }
    }

    fn should_stop_and_finalize(&self, p: &WriteProgress) -> bool {
        let Some(max) = self.max_volume_bytes else {
            return false;
        };
        p.stage == WriteStage::WritingMessages && p.current_physical_size >= max
    }

    fn should_cancel(&self, _p: &WriteProgress) -> bool {
        self.cancel
            .as_ref()
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(false)
    }
}

fn cancel_requested(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .map(|c| c.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// Emit a stage/log line to stderr (optional) and `on_log`.
fn emit_log(stderr: bool, on_log: &Option<LogCb>, msg: &str) {
    let line = format!("unique-pst: {msg}");
    if stderr {
        let _ = writeln!(std::io::stderr(), "{line}");
    }
    if let Some(log) = on_log {
        if let Ok(mut g) = log.lock() {
            g(line);
        }
    }
}

fn emit_stage_progress(
    on_progress: &Option<ProgressCb>,
    stage: &str,
    volume_index: u32,
    messages_written: u64,
    messages_written_cumulative: u64,
    physical_bytes: u64,
    winners_total: Option<u64>,
) {
    if let Some(cb) = on_progress {
        if let Ok(mut g) = cb.lock() {
            g(UniquePstProgress {
                stage: stage.into(),
                volume_index,
                messages_written,
                messages_written_cumulative,
                physical_bytes,
                winners_total,
            });
        }
    }
}

/// Paths + policy context for a minimal cancelled `summary.json`.
struct CancelledSummaryCtx<'a> {
    summary_path: &'a Path,
    inputs: &'a [PathBuf],
    out: &'a Path,
    report_dir: &'a Path,
    policy: KeepPolicy,
    family_policy: FamilyPolicy,
    mode: ScanMode,
    folder_layout: FolderLayoutArg,
    max_volume_bytes: Option<u64>,
    duration_ms: u64,
    phase_timings: PhaseTimings,
    source_pst_opens: u64,
    messages_materialized: u64,
    artifact_state: crate::export_outcome::ArtifactState,
    /// Operator-requested Mode A flag (echo into cancelled summary; 0083).
    promote_on_attach_fail: bool,
}

/// Quarantine written volumes after cancel: rename each volume to
/// `{filename}.cancelled-{unix_secs}.partial` (e.g. `unique.pst` →
/// `unique.pst.cancelled-1720000000.partial`) so `--out` is free for retry.
///
/// Returns overall result for `artifact_state` (0078 §3.6).
pub fn quarantine_cancelled_volumes(
    out: &Path,
    volume_count: u32,
) -> crate::export_outcome::QuarantineResult {
    quarantine_cancelled_volumes_with(out, volume_count, |from, to| fs::rename(from, to))
}

/// Testable quarantine: inject rename failures via `rename_fn`.
pub fn quarantine_cancelled_volumes_with<F>(
    out: &Path,
    volume_count: u32,
    rename_fn: F,
) -> crate::export_outcome::QuarantineResult
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let stamp = quarantine_utc_stamp();
    quarantine_cancelled_volumes_with_stamp(out, volume_count, &stamp, rename_fn)
}

/// Quarantine with an injectable stamp (tests: same-second collision safety).
pub fn quarantine_cancelled_volumes_with_stamp<F>(
    out: &Path,
    volume_count: u32,
    stamp: &str,
    mut rename_fn: F,
) -> crate::export_outcome::QuarantineResult
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    use crate::export_outcome::QuarantineResult;
    if volume_count == 0 {
        // Still check primary out if present (mid-write may not have pushed a volume row).
        if !out.exists() {
            return QuarantineResult::NoVolumes;
        }
    }
    let mut attempted = 0u32;
    let mut any_fail = false;
    let mut any_ok = false;
    let max_idx = volume_count.max(1);
    for idx in 1..=max_idx {
        let path = volume_path_for(out, idx);
        if !path.exists() {
            continue;
        }
        attempted += 1;
        let quarantined = cancelled_partial_path(&path, stamp);
        match rename_fn(&path, &quarantined) {
            Ok(()) => any_ok = true,
            Err(e) => {
                any_fail = true;
                tracing::warn!(
                    from = %path.display(),
                    to = %quarantined.display(),
                    "cancel quarantine rename failed: {e}"
                );
            }
        }
    }
    if attempted == 0 {
        QuarantineResult::NoVolumes
    } else if any_fail {
        QuarantineResult::Failed
    } else if any_ok {
        QuarantineResult::Succeeded
    } else {
        QuarantineResult::NoVolumes
    }
}

/// Collision-resistant quarantine destination: never overwrites an existing partial.
///
/// Form: `{filename}.cancelled-{stamp}.partial`, then
/// `{filename}.cancelled-{stamp}_2.partial`, `_3`, … if the name is taken.
fn cancelled_partial_path(volume: &Path, stamp: &str) -> PathBuf {
    let parent = volume.parent().unwrap_or_else(|| Path::new("."));
    let name = volume
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unique.pst".into());
    let primary = parent.join(format!("{name}.cancelled-{stamp}.partial"));
    if !primary.exists() {
        return primary;
    }
    let mut n = 2u32;
    loop {
        let alt = parent.join(format!("{name}.cancelled-{stamp}_{n}.partial"));
        if !alt.exists() {
            return alt;
        }
        n = n.saturating_add(1);
        if n > 10_000 {
            // Pathological: fall back to process-unique suffix so rename can still proceed.
            let pid = std::process::id();
            return parent.join(format!("{name}.cancelled-{stamp}_{n}_{pid}.partial"));
        }
    }
}

/// Stamp with sub-second resolution: `{unix_secs}-{millis}` (e.g. `1720000000-042`).
fn quarantine_utc_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{:03}", d.as_secs(), d.subsec_millis())
}

fn cancelled_outcome(
    report_dir: PathBuf,
    summary_path: PathBuf,
    out: PathBuf,
    artifact_state: crate::export_outcome::ArtifactState,
) -> UniquePstOutcome {
    use crate::error::CliExit;
    use crate::export_outcome::{reason, ExportFidelity};
    UniquePstOutcome {
        ok: false,
        cancelled: true,
        report_dir,
        summary_path,
        out,
        messages_written_total: 0,
        unique: 0,
        volume_count: 0,
        volumes: vec![],
        error_message: Some("cancelled".into()),
        export_risk: dedup_engine::integrity::PreflightRecommendation::Ok,
        exit: CliExit::Cancelled,
        fidelity: ExportFidelity::Failed,
        exit_reasons: vec![reason::CANCELLED],
        artifact_state,
    }
}

/// Minimal cancelled `summary.json` when cancel hits after report-dir prepare but
/// before a full report pack can be built (pre-scan / early exit).
fn write_cancelled_summary_json(ctx: &CancelledSummaryCtx<'_>) {
    let preflight = compute_preflight(&PreflightInputs::without_attach_probe(
        ctx.mode,
        0,
        0,
        0,
        0,
        ctx.inputs.len() as u64,
        IntegrityThresholds::default(),
    ));
    let scan = crate::scan::ScanSummary {
        schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: ctx.mode,
        files: vec![],
        total_messages: 0,
        unique: 0,
        duplicates: 0,
        tier1_hits: 0,
        tier2_hits: 0,
        savings_bytes: 0,
        skipped: 0,
        skipped_by_reason: std::collections::BTreeMap::new(),
        recoverable_messages: 0,
        degraded_messages: 0,
        degraded_by_reason: std::collections::BTreeMap::new(),
        orphaned_messages: 0,
        failed_files: 0,
        partial_files: 0,
        opened_files: 0,
        duration_secs: 0.0,
        preflight,
        skips: vec![],
        integrity_csv: None,
        grouping: Default::default(),
        page_crc_mismatches: 0,
        block_crc_mismatches: 0,
        block_bid_mismatches: 0,
        distinct_bad_bids: 0,
        distinct_bad_bids_exact: true,
        crc_suspect_messages: 0,
        page_reads: 0,
        block_reads: 0,
        block_crc_rate: 0.0,
        block_crc_read_rate: 0.0,
        poly_class_crc_sources: 0,
    };
    let keep_set = KeepSet {
        schema: KEEP_SET_SCHEMA.to_string(),
        policy: ctx.policy,
        family_policy: ctx.family_policy,
        created_from: None,
        identity_level: None,
        dedupe_scope: None,
        winners: vec![],
        stats: KeepSetStats::default(),
    };
    let export_risk = crate::unique_export_report::compute_export_risk(
        &scan.preflight.recommendation,
        &crate::unique_export_report::ExportRiskInputs {
            attach_fail_rate: 0.0,
            block_crc_rate: 0.0,
            block_crc_read_rate: 0.0,
            degraded_winner_rate: 0.0,
            partial: true,
            failed_volume_index: None,
            scan_recommendation: scan.preflight.recommendation,
            attach_stream_crc_events: 0,
        },
    );
    let summary_abs =
        std::path::absolute(ctx.summary_path).unwrap_or_else(|_| ctx.summary_path.to_path_buf());
    let summary = UniqueExportSummary {
        schema: UNIQUE_EXPORT_REPORT_SCHEMA.to_string(),
        ok: false,
        fidelity: crate::export_outcome::ExportFidelity::Failed,
        exit_code: crate::error::CliExit::Cancelled.as_u8(),
        exit_reason: vec![crate::export_outcome::reason::CANCELLED.to_string()],
        artifact_state: ctx.artifact_state,
        summary_path: summary_abs.display().to_string(),
        inputs: ctx.inputs.iter().map(|p| p.display().to_string()).collect(),
        policy: ctx.policy.as_str().to_string(),
        family_policy: ctx.family_policy.as_str().to_string(),
        mode: ctx.mode.as_str().to_string(),
        folder_layout: ctx.folder_layout.as_str().to_string(),
        out: ctx.out.display().to_string(),
        report_dir: ctx.report_dir.display().to_string(),
        keep_set,
        scan,
        export: ExportSection {
            volumes: vec![],
            partial: true,
            messages_written_total: 0,
            attachments_written: 0,
            attachments_failed: 0,
            attachments_omitted_by_policy: None,
            attachments_failed_by_reason: None,
            attachment_ledger: None,
            attachment_ledger_mode: None,
            attachment_ledger_truncated: None,
            attachment_ledger_rows_written: None,
            error: Some("cancelled".into()),
            failed_volume_index: None,
            attachment_fidelity_events_truncated: None,
            attachment_fidelity_events_total: None,
            include_bcc_recipients: false,
        },
        verification: VerificationReport {
            ok: false,
            volumes: vec![],
            rehash_ran: false,
        },
        duration_ms: ctx.duration_ms,
        phase_timings: ctx.phase_timings,
        source_pst_opens: ctx.source_pst_opens,
        messages_materialized: ctx.messages_materialized,
        bytes_written_total: 0,
        prepared_bytes_peak: 0,
        hash_ms: 0,
        max_volume_bytes: ctx.max_volume_bytes,
        decision_csv: None,
        keep_set_json: None,
        error: Some(SummaryError {
            code: "cancelled".into(),
            message: "cancelled".into(),
        }),
        export_risk,
        bcc_suppressed_message_count: 0,
        sent_message_with_no_recipients_count: 0,
        // Cancel is a retryable class (0082 D-0078-retryable).
        retryable: true,
        promote_on_attach_fail: ctx.promote_on_attach_fail,
        promoted_after_attach_incomplete_count: 0,
        mode_c_fallback_all_peers_incomplete_count: 0,
        messages_with_body_cloud_links: 0,
        body_cloud_links_total: 0,
        body_cloud_link_truncated_messages: 0,
    };
    if let Err(e) = write_summary_json(ctx.summary_path, &summary) {
        tracing::warn!(
            path = %ctx.summary_path.display(),
            "cancelled summary.json write failed: {e}"
        );
    }
}

/// Iterator that moves `WriteMessage`s out of a prepared slice (for early finalize).
struct TakeWriteMsgs<'a> {
    slice: &'a mut [PreparedWinner],
    pos: usize,
}

impl Iterator for TakeWriteMsgs<'_> {
    type Item = WriteMessage;

    fn next(&mut self) -> Option<WriteMessage> {
        if self.pos >= self.slice.len() {
            return None;
        }
        let msg = std::mem::take(&mut self.slice[self.pos].write_msg);
        self.pos += 1;
        Some(msg)
    }
}

/// Run unique-pst orchestration end-to-end (CLI entry).
///
/// Defaults to stderr stage lines; returns classified [`CliExit`] (0078).
pub fn run_unique_pst(args: UniquePstCliArgs) -> Result<crate::error::CliExit> {
    let json = args.json;
    let stderr_progress = true;
    let outcome = run_unique_pst_with_options(
        args,
        UniquePstRunOptions {
            cancel: None,
            stderr_progress,
            on_progress: None,
            on_log: None,
        },
    )?;
    // Human mode: point operators at the summary on any non-zero exit (0078).
    if !json && outcome.exit != crate::error::CliExit::Success && stderr_progress {
        let abs = std::path::absolute(&outcome.summary_path)
            .unwrap_or_else(|_| outcome.summary_path.clone());
        let _ = writeln!(std::io::stderr(), "summary: {}", abs.display());
    }
    Ok(outcome.exit)
}

/// Library / GUI entry: same orchestration as CLI with cancel, progress, and log hooks.
///
/// Returns a structured [`UniquePstOutcome`] even on soft cancel/partial when the
/// report pack could be flushed. Hard usage/path errors still return [`Err`].
///
/// When `args.json` is true, the summary is printed to stdout (CLI contract). On
/// JSON failure the function returns [`CliError::AlreadyEmitted`] after printing.
pub fn run_unique_pst_with_options(
    args: UniquePstCliArgs,
    run_opts: UniquePstRunOptions,
) -> Result<UniquePstOutcome> {
    let started = Instant::now();
    let cancel = run_opts.cancel.clone();
    let stderr = run_opts.stderr_progress;
    let on_progress = run_opts.on_progress.map(|f| Arc::new(Mutex::new(f)));
    let on_log = run_opts.on_log.map(|f| Arc::new(Mutex::new(f)));

    pst_reader::integrity_telemetry::set_log_limit(
        args.crc_log_limit,
        std::time::Duration::from_secs(args.crc_log_interval_secs),
    );

    // ── Phase 0: resolve paths, guards, prepare report-dir ──────────────────
    let mut paths = resolve_pst_paths(&args.paths)?;
    sort_input_paths(&mut paths);

    let out = resolve_cli_path_maybe_missing(&args.out)?.into_std_path_buf();
    if out
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| !e.eq_ignore_ascii_case("pst"))
        .unwrap_or(true)
    {
        // Soft warn only — allow any extension if caller insists.
        tracing::warn!(path = %out.display(), "unique-pst --out does not end in .pst");
        emit_log(stderr, &on_log, "warning: --out does not end in .pst");
    }

    let report_dir = match &args.report_dir {
        Some(p) => resolve_cli_path_maybe_missing(p)?.into_std_path_buf(),
        None => default_report_dir(&out),
    };

    let decision_csv = match &args.decision_csv {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => Some(report_dir.join("decisions.csv")),
    };
    let keep_set_json = match &args.keep_set_json {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => Some(report_dir.join("keepset.json")),
    };
    let integrity_csv = match &args.integrity_csv {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => None,
    };

    if let Some(eml) = &args.also_eml {
        let _ = resolve_cli_path_maybe_missing(eml)?;
        // Soft residual: co-export not implemented in this track.
        tracing::warn!(
            path = %eml.display(),
            "--also-eml is accepted but not implemented (D-0071-also-eml residual); ignoring"
        );
        emit_log(
            stderr,
            &on_log,
            &format!(
                "warning: --also-eml is accepted but not implemented (D-0071-also-eml residual); ignoring {}",
                eml.display()
            ),
        );
    }

    guard_unique_pst_paths(
        &paths,
        &out,
        &report_dir,
        decision_csv.as_deref(),
        keep_set_json.as_deref(),
        integrity_csv.as_deref(),
    )?;

    // Refuse existing primary out without --overwrite.
    if out.exists() && !args.overwrite {
        return Err(CliError::Usage(format!(
            "--out already exists (pass --overwrite to replace): {}",
            out.display()
        )));
    }

    prepare_report_dir(&report_dir, args.overwrite)?;

    // Remove stale primary out if overwrite.
    if out.exists() && args.overwrite {
        // Re-check collision after report-dir prep (inputs must never be deleted).
        if path_collides_with_inputs(&out, &paths) {
            return Err(CliError::Usage(format!(
                "refusing to overwrite --out that equals an input PST: {}",
                out.display()
            )));
        }
        if out.is_file() {
            fs::remove_file(&out).map_err(|e| {
                CliError::Msg(format!("remove existing --out {}: {e}", out.display()))
            })?;
        } else {
            return Err(CliError::Usage(format!(
                "--out exists and is not a file: {}",
                out.display()
            )));
        }
    }
    // Clear stale multi-volume siblings on overwrite so prior runs don't linger.
    // Never deletes paths that equal/contain inputs; refuses if any sibling collides.
    if args.overwrite {
        clear_stale_volume_siblings(&out, &paths)?;
    }

    let summary_path = report_dir.join("summary.json");
    let mut cancelled = false;
    let mut phase_timings = PhaseTimings::default();
    let mut source_pst_opens = 0u64;
    let mut messages_materialized = 0u64;
    let mut prepared_bytes_peak = 0u64;
    let mut hash_ms = 0u64;

    emit_log(stderr, &on_log, "stage=scan");
    emit_stage_progress(&on_progress, "scan", 0, 0, 0, 0, None);

    if cancel_requested(&cancel) {
        // Report dir already prepared — write a minimal cancelled summary so
        // Open report / operators see ok=false rather than a missing summary.
        emit_log(stderr, &on_log, "cancelled before scan");
        let artifact_state = crate::export_outcome::ArtifactState::Absent;
        let total_ms = started.elapsed().as_millis() as u64;
        phase_timings.finalize(total_ms);
        write_cancelled_summary_json(&CancelledSummaryCtx {
            summary_path: &summary_path,
            inputs: &paths,
            out: &out,
            report_dir: &report_dir,
            policy: args.policy,
            family_policy: args.family_policy,
            mode: args.mode,
            folder_layout: args.folder_layout,
            max_volume_bytes: args.max_volume_bytes,
            duration_ms: total_ms,
            phase_timings,
            source_pst_opens,
            messages_materialized,
            artifact_state,
            promote_on_attach_fail: args.promote_on_attach_fail,
        });
        return Ok(cancelled_outcome(
            report_dir,
            summary_path,
            out,
            artifact_state,
        ));
    }

    let opts = ScanOptions {
        enable_tier2: !args.no_tier2,
        include_attachments: !args.no_attachments,
        mode: args.mode,
        thresholds: IntegrityThresholds {
            max_skip_rate: args.max_skip_rate,
            max_crc_skip_rate: args.max_crc_skip_rate,
            max_failed_file_rate: args.max_failed_file_rate,
            max_attach_fail_rate: args.max_attach_fail_rate,
        },
        allow_failed_files: args.allow_failed_files,
        integrity_csv: integrity_csv.clone(),
        csv: None,
        skip_limit: args.skip_limit,
        retain_rows: false,
        retain_candidates: true,
        // Cooperative cancel checked between files/folders/messages in run_scan.
        cancel: cancel.clone(),
        // Unique-pst runs group-aware peer-capped probe after scan (not flat scan probe).
        deep_attach_preflight: false,
        deep_attach_level: args.deep_attach_level.clone(),
        deep_attach_max_attaches: args.deep_attach_max_attaches,
        deep_attach_max_probe_bytes: args.deep_attach_max_probe_bytes,
        deep_attach_per_attach_max_bytes: args.deep_attach_per_attach_max_bytes,
        deep_attach_max_probe_time_ms: args.deep_attach_max_probe_time_ms,
        deep_attach_max_open_psts: args.deep_attach_max_open_psts,
        deep_attach_max_peer_probes_per_group: args.deep_attach_max_peer_probes,
        grouping: crate::grouping_cli::grouping_context_from_cli(
            args.no_tier2,
            &args.strong_content_hash,
            &args.dedupe_scope,
            &args.tier1_verify,
            args.allow_cross_mid_tier2,
            args.allow_degenerate_tier2,
            args.allow_crc_suspect_tier2,
            args.tier1_backfill,
            args.identity_ignore_inline_attachments,
            args.no_attachments,
        )
        .map_err(CliError::Usage)?,
        strong_hash_attach_max_attaches: args.strong_hash_attach_max_attaches,
        strong_hash_attach_max_bytes: args.strong_hash_attach_max_bytes,
        strong_hash_attach_per_attach_max_bytes: args.strong_hash_attach_per_attach_max_bytes,
    };

    // ── Phase 1: integrity scan ─────────────────────────────────────────────
    // Dual-rate poly sources reclassify (clear) false-positive CRC_SUSPECT in
    // run_scan so keep-set sees clean identity without Tier-2 auto-allow.
    let t_scan = Instant::now();
    let mut outcome = run_scan(&paths, &opts)?;
    phase_timings.scan_ms = t_scan.elapsed().as_millis() as u64;

    // Scan-level integrity warnings must reach on_log (GUI Log panel), not only tracing.
    {
        let s = &outcome.summary;
        if s.skipped > 0 || s.degraded_messages > 0 || s.failed_files > 0 || s.partial_files > 0 {
            emit_log(
                stderr,
                &on_log,
                &format!(
                    "warning: scan integrity degraded/skips: skipped={} degraded={} failed_files={} partial_files={}",
                    s.skipped, s.degraded_messages, s.failed_files, s.partial_files
                ),
            );
        }
        if !s.degraded_by_reason.is_empty() {
            let reasons: Vec<String> = s
                .degraded_by_reason
                .iter()
                .map(|(k, n)| format!("{k}={n}"))
                .collect();
            emit_log(
                stderr,
                &on_log,
                &format!("warning: scan degraded_by_reason: {}", reasons.join(", ")),
            );
        }
        if !s.skipped_by_reason.is_empty() {
            let reasons: Vec<String> = s
                .skipped_by_reason
                .iter()
                .map(|(k, n)| format!("{k}={n}"))
                .collect();
            emit_log(
                stderr,
                &on_log,
                &format!("warning: scan skipped_by_reason: {}", reasons.join(", ")),
            );
        }
    }

    if cancel_requested(&cancel) {
        cancelled = true;
        emit_log(stderr, &on_log, "cancelled after scan");
        // Continue to resolve so report pack can still be honest if we get far enough;
        // write loop will skip when cancelled.
    }

    let provenance = KeepSetProvenance {
        scan_integrity_schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: args.mode.as_str().to_string(),
        input_files: paths.iter().map(|p| p.display().to_string()).collect(),
    };

    // ── Phase 1b: deep attach preflight (winner/group path, 0074) ───────────
    // Opt-in; skipped for parents_only / --no-attachments. Peer-capped per group.
    let effective_family_for_probe = if args.no_attachments {
        FamilyPolicy::ParentsOnly
    } else {
        args.family_policy
    };
    // Phase-1b result cache for materializer stream_available (no re-I/O).
    let mut phase1b_probe_cache: Option<(
        crate::attach_probe::ProbeResultCache,
        crate::attach_probe::ProbeLevel,
    )> = None;
    if args.deep_attach_preflight
        && effective_family_for_probe == FamilyPolicy::KeepAttachmentsWithParent
    {
        use crate::attach_probe::{
            probe_keep_set_groups, KeepSetProbeOpts, ProbeBudgets, ProbeLevel, ProbeProgressCb,
        };
        let level = ProbeLevel::parse(&args.deep_attach_level).unwrap_or(ProbeLevel::Head);

        // Cancel between scan and probe: mark attach_probe incomplete (0074 P1-D).
        if cancel_requested(&cancel) {
            emit_log(stderr, &on_log, "cancelled before deep_attach_preflight");
            let crc_skips = outcome
                .summary
                .skipped_by_reason
                .get(IntegrityReason::CrcMismatch.as_str())
                .copied()
                .unwrap_or(0);
            let mut thresholds = outcome.summary.preflight.thresholds;
            thresholds.max_attach_fail_rate = args.max_attach_fail_rate;
            outcome.summary.preflight = compute_preflight(&PreflightInputs {
                mode: args.mode,
                recoverable: outcome.summary.recoverable_messages,
                skipped: outcome.summary.skipped,
                crc_skips,
                failed_files: outcome.summary.failed_files,
                input_file_count: paths.len() as u64,
                thresholds,
                attach_probe_enabled: true,
                attach_probe_level: level.as_str().to_string(),
                attach_attempted: 0,
                attach_failed: 0,
                attach_probe_truncated: false,
                peer_probe_capped_groups: 0,
                attach_probe_cancelled: true,
            });
            let artifact_state = crate::export_outcome::ArtifactState::Absent;
            let total_ms = started.elapsed().as_millis() as u64;
            phase_timings.finalize(total_ms);
            write_cancelled_summary_json(&CancelledSummaryCtx {
                summary_path: &summary_path,
                inputs: &paths,
                out: &out,
                report_dir: &report_dir,
                policy: args.policy,
                family_policy: args.family_policy,
                mode: args.mode,
                folder_layout: args.folder_layout,
                max_volume_bytes: args.max_volume_bytes,
                duration_ms: total_ms,
                phase_timings,
                source_pst_opens,
                messages_materialized,
                artifact_state,
                promote_on_attach_fail: args.promote_on_attach_fail,
            });
            return Ok(cancelled_outcome(
                report_dir,
                summary_path,
                out,
                artifact_state,
            ));
        }

        emit_log(stderr, &on_log, "stage=deep_attach_preflight");
        emit_stage_progress(&on_progress, "deep_attach_preflight", 0, 0, 0, 0, None);
        let t_preflight = Instant::now();
        let budgets = ProbeBudgets {
            max_attaches: args.deep_attach_max_attaches,
            max_probe_bytes: args.deep_attach_max_probe_bytes,
            per_attach_max_bytes: args.deep_attach_per_attach_max_bytes,
            max_probe_time_ms: args.deep_attach_max_probe_time_ms,
            max_open_psts: args.deep_attach_max_open_psts,
            max_peer_probes_per_group: args.deep_attach_max_peer_probes,
        };
        let log_for_progress = on_log.clone();
        let stderr_p = stderr;
        let progress_cb: Option<ProbeProgressCb> = Some(Box::new(move |attempted, bytes, base| {
            if attempted.is_multiple_of(500) || attempted == 1 {
                let line = format!(
                    "deep-attach-preflight: attempted={attempted} bytes={bytes} source={base}"
                );
                if stderr_p {
                    let _ = writeln!(std::io::stderr(), "unique-pst: {line}");
                }
                if let Some(log) = &log_for_progress {
                    if let Ok(mut g) = log.lock() {
                        g(format!("unique-pst: {line}"));
                    }
                }
            }
        }));
        let (probe_summary, probe_cache) = probe_keep_set_groups(
            &mut outcome.candidates,
            KeepSetProbeOpts {
                budgets,
                level,
                policy: args.policy,
                family: effective_family_for_probe,
                prefer_path: &args.prefer_path_contains,
                grouping: opts.grouping.clone(),
                mode: args.mode,
                cancel: cancel.clone(),
                progress: progress_cb,
            },
        );
        phase1b_probe_cache = Some((probe_cache, level));
        phase_timings.deep_attach_preflight_ms = t_preflight.elapsed().as_millis() as u64;

        // Cancel during probe must not resolve/materialize/write with partial integrity.
        if probe_summary.cancelled || cancel_requested(&cancel) {
            emit_log(stderr, &on_log, "cancelled during deep_attach_preflight");
            let artifact_state = crate::export_outcome::ArtifactState::Absent;
            let total_ms = started.elapsed().as_millis() as u64;
            phase_timings.finalize(total_ms);
            write_cancelled_summary_json(&CancelledSummaryCtx {
                summary_path: &summary_path,
                inputs: &paths,
                out: &out,
                report_dir: &report_dir,
                policy: args.policy,
                family_policy: args.family_policy,
                mode: args.mode,
                folder_layout: args.folder_layout,
                max_volume_bytes: args.max_volume_bytes,
                duration_ms: total_ms,
                phase_timings,
                source_pst_opens,
                messages_materialized,
                artifact_state,
                promote_on_attach_fail: args.promote_on_attach_fail,
            });
            return Ok(cancelled_outcome(
                report_dir,
                summary_path,
                out,
                artifact_state,
            ));
        }

        // Strict: probe fails must not win — remove from recoverable candidates (skip).
        if args.mode == ScanMode::Strict {
            use dedup_engine::integrity::{
                tally_reason, IntegrityCsvWriter, IntegrityLedgerWriter,
            };
            use dedup_engine::SkipRecord;
            let mut probe_skips: Vec<SkipRecord> = Vec::new();
            outcome.candidates.retain(|c| {
                if let Some(r) = c
                    .integrity
                    .degraded_reasons
                    .iter()
                    .copied()
                    .find(|r| r.is_attach_probe_fail())
                {
                    probe_skips.push(SkipRecord {
                        source_path: c.locus.source_path.clone(),
                        source_pst: c.locus.source_pst.clone(),
                        folder_path: c.locus.folder_path.clone(),
                        is_orphaned: c.locus.is_orphaned,
                        nid: c.locus.nid,
                        reason: r,
                        detail: format!("strict deep-attach-preflight skip: {}", r.as_str()),
                        mode: args.mode,
                    });
                    false
                } else {
                    true
                }
            });
            let skipped_probe = probe_skips.len() as u64;
            for skip in &probe_skips {
                tally_reason(&mut outcome.summary.skipped_by_reason, skip.reason);
                if outcome.summary.skips.len() < args.skip_limit {
                    outcome.summary.skips.push(skip.clone());
                }
            }
            outcome.summary.skipped = outcome.summary.skipped.saturating_add(skipped_probe);
            // Per-file tallies: skipped/messages/recoverable/status must match aggregate.
            apply_strict_probe_skips_to_file_stats(&mut outcome.summary.files, &probe_skips);
            // Append integrity CSV rows when path was requested (scan already closed its writer).
            if let Some(path) = integrity_csv.as_ref() {
                if !probe_skips.is_empty() {
                    match IntegrityCsvWriter::open_append(path) {
                        Ok(mut wtr) => {
                            for skip in &probe_skips {
                                if let Err(e) = wtr.write_skip(skip) {
                                    tracing::warn!(
                                        path = %path.display(),
                                        error = %e,
                                        "failed to append deep-probe strict skip to integrity CSV"
                                    );
                                    break;
                                }
                            }
                            let _ = wtr.flush();
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "failed to open integrity CSV for deep-probe strict skip append"
                            );
                        }
                    }
                }
            }
            // Reconcile recoverable/unique/dup so preflight is not pre-probe stale.
            // Use the same GroupingContext as scan/resolve (0076 guards), not
            // tier2-only defaults — otherwise deep-attach rebuild can re-bind
            // under different identity rules than the keep-set path.
            outcome.summary.recoverable_messages = outcome.candidates.len() as u64;
            outcome.summary.total_messages = outcome.summary.recoverable_messages;
            let empty_refs = std::collections::HashMap::new();
            let rebuild =
                rebuild_dedup_results_with_ctx(&outcome.candidates, &empty_refs, &opts.grouping);
            outcome.summary.unique = rebuild.unique_count;
            outcome.summary.duplicates = rebuild.duplicate_count;
            outcome.summary.tier1_hits = rebuild.tier1_hits;
            outcome.summary.tier2_hits = rebuild.tier2_hits;
            outcome.summary.savings_bytes = rebuild.total_savings;
            recompute_per_file_dup_from_results(&mut outcome.summary.files, &rebuild.results);
            let (partial, opened) = recompute_file_status_counts(&outcome.summary.files);
            outcome.summary.partial_files = partial;
            outcome.summary.opened_files = opened;
        }

        // Recompute degraded tallies after probe (honest for newly probe-degraded).
        {
            use dedup_engine::integrity::tally_reason;
            let mut degraded_messages = 0u64;
            let mut degraded_by_reason = std::collections::BTreeMap::new();
            for c in &outcome.candidates {
                if c.integrity.degraded {
                    degraded_messages += 1;
                    for r in &c.integrity.degraded_reasons {
                        tally_reason(&mut degraded_by_reason, *r);
                    }
                }
            }
            outcome.summary.degraded_messages = degraded_messages;
            outcome.summary.degraded_by_reason = degraded_by_reason;
            // Per-file degraded must match aggregate (best-effort + residual after strict).
            recompute_per_file_degraded_from_candidates(
                &mut outcome.summary.files,
                &outcome.candidates,
            );
            let (partial, opened) = recompute_file_status_counts(&outcome.summary.files);
            outcome.summary.partial_files = partial;
            outcome.summary.opened_files = opened;
        }

        // Full preflight recompute with updated skipped/recoverable + attach probe tallies.
        let crc_skips = outcome
            .summary
            .skipped_by_reason
            .get(IntegrityReason::CrcMismatch.as_str())
            .copied()
            .unwrap_or(0);
        let mut thresholds = outcome.summary.preflight.thresholds;
        thresholds.max_attach_fail_rate = args.max_attach_fail_rate;
        outcome.summary.preflight = compute_preflight(&PreflightInputs {
            mode: args.mode,
            recoverable: outcome.summary.recoverable_messages,
            skipped: outcome.summary.skipped,
            crc_skips,
            failed_files: outcome.summary.failed_files,
            input_file_count: paths.len() as u64,
            thresholds,
            attach_probe_enabled: true,
            attach_probe_level: level.as_str().to_string(),
            attach_attempted: probe_summary.attempted,
            attach_failed: probe_summary.failed,
            attach_probe_truncated: probe_summary.truncated,
            peer_probe_capped_groups: probe_summary.peer_probe_capped_groups,
            attach_probe_cancelled: probe_summary.cancelled,
        });
        if probe_summary.attempted > 0 || probe_summary.truncated || probe_summary.cancelled {
            emit_log(
                stderr,
                &on_log,
                &format!(
                    "deep-attach-preflight: attempted={} failed={} truncated={} cancelled={} peer_capped_groups={} recommendation={}",
                    probe_summary.attempted,
                    probe_summary.failed,
                    outcome.summary.preflight.attach_probe.truncated,
                    probe_summary.cancelled,
                    probe_summary.peer_probe_capped_groups,
                    outcome.summary.preflight.recommendation.as_str()
                ),
            );
        }
    }

    // ── Phase 2 / 2b: resolve + promote ─────────────────────────────────────
    emit_log(stderr, &on_log, "stage=resolve");
    emit_stage_progress(&on_progress, "resolve", 0, 0, 0, 0, None);
    let t_resolve = Instant::now();
    let rank_ctx = rank_context_from_cli(
        args.policy,
        &args.prefer_path_contains,
        args.prefer_bcc_copy,
        args.prefer_folder_class,
        &args.folder_rank,
        &args.source_rank,
        args.rank_folder_class_first,
        &args.fidelity_rank,
    );
    let grouping = crate::grouping_cli::grouping_context_from_cli(
        args.no_tier2,
        &args.strong_content_hash,
        &args.dedupe_scope,
        &args.tier1_verify,
        args.allow_cross_mid_tier2,
        args.allow_degenerate_tier2,
        args.allow_crc_suspect_tier2,
        args.tier1_backfill,
        args.identity_ignore_inline_attachments,
        args.no_attachments,
    )
    .map_err(CliError::Usage)?;
    let mut resolved = resolve_groups_with_grouping(
        outcome.candidates,
        args.family_policy,
        &rank_ctx,
        &grouping,
        Some(provenance),
    );
    phase_timings.resolve_ms = t_resolve.elapsed().as_millis() as u64;

    if cancel_requested(&cancel) {
        cancelled = true;
        emit_log(stderr, &on_log, "cancelled after resolve");
    }

    emit_log(stderr, &on_log, "stage=materialize");
    emit_stage_progress(&on_progress, "materialize", 0, 0, 0, 0, None);
    // `--no-attachments` forces parents-only materialize/write so attach streams
    // are not opened (scan already omitted attach metadata when the flag is set).
    let effective_family = if args.no_attachments {
        FamilyPolicy::ParentsOnly
    } else {
        args.family_policy
    };
    // Bridge materializer soft attach/open warnings into on_log (GUI Details parity).
    let mat_warn: Option<crate::pst_materializer::MaterializeWarnCb> = on_log.as_ref().map(|log| {
        let log = Arc::clone(log);
        let stderr_w = stderr;
        Arc::new(Mutex::new(move |msg: String| {
            // emit_log style without re-locking options: prefix for GUI filter.
            let line = format!("unique-pst: {msg}");
            if stderr_w {
                let _ = writeln!(std::io::stderr(), "{line}");
            }
            if let Ok(mut g) = log.lock() {
                g(line);
            }
        })) as crate::pst_materializer::MaterializeWarnCb
    });
    // Phase 1b already ran the budgeted deep probe. Pass the result cache so
    // materialize sets stream_available from probe outcomes without re-I/O
    // (0074 P1-A). Unprobed attaches stay optimistic (honest via truncated).
    // Residual mid-tail fails go to the 0073 export ledger.
    // 0079: one bounded LRU shared by materializer + attach stream (D-0074-mat-lru).
    let handle_cache = Rc::new(RefCell::new(PstHandleCache::new(args.max_open_psts)));
    let mut mat = match mat_warn {
        Some(cb) => PstMaterializer::with_handle_cache(effective_family, Rc::clone(&handle_cache))
            .with_warn_sink(cb),
        None => PstMaterializer::with_handle_cache(effective_family, Rc::clone(&handle_cache)),
    };
    if let Some((cache, level)) = phase1b_probe_cache {
        mat = mat.with_probe_result_cache(cache, level);
    }
    let mut attach_src = PstAttachStreamSource::with_handle_cache(Rc::clone(&handle_cache));

    // 0079 D1: convert each winner to PreparedWinner in on_winner (single materialize).
    // Keyed by (source_path, nid); write order still follows keep_set.winners (item index).
    let mut prepared_by_locus: HashMap<(String, u64), PreparedWinner> = HashMap::new();
    let t_materialize = Instant::now();
    let mat_opts = MaterializeFinalizeOpts {
        promote_on_attach_fail: args.promote_on_attach_fail,
    };
    let materialized_count =
        finalize_with_materialize_opts(&mut resolved, &mut mat, &mat_opts, &mut |msg| {
            let key = (msg.locus.source_path.clone(), msg.locus.nid);
            match prepared_winner_from_canonical(msg) {
                Ok(p) => {
                    prepared_by_locus.insert(key, p);
                    Ok(())
                }
                Err(e) => Err(dedup_engine::keepset::KeepSetError::Other(e)),
            }
        })
        .map_err(|e| CliError::Msg(format!("materialize/promote: {e}")))?;
    phase_timings.materialize_ms = t_materialize.elapsed().as_millis() as u64;
    messages_materialized = mat.messages_materialized();
    // finalize count and materializer counter should agree.
    let _ = materialized_count;

    if cancel_requested(&cancel) {
        cancelled = true;
        emit_log(stderr, &on_log, "cancelled after materialize");
    }

    let keep_set = resolved.to_keep_set();
    if let Some(hint) = recoverable_items_hint(keep_set.stats.winners_from_recoverable_items) {
        emit_log(stderr, &on_log, &format!("note: {hint}"));
    }
    let winners_total = Some(keep_set.stats.unique);

    // Assemble prepared winners in keep_set (item index) order — no re-materialize.
    emit_log(stderr, &on_log, "stage=prepare_winners");
    emit_stage_progress(&on_progress, "prepare_winners", 0, 0, 0, 0, winners_total);
    let t_prepare = Instant::now();
    let mut prepared: Vec<PreparedWinner> = Vec::with_capacity(keep_set.winners.len());
    let mut prepare_errors: Vec<String> = Vec::new();
    for entry in &keep_set.winners {
        if cancel_requested(&cancel) {
            cancelled = true;
            break;
        }
        let key = (entry.locus.source_path.clone(), entry.locus.nid);
        match prepared_by_locus.remove(&key) {
            Some(p) => {
                prepared_bytes_peak =
                    prepared_bytes_peak.saturating_add(prepared_winner_retained_bytes(&p));
                prepared.push(p);
            }
            None => {
                let msg = format!(
                    "nid={:#x}: missing prepared winner after materialize",
                    entry.locus.nid
                );
                emit_log(stderr, &on_log, &format!("warning: prepare error: {msg}"));
                prepare_errors.push(msg);
            }
        }
    }
    phase_timings.prepare_ms = t_prepare.elapsed().as_millis() as u64;
    if prepared_bytes_peak > PREPARED_BYTES_PEAK_WARN_THRESHOLD {
        emit_log(
            stderr,
            &on_log,
            &format!(
                "warning: prepared_bytes_peak={prepared_bytes_peak} exceeds threshold {} (1 GiB); consider streaming prepare→write (D-0079-stream-prepare) when available",
                PREPARED_BYTES_PEAK_WARN_THRESHOLD
            ),
        );
    }
    if !prepare_errors.is_empty() {
        emit_log(
            stderr,
            &on_log,
            &format!("warning: prepare errors total={}", prepare_errors.len()),
        );
    }
    // 0079: prepare is a pure re-order of on_winner materialize output — no second
    // materialize. Missing prepared winners are a hard pipeline defect (or cancel);
    // refuse to write an incomplete keep-set.
    let prepare_incomplete = !prepare_errors.is_empty();

    let folder_layout = match args.folder_layout {
        FolderLayoutArg::Preserve => FolderLayoutPolicy::PreservePaths {
            multi_source_prefix: true,
        },
        FolderLayoutArg::Flat => FolderLayoutPolicy::Flat {
            folder_display_name: "Unique Mail".to_string(),
        },
    };
    let parents_only = effective_family == FamilyPolicy::ParentsOnly || args.no_attachments;

    let write_opts_base = WritePstOpts {
        folder_display_name: "Unique Mail".to_string(),
        folder_layout,
        overwrite: args.overwrite,
        max_embedded_depth: 3,
        parents_only,
        // 0082: default OFF (BCC omit / over-disclosure policy).
        include_bcc_recipients: args.include_bcc_recipients,
    };

    // ── Phase 3: multi-volume streaming write ───────────────────────────────
    // Re-check cancel after prepare (including empty keep-set): write loop body
    // is skipped when `prepared` is empty, so this checkpoint is required.
    if cancel_requested(&cancel) {
        cancelled = true;
        emit_log(stderr, &on_log, "cancelled before write");
    }

    emit_log(stderr, &on_log, "stage=write");
    emit_stage_progress(&on_progress, "write", 0, 0, 0, 0, winners_total);
    let t_write = Instant::now();
    let mut volumes: Vec<VolumeReportRow> = Vec::new();
    let mut export_rows: Vec<ExportMessageRow> = Vec::new();
    // 0085: body-inline document-shaped cloud link hit-list (independent of attach ledger).
    let mut body_cloud_link_rows: Vec<BodyCloudLinkRow> = Vec::new();
    let mut messages_with_body_cloud_links: u64 = 0;
    let mut body_cloud_links_total: u64 = 0;
    let mut body_cloud_link_truncated_messages: u64 = 0;
    // QC sample meta ordered by prepare/write order (export_message_index assigned later).
    let mut qc_meta_by_prepare_idx: Vec<crate::unique_pst_qc::QcSampleCandidate> = prepared
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let (has_degraded, crc_suspect, body_unavailable, body_incomplete) = keep_set
                .winners
                .get(i)
                .map(|w| {
                    use dedup_engine::integrity::IntegrityReason;
                    let reasons = &w.integrity.degraded_reasons;
                    (
                        !reasons.is_empty(),
                        reasons.contains(&IntegrityReason::CrcSuspect),
                        reasons.contains(&IntegrityReason::BodyUnavailable)
                            || p.write_msg.body_unavailable,
                        reasons.contains(&IntegrityReason::BodyTruncated)
                            || p.write_msg.body_incomplete,
                    )
                })
                .unwrap_or((
                    false,
                    false,
                    p.write_msg.body_unavailable,
                    p.write_msg.body_incomplete,
                ));
            let mut meta = crate::unique_pst_qc::candidate_from_write_msg(
                crate::unique_pst_qc::CandidateFromWriteMsg {
                    export_message_index: 0, // filled when export_message_index is known
                    volume_index: 0,
                    source_path: &p.source_path,
                    source_nid: p.nid,
                    folder_path: &p.folder_path,
                    message_id_norm: &p.message_id_norm,
                    subject: &p.subject,
                    write_msg: &p.write_msg,
                    has_degraded,
                    has_ledger_fail: false,
                    display_bcc: &p.display_bcc,
                },
            );
            meta.crc_suspect = crc_suspect;
            meta.body_unavailable = body_unavailable;
            meta.body_incomplete = body_incomplete;
            meta
        })
        .collect();
    let mut export_message_index: u64 = 0;
    let mut attach_written_total: u64 = 0;
    let mut attach_failed_total: u64 = 0;
    let mut attach_omitted_total: u64 = 0;
    // 0077 DoD-11: surface writer attach-event cap totals on ExportSection.
    let mut attach_fidelity_events_total: u64 = 0;
    let mut attach_fidelity_events_truncated = false;
    // 0077 P1-2: final-write ATTACH_STREAM_CRC Info events → export_risk only.
    let mut attach_stream_crc_events: u64 = 0;
    let mut export_partial = false;
    let mut export_error: Option<String> = None;
    // Summary error.code for retryable classification (0082 P2-1).
    // Transient disk/write IO uses write_io; permanent writer failures use export.
    let mut export_error_code: Option<&'static str> = None;
    let mut failed_volume_index: Option<u32> = None;
    let mut cursor = 0usize;
    let mut volume_index: u32 = 0;
    let mut messages_written_prior: u64 = 0;

    let protected: Vec<PathBuf> = paths.clone();
    // 0073: attach ledger (histogram always unless off; CSV when full).
    // Input path strings match `summary.inputs` and in-memory export_messages
    // `source_path` (full). On-disk CSV `source_path` may be basenamed (0081);
    // join origin via `source_id` (0-based index into this list).
    let input_path_strings: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    // Ledger init failure with mode != Off is a report-pack error (fail closed).
    let mut ledger_init_error: Option<String> = None;
    let mut attach_ledger = match AttachLedgerSink::new(
        args.attach_ledger,
        args.attach_ledger_max_rows,
        &report_dir,
        &input_path_strings,
        args.ledger_path_mode,
    ) {
        Ok(s) => Some(s),
        Err(e) => {
            let msg = format!("attach ledger init failed: {e}");
            tracing::warn!("{msg}");
            emit_log(stderr, &on_log, &format!("warning: {msg}"));
            if args.attach_ledger != AttachLedgerMode::Off {
                // Operator requested ledger/histogram — do not silently continue with None.
                ledger_init_error = Some(msg);
            }
            // Off: init can stay None (no CSV/hist expected).
            None
        }
    };

    // 0083 Mode A honesty: mark promoted winners + emit soft-skip incomplete rows.
    if let Some(ledger) = attach_ledger.as_mut() {
        for w in &keep_set.winners {
            if w.promoted_from_failure {
                ledger.mark_promoted_winner(&w.locus.source_path, w.locus.nid);
            }
        }
        for rec in &resolved.soft_skip_attach_records {
            let source_id = crate::unique_export_report::resolve_input_source_id(
                &rec.source_path,
                &input_path_strings,
            );
            let peer_source_id = crate::unique_export_report::resolve_input_source_id(
                &rec.peer_source_path,
                &input_path_strings,
            )
            .map(|id| id.to_string())
            .unwrap_or_default();
            let row = crate::unique_export_report::AttachLedgerRow {
                source_id: source_id.map(|id| id.to_string()).unwrap_or_default(),
                source_path: crate::unique_export_report::format_ledger_source_path(
                    &rec.source_path,
                    args.ledger_path_mode,
                ),
                folder_path: rec.folder_path.clone(),
                msg_nid: rec.msg_nid,
                attach_nid: rec.attach_nid.map(|n| n.to_string()).unwrap_or_default(),
                attach_index: rec.attach_index,
                filename: rec.filename.clone(),
                size: if rec.size == 0 {
                    String::new()
                } else {
                    rec.size.to_string()
                },
                attach_method: rec.attach_method,
                reason_code: rec.reason_code.clone(),
                severity: "fail".into(),
                volume_path: String::new(),
                volume_index: String::new(),
                winner_promoted: true,
                peer_source_id,
                peer_msg_nid: rec.peer_msg_nid.to_string(),
                message_subject: String::new(),
                cloud_provider: rec.cloud_provider.clone(),
                cloud_url: rec.cloud_url.clone(),
            };
            ledger.enqueue_soft_skip_row(row);
        }
    }

    if cancelled {
        export_partial = true;
        export_error = Some("cancelled".into());
    } else if prepare_incomplete {
        // Hard-fail before write (unless cancel already owns the outcome).
        export_partial = true;
        export_error = Some(format!(
            "prepare/materialize errors ({}): {:?} — refusing write with incomplete keep-set",
            prepare_errors.len(),
            prepare_errors
        ));
        emit_log(
            stderr,
            &on_log,
            "error: missing prepared winners after single materialize; write skipped",
        );
    }

    while cursor < prepared.len() && !cancelled && export_error.is_none() {
        if cancel_requested(&cancel) {
            cancelled = true;
            export_partial = true;
            export_error = Some("cancelled".into());
            emit_log(stderr, &on_log, "cancelled before volume write");
            break;
        }

        volume_index += 1;
        let vol_path = volume_path_for(&out, volume_index);

        // Source protection: never write/delete a volume path that collides with input.
        if path_collides_with_inputs(&vol_path, &paths) {
            export_partial = true;
            export_error = Some(format!(
                "refusing volume path equal to an input PST: {}",
                vol_path.display()
            ));
            failed_volume_index = Some(volume_index);
            break;
        }

        // Refuse existing secondary volumes without overwrite.
        if vol_path.exists() && !args.overwrite {
            export_partial = true;
            export_error = Some(format!(
                "volume path already exists (pass --overwrite): {}",
                vol_path.display()
            ));
            failed_volume_index = Some(volume_index);
            break;
        }
        if vol_path.exists() && args.overwrite {
            if path_collides_with_inputs(&vol_path, &paths) {
                export_partial = true;
                export_error = Some(format!(
                    "refusing to overwrite volume path equal to an input PST: {}",
                    vol_path.display()
                ));
                failed_volume_index = Some(volume_index);
                break;
            }
            if vol_path.is_file() {
                if let Err(e) = fs::remove_file(&vol_path) {
                    export_partial = true;
                    export_error = Some(format!(
                        "cannot remove existing volume {}: {e}",
                        vol_path.display()
                    ));
                    export_error_code = Some("write_io");
                    failed_volume_index = Some(volume_index);
                    break;
                }
            } else {
                // Directory or other — will fail create; useful for fail-atomicity tests.
            }
        }

        // Ensure parent exists.
        if let Some(parent) = vol_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                export_partial = true;
                export_error = Some(format!("create volume parent {}: {e}", parent.display()));
                export_error_code = Some("write_io");
                failed_volume_index = Some(volume_index);
                break;
            }
        }

        emit_log(
            stderr,
            &on_log,
            &format!(
                "stage=write_volume volume={volume_index} path={} remaining={}",
                vol_path.display(),
                prepared.len() - cursor
            ),
        );
        emit_stage_progress(
            &on_progress,
            "write",
            volume_index,
            0,
            messages_written_prior,
            0,
            winners_total,
        );

        let mut sink = VolumeProgressSink {
            max_volume_bytes: args.max_volume_bytes,
            volume_index,
            messages_written_prior,
            stderr,
            cancel: cancel.clone(),
            winners_total,
            on_progress: on_progress.clone(),
            on_log: on_log.clone(),
            last_log_at: Instant::now()
                .checked_sub(WRITE_PROGRESS_LOG_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_logged_messages: 0,
            first_write_log: true,
        };
        let mut adapter = WriterAttachAdapter {
            inner: &mut attach_src,
        };

        // Per-volume overwrite: primary may already be cleared; secondary needs true
        // when we just deleted, false when fresh. Writer refuses existing unless overwrite.
        let mut vol_opts = write_opts_base.clone();
        vol_opts.overwrite = true; // we already enforced / deleted

        let remaining = &mut prepared[cursor..];
        let start_cursor = cursor;
        let iter = TakeWriteMsgs {
            slice: remaining,
            pos: 0,
        };

        let vol_path_str = vol_path.display().to_string();
        if let Some(ledger) = attach_ledger.as_mut() {
            ledger.set_volume(&vol_path_str, volume_index);
        }

        // Volume-local buffer: only commit attach events after Ok(report) so a hard
        // volume failure does not pollute CSV / histogram / msg fail counts.
        let mut vol_attach_buf = VolumeAttachBuffer::new();
        let write_result = write_unicode_pst_streaming(
            &vol_path,
            iter,
            &protected,
            &vol_opts,
            Some(&mut adapter),
            Some(&mut sink),
            Some(&mut vol_attach_buf as &mut dyn pst_writer::AttachEventSink),
        );

        match write_result {
            Ok(report) => {
                // Commit volume attach events into global ledger (if any).
                if let Some(ledger) = attach_ledger.as_mut() {
                    vol_attach_buf.commit_into(ledger);
                } else {
                    drop(vol_attach_buf);
                }

                let written = report.messages_written as usize;
                let exceeded = args
                    .max_volume_bytes
                    .map(|max| report.bytes > max)
                    .unwrap_or(false);

                // Export rows for written messages (meta still on prepared[start..]).
                for i in 0..written {
                    let p = &prepared[start_cursor + i];
                    export_message_index += 1;
                    let attach_fails = attach_ledger
                        .as_ref()
                        .map(|l| l.fail_count_for(&p.source_path, p.nid))
                        .unwrap_or(0);
                    let attach_fail_names = attach_ledger
                        .as_ref()
                        .map(|l| l.fail_filenames_for(&p.source_path, p.nid))
                        .unwrap_or_default();
                    // Match keep-set winner for 0075 All-Custodians aggregate.
                    let (dup_count, dup_sources) = keep_set
                        .winners
                        .iter()
                        .find(|w| {
                            w.locus.nid == p.nid
                                && (w.locus.source_path == p.source_path
                                    || w.locus.source_pst == p.source_path
                                    || Path::new(&w.locus.source_path)
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        == Path::new(&p.source_path)
                                            .file_name()
                                            .and_then(|n| n.to_str()))
                        })
                        .map(|w| (w.duplicate_source_count, w.duplicate_sources.join("|")))
                        .unwrap_or((0, String::new()));
                    // source_id: decimal index into inputs, or empty when unmapped (0081).
                    let source_id = crate::unique_export_report::resolve_input_source_id(
                        &p.source_path,
                        &input_path_strings,
                    )
                    .map(|id| id.to_string())
                    .unwrap_or_default();
                    // 0085: body cloud hits were scanned at prepare (before write moves bodies).
                    let body_cloud_link_count = p.body_cloud_hits.len() as u64;
                    if body_cloud_link_count > 0 {
                        messages_with_body_cloud_links =
                            messages_with_body_cloud_links.saturating_add(1);
                        body_cloud_links_total =
                            body_cloud_links_total.saturating_add(body_cloud_link_count);
                        for (link_index, (url, url_source)) in p.body_cloud_hits.iter().enumerate()
                        {
                            body_cloud_link_rows.push(BodyCloudLinkRow {
                                source_id: source_id.clone(),
                                source_path: p.source_path.clone(),
                                folder_path: p.folder_path.clone(),
                                msg_nid: p.nid,
                                link_index: link_index as u32,
                                cloud_url: url.clone(),
                                url_source: url_source.clone(),
                                truncated: false,
                                message_subject: p.subject.clone(),
                                reason: REASON_BODY_CLOUD_LINK.into(),
                            });
                        }
                    }
                    if p.body_cloud_truncated {
                        body_cloud_link_truncated_messages =
                            body_cloud_link_truncated_messages.saturating_add(1);
                        body_cloud_link_rows.push(BodyCloudLinkRow::truncated_marker(
                            source_id.clone(),
                            p.source_path.clone(),
                            p.folder_path.clone(),
                            p.nid,
                            p.subject.clone(),
                        ));
                    }
                    export_rows.push(ExportMessageRow {
                        source_path: p.source_path.clone(),
                        folder_path: p.folder_path.clone(),
                        nid: p.nid,
                        message_id_norm: p.message_id_norm.clone(),
                        edrm_mih: p.edrm_mih.clone(),
                        content_hash_hex: p.content_hash_hex.clone(),
                        volume_path: vol_path_str.clone(),
                        volume_index,
                        export_message_index,
                        attachments_failed_count: attach_fails,
                        duplicate_source_count: dup_count,
                        duplicate_sources: dup_sources,
                        source_id,
                        // true when source had BCC and write path omitted it (0082 rule 7).
                        bcc_suppressed: p.source_has_bcc && !args.include_bcc_recipients,
                        body_cloud_link_count,
                        subject: p.subject.clone(),
                    });
                    // Bind pre-write QC meta to this export index / volume.
                    let prep_idx = start_cursor + i;
                    if let Some(meta) = qc_meta_by_prepare_idx.get_mut(prep_idx) {
                        meta.export_message_index = export_message_index;
                        meta.volume_index = volume_index;
                        meta.has_ledger_fail = attach_fails > 0;
                        meta.ledger_failed_attach_names = attach_fail_names;
                    }
                }

                volumes.push(VolumeReportRow {
                    volume_index,
                    path: vol_path_str,
                    bytes: report.bytes,
                    sha256_hex: report.sha256_hex,
                    md5_hex: report.md5_hex,
                    messages_written: report.messages_written,
                    finalized_early: report.finalized_early,
                    volume_exceeded_soft_limit: exceeded,
                });
                hash_ms = hash_ms.saturating_add(report.hash_ms);
                attach_written_total =
                    attach_written_total.saturating_add(report.attachments_written);
                attach_failed_total = attach_failed_total.saturating_add(report.attachments_failed);
                attach_omitted_total =
                    attach_omitted_total.saturating_add(report.attachments_omitted_by_policy);
                attach_fidelity_events_total = attach_fidelity_events_total
                    .saturating_add(report.attachment_fidelity_events_total);
                attach_fidelity_events_truncated =
                    attach_fidelity_events_truncated || report.attachment_fidelity_events_truncated;
                // Uncapped counter (not the first-N Vec) so export_risk sees CRC past the cap.
                attach_stream_crc_events =
                    attach_stream_crc_events.saturating_add(report.attach_stream_crc_events);

                cursor = start_cursor + written;
                messages_written_prior =
                    messages_written_prior.saturating_add(report.messages_written);

                if cancel_requested(&cancel) {
                    cancelled = true;
                    export_partial = true;
                    export_error = Some("cancelled".into());
                    emit_log(stderr, &on_log, "cancelled after volume");
                    break;
                }

                if !report.finalized_early {
                    // Consumed all remaining (or empty).
                    break;
                }
                // Early finalize: continue remaining winners on next volume.
                if written == 0 {
                    export_partial = true;
                    export_error = Some(format!(
                        "volume {volume_index} finalized with 0 messages written"
                    ));
                    failed_volume_index = Some(volume_index);
                    break;
                }
            }
            Err(e) => {
                // Discard volume-local attach events (do not pollute ledger).
                drop(vol_attach_buf);
                // §3.3.1: delete incomplete current volume (and temp sibling); keep prior.
                // Writer TempGuard also deletes staging on cancel/error; this covers
                // any residual final path and same-dir temp.
                delete_incomplete_volume(&vol_path);
                export_partial = true;
                let is_cancel = matches!(e, WriterError::Cancelled)
                    || e.to_string().eq_ignore_ascii_case("cancelled");
                if is_cancel {
                    cancelled = true;
                    export_error = Some("cancelled".into());
                    export_error_code = Some("cancelled");
                    emit_log(
                        stderr,
                        &on_log,
                        &format!("volume {volume_index} cancelled mid-write"),
                    );
                } else {
                    // Typed summary code: WriterError::Io → write_io (retryable);
                    // layout/capacity/refusal → export (permanent).
                    export_error_code = Some(writer_error_summary_code(&e));
                    export_error = Some(format!("volume {volume_index} write failed: {e}"));
                }
                failed_volume_index = Some(volume_index);
                break;
            }
        }
    }

    phase_timings.write_ms = t_write.elapsed().as_millis() as u64;

    // Empty keep-set (or loop never entered): still honour cancel so outcome
    // is not reported as a successful zero-message export when the user aborted.
    if !cancelled && cancel_requested(&cancel) {
        cancelled = true;
        export_partial = true;
        export_error = Some("cancelled".into());
        emit_log(
            stderr,
            &on_log,
            "cancelled after write phase (empty or idle)",
        );
    }

    // Prepare-errors mean some winners never written.
    if !prepare_errors.is_empty() && export_error.is_none() {
        export_partial = true;
        export_error = Some(format!(
            "prepare/materialize errors ({}): {:?}",
            prepare_errors.len(),
            prepare_errors
        ));
    }

    // Attachment stream failures: PST retained (message-complete); fidelity is
    // partial (exit 64) via attach_failed_total — do **not** set export_error
    // (that would hard-fail as COUNT_MISMATCH / Generic). 0078 refinement.
    if attach_failed_total > 0 {
        let msg = format!("attachment write failures: {attach_failed_total} (partial fidelity)");
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
    }

    let messages_written_total: u64 = volumes.iter().map(|v| v.messages_written).sum();
    let count_mismatch = messages_written_total != keep_set.stats.unique && !export_partial;
    if count_mismatch {
        export_partial = true;
        export_error = Some(format!(
            "messages_written_total ({messages_written_total}) != unique ({})",
            keep_set.stats.unique
        ));
    }

    // ── Phase 4: report pack (always flush before exit) ─────────────────────
    emit_log(stderr, &on_log, "stage=report");
    emit_stage_progress(
        &on_progress,
        "report",
        volume_index,
        messages_written_total,
        messages_written_total,
        0,
        winners_total,
    );
    let t_report = Instant::now();
    let mut report_write_errors: Vec<String> = Vec::new();
    if let Some(msg) = ledger_init_error.take() {
        report_write_errors.push(msg);
    }
    let mut decision_csv_out: Option<String> = None;
    if let Some(path) = &decision_csv {
        match DecisionCsvWriter::create(path) {
            Ok(mut wtr) => {
                if let Err(e) = resolved.write_decisions_csv(&mut wtr) {
                    let msg = format!("decision csv write failed: {e}");
                    tracing::warn!("{msg}");
                    emit_log(stderr, &on_log, &format!("warning: {msg}"));
                    report_write_errors.push(msg);
                } else if let Err(e) = wtr.flush() {
                    let msg = format!("decision csv flush failed: {e}");
                    tracing::warn!("{msg}");
                    emit_log(stderr, &on_log, &format!("warning: {msg}"));
                    report_write_errors.push(msg);
                } else {
                    decision_csv_out = Some(path.display().to_string());
                }
            }
            Err(e) => {
                let msg = format!("decision csv create failed: {e}");
                tracing::warn!("{msg}");
                emit_log(stderr, &on_log, &format!("warning: {msg}"));
                report_write_errors.push(msg);
            }
        }
    }

    let mut keep_set_json_out: Option<String> = None;
    if let Some(path) = &keep_set_json {
        match write_keep_set_json(path, &keep_set) {
            Ok(()) => keep_set_json_out = Some(path.display().to_string()),
            Err(e) => {
                let msg = format!("keepset.json write failed: {e}");
                tracing::warn!("{msg}");
                emit_log(stderr, &on_log, &format!("warning: {msg}"));
                report_write_errors.push(msg);
            }
        }
    }

    let volumes_csv_path = report_dir.join("volumes.csv");
    if let Err(e) = write_volumes_csv(&volumes_csv_path, &volumes) {
        let msg = format!("volumes.csv write failed: {e}");
        tracing::warn!("{msg}");
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
        report_write_errors.push(msg);
    }

    // Flush attach ledger (background CSV join) before export_messages / summary.
    let attach_ledger_finish = match attach_ledger.take() {
        Some(ledger) => match ledger.finish() {
            Ok(f) => Some(f),
            Err(e) => {
                let msg = format!("attach ledger flush failed: {e}");
                tracing::warn!("{msg}");
                emit_log(stderr, &on_log, &format!("warning: {msg}"));
                report_write_errors.push(msg);
                None
            }
        },
        None => None,
    };
    // Backfill fail counts if export rows were built before a late tally (should already match).
    if let Some(finish) = attach_ledger_finish.as_ref() {
        for row in &mut export_rows {
            row.attachments_failed_count = finish.fail_count_for(&row.source_path, row.nid);
        }
    }

    // export_messages.csv mandatory (always attempt; empty header when zero winners).
    // Basename is applied only at CSV serialization; in-memory rows keep full paths for QC.
    let export_messages_path = report_dir.join("export_messages.csv");
    if messages_written_total > 0 || !export_rows.is_empty() {
        if let Err(e) =
            write_export_messages_csv(&export_messages_path, &export_rows, args.ledger_path_mode)
        {
            let msg = format!("export_messages.csv write failed: {e}");
            tracing::warn!("{msg}");
            emit_log(stderr, &on_log, &format!("warning: {msg}"));
            report_write_errors.push(msg);
        }
    } else if let Err(e) =
        write_export_messages_csv(&export_messages_path, &[], args.ledger_path_mode)
    {
        let msg = format!("export_messages.csv write failed: {e}");
        tracing::warn!("{msg}");
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
        report_write_errors.push(msg);
    }

    // 0085: body cloud links CSV when report pack exists (independent of attach-ledger mode).
    let body_cloud_csv_path = report_dir.join(EXPORT_BODY_CLOUD_LINKS_CSV_NAME);
    if let Err(e) = write_body_cloud_links_csv(
        &body_cloud_csv_path,
        &body_cloud_link_rows,
        args.ledger_path_mode,
    ) {
        let msg = format!("export_body_cloud_links.csv write failed: {e}");
        tracing::warn!("{msg}");
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
        report_write_errors.push(msg);
    }

    phase_timings.report_ms = t_report.elapsed().as_millis() as u64;

    // ── Phase 5: verify completed volumes ───────────────────────────────────
    emit_log(stderr, &on_log, "stage=verify");
    emit_stage_progress(
        &on_progress,
        "verify",
        volume_index,
        messages_written_total,
        messages_written_total,
        0,
        winners_total,
    );
    let t_verify = Instant::now();
    let mut verification = verify_volumes(&volumes, &export_rows, args.verify_hash);
    phase_timings.verify_ms = t_verify.elapsed().as_millis() as u64;

    // ── Phase 5b: source-differential QC (0080) ─────────────────────────────
    // When qc-level is off, legacy verify_volumes remains the structural baseline.
    // Empty volumes still run QC when enabled (zero-winner export emits qc_report_v1).
    let mut qc_hard_fail = false;
    if args.qc_level != crate::unique_pst_qc::QcLevel::Off {
        emit_log(stderr, &on_log, "stage=qc");
        let t_qc = Instant::now();
        let candidates: Vec<crate::unique_pst_qc::QcSampleCandidate> = {
            // Prefer pre-write meta (body/attach sizes) bound to export indices.
            let mut by_idx: HashMap<u64, crate::unique_pst_qc::QcSampleCandidate> = HashMap::new();
            for m in &qc_meta_by_prepare_idx {
                if m.export_message_index > 0 {
                    by_idx.insert(m.export_message_index, m.clone());
                }
            }
            export_rows
                .iter()
                .map(|row| {
                    by_idx
                        .get(&row.export_message_index)
                        .cloned()
                        .unwrap_or_else(|| crate::unique_pst_qc::QcSampleCandidate {
                            export_message_index: row.export_message_index,
                            volume_index: row.volume_index,
                            source_path: row.source_path.clone(),
                            source_nid: row.nid,
                            folder_path: row.folder_path.clone(),
                            subject: row.subject.clone(),
                            sender: String::new(),
                            message_id_norm: row.message_id_norm.clone(),
                            body_plain_len: 0,
                            body_html_len: 0,
                            attach_count: 0,
                            max_attach_size: 0,
                            has_zero_byte_attach: false,
                            has_embedded: false,
                            has_degraded: false,
                            has_ledger_fail: row.attachments_failed_count > 0,
                            ledger_failed_attach_names: Vec::new(),
                            body_unavailable: false,
                            body_incomplete: false,
                            crc_suspect: false,
                            subject_non_ascii: !row.subject.is_ascii(),
                            display_cc: String::new(),
                            display_bcc: String::new(),
                        })
                })
                .collect()
        };
        let qc_report = crate::unique_pst_qc::run_unique_pst_qc(crate::unique_pst_qc::QcRunInput {
            level: args.qc_level,
            sample_max: args.qc_sample_max,
            report_dir: &report_dir,
            volumes: &volumes,
            export_rows: &export_rows,
            candidates: &candidates,
            external_reader: args.qc_external_reader.as_deref(),
            run_scanpst: args.qc_scanpst,
            max_open_psts: args.max_open_psts,
            source_differential: true,
            parents_only,
            include_bcc_recipients: args.include_bcc_recipients,
            probe_unexplained_property: None,
        });
        phase_timings.qc_ms = t_qc.elapsed().as_millis() as u64;
        // Never lower an exit already set; only force verify failure on hard findings.
        if qc_report.hard_fail {
            qc_hard_fail = true;
            verification.ok = false;
            emit_log(
                stderr,
                &on_log,
                &format!(
                    "qc hard findings: defect={} unexplained_loss={}",
                    qc_report.findings.defect, qc_report.findings.unexplained_loss
                ),
            );
        } else {
            emit_log(
                stderr,
                &on_log,
                &format!(
                    "qc ok: level={} messages_compared={} known_gap={}",
                    qc_report.qc_level, qc_report.messages_compared, qc_report.findings.known_gap
                ),
            );
        }
        let _ = qc_hard_fail;
    }

    // Spec §3.3.1: partial export forces overall + verification honesty flags.
    if export_partial {
        verification.ok = false;
    }

    let exit_err = evaluate_exit_policy(&outcome.summary, &opts).err();
    let verify_err = if verification.ok {
        None
    } else if export_partial {
        // Partial already counted via export_partial / export_err; avoid double noise.
        None
    } else {
        Some("verification failed".to_string())
    };
    let export_err = export_error.clone();
    let report_err_msg = if report_write_errors.is_empty() {
        None
    } else {
        Some(format!(
            "mandatory report artifact write failed ({}): {}",
            report_write_errors.len(),
            report_write_errors.join("; ")
        ))
    };

    // ── 0078: cancel quarantine before classify (D7) ────────────────────────
    let mut quarantine = crate::export_outcome::QuarantineResult::NotAttempted;
    if cancelled {
        let t_quarantine = Instant::now();
        let bytes_on_disk = volumes.iter().any(|v| Path::new(&v.path).exists()) || out.exists();
        if bytes_on_disk {
            let vol_count = volumes.len() as u32;
            quarantine = quarantine_cancelled_volumes(&out, vol_count);
            emit_log(
                stderr,
                &on_log,
                &format!("cancel quarantine: {:?}", quarantine),
            );
        } else {
            quarantine = crate::export_outcome::QuarantineResult::NoVolumes;
        }
        phase_timings.quarantine_ms = t_quarantine.elapsed().as_millis() as u64;
    }
    // Recompute total after quarantine so cancelled runs still report honest timings.
    let duration_ms = started.elapsed().as_millis() as u64;
    phase_timings.finalize(duration_ms);
    // Refresh opens in case attach stream opened additional sources during write.
    source_pst_opens = handle_cache.borrow().opens();
    let bytes_written_total: u64 = volumes.iter().map(|v| v.bytes).sum();

    let export_ok_input = ExportOkInput {
        scan_ok: exit_err.is_none(),
        verify_ok: verify_err.is_none(),
        export_err_absent: export_err.is_none(),
        export_partial,
        messages_written_total,
        unique: keep_set.stats.unique,
        attach_failed_total,
        body_soft_fail_total: 0,
        report_ok: report_write_errors.is_empty(),
    };
    // Legacy bool (tests + ok field): complete fidelity only; cancel forces false.
    let ok_export = compute_export_ok(export_ok_input) && !cancelled;

    let risk_gate = args
        .fail_on_export_risk
        .as_deref()
        .and_then(crate::export_outcome::RiskGate::parse)
        .unwrap_or(crate::export_outcome::RiskGate::Off);
    let fail_on_partial = args.fail_on_partial_fidelity && !args.allow_partial_fidelity;

    // export_risk needs export_section.partial — build section first (provisional ok).
    let export_section = {
        let mut section = ExportSection {
            volumes: volumes.clone(),
            partial: export_partial || !ok_export && messages_written_total < keep_set.stats.unique,
            messages_written_total,
            attachments_written: attach_written_total,
            attachments_failed: attach_failed_total,
            attachments_omitted_by_policy: Some(attach_omitted_total),
            attachments_failed_by_reason: None,
            attachment_ledger: None,
            attachment_ledger_mode: Some(args.attach_ledger.as_str().to_string()),
            attachment_ledger_truncated: None,
            attachment_ledger_rows_written: None,
            error: export_error.clone(),
            failed_volume_index,
            attachment_fidelity_events_truncated: Some(attach_fidelity_events_truncated),
            attachment_fidelity_events_total: Some(attach_fidelity_events_total),
            include_bcc_recipients: args.include_bcc_recipients,
        };
        if let Some(finish) = attach_ledger_finish.as_ref() {
            finish.apply_to_export_section(&mut section);
        }
        section.attachments_omitted_by_policy = Some(attach_omitted_total);
        section
    };
    let attach_attempts = attach_written_total.saturating_add(attach_failed_total);
    let attach_fail_rate = attach_failed_total as f64 / (attach_attempts.max(1) as f64);
    let degraded_winner_rate = if keep_set.stats.unique > 0 {
        keep_set.stats.degraded_winners as f64 / keep_set.stats.unique as f64
    } else {
        0.0
    };
    let export_risk = crate::unique_export_report::compute_export_risk(
        &outcome.summary.preflight.recommendation,
        &crate::unique_export_report::ExportRiskInputs {
            attach_fail_rate,
            block_crc_rate: outcome.summary.block_crc_rate,
            block_crc_read_rate: outcome.summary.block_crc_read_rate,
            degraded_winner_rate,
            partial: export_section.partial,
            failed_volume_index: export_section.failed_volume_index,
            scan_recommendation: outcome.summary.preflight.recommendation,
            attach_stream_crc_events,
        },
    );

    let classified = crate::export_outcome::classify_export(
        export_ok_input,
        export_risk.level,
        risk_gate,
        fail_on_partial,
        cancelled,
    );
    let bytes_written =
        messages_written_total > 0 || volumes.iter().any(|v| v.bytes > 0) || out.exists();
    let artifact_state =
        crate::export_outcome::artifact_state_for(&classified, bytes_written, quarantine);
    // ok retained: complete fidelity only (non-cancelled success path).
    let ok = classified.fidelity == crate::export_outcome::ExportFidelity::Complete && !cancelled;

    let summary_error = if !ok || cancelled {
        let (code, message) = if cancelled {
            ("cancelled", "cancelled".to_string())
        } else if let Some(msg) = export_err.as_ref() {
            // Prefer writer/disk typed code (write_io) when the write phase failed.
            (export_error_code.unwrap_or("export"), msg.clone())
        } else if let Some(msg) = report_err_msg.as_ref() {
            ("report", msg.clone())
        } else if let Some(msg) = verify_err.as_ref() {
            ("verification", msg.clone())
        } else if let Some(msg) = exit_err.as_ref() {
            ("scan_integrity", msg.clone())
        } else if classified.fidelity == crate::export_outcome::ExportFidelity::Partial {
            (
                "partial_fidelity",
                "unique-pst partial fidelity".to_string(),
            )
        } else {
            ("export", "unique-pst incomplete".to_string())
        };
        Some(SummaryError {
            code: code.to_string(),
            message,
        })
    } else {
        None
    };

    let bcc_suppressed_message_count =
        export_rows.iter().filter(|r| r.bcc_suppressed).count() as u64;
    let sent_message_with_no_recipients_count = prepared
        .iter()
        .filter(|p| p.sent_message_with_no_recipients)
        .count() as u64;
    let retryable = crate::export_outcome::summary_is_retryable(
        classified.exit,
        cancelled,
        &classified.reasons,
        summary_error.as_ref().map(|e| e.code.as_str()),
    );

    let summary_abs = std::path::absolute(&summary_path).unwrap_or_else(|_| summary_path.clone());
    let mut summary = UniqueExportSummary {
        schema: UNIQUE_EXPORT_REPORT_SCHEMA.to_string(),
        ok,
        fidelity: classified.fidelity,
        exit_code: classified.exit.as_u8(),
        exit_reason: classified
            .reasons
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        artifact_state,
        summary_path: summary_abs.display().to_string(),
        inputs: paths.iter().map(|p| p.display().to_string()).collect(),
        policy: args.policy.as_str().to_string(),
        family_policy: args.family_policy.as_str().to_string(),
        mode: args.mode.as_str().to_string(),
        folder_layout: args.folder_layout.as_str().to_string(),
        out: out.display().to_string(),
        report_dir: report_dir.display().to_string(),
        keep_set: keep_set.clone(),
        scan: outcome.summary,
        export: export_section,
        verification,
        duration_ms,
        phase_timings,
        source_pst_opens,
        messages_materialized,
        bytes_written_total,
        prepared_bytes_peak,
        hash_ms,
        max_volume_bytes: args.max_volume_bytes,
        decision_csv: decision_csv_out.clone(),
        keep_set_json: keep_set_json_out.clone(),
        error: summary_error.clone(),
        export_risk,
        bcc_suppressed_message_count,
        sent_message_with_no_recipients_count,
        retryable,
        promote_on_attach_fail: args.promote_on_attach_fail,
        promoted_after_attach_incomplete_count: keep_set
            .stats
            .promoted_after_attach_incomplete_count,
        mode_c_fallback_all_peers_incomplete_count: keep_set
            .stats
            .mode_c_fallback_all_peers_incomplete_count,
        messages_with_body_cloud_links,
        body_cloud_links_total,
        body_cloud_link_truncated_messages,
    };

    // Fail-closed: if summary.json itself fails, force non-success exit even if
    // summary.ok was true (re-emit corrected summary is impossible; exit non-zero).
    let mut summary_write_failed: Option<String> = None;
    if let Err(e) = write_summary_json(&summary_path, &summary) {
        let msg = format!("summary.json write failed: {e}");
        tracing::warn!("{msg}");
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
        summary_write_failed = Some(msg);
    }
    let mut classified = classified;
    let mut ok = ok;
    let mut summary_error = summary_error;
    if summary_write_failed.is_some() {
        ok = false;
        // Force hard-fail classify dimensions for report write.
        let mut forced = export_ok_input;
        forced.report_ok = false;
        classified = crate::export_outcome::classify_export(
            forced,
            summary.export_risk.level,
            risk_gate,
            fail_on_partial,
            cancelled,
        );
        summary.ok = false;
        summary.fidelity = classified.fidelity;
        summary.exit_code = classified.exit.as_u8();
        summary.exit_reason = classified
            .reasons
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        if let Some(msg) = summary_write_failed.clone() {
            summary_error = Some(SummaryError {
                code: "report".to_string(),
                message: msg,
            });
            summary.error = summary_error.clone();
        }
        summary.retryable = crate::export_outcome::summary_is_retryable(
            classified.exit,
            cancelled,
            &classified.reasons,
            summary_error.as_ref().map(|e| e.code.as_str()),
        );
        // Best-effort rewrite with corrected fields.
        let _ = write_summary_json(&summary_path, &summary);
    }
    let summary_error = match (ok, summary_write_failed, summary_error) {
        (false, Some(msg), None) => Some(SummaryError {
            code: "report".to_string(),
            message: msg,
        }),
        (_, _, existing) => existing,
    };

    let structured = UniquePstOutcome {
        ok,
        cancelled,
        report_dir: report_dir.clone(),
        summary_path: summary_path.clone(),
        out: out.clone(),
        messages_written_total,
        unique: keep_set.stats.unique,
        volume_count: volumes.len(),
        volumes: volumes.iter().map(UniqueVolumeDigest::from).collect(),
        error_message: summary_error
            .as_ref()
            .map(|e| e.message.clone())
            .or_else(|| {
                if cancelled {
                    Some("cancelled".into())
                } else if !ok {
                    Some("unique-pst failed".into())
                } else {
                    None
                }
            }),
        export_risk: summary.export_risk.level,
        exit: classified.exit,
        fidelity: classified.fidelity,
        exit_reasons: classified.reasons.clone(),
        artifact_state,
    };

    emit_log(
        stderr,
        &on_log,
        &format!(
            "stage=done ok={ok} cancelled={cancelled} exit={} messages_written={messages_written_total}",
            classified.exit.as_u8()
        ),
    );
    emit_stage_progress(
        &on_progress,
        "done",
        volume_index,
        messages_written_total,
        messages_written_total,
        0,
        winners_total,
    );

    // ── Phase 6: exit (CLI stdout) ──────────────────────────────────────────
    if args.json {
        let mut stdout_summary = summary;
        if !ok {
            stdout_summary.ok = false;
            if stdout_summary.error.is_none() {
                stdout_summary.error = summary_error.clone();
            }
        }
        // Keep exit_code aligned with process status.
        stdout_summary.exit_code = classified.exit.as_u8();
        println!("{}", serde_json::to_string_pretty(&stdout_summary)?);
        // Return outcome with classified exit; main maps Ok(CliExit) / AlreadyEmitted.
        if classified.exit != crate::error::CliExit::Success {
            let msg = summary_error
                .map(|e| e.message)
                .unwrap_or_else(|| "unique-pst failed".into());
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: classified.exit,
            });
        }
        return Ok(structured);
    }

    // Human summary only when stderr_progress is on (CLI). GUI leaves it false.
    if stderr {
        println!(
            "=== Unique PST export ({UNIQUE_EXPORT_REPORT_SCHEMA}) policy={} family={} ===",
            args.policy.as_str(),
            args.family_policy.as_str()
        );
        println!("  out:              {}", out.display());
        println!("  report_dir:       {}", report_dir.display());
        println!(
            "  messages_written: {}  unique: {}  volumes: {}",
            messages_written_total,
            keep_set.stats.unique,
            volumes.len()
        );
        println!(
            "  attach written:   {}  attach failed: {}",
            attach_written_total, attach_failed_total
        );
        println!(
            "  partial:          {}  ok: {ok}  cancelled: {cancelled}",
            summary.export.partial
        );
        println!(
            "  fidelity:         {}  exit: {}  artifact: {}",
            classified.fidelity.as_str(),
            classified.exit.as_u8(),
            artifact_state.as_str()
        );
        // 0077 DoD-13: numbers/codes only — no PST-derived strings.
        println!("  export_risk:      {}", summary.export_risk.level.as_str());
        // 0075 honesty counters (always printed, including when 0).
        println!(
            "  winners_from_recoverable_items: {}",
            keep_set.stats.winners_from_recoverable_items
        );
        println!(
            "  winners_without_bcc_peer_had_bcc: {}",
            keep_set.stats.winners_without_bcc_peer_had_bcc
        );
        println!(
            "  groups_date_source_mixed: {}",
            keep_set.stats.groups_date_source_mixed
        );
        for line in crate::grouping_cli::format_grouping_stats_human(&keep_set.stats.grouping) {
            println!("{line}");
        }
        for v in &volumes {
            println!(
                "  volume {}: {} ({} msgs, {} bytes)",
                v.volume_index, v.path, v.messages_written, v.bytes
            );
        }
        if let Some(p) = &decision_csv_out {
            println!("  decision_csv:     {p}");
        }
        if let Some(p) = &keep_set_json_out {
            println!("  keep_set_json:    {p}");
        }
        println!("  summary:          {}", summary_path.display());
    }

    // Library/GUI path: return structured outcome even when !ok (caller maps).
    // CLI `run_unique_pst` maps outcome.exit.
    Ok(structured)
}

/// Zero-recip anomaly (0082 §2.5 rule 8): empty table + flags present + NOT unsent.
/// Missing flags → skip (do not invent UNSENT). Empty + unsent → not an anomaly.
fn is_sent_message_with_no_recipients(recipients_empty: bool, message_flags: Option<u32>) -> bool {
    recipients_empty && message_flags.is_some_and(|f| !pst_reader::message_flags_is_unsent(f))
}

/// Convert a just-materialized winner into a write-ready DTO (0079 D1 + D11).
///
/// `msg` already has fidelity merged from `finalize_with_materialize` and
/// scan keys (MID/hash/MIH) applied. Bodies/attach payloads are **moved**.
fn prepared_winner_from_canonical(
    msg: dedup_engine::keepset::CanonicalMessage,
) -> std::result::Result<PreparedWinner, String> {
    let source_path = msg.locus.source_path.clone();
    let folder_path = msg.locus.folder_path.clone();
    let nid = msg.locus.nid;
    let message_id_norm = msg.message_id_norm.clone().unwrap_or_default();
    let edrm_mih = msg.edrm_mih_hex.clone().unwrap_or_default();
    let content_hash_hex = msg
        .content_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    // Capture BCC before adapter map — dedicated source signal (display + table Bcc).
    // Do not couple to adapter_dropped: that counter also tracks disclosure policy.
    let display_bcc = msg.display_bcc.clone().unwrap_or_default();
    let source_has_bcc =
        !display_bcc.trim().is_empty() || msg.recipients.iter().any(|r| r.recipient_type.is_bcc());
    let sent_message_with_no_recipients =
        is_sent_message_with_no_recipients(msg.recipients.is_empty(), msg.message_flags);
    // 0085: scan bodies before move into WriteMessage / before write-path mem::take.
    // Body hits never set is_attach_incomplete (Mode A non-interaction).
    let body_scan =
        dedup_engine::scan_body_cloud_links(msg.body_html.as_deref(), msg.body_plain.as_deref());
    let body_cloud_hits: Vec<(String, String)> = body_scan
        .hits
        .into_iter()
        .map(|h| (h.url, h.source.as_str().to_string()))
        .collect();
    let body_cloud_truncated = body_scan.truncated;
    let (write_msg, _adapter_dropped) = from_canonical_message_owned(msg);
    let subject = write_msg.subject.clone();
    Ok(PreparedWinner {
        source_path,
        folder_path,
        nid,
        message_id_norm,
        edrm_mih,
        content_hash_hex,
        subject,
        write_msg,
        display_bcc,
        source_has_bcc,
        sent_message_with_no_recipients,
        body_cloud_hits,
        body_cloud_truncated,
    })
}

/// Retained body + buffered attach payload bytes for `prepared_bytes_peak` (0079 §3.9).
fn prepared_winner_retained_bytes(p: &PreparedWinner) -> u64 {
    let mut n = 0u64;
    if let Some(ref s) = p.write_msg.body_plain {
        n = n.saturating_add(s.len() as u64);
    }
    if let Some(ref h) = p.write_msg.body_html {
        n = n.saturating_add(h.len() as u64);
    }
    for a in &p.write_msg.attachments {
        if let Some(ref d) = a.data {
            n = n.saturating_add(d.len() as u64);
        }
    }
    n
}

/// Delete incomplete volume file and same-dir temp sibling (writer cleanup best-effort).
fn delete_incomplete_volume(vol_path: &Path) {
    let tmp = temp_sibling_path(vol_path);
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }
    if vol_path.exists() {
        let _ = fs::remove_file(vol_path);
    }
    // If vol_path is a directory (fail-injection), leave it — we didn't create a PST there.
}

/// Remove stale multi-volume siblings (`out_vol002.pst` …) when overwriting.
///
/// Never deletes a path that equals or resolves to an input PST. If any planned
/// volume path collides with an input, refuses the export (source protection).
fn clear_stale_volume_siblings(out: &Path, inputs: &[PathBuf]) -> Result<()> {
    for i in 2u32..=MAX_VOLUME_SIBLING_INDEX {
        let p = volume_path_for(out, i);
        if path_collides_with_inputs(&p, inputs) {
            return Err(CliError::Usage(format!(
                "refusing multi-volume path equal to an input PST: {}",
                p.display()
            )));
        }
        if !p.exists() {
            // Contiguous siblings from prior runs; stop at first missing so we
            // do not scan all 998, but still guarded every existing candidate
            // we would touch. Collision check above also covers non-existing
            // planned names (e.g. input named unique_vol003.pst).
            // Continue checking non-existing planned paths for collisions only:
            // already done via path_collides; break existence scan.
            // Keep scanning for collisions against all planned indices even if
            // intermediate siblings are missing (inputs may sit at vol003+).
            continue;
        }
        if p.is_file() {
            fs::remove_file(&p)
                .map_err(|e| CliError::Msg(format!("remove stale volume {}: {e}", p.display())))?;
        }
    }
    Ok(())
}

/// True when `candidate` equals (resolved) any protected input PST path.
fn path_collides_with_inputs(candidate: &Path, inputs: &[PathBuf]) -> bool {
    inputs
        .iter()
        .any(|input| paths_equal_resolved(candidate, input) || paths_equal(candidate, input))
}

// Re-export for existing tests / call sites that import from this module.
pub use crate::export_outcome::ExportOkInput;

/// Pure gate for export success (honesty). Extracted for unit tests.
///
/// Re-expressed via [`crate::export_outcome::classify_export`] so existing tests
/// remain the back-compat guard for 0078 refinement (rule 4).
///
/// `scan_ok` / `verify_ok` / `export_err_absent` / `report_ok` are positive flags
/// (true = no failure in that dimension).
pub(crate) fn compute_export_ok(i: ExportOkInput) -> bool {
    use crate::export_outcome::{classify_export, ExportFidelity, RiskGate};
    classify_export(
        i,
        dedup_engine::integrity::PreflightRecommendation::Ok,
        RiskGate::Off,
        true,
        false,
    )
    .fidelity
        == ExportFidelity::Complete
}

fn prepare_report_dir(report_dir: &Path, overwrite: bool) -> Result<()> {
    if report_dir.exists() {
        if !report_dir.is_dir() {
            return Err(CliError::Usage(format!(
                "--report-dir exists and is not a directory: {}",
                report_dir.display()
            )));
        }
        let non_empty = fs::read_dir(report_dir)
            .map_err(|e| CliError::Msg(format!("read report-dir {}: {e}", report_dir.display())))?
            .next()
            .is_some();
        if non_empty && !overwrite {
            return Err(CliError::Usage(format!(
                "--report-dir is not empty (pass --overwrite to replace contents): {}",
                report_dir.display()
            )));
        }
        if non_empty && overwrite {
            for entry in fs::read_dir(report_dir).map_err(|e| {
                CliError::Msg(format!("read report-dir {}: {e}", report_dir.display()))
            })? {
                let entry = entry.map_err(|e| CliError::Msg(format!("read_dir entry: {e}")))?;
                let p = entry.path();
                if p.is_dir() {
                    fs::remove_dir_all(&p)
                        .map_err(|e| CliError::Msg(format!("remove {}: {e}", p.display())))?;
                } else {
                    fs::remove_file(&p)
                        .map_err(|e| CliError::Msg(format!("remove {}: {e}", p.display())))?;
                }
            }
        }
    } else {
        fs::create_dir_all(report_dir).map_err(|e| {
            CliError::Msg(format!("create --report-dir {}: {e}", report_dir.display()))
        })?;
    }
    Ok(())
}

/// Path guards: refuse layouts that would overwrite or nest under source PSTs.
///
/// Checks primary `--out`, `--report-dir`, report artifacts, **and every**
/// generated multi-volume sibling path (`_vol002` … `_vol999`) against inputs
/// using resolved (parent-canonicalized) equality so junction aliases are caught.
fn guard_unique_pst_paths(
    inputs: &[PathBuf],
    out: &Path,
    report_dir: &Path,
    decision_csv: Option<&Path>,
    keep_set_json: Option<&Path>,
    integrity_csv: Option<&Path>,
) -> Result<()> {
    for input in inputs {
        if paths_equal_resolved(out, input) || paths_equal(out, input) {
            return Err(CliError::Usage(format!(
                "refusing --out equal to an input PST: {}",
                out.display()
            )));
        }
        if is_same_or_under_resolved(out, input) || is_same_or_under(out, input) {
            return Err(CliError::Usage(format!(
                "refusing --out nested under an input PST: out={} input={}",
                out.display(),
                input.display()
            )));
        }
        if paths_equal_resolved(report_dir, input) || paths_equal(report_dir, input) {
            return Err(CliError::Usage(format!(
                "refusing --report-dir equal to an input PST: {}",
                report_dir.display()
            )));
        }
        // Report-dir must not contain an input (recursive clear on overwrite).
        if is_same_or_under_resolved(input, report_dir) || is_same_or_under(input, report_dir) {
            return Err(CliError::Usage(format!(
                "refusing --report-dir that contains an input PST: report_dir={} input={}",
                report_dir.display(),
                input.display()
            )));
        }
        for art in [decision_csv, keep_set_json, integrity_csv]
            .into_iter()
            .flatten()
        {
            if paths_equal_resolved(art, input) || paths_equal(art, input) {
                return Err(CliError::Usage(format!(
                    "refusing report artifact that equals an input PST: {}",
                    art.display()
                )));
            }
        }
        // Every planned multi-volume path (vol 1 already checked as `out`).
        for vol_idx in 2u32..=MAX_VOLUME_SIBLING_INDEX {
            let vol = volume_path_for(out, vol_idx);
            if paths_equal_resolved(&vol, input) || paths_equal(&vol, input) {
                return Err(CliError::Usage(format!(
                    "refusing multi-volume path equal to an input PST: {}",
                    vol.display()
                )));
            }
        }
    }
    Ok(())
}

fn verify_volumes(
    volumes: &[VolumeReportRow],
    export_rows: &[ExportMessageRow],
    verify_hash: bool,
) -> VerificationReport {
    let mut vol_results = Vec::new();
    let mut all_ok = true;

    for vol in volumes {
        let path = PathBuf::from(&vol.path);
        let mut open_ok = false;
        let mut message_count_match = false;
        let mut messages_found = 0u64;
        let mut sample_mid_ok = true;
        let mut hash_match: Option<bool> = None;
        let mut error: Option<String> = None;

        match PstFile::open(&path) {
            Ok(mut pst) => {
                open_ok = true;
                match pst.folders() {
                    Ok(folders) => {
                        messages_found = folders.iter().map(|f| f.message_nids.len() as u64).sum();
                        message_count_match = messages_found == vol.messages_written;

                        // Sample min(5, N) Message-IDs or subjects vs export_messages for volume.
                        let vol_exports: Vec<&ExportMessageRow> = export_rows
                            .iter()
                            .filter(|r| r.volume_index == vol.volume_index)
                            .collect();
                        let sample_n = (vol_exports.len()).min(5);
                        if sample_n > 0 {
                            // Collect *all* written message IDs/subjects so sample rows that
                            // land late in folder traversal cannot falsely fail (Codex r2 P2).
                            // Cost is O(messages_in_volume) property reads — acceptable for
                            // Phase 5 structural verify; multi-GB full-file rehash remains opt-in.
                            let mut written_mids: Vec<String> = Vec::new();
                            let mut written_subjects: Vec<String> = Vec::new();
                            for folder in &folders {
                                for &nid in &folder.message_nids {
                                    if let Ok(props) = pst.read_message_properties(nid) {
                                        if let Some(mid) = props.message_id {
                                            written_mids.push(normalize_mid_exact(&mid));
                                        }
                                        if let Some(sub) = props.subject {
                                            written_subjects.push(normalize_subject(&sub));
                                        }
                                    }
                                }
                            }
                            for r in vol_exports.iter().take(sample_n) {
                                match sample_row_matches(r, &written_mids, &written_subjects) {
                                    SampleMatch::Ok => {}
                                    SampleMatch::Fail(reason) => {
                                        sample_mid_ok = false;
                                        error = Some(reason);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error = Some(format!("folders: {e}"));
                        sample_mid_ok = false;
                    }
                }
            }
            Err(e) => {
                error = Some(format!("open: {e}"));
            }
        }

        if verify_hash {
            match sha256_file(&path) {
                Ok(hex) => {
                    let m = hex.eq_ignore_ascii_case(&vol.sha256_hex);
                    hash_match = Some(m);
                    if !m {
                        error = Some(format!(
                            "sha256 mismatch: report={} rehash={}",
                            vol.sha256_hex, hex
                        ));
                    }
                }
                Err(e) => {
                    hash_match = Some(false);
                    error = Some(format!("rehash: {e}"));
                }
            }
        }

        let vol_ok = open_ok && message_count_match && sample_mid_ok && hash_match.unwrap_or(true);
        if !vol_ok {
            all_ok = false;
        }
        vol_results.push(VolumeVerification {
            volume_index: vol.volume_index,
            path: vol.path.clone(),
            open_ok,
            message_count_match,
            messages_found,
            messages_expected: vol.messages_written,
            sample_mid_ok,
            hash_match,
            error,
        });
    }

    // Empty volume list: structural verify of "nothing" is OK. Export partial /
    // count mismatch / zero-winner policy is decided by the orchestrator, not here.
    // (Previously failing empty lists made successful unique==0 exports always fail.)
    if volumes.is_empty() {
        all_ok = true;
    }

    VerificationReport {
        ok: all_ok,
        volumes: vol_results,
        rehash_ran: verify_hash,
    }
}

/// Exact normalized Message-ID for sample verification (no substring match).
fn normalize_mid_exact(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_ascii_lowercase()
}

/// Subject normalize: trim + case-insensitive compare basis.
fn normalize_subject(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

/// Result of matching one export_messages sample row against written identities.
#[derive(Debug, PartialEq, Eq)]
enum SampleMatch {
    Ok,
    Fail(String),
}

/// Exact MID equality when MID present; else exact normalized subject; fail if
/// neither identity is available on the export row.
fn sample_row_matches(
    row: &ExportMessageRow,
    written_mids: &[String],
    written_subjects: &[String],
) -> SampleMatch {
    sample_identity_matches(
        &row.message_id_norm,
        if row.subject.is_empty() {
            None
        } else {
            Some(row.subject.as_str())
        },
        written_mids,
        written_subjects,
    )
}

/// Subject-aware sample match: exact normalized MID only (no substring); for
/// empty MID compare normalized subjects; fail when neither identity exists.
fn sample_identity_matches(
    expected_mid: &str,
    expected_subject: Option<&str>,
    written_mids: &[String],
    written_subjects: &[String],
) -> SampleMatch {
    if !expected_mid.is_empty() {
        let want = normalize_mid_exact(expected_mid);
        if written_mids.iter().any(|m| m == &want) {
            return SampleMatch::Ok;
        }
        return SampleMatch::Fail(format!(
            "sample MID not found in volume (exact match): {expected_mid}"
        ));
    }
    if let Some(sub) = expected_subject {
        let want = normalize_subject(sub);
        if want.is_empty() {
            return SampleMatch::Fail("sample row has empty Message-ID and empty subject".into());
        }
        if written_subjects.iter().any(|s| s == &want) {
            return SampleMatch::Ok;
        }
        return SampleMatch::Fail(format!("sample subject not found in volume: {sub}"));
    }
    SampleMatch::Fail(
        "sample row has empty Message-ID and no subject identity for verification".into(),
    )
}

fn sha256_file(path: &Path) -> std::result::Result<String, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_rejects_out_equal_input() {
        let inputs = vec![PathBuf::from(r"C:\data\mail.pst")];
        let out = PathBuf::from(r"C:\data\mail.pst");
        let report = PathBuf::from(r"C:\data\mail_report");
        assert!(guard_unique_pst_paths(&inputs, &out, &report, None, None, None).is_err());
    }

    #[test]
    fn guard_rejects_report_dir_contains_input() {
        let inputs = vec![PathBuf::from(r"C:\data\pack\mail.pst")];
        let out = PathBuf::from(r"C:\data\unique.pst");
        let report = PathBuf::from(r"C:\data\pack");
        assert!(guard_unique_pst_paths(&inputs, &out, &report, None, None, None).is_err());
    }

    #[test]
    fn guard_accepts_disjoint() {
        let inputs = vec![PathBuf::from(r"C:\data\mail.pst")];
        let out = PathBuf::from(r"C:\export\unique.pst");
        let report = PathBuf::from(r"C:\export\unique_report");
        let dec = PathBuf::from(r"C:\export\unique_report\decisions.csv");
        guard_unique_pst_paths(&inputs, &out, &report, Some(&dec), None, None).expect("ok");
    }

    #[test]
    fn guard_rejects_volume_3_sibling_equal_input() {
        // Input named like multi-volume sibling of --out unique.pst.
        let inputs = vec![PathBuf::from(r"C:\export\unique_vol003.pst")];
        let out = PathBuf::from(r"C:\export\unique.pst");
        let report = PathBuf::from(r"C:\export\unique_report");
        let err = guard_unique_pst_paths(&inputs, &out, &report, None, None, None).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("multi-volume") || msg.contains("vol003") || msg.contains("input"),
            "expected volume collision error, got: {msg}"
        );
    }

    fn ok_base() -> ExportOkInput {
        ExportOkInput {
            scan_ok: true,
            verify_ok: true,
            export_err_absent: true,
            export_partial: false,
            messages_written_total: 5,
            unique: 5,
            attach_failed_total: 0,
            body_soft_fail_total: 0,
            report_ok: true,
        }
    }

    #[test]
    fn compute_export_ok_requires_zero_attach_failures() {
        assert!(compute_export_ok(ok_base()));
        let mut bad = ok_base();
        bad.attach_failed_total = 1;
        assert!(!compute_export_ok(bad));
    }

    #[test]
    fn compute_export_ok_requires_report_ok() {
        let mut bad = ok_base();
        bad.report_ok = false;
        assert!(!compute_export_ok(bad));
    }

    #[test]
    fn compute_export_ok_count_and_partial() {
        let mut partial = ok_base();
        partial.export_partial = true;
        assert!(!compute_export_ok(partial));
        let mut count = ok_base();
        count.messages_written_total = 4;
        assert!(!compute_export_ok(count));
    }

    /// 0082 P2-1: WriterError::Io → write_io (retryable); permanent variants → export.
    #[test]
    fn writer_error_summary_code_io_retryable_layout_permanent() {
        let io_err = WriterError::Io(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "disk full",
        ));
        assert_eq!(writer_error_summary_code(&io_err), "write_io");
        assert!(crate::export_outcome::summary_is_retryable(
            crate::error::CliExit::Generic,
            false,
            &[crate::export_outcome::reason::COUNT_MISMATCH],
            Some(writer_error_summary_code(&io_err)),
        ));

        let layout = WriterError::Layout("bad heap".into());
        assert_eq!(writer_error_summary_code(&layout), "export");
        assert!(!crate::export_outcome::summary_is_retryable(
            crate::error::CliExit::Generic,
            false,
            &[crate::export_outcome::reason::COUNT_MISMATCH],
            Some(writer_error_summary_code(&layout)),
        ));

        let refused = WriterError::Refused("exists".into());
        assert_eq!(writer_error_summary_code(&refused), "export");
        assert!(!crate::export_outcome::summary_is_retryable(
            crate::error::CliExit::Generic,
            false,
            &[crate::export_outcome::reason::COUNT_MISMATCH],
            Some(writer_error_summary_code(&refused)),
        ));

        assert_eq!(
            writer_error_summary_code(&WriterError::Cancelled),
            "cancelled"
        );
    }

    /// 0082 DoD-10: zero-recip anomaly boundaries.
    #[test]
    fn zero_recip_anomaly_sent_counts_draft_skips_missing_flags_skip() {
        // Sent (flags=0) + empty → count.
        assert!(is_sent_message_with_no_recipients(true, Some(0)));
        // Unsent bit set → no count.
        assert!(!is_sent_message_with_no_recipients(
            true,
            Some(pst_reader::MSGFLAG_UNSENT)
        ));
        // Missing flags → skip (no invent).
        assert!(!is_sent_message_with_no_recipients(true, None));
        // Non-empty table → no count even if sent.
        assert!(!is_sent_message_with_no_recipients(false, Some(0)));
    }

    #[test]
    fn sample_mid_exact_not_substring() {
        let written = vec!["abc@example.com".to_string()];
        // Substring-only match must fail.
        assert!(matches!(
            sample_identity_matches("bc@example", None, &written, &[]),
            SampleMatch::Fail(_)
        ));
        // Exact normalized match (angle brackets stripped).
        assert_eq!(
            sample_identity_matches("<ABC@example.com>", None, &written, &[]),
            SampleMatch::Ok
        );
    }

    #[test]
    fn sample_empty_mid_uses_subject() {
        let subjects = vec!["hello world".to_string()];
        assert_eq!(
            sample_identity_matches("", Some("Hello World"), &[], &subjects),
            SampleMatch::Ok
        );
        assert!(matches!(
            sample_identity_matches("", Some("other"), &[], &subjects),
            SampleMatch::Fail(_)
        ));
        assert!(matches!(
            sample_identity_matches("", None, &[], &subjects),
            SampleMatch::Fail(_)
        ));
    }

    #[test]
    fn normalize_mid_exact_strips_brackets_lowercase() {
        assert_eq!(normalize_mid_exact(" <Id@X.com> "), "id@x.com");
    }

    /// Sample matching must succeed against a late identity in a large set
    /// (regression for former 64-identity cap false-negative).
    #[test]
    fn sample_identity_matches_late_entry_beyond_64() {
        let mut mids: Vec<String> = (0..100).map(|i| format!("id{i}@example.com")).collect();
        mids.push("late@example.com".into());
        let mut subjects: Vec<String> = (0..100).map(|i| format!("subject {i}")).collect();
        subjects.push("late subject".into());

        assert_eq!(
            sample_identity_matches("late@example.com", None, &mids, &subjects),
            SampleMatch::Ok
        );
        assert_eq!(
            sample_identity_matches("", Some("late subject"), &mids, &subjects),
            SampleMatch::Ok
        );
    }

    fn require_sample_pst() -> PathBuf {
        let sample =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst");
        assert!(
            sample.is_file(),
            "required fixture missing (fail-closed): {}",
            sample.display()
        );
        sample
    }

    #[test]
    fn log_and_progress_callbacks_fire() {
        let sample = require_sample_pst();
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let report = dir.path().join("report");
        let also_eml = dir.path().join("also_eml_unused");

        let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let progress: Arc<Mutex<Vec<UniquePstProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let logs_c = Arc::clone(&logs);
        let progress_c = Arc::clone(&progress);

        let args = UniquePstCliArgs {
            paths: vec![sample],
            out,
            report_dir: Some(report),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path_contains: vec![],
            prefer_bcc_copy: false,
            prefer_folder_class: false,
            folder_rank: vec![],
            source_rank: vec![],
            rank_folder_class_first: false,
            fidelity_rank: "binary".into(),
            decision_csv: None,
            keep_set_json: None,
            folder_layout: FolderLayoutArg::Preserve,
            max_volume_bytes: None,
            overwrite: false,
            verify_hash: false,
            // Forces a production warning through on_log (D-0071 residual).
            also_eml: Some(also_eml),
            no_tier2: false,
            no_attachments: true,
            json: false,
            mode: ScanMode::BestEffort,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
            attach_ledger: AttachLedgerMode::Full,
            attach_ledger_max_rows: DEFAULT_ATTACH_LEDGER_MAX_ROWS,
            ledger_path_mode: LedgerPathMode::Full,
            deep_attach_preflight: false,
            deep_attach_level: "head".into(),
            deep_attach_max_attaches: 50_000,
            deep_attach_max_probe_bytes: 268_435_456,
            deep_attach_per_attach_max_bytes: 1_048_576,
            deep_attach_max_probe_time_ms: 2000,
            deep_attach_max_open_psts: 32,
            deep_attach_max_peer_probes: 3,
            max_attach_fail_rate: 0.05,
            strong_content_hash: "off".into(),
            strong_hash_attach_max_attaches: 50_000,
            strong_hash_attach_max_bytes: 1_073_741_824,
            strong_hash_attach_per_attach_max_bytes: 536_870_912,
            dedupe_scope: "global".into(),
            tier1_verify: "off".into(),
            tier1_backfill: false,
            identity_ignore_inline_attachments: false,
            allow_cross_mid_tier2: false,
            allow_degenerate_tier2: false,
            allow_crc_suspect_tier2: false,
            crc_log_limit: 10,
            crc_log_interval_secs: 30,
            fail_on_partial_fidelity: true,
            allow_partial_fidelity: false,
            fail_on_export_risk: None,
            max_open_psts: DEFAULT_MAX_OPEN_PSTS,
            qc_level: crate::unique_pst_qc::QcLevel::Off,
            qc_sample_max: 64,
            qc_external_reader: None,
            qc_scanpst: false,
            include_bcc_recipients: false,
            promote_on_attach_fail: false,
        };
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: None,
                stderr_progress: false,
                on_progress: Some(Box::new(move |p| {
                    progress_c.lock().unwrap_or_else(|e| e.into_inner()).push(p);
                })),
                on_log: Some(Box::new(move |line| {
                    logs_c.lock().unwrap_or_else(|e| e.into_inner()).push(line);
                })),
            },
        )
        .expect("run");
        assert!(outcome.ok, "outcome: {:?}", outcome.error_message);
        let log_lines = logs.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            log_lines.iter().any(|l| l.contains("stage=scan")),
            "expected stage=scan in logs: {log_lines:?}"
        );
        assert!(
            log_lines
                .iter()
                .any(|l| l.contains("warning") && l.contains("also-eml")),
            "expected also-eml warning via on_log: {log_lines:?}"
        );
        let ticks = progress.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            ticks
                .iter()
                .any(|t| t.stage == "scan" || t.stage == "write" || t.stage == "done"),
            "expected progress stages: {:?}",
            ticks.iter().map(|t| t.stage.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn cancel_before_write_returns_cancelled_outcome() {
        let sample = require_sample_pst();
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let report = dir.path().join("report");
        let cancel = Arc::new(AtomicBool::new(true)); // cancel immediately

        let args = UniquePstCliArgs {
            paths: vec![sample],
            out: out.clone(),
            report_dir: Some(report.clone()),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path_contains: vec![],
            prefer_bcc_copy: false,
            prefer_folder_class: false,
            folder_rank: vec![],
            source_rank: vec![],
            rank_folder_class_first: false,
            fidelity_rank: "binary".into(),
            decision_csv: None,
            keep_set_json: None,
            folder_layout: FolderLayoutArg::Preserve,
            max_volume_bytes: None,
            overwrite: false,
            verify_hash: false,
            also_eml: None,
            no_tier2: false,
            no_attachments: true,
            json: false,
            mode: ScanMode::BestEffort,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
            attach_ledger: AttachLedgerMode::Full,
            attach_ledger_max_rows: DEFAULT_ATTACH_LEDGER_MAX_ROWS,
            ledger_path_mode: LedgerPathMode::Full,
            deep_attach_preflight: false,
            deep_attach_level: "head".into(),
            deep_attach_max_attaches: 50_000,
            deep_attach_max_probe_bytes: 268_435_456,
            deep_attach_per_attach_max_bytes: 1_048_576,
            deep_attach_max_probe_time_ms: 2000,
            deep_attach_max_open_psts: 32,
            deep_attach_max_peer_probes: 3,
            max_attach_fail_rate: 0.05,
            strong_content_hash: "off".into(),
            strong_hash_attach_max_attaches: 50_000,
            strong_hash_attach_max_bytes: 1_073_741_824,
            strong_hash_attach_per_attach_max_bytes: 536_870_912,
            dedupe_scope: "global".into(),
            tier1_verify: "off".into(),
            tier1_backfill: false,
            identity_ignore_inline_attachments: false,
            allow_cross_mid_tier2: false,
            allow_degenerate_tier2: false,
            allow_crc_suspect_tier2: false,
            crc_log_limit: 10,
            crc_log_interval_secs: 30,
            fail_on_partial_fidelity: true,
            allow_partial_fidelity: false,
            fail_on_export_risk: None,
            max_open_psts: DEFAULT_MAX_OPEN_PSTS,
            qc_level: crate::unique_pst_qc::QcLevel::Off,
            qc_sample_max: 64,
            qc_external_reader: None,
            qc_scanpst: false,
            include_bcc_recipients: false,
            promote_on_attach_fail: false,
        };
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: Some(cancel),
                stderr_progress: false,
                on_progress: None,
                on_log: None,
            },
        )
        .expect("structured outcome");
        // Hard assert: cancel-from-start must always yield cancelled outcome.
        assert!(outcome.cancelled, "expected cancelled=true");
        assert!(!outcome.ok);
        assert_eq!(outcome.error_message.as_deref(), Some("cancelled"));
        assert!(outcome.volumes.is_empty());
        // Report-dir was prepared: minimal summary.json must exist for Open report honesty.
        assert!(
            outcome.summary_path.is_file(),
            "cancelled path must write summary.json at {}",
            outcome.summary_path.display()
        );
        let body = fs::read_to_string(&outcome.summary_path).expect("read summary");
        assert!(
            body.contains("\"ok\": false") || body.contains("\"ok\":false"),
            "summary must report ok=false: {body}"
        );
        assert!(
            body.contains("cancelled"),
            "summary must mention cancelled: {body}"
        );
        // Incomplete final path must not exist; no orphan temp sibling.
        assert!(!out.exists());
        let tmp = temp_sibling_path(&out);
        assert!(!tmp.exists(), "orphan temp left: {}", tmp.display());
    }

    /// Cancel as soon as the write stage is entered (before/around first volume).
    ///
    /// Hard-asserts `cancelled == true`. True mid-message cancel while the
    /// writer is inside a large volume is proven by
    /// `pst-writer::tests::cancel_mid_write_no_final_pst_temp_cleaned` —
    /// tiny fixtures can finish the write before the flag is observed mid-loop.
    #[test]
    fn cancel_at_write_stage_returns_cancelled_no_orphan_temp() {
        let sample = require_sample_pst();
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let report = dir.path().join("report");
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_c = Arc::clone(&cancel);

        let args = UniquePstCliArgs {
            paths: vec![sample],
            out: out.clone(),
            report_dir: Some(report),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path_contains: vec![],
            prefer_bcc_copy: false,
            prefer_folder_class: false,
            folder_rank: vec![],
            source_rank: vec![],
            rank_folder_class_first: false,
            fidelity_rank: "binary".into(),
            decision_csv: None,
            keep_set_json: None,
            folder_layout: FolderLayoutArg::Preserve,
            max_volume_bytes: None,
            overwrite: false,
            verify_hash: false,
            also_eml: None,
            no_tier2: false,
            no_attachments: true,
            json: false,
            mode: ScanMode::BestEffort,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
            attach_ledger: AttachLedgerMode::Full,
            attach_ledger_max_rows: DEFAULT_ATTACH_LEDGER_MAX_ROWS,
            ledger_path_mode: LedgerPathMode::Full,
            deep_attach_preflight: false,
            deep_attach_level: "head".into(),
            deep_attach_max_attaches: 50_000,
            deep_attach_max_probe_bytes: 268_435_456,
            deep_attach_per_attach_max_bytes: 1_048_576,
            deep_attach_max_probe_time_ms: 2000,
            deep_attach_max_open_psts: 32,
            deep_attach_max_peer_probes: 3,
            max_attach_fail_rate: 0.05,
            strong_content_hash: "off".into(),
            strong_hash_attach_max_attaches: 50_000,
            strong_hash_attach_max_bytes: 1_073_741_824,
            strong_hash_attach_per_attach_max_bytes: 536_870_912,
            dedupe_scope: "global".into(),
            tier1_verify: "off".into(),
            tier1_backfill: false,
            identity_ignore_inline_attachments: false,
            allow_cross_mid_tier2: false,
            allow_degenerate_tier2: false,
            allow_crc_suspect_tier2: false,
            crc_log_limit: 10,
            crc_log_interval_secs: 30,
            fail_on_partial_fidelity: true,
            allow_partial_fidelity: false,
            fail_on_export_risk: None,
            max_open_psts: DEFAULT_MAX_OPEN_PSTS,
            qc_level: crate::unique_pst_qc::QcLevel::Off,
            qc_sample_max: 64,
            qc_external_reader: None,
            qc_scanpst: false,
            include_bcc_recipients: false,
            promote_on_attach_fail: false,
        };
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: Some(cancel),
                stderr_progress: false,
                on_progress: Some(Box::new(move |p| {
                    // Trip cancel as soon as write stage begins (pre-volume or first tick).
                    if p.stage == "write" {
                        cancel_c.store(true, Ordering::SeqCst);
                    }
                })),
                on_log: None,
            },
        )
        .expect("structured outcome");
        let tmp = temp_sibling_path(&out);
        assert!(
            !tmp.exists(),
            "incomplete temp must not remain as permanent orphan: {}",
            tmp.display()
        );
        // First write-stage progress sets the flag; prepare/before-volume checkpoints
        // and empty-keep-set re-check must honour it.
        assert!(
            outcome.cancelled,
            "cancel at write stage must yield cancelled=true (got ok={} err={:?})",
            outcome.ok, outcome.error_message
        );
        assert!(!outcome.ok);
    }

    /// Trip cancel on first `stage=scan` progress (before/around `run_scan`).
    /// With cooperative scan cancel, the pipeline still returns cancelled + report.
    #[test]
    fn cancel_on_scan_stage_returns_cancelled() {
        let sample = require_sample_pst();
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let report = dir.path().join("report");
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_c = Arc::clone(&cancel);

        let args = UniquePstCliArgs {
            paths: vec![sample],
            out: out.clone(),
            report_dir: Some(report),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path_contains: vec![],
            prefer_bcc_copy: false,
            prefer_folder_class: false,
            folder_rank: vec![],
            source_rank: vec![],
            rank_folder_class_first: false,
            fidelity_rank: "binary".into(),
            decision_csv: None,
            keep_set_json: None,
            folder_layout: FolderLayoutArg::Preserve,
            max_volume_bytes: None,
            overwrite: false,
            verify_hash: false,
            also_eml: None,
            no_tier2: false,
            no_attachments: true,
            json: false,
            mode: ScanMode::BestEffort,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
            attach_ledger: AttachLedgerMode::Full,
            attach_ledger_max_rows: DEFAULT_ATTACH_LEDGER_MAX_ROWS,
            ledger_path_mode: LedgerPathMode::Full,
            deep_attach_preflight: false,
            deep_attach_level: "head".into(),
            deep_attach_max_attaches: 50_000,
            deep_attach_max_probe_bytes: 268_435_456,
            deep_attach_per_attach_max_bytes: 1_048_576,
            deep_attach_max_probe_time_ms: 2000,
            deep_attach_max_open_psts: 32,
            deep_attach_max_peer_probes: 3,
            max_attach_fail_rate: 0.05,
            strong_content_hash: "off".into(),
            strong_hash_attach_max_attaches: 50_000,
            strong_hash_attach_max_bytes: 1_073_741_824,
            strong_hash_attach_per_attach_max_bytes: 536_870_912,
            dedupe_scope: "global".into(),
            tier1_verify: "off".into(),
            tier1_backfill: false,
            identity_ignore_inline_attachments: false,
            allow_cross_mid_tier2: false,
            allow_degenerate_tier2: false,
            allow_crc_suspect_tier2: false,
            crc_log_limit: 10,
            crc_log_interval_secs: 30,
            fail_on_partial_fidelity: true,
            allow_partial_fidelity: false,
            fail_on_export_risk: None,
            max_open_psts: DEFAULT_MAX_OPEN_PSTS,
            qc_level: crate::unique_pst_qc::QcLevel::Off,
            qc_sample_max: 64,
            qc_external_reader: None,
            qc_scanpst: false,
            include_bcc_recipients: false,
            promote_on_attach_fail: false,
        };
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: Some(cancel),
                stderr_progress: false,
                on_progress: Some(Box::new(move |p| {
                    if p.stage == "scan" {
                        cancel_c.store(true, Ordering::SeqCst);
                    }
                })),
                on_log: None,
            },
        )
        .expect("structured outcome");
        assert!(
            outcome.cancelled,
            "cancel on scan stage must yield cancelled=true (got ok={} err={:?})",
            outcome.ok, outcome.error_message
        );
        assert!(!outcome.ok);
        assert!(!out.exists());
        let tmp = temp_sibling_path(&out);
        assert!(!tmp.exists(), "orphan temp left: {}", tmp.display());
        assert!(
            outcome.summary_path.is_file(),
            "cancelled path must write summary.json"
        );
        assert_eq!(outcome.exit, crate::error::CliExit::Cancelled);
        assert_eq!(
            outcome.exit_reasons,
            vec![crate::export_outcome::reason::CANCELLED]
        );
    }

    #[test]
    fn quarantine_renames_primary_and_sibling() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let vol2 = volume_path_for(&out, 2);
        fs::write(&out, b"vol1").expect("write vol1");
        fs::write(&vol2, b"vol2").expect("write vol2");
        let q = quarantine_cancelled_volumes(&out, 2);
        assert_eq!(q, crate::export_outcome::QuarantineResult::Succeeded);
        assert!(!out.exists(), "--out must be free after quarantine");
        assert!(!vol2.exists());
        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("rd")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries.len(), 2);
        for name in &entries {
            assert!(
                name.contains("cancelled-") && name.ends_with(".partial"),
                "unexpected name {name}"
            );
        }
    }

    #[test]
    fn quarantine_rename_failure_is_failed() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        fs::write(&out, b"x").expect("write");
        let q = quarantine_cancelled_volumes_with(&out, 1, |_from, _to| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "simulated lock",
            ))
        });
        assert_eq!(q, crate::export_outcome::QuarantineResult::Failed);
        assert!(out.exists(), "file remains on rename fail");
        let art = crate::export_outcome::artifact_state_for(
            &crate::export_outcome::ExportOutcome {
                fidelity: crate::export_outcome::ExportFidelity::Failed,
                exit: crate::error::CliExit::Cancelled,
                reasons: vec![crate::export_outcome::reason::CANCELLED],
                cancelled: true,
            },
            true,
            q,
        );
        assert_eq!(art, crate::export_outcome::ArtifactState::InvalidInPlace);
    }

    /// Same stamp twice must not overwrite; both partials retained and `--out` free.
    #[test]
    fn quarantine_same_stamp_collision_keeps_both_partials() {
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let stamp = "1720000000-000";
        fs::write(&out, b"first").expect("write v1");
        let q1 = quarantine_cancelled_volumes_with_stamp(&out, 1, stamp, |from, to| {
            fs::rename(from, to)
        });
        assert_eq!(q1, crate::export_outcome::QuarantineResult::Succeeded);
        assert!(!out.exists(), "--out free after first quarantine");
        let first_partial = dir
            .path()
            .join("unique.pst.cancelled-1720000000-000.partial");
        assert!(first_partial.is_file(), "primary partial must exist");
        let first_bytes = fs::read(&first_partial).expect("read first");
        assert_eq!(first_bytes, b"first");

        // Second cancel of a new write at the same stamp.
        fs::write(&out, b"second").expect("write v2");
        let q2 = quarantine_cancelled_volumes_with_stamp(&out, 1, stamp, |from, to| {
            fs::rename(from, to)
        });
        assert_eq!(q2, crate::export_outcome::QuarantineResult::Succeeded);
        assert!(!out.exists(), "--out free after second quarantine");
        let second_partial = dir
            .path()
            .join("unique.pst.cancelled-1720000000-000_2.partial");
        assert!(
            second_partial.is_file(),
            "collision suffix _2 partial must exist"
        );
        assert_eq!(fs::read(&second_partial).expect("read second"), b"second");
        // First partial untouched (never overwrite).
        assert_eq!(
            fs::read(&first_partial).expect("read first again"),
            b"first"
        );
        let partials: Vec<_> = fs::read_dir(dir.path())
            .expect("rd")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("cancelled-") && n.ends_with(".partial"))
            .collect();
        assert_eq!(partials.len(), 2, "both partials retained: {partials:?}");
    }
}
