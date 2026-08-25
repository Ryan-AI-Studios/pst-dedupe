//! Shared scan orchestration for CLI commands (track 0065 integrity).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use dedup_engine::{
    hasher::{self, AttachmentInfo, StrongHashInput, Tier2IneligibleReason},
    integrity::{
        classify_attach_enum_for_identity, classify_attach_meta_fail, classify_body_flags,
        classify_orphaned, compute_preflight, integrity_sidecar_path, merge_recoverable,
        reason_from_pst_error, tally_reason, FileScanStatus, IntegrityCsvWriter,
        IntegrityLedgerWriter, IntegrityReason, IntegrityThresholds, MessageClassification,
        PreflightInputs, PreflightReport, RecoverableIntegrity, ScanMode, SkipRecord,
        SCAN_INTEGRITY_SCHEMA,
    },
    keepset::{MessageLocus, RecoverableScanItem},
    report::{write_summary_report, ReportRow, StreamingCsvReportWriter},
    DedupIndex, DedupResult, GroupingContext, GroupingStats, IndexItem, MessageRef,
};
use pst_reader::PstFile;
use serde::{Deserialize, Serialize};

use crate::error::{CliError, Result};

/// Options controlling a scan (including integrity modes / ledgers).
#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub enable_tier2: bool,
    pub include_attachments: bool,
    pub mode: ScanMode,
    pub thresholds: IntegrityThresholds,
    pub allow_failed_files: bool,
    /// Explicit integrity CSV path (overrides sidecar).
    pub integrity_csv: Option<PathBuf>,
    /// Dedup CSV path (streamed during scan when set).
    pub csv: Option<PathBuf>,
    /// Cap on JSON skip sample size.
    pub skip_limit: usize,
    /// Retain `ReportRow`s in memory (needed for dups listing).
    pub retain_rows: bool,
    /// Retain keep-set candidates (mid + content_hash + integrity) for Phase 2 resolve.
    pub retain_candidates: bool,
    /// Cooperative cancel (GUI / library). When set and true, `run_scan` stops at
    /// safe boundaries and returns `Ok` with partial results so far. CLI leaves
    /// this `None` so scan/dups behavior is unchanged.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Opt-in deep attach stream preflight (0074). Default **off**.
    pub deep_attach_preflight: bool,
    /// Deep probe level: `head` (L2 default) or `full` (L3).
    pub deep_attach_level: String,
    pub deep_attach_max_attaches: u64,
    pub deep_attach_max_probe_bytes: u64,
    pub deep_attach_per_attach_max_bytes: u64,
    pub deep_attach_max_probe_time_ms: u64,
    pub deep_attach_max_open_psts: usize,
    pub deep_attach_max_peer_probes_per_group: u64,
    /// 0076 identity-binding context (guards, scope, strong hash level).
    pub grouping: GroupingContext,
    /// Max attaches full-stream digested under `body-recip-attach` (0086; default 50_000).
    pub strong_hash_attach_max_attaches: u64,
    /// Max digest bytes per run under `body-recip-attach` (0086; default 1 GiB).
    pub strong_hash_attach_max_bytes: u64,
    /// Per-attach max digest bytes under `body-recip-attach` (0086; default 512 MiB).
    /// Distinct from deep-attach L2 head caps — identity always full-streams.
    pub strong_hash_attach_per_attach_max_bytes: u64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            enable_tier2: true,
            include_attachments: true,
            mode: ScanMode::BestEffort,
            thresholds: IntegrityThresholds::default(),
            allow_failed_files: false,
            integrity_csv: None,
            csv: None,
            skip_limit: 10_000,
            retain_rows: true,
            retain_candidates: false,
            cancel: None,
            deep_attach_preflight: false,
            deep_attach_level: "head".into(),
            deep_attach_max_attaches: 50_000,
            deep_attach_max_probe_bytes: 256 * 1024 * 1024,
            deep_attach_per_attach_max_bytes: 1024 * 1024,
            deep_attach_max_probe_time_ms: 2000,
            deep_attach_max_open_psts: 32,
            deep_attach_max_peer_probes_per_group: 3,
            grouping: GroupingContext::default(),
            strong_hash_attach_max_attaches: crate::attach_content_hash::DEFAULT_MAX_ATTACHES,
            strong_hash_attach_max_bytes: crate::attach_content_hash::DEFAULT_MAX_BYTES,
            strong_hash_attach_per_attach_max_bytes:
                crate::attach_content_hash::DEFAULT_PER_ATTACH_MAX_BYTES,
        }
    }
}

fn scan_cancel_requested(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .map(|c| c.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// Per-file outcome with integrity status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileScanStats {
    pub path: String,
    pub name: String,
    pub status: FileScanStatus,
    pub folders: u64,
    pub messages: u64,
    pub recoverable_messages: u64,
    pub duplicates: u64,
    pub skipped: u64,
    pub skipped_by_reason: BTreeMap<String, u64>,
    pub degraded_messages: u64,
    pub degraded_by_reason: BTreeMap<String, u64>,
    pub error_code: Option<IntegrityReason>,
    pub error: Option<String>,
    /// Page CRC mismatches observed while reading this source (0077).
    #[serde(default)]
    pub page_crc_mismatches: u64,
    /// Block CRC mismatches observed while reading this source (0077).
    #[serde(default)]
    pub block_crc_mismatches: u64,
    /// Block BID mismatches observed while reading this source (0077).
    #[serde(default)]
    pub block_bid_mismatches: u64,
    /// Distinct bad BIDs observed while this source was open (0077; source-local).
    #[serde(default)]
    pub distinct_bad_bids: u64,
    /// Messages that saw block CRC/BID mismatch during read (0077; pre-poly-clear).
    #[serde(default)]
    pub crc_suspect_messages: u64,
    /// Page validate paths while reading this source (0077 rate denominator).
    #[serde(default)]
    pub page_reads: u64,
    /// Block read paths while reading this source (0077 rate denominator).
    #[serde(default)]
    pub block_reads: u64,
    /// Dual-rate poly-class CRC for this source (page+block rate ≥ 0.50).
    ///
    /// When true, identity `CRC_SUSPECT` was cleared as poly false-positive;
    /// raw page/block CRC counters are retained for reporting / `export_risk`.
    #[serde(default)]
    pub poly_class_crc: bool,
}

/// Full scan outcome (schema `scan_integrity_v1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSummary {
    pub schema: String,
    pub mode: ScanMode,
    pub files: Vec<FileScanStats>,
    pub total_messages: u64,
    pub unique: u64,
    pub duplicates: u64,
    pub tier1_hits: u64,
    pub tier2_hits: u64,
    pub savings_bytes: u64,
    pub skipped: u64,
    pub skipped_by_reason: BTreeMap<String, u64>,
    pub recoverable_messages: u64,
    pub degraded_messages: u64,
    pub degraded_by_reason: BTreeMap<String, u64>,
    pub orphaned_messages: u64,
    pub failed_files: u64,
    pub partial_files: u64,
    pub opened_files: u64,
    pub duration_secs: f64,
    pub preflight: PreflightReport,
    /// Capped skip sample for JSON (not the legal ledger).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skips: Vec<SkipRecord>,
    /// Path of streaming integrity CSV if written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity_csv: Option<String>,
    /// 0076 identity-binding honesty counters.
    #[serde(default, skip_serializing_if = "grouping_stats_empty")]
    pub grouping: GroupingStats,
    /// Totals of page CRC mismatches across sources (0077).
    #[serde(default)]
    pub page_crc_mismatches: u64,
    #[serde(default)]
    pub block_crc_mismatches: u64,
    #[serde(default)]
    pub block_bid_mismatches: u64,
    #[serde(default)]
    pub distinct_bad_bids: u64,
    /// Whether `distinct_bad_bids` is exact (false past the 1024 cap).
    #[serde(default)]
    pub distinct_bad_bids_exact: bool,
    #[serde(default)]
    pub crc_suspect_messages: u64,
    #[serde(default)]
    pub page_reads: u64,
    #[serde(default)]
    pub block_reads: u64,
    /// `(page_crc + block_crc) / max(1, recoverable_messages)` — corruption per document.
    #[serde(default)]
    pub block_crc_rate: f64,
    /// `(page_crc + block_crc) / max(1, page_reads + block_reads)` — fraction of reads; ∈ [0,1].
    #[serde(default)]
    pub block_crc_read_rate: f64,
    /// Count of sources classified dual-rate poly-class (`poly_class_crc`) (0077).
    #[serde(default)]
    pub poly_class_crc_sources: u64,
}

fn grouping_stats_empty(s: &GroupingStats) -> bool {
    s == &GroupingStats::default()
}

/// One duplicate pair for listing.
#[derive(Debug, Clone, Serialize)]
pub struct DupRow {
    pub tier: String,
    pub subject: String,
    pub sender: String,
    pub folder: String,
    pub pst: String,
    pub size: u32,
    pub original_subject: String,
    pub original_folder: String,
    pub original_pst: String,
}

/// Full scan payload retained for report/dup listing.
pub struct ScanOutcome {
    pub summary: ScanSummary,
    pub rows: Vec<ReportRow>,
    /// Keep-set candidates (populated when [`ScanOptions::retain_candidates`]).
    pub candidates: Vec<RecoverableScanItem>,
    /// True when dedup CSV was already streamed during the scan.
    pub csv_streamed: bool,
}

/// Outcome of rebuilding dedup relationships from a surviving candidate set.
#[derive(Debug, Clone)]
pub struct RebuildDedupOutcome {
    /// `(source_pst basename, nid) → DedupResult` after insert in scan_order.
    pub results: HashMap<(String, u64), DedupResult>,
    pub unique_count: u64,
    pub duplicate_count: u64,
    pub tier1_hits: u64,
    pub tier2_hits: u64,
    pub total_savings: u64,
    /// Identity-binding counters from the rebuild index (0076 honesty).
    pub grouping_stats: GroupingStats,
}

/// Rebuild `DedupResult` for remaining candidates in `scan_order`.
///
/// Used after strict deep-attach probe removes candidates so buffered CSV rows
/// never still say `DuplicateOf` a skipped message (0074 P1-1).
///
/// `message_refs` supplies full [`MessageRef`] identity (subject/folder/sender)
/// keyed by `(source_pst, nid)` — typically from pre-probe buffered report rows.
/// Missing keys fall back to a minimal ref built from the candidate locus.
pub fn rebuild_dedup_results(
    candidates: &[RecoverableScanItem],
    message_refs: &HashMap<(String, u64), MessageRef>,
    enable_tier2: bool,
) -> RebuildDedupOutcome {
    rebuild_dedup_results_with_ctx(
        candidates,
        message_refs,
        &GroupingContext::with_tier2(enable_tier2),
    )
}

/// Rebuild streaming index under an explicit [`GroupingContext`] (0076).
pub fn rebuild_dedup_results_with_ctx(
    candidates: &[RecoverableScanItem],
    message_refs: &HashMap<(String, u64), MessageRef>,
    ctx: &GroupingContext,
) -> RebuildDedupOutcome {
    let mut ordered: Vec<&RecoverableScanItem> = candidates.iter().collect();
    ordered.sort_by_key(|c| c.scan_order);

    let mut index = DedupIndex::with_capacity_and_context(ordered.len().max(1), ctx.clone());
    let mut results: HashMap<(String, u64), DedupResult> = HashMap::with_capacity(ordered.len());
    let mut total_savings = 0u64;

    for c in ordered {
        let key = (c.locus.source_pst.clone(), c.locus.nid);
        let msg_ref = message_refs
            .get(&key)
            .cloned()
            .unwrap_or_else(|| MessageRef {
                pst_index: 0,
                pst_name: c.locus.source_pst.clone(),
                folder_path: c.locus.folder_path.clone(),
                nid: c.locus.nid,
                subject: String::new(),
                submit_time: None,
                sender: String::new(),
                size: c.size,
            });
        let eligible = match c.assess_tier2_eligibility() {
            Ok(()) => true,
            Err(Tier2IneligibleReason::UnreadableBody) => {
                if ctx.enforce_readable_body() {
                    index.record_tier2_block_unreadable();
                    false
                } else {
                    true
                }
            }
            Err(Tier2IneligibleReason::Degenerate) => {
                if ctx.enforce_readable_body() {
                    index.record_tier2_block_degenerate();
                    false
                } else {
                    true
                }
            }
            Err(Tier2IneligibleReason::CrcSuspect) => {
                if ctx.allow_crc_suspect_tier2_for(&c.path_key()) {
                    true
                } else {
                    index.record_tier2_block_crc_suspect();
                    false
                }
            }
        };
        if c.preview_bytes_over_budget {
            index.record_preview_over_budget();
        }
        let item = IndexItem {
            message_id: c.message_id_norm.clone(),
            content_hash: c.content_hash,
            strong_content_hash: c.strong_content_hash,
            tier2_eligible: eligible,
            source_key: c.path_key(),
            fp_body: c.fp_body,
            fp_header: c.fp_header,
            fp_recipients: c.fp_recipients,
            fp_attachments: c.fp_attachments,
            msg_ref,
        };
        let result = index.check_and_insert_item(item);
        if let DedupResult::DuplicateOf { .. } = &result {
            total_savings = total_savings.saturating_add(c.size as u64);
        }
        results.insert(key, result);
    }

    RebuildDedupOutcome {
        results,
        unique_count: index.unique_count,
        duplicate_count: index.duplicate_count,
        tier1_hits: index.tier1_hits,
        tier2_hits: index.tier2_hits,
        total_savings,
        grouping_stats: index.stats,
    }
}

/// Apply strict probe skips to per-file scan tallies (scan / unique-pst shared).
///
/// Increments `skipped`, decrements `messages` and `recoverable_messages`,
/// tallies reason, and flips `Opened → Partial` when any skip lands on a
/// previously clean open. Callers must also run
/// [`recompute_per_file_dup_from_results`] so per-file `duplicates` match the
/// post-probe index rebuild.
pub fn apply_strict_probe_skips_to_file_stats(files: &mut [FileScanStats], skips: &[SkipRecord]) {
    for skip in skips {
        if let Some(fs) = files.iter_mut().find(|f| f.path == skip.source_path) {
            fs.skipped = fs.skipped.saturating_add(1);
            tally_reason(&mut fs.skipped_by_reason, skip.reason);
            fs.recoverable_messages = fs.recoverable_messages.saturating_sub(1);
            // `messages` counts recoverable scan hits for the file (pre-probe);
            // a strict skip removes that message from recoverable output.
            fs.messages = fs.messages.saturating_sub(1);
            if fs.status == FileScanStatus::Opened {
                fs.status = FileScanStatus::Partial;
            }
        }
    }
}

/// Zero then recompute per-file `duplicates` from post-probe `DedupResult` map.
///
/// Keyed by `(source_pst basename, nid)` — same identity as
/// [`rebuild_dedup_results`]. Matches file stats via `FileScanStats.name`
/// (basename) when path-based lookup fails.
pub fn recompute_per_file_dup_from_results(
    files: &mut [FileScanStats],
    results: &HashMap<(String, u64), DedupResult>,
) {
    for fs in files.iter_mut() {
        fs.duplicates = 0;
    }
    for ((pst_name, _nid), result) in results {
        if let DedupResult::DuplicateOf { .. } = result {
            if let Some(fs) = files
                .iter_mut()
                .find(|f| f.name == *pst_name || f.path.ends_with(pst_name.as_str()))
            {
                fs.duplicates = fs.duplicates.saturating_add(1);
            }
        }
    }
}

/// Recompute per-file degraded tallies from post-probe candidates (best-effort).
///
/// Replaces per-file `degraded_messages` / `degraded_by_reason` from the current
/// candidate set so file-level output matches aggregate after phase-1b probe.
pub fn recompute_per_file_degraded_from_candidates(
    files: &mut [FileScanStats],
    candidates: &[RecoverableScanItem],
) {
    for fs in files.iter_mut() {
        fs.degraded_messages = 0;
        fs.degraded_by_reason.clear();
    }
    for c in candidates {
        if !c.integrity.degraded {
            continue;
        }
        if let Some(fs) = files
            .iter_mut()
            .find(|f| f.path == c.locus.source_path || f.name == c.locus.source_pst)
        {
            fs.degraded_messages = fs.degraded_messages.saturating_add(1);
            for r in &c.integrity.degraded_reasons {
                tally_reason(&mut fs.degraded_by_reason, *r);
            }
            if fs.status == FileScanStatus::Opened {
                fs.status = FileScanStatus::Partial;
            }
        }
    }
}

/// Recompute aggregate `partial_files` / `opened_files` from per-file stats.
pub fn recompute_file_status_counts(files: &[FileScanStats]) -> (u64, u64) {
    let partial_files = files
        .iter()
        .filter(|f| f.status == FileScanStatus::Partial)
        .count() as u64;
    let opened_files = files
        .iter()
        .filter(|f| f.status == FileScanStatus::Opened)
        .count() as u64;
    (partial_files, opened_files)
}

/// Validate and normalize input PST paths to absolute/canonical form.
pub fn resolve_pst_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        return Err(CliError::Msg("at least one PST path is required".into()));
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        if !p.exists() {
            return Err(CliError::PathNotFound(p.clone()));
        }
        let is_pst = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pst"))
            .unwrap_or(false);
        if !is_pst {
            return Err(CliError::NotPst(p.clone()));
        }
        // Absolute/normalized for provenance (SkipRecord.source_path / file stats).
        let resolved = crate::paths::resolve_cli_path(p)?;
        out.push(resolved.into_std_path_buf());
    }
    Ok(out)
}

/// Resolve integrity CSV path: explicit flag wins; else sidecar when `--csv` is set.
fn resolve_integrity_path(opts: &ScanOptions) -> Option<PathBuf> {
    if let Some(p) = &opts.integrity_csv {
        return Some(p.clone());
    }
    opts.csv.as_ref().map(|p| integrity_sidecar_path(p))
}

/// Scan one or more PST files and build the dedup index result.
///
/// Integrity and dedup CSVs (when enabled) stream from scan start — O(1) ledger memory.
/// Writers are always flushed before return.
pub fn run_scan(paths: &[PathBuf], opts: &ScanOptions) -> Result<ScanOutcome> {
    let start = Instant::now();
    // Test/CI hook only: force every message to hard-skip after a successful open
    // (env PST_DEDUPE_TEST_FORCE_SKIP=1). Not an operator-facing feature.
    let force_skip = std::env::var_os("PST_DEDUPE_TEST_FORCE_SKIP").is_some_and(|v| v == "1");
    let mut grouping = opts.grouping.clone();
    grouping.tier2_enabled = opts.enable_tier2;
    let mut index = DedupIndex::with_capacity_and_context(100_000, grouping.clone());
    // 0086: run-level attach-content digest budgets (only used when identity includes attach).
    let mut attach_hash_state = crate::attach_content_hash::AttachContentHashState::default();
    let mut all_rows: Vec<ReportRow> = Vec::new();
    let mut candidates: Vec<RecoverableScanItem> = Vec::new();
    let mut scan_order: u64 = 0;
    let mut file_stats: Vec<FileScanStats> = Vec::new();
    let mut total_savings: u64 = 0;
    let mut total_skipped: u64 = 0;
    let mut skipped_by_reason: BTreeMap<String, u64> = BTreeMap::new();
    let mut degraded_by_reason: BTreeMap<String, u64> = BTreeMap::new();
    let mut total_degraded: u64 = 0;
    let mut total_orphaned: u64 = 0;
    let mut crc_skips: u64 = 0;
    let mut skip_sample: Vec<SkipRecord> = Vec::new();
    let skip_limit = opts.skip_limit;
    // When deep attach preflight is on, defer dedup CSV / all_rows until after probe
    // so row integrity matches post-probe candidates (0074 P1-B).
    let defer_dedup_rows = opts.deep_attach_preflight;
    let mut buffered_rows: Vec<ReportRow> = Vec::new();

    // Open streaming writers at start (after path validation is caller's job).
    let integrity_path = resolve_integrity_path(opts);
    let mut integrity_wtr: Option<IntegrityCsvWriter> = match &integrity_path {
        Some(p) => Some(
            IntegrityCsvWriter::create(p).map_err(|source| CliError::CsvWrite {
                path: p.clone(),
                source: Box::new(source),
            })?,
        ),
        None => None,
    };

    let mut dedup_wtr: Option<StreamingCsvReportWriter> = match &opts.csv {
        Some(p) => {
            Some(
                StreamingCsvReportWriter::create(p).map_err(|source| CliError::CsvWrite {
                    path: p.clone(),
                    source,
                })?,
            )
        }
        None => None,
    };
    let csv_streamed = dedup_wtr.is_some();

    // D-0077-parallel-attrib: per-source deltas are exact only while sources are
    // processed sequentially on one thread. When 0079 parallelizes materialize,
    // read thread-local counters on the worker that did the work before merge.
    for (file_idx, path) in paths.iter().enumerate() {
        // Safe boundary: before each file open.
        if scan_cancel_requested(&opts.cancel) {
            break;
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("file_{file_idx}"));
        let path_str = path.display().to_string();

        // Snapshot integrity telemetry before this source (0077 attribution).
        // begin_source clears per-source distinct BID set so deltas are local.
        let telemetry_before = pst_reader::integrity_telemetry::begin_source();

        let mut pst = match PstFile::open(path) {
            Ok(p) => p,
            Err(source) => {
                let code = reason_from_pst_error(&source);
                // Prefer more specific open codes when applicable.
                let code = match &source {
                    pst_reader::PstError::AnsiPstNotSupported(_) => {
                        IntegrityReason::AnsiUnsupported
                    }
                    pst_reader::PstError::UnsupportedCryptMethod(_) => {
                        IntegrityReason::UnsupportedCrypt
                    }
                    pst_reader::PstError::InvalidMagic(_) => IntegrityReason::OpenFailed,
                    _ => {
                        if code == IntegrityReason::OpenFailed {
                            IntegrityReason::OpenFailed
                        } else if matches!(
                            code,
                            IntegrityReason::AnsiUnsupported | IntegrityReason::UnsupportedCrypt
                        ) {
                            code
                        } else {
                            IntegrityReason::OpenFailed
                        }
                    }
                };
                let tel = pst_reader::integrity_telemetry::end_source_delta(&telemetry_before);
                file_stats.push(FileScanStats {
                    path: path_str,
                    name: name.clone(),
                    status: FileScanStatus::Failed,
                    folders: 0,
                    messages: 0,
                    recoverable_messages: 0,
                    duplicates: 0,
                    skipped: 0,
                    skipped_by_reason: BTreeMap::new(),
                    degraded_messages: 0,
                    degraded_by_reason: BTreeMap::new(),
                    error_code: Some(code),
                    error: Some(source.to_string()),
                    page_crc_mismatches: tel.page_crc_mismatches,
                    block_crc_mismatches: tel.block_crc_mismatches,
                    block_bid_mismatches: tel.block_bid_mismatches,
                    distinct_bad_bids: tel.distinct_bad_bids,
                    crc_suspect_messages: 0,
                    page_reads: tel.page_reads,
                    block_reads: tel.block_reads,
                    poly_class_crc: false,
                });
                continue;
            }
        };

        let folders = match pst.folders() {
            Ok(f) => f,
            Err(source) => {
                let code = IntegrityReason::FolderWalkFailed;
                let tel = pst_reader::integrity_telemetry::end_source_delta(&telemetry_before);
                file_stats.push(FileScanStats {
                    path: path_str,
                    name: name.clone(),
                    status: FileScanStatus::Failed,
                    folders: 0,
                    messages: 0,
                    recoverable_messages: 0,
                    duplicates: 0,
                    skipped: 0,
                    skipped_by_reason: BTreeMap::new(),
                    degraded_messages: 0,
                    degraded_by_reason: BTreeMap::new(),
                    error_code: Some(code),
                    error: Some(source.to_string()),
                    page_crc_mismatches: tel.page_crc_mismatches,
                    block_crc_mismatches: tel.block_crc_mismatches,
                    block_bid_mismatches: tel.block_bid_mismatches,
                    distinct_bad_bids: tel.distinct_bad_bids,
                    crc_suspect_messages: 0,
                    page_reads: tel.page_reads,
                    block_reads: tel.block_reads,
                    poly_class_crc: false,
                });
                continue;
            }
        };

        let mut file_messages = 0u64;
        let mut file_duplicates = 0u64;
        let mut file_skipped = 0u64;
        let mut file_degraded = 0u64;
        let mut file_crc_suspect = 0u64;
        let mut file_skipped_by_reason: BTreeMap<String, u64> = BTreeMap::new();
        let mut file_degraded_by_reason: BTreeMap<String, u64> = BTreeMap::new();
        // Buffer degraded integrity rows until end-of-source so poly-class identity
        // strip can reconcile integrity.csv with candidates (0077 r2 P2-2).
        let mut file_integrity_degraded: Vec<SkipRecord> = Vec::new();
        let folder_count = folders.len() as u64;
        // Progress: emit at most every N messages or once per folder (§3.11).
        const PROGRESS_EVERY_MSGS: u64 = 500;
        let mut msgs_seen_file = 0u64;

        let mut cancelled_this_file = false;
        'folders: for folder in &folders {
            // Safe boundary: between folders.
            if scan_cancel_requested(&opts.cancel) {
                cancelled_this_file = true;
                break 'folders;
            }
            // Walker always gives folder paths today; is_orphaned residual D-0065-orphan-walk.
            let is_orphaned = false;
            let folder_path = folder.path.clone();
            tracing::info!(
                file = %name,
                folder = %folder_path,
                recoverable = file_messages,
                skipped = file_skipped,
                "scan progress"
            );

            for &msg_nid in &folder.message_nids {
                // Safe boundary: between messages in the main per-message loop.
                if scan_cancel_requested(&opts.cancel) {
                    cancelled_this_file = true;
                    break 'folders;
                }
                msgs_seen_file += 1;
                if msgs_seen_file.is_multiple_of(PROGRESS_EVERY_MSGS) {
                    tracing::info!(
                        file = %name,
                        folder = %folder_path,
                        msg_i = msgs_seen_file,
                        recoverable = file_messages,
                        skipped = file_skipped,
                        "scan progress"
                    );
                }
                if force_skip {
                    record_skip(
                        &mut SkipAccum {
                            integrity_wtr: &mut integrity_wtr,
                            skip_sample: &mut skip_sample,
                            skip_limit,
                            total_skipped: &mut total_skipped,
                            file_skipped: &mut file_skipped,
                            skipped_by_reason: &mut skipped_by_reason,
                            file_skipped_by_reason: &mut file_skipped_by_reason,
                            crc_skips: &mut crc_skips,
                        },
                        SkipRecord {
                            source_path: path_str.clone(),
                            source_pst: name.clone(),
                            folder_path: folder_path.clone(),
                            is_orphaned,
                            nid: msg_nid.0,
                            reason: IntegrityReason::MessageReadFailed,
                            detail: "test force skip".into(),
                            mode: opts.mode,
                        },
                    )?;
                    continue;
                }

                let read_opts = pst_reader::MessageReadOpts {
                    compute_body_digest: grouping.identity.is_strong(),
                };
                let props = match pst.read_message_properties_with_opts(msg_nid, read_opts) {
                    Ok(p) => p,
                    Err(e) => {
                        let mut reason = reason_from_pst_error(&e);
                        if reason == IntegrityReason::ReadError
                            || reason == IntegrityReason::OpenFailed
                        {
                            reason = IntegrityReason::MessageReadFailed;
                        }
                        // Structural/prop failures on hard PC load → message read failed mapping.
                        if matches!(
                            reason,
                            IntegrityReason::PropertyError | IntegrityReason::InvalidStructure
                        ) {
                            // keep mapped reason
                        }
                        record_skip(
                            &mut SkipAccum {
                                integrity_wtr: &mut integrity_wtr,
                                skip_sample: &mut skip_sample,
                                skip_limit,
                                total_skipped: &mut total_skipped,
                                file_skipped: &mut file_skipped,
                                skipped_by_reason: &mut skipped_by_reason,
                                file_skipped_by_reason: &mut file_skipped_by_reason,
                                crc_skips: &mut crc_skips,
                            },
                            SkipRecord {
                                source_path: path_str.clone(),
                                source_pst: name.clone(),
                                folder_path: folder_path.clone(),
                                is_orphaned,
                                nid: msg_nid.0,
                                reason,
                                detail: e.to_string(),
                                mode: opts.mode,
                            },
                        )?;
                        continue;
                    }
                };

                // Body integrity classification.
                let body_cls =
                    classify_body_flags(opts.mode, props.body_incomplete, props.body_unavailable);

                // Orphan classification (always false from walker today).
                let orphan_cls = if is_orphaned {
                    classify_orphaned(opts.mode)
                } else {
                    MessageClassification::Recoverable {
                        integrity: RecoverableIntegrity::clean(),
                    }
                };

                // Attachments — 0077: attach-meta block CRC ORs into message CRC_SUSPECT
                // (props scope already exited; wrap meta walk so attach PC reads taint).
                // 0086: when identity includes attach content, list_attachments (nid +
                // is_cloud_link) + full-stream digests; lower levels keep meta-only path.
                let mut attach_cls = MessageClassification::Recoverable {
                    integrity: RecoverableIntegrity::clean(),
                };
                let mut msg_crc_suspect = props.crc_suspect;
                let need_attach_content = grouping.identity.includes_attach_content();
                // body-recip-attach always attempts enumeration when attachments are
                // included — do not trust missing has_attachments alone as "zero attaches"
                // (Choice B: omit would false-merge). Meta-only levels keep the historical
                // has_attachments gate.
                let walk_attaches = opts.include_attachments
                    && (need_attach_content || props.has_attachments.unwrap_or(false));
                let attachments = if walk_attaches {
                    pst_reader::integrity_telemetry::with_crc_scope(&mut msg_crc_suspect, || {
                        if need_attach_content {
                            // Strict enumeration: any unreadable attach PC → Err (not partial Ok).
                            match pst.list_attachments_strict(msg_nid) {
                                Ok(atts) => {
                                    // Claimed attaches but empty strict list → fail closed.
                                    if atts.is_empty() && props.has_attachments == Some(true) {
                                        attach_cls = classify_attach_enum_for_identity(
                                            true,
                                            opts.mode,
                                            "has_attachments=true but attachment table empty after strict enumeration",
                                        );
                                        return Vec::new();
                                    }
                                    let mut out = Vec::with_capacity(atts.len());
                                    let budgets =
                                        crate::attach_content_hash::AttachContentHashBudgets {
                                            max_attaches: opts.strong_hash_attach_max_attaches,
                                            max_bytes: opts.strong_hash_attach_max_bytes,
                                            per_attach_max_bytes: opts
                                                .strong_hash_attach_per_attach_max_bytes,
                                        };
                                    for a in atts {
                                        if a.is_inline && grouping.ignore_inline_attachments {
                                            index.record_inline_attachment_ignored();
                                            // Still skip digest work for ignored inline.
                                            let mut info = AttachmentInfo::new(a.filename, a.size);
                                            info.is_inline = a.is_inline;
                                            out.push(info);
                                            continue;
                                        }
                                        let mut info =
                                            AttachmentInfo::new(a.filename.clone(), a.size);
                                        info.is_inline = a.is_inline;
                                        let parent = match pst.message_node_from_nbt(msg_nid) {
                                            Ok(p) => p,
                                            Err(_) => {
                                                let sentinel = dedup_engine::attach_unread_sentinel(
                                                    &a.filename,
                                                    a.size,
                                                );
                                                info.content_sha256 = Some(sentinel);
                                                index.record_strong_hash_attach_unread();
                                                out.push(info);
                                                continue;
                                            }
                                        };
                                        let result = crate::attach_content_hash::hash_attachment_for_identity(
                                            &mut pst,
                                            &parent,
                                            a.nid,
                                            &a.filename,
                                            a.size,
                                            a.attach_method,
                                            a.mime_tag.as_deref(),
                                            a.is_cloud_link,
                                            0,
                                            grouping.ignore_inline_attachments,
                                            &budgets,
                                            &mut attach_hash_state,
                                            &opts.cancel,
                                        );
                                        match result {
                                            crate::attach_content_hash::AttachDigestResult::Real {
                                                digest,
                                                bytes,
                                            } => {
                                                info.content_sha256 = Some(digest);
                                                index.record_strong_hash_attach_digested(bytes);
                                            }
                                            crate::attach_content_hash::AttachDigestResult::Embedded {
                                                digest,
                                                ..
                                            } => {
                                                // Embedded identity: QC uses embedded_* counters
                                                // flushed from attach_hash_state — not stream digests.
                                                info.content_sha256 = Some(digest);
                                            }
                                            crate::attach_content_hash::AttachDigestResult::Unread {
                                                sentinel,
                                            } => {
                                                info.content_sha256 = Some(sentinel);
                                                index.record_strong_hash_attach_unread();
                                            }
                                            crate::attach_content_hash::AttachDigestResult::DepthLimit {
                                                sentinel,
                                            } => {
                                                info.content_sha256 = Some(sentinel);
                                                // Depth-limit honesty: embedded_depth_limit flush only.
                                            }
                                        }
                                        out.push(info);
                                    }
                                    out
                                }
                                Err(e) => {
                                    // Fail closed under body-recip-attach: empty/partial
                                    // attach list would false-merge with fewer-attach peers.
                                    attach_cls = classify_attach_enum_for_identity(
                                        need_attach_content,
                                        opts.mode,
                                        e.to_string(),
                                    );
                                    Vec::new()
                                }
                            }
                        } else {
                            match pst.read_attachment_metadata(msg_nid) {
                                Ok(atts) => {
                                    let mut out = Vec::with_capacity(atts.len());
                                    for a in atts {
                                        if a.is_inline && grouping.ignore_inline_attachments {
                                            index.record_inline_attachment_ignored();
                                        }
                                        let mut info = AttachmentInfo::new(a.filename, a.size);
                                        info.is_inline = a.is_inline;
                                        out.push(info);
                                    }
                                    out
                                }
                                Err(e) => {
                                    attach_cls =
                                        classify_attach_meta_fail(opts.mode, e.to_string());
                                    Vec::new()
                                }
                            }
                        }
                    })
                } else {
                    Vec::new()
                };

                let classification = merge_recoverable([body_cls, orphan_cls, attach_cls]);

                match classification {
                    MessageClassification::Skip { reason, detail } => {
                        record_skip(
                            &mut SkipAccum {
                                integrity_wtr: &mut integrity_wtr,
                                skip_sample: &mut skip_sample,
                                skip_limit,
                                total_skipped: &mut total_skipped,
                                file_skipped: &mut file_skipped,
                                skipped_by_reason: &mut skipped_by_reason,
                                file_skipped_by_reason: &mut file_skipped_by_reason,
                                crc_skips: &mut crc_skips,
                            },
                            SkipRecord {
                                source_path: path_str.clone(),
                                source_pst: name.clone(),
                                folder_path: folder_path.clone(),
                                is_orphaned,
                                nid: msg_nid.0,
                                reason,
                                detail,
                                mode: opts.mode,
                            },
                        )?;
                        continue;
                    }
                    MessageClassification::Recoverable { mut integrity } => {
                        // 0077: CRC_SUSPECT from props scope and/or attach-meta scope.
                        if msg_crc_suspect {
                            if !integrity
                                .degraded_reasons
                                .contains(&IntegrityReason::CrcSuspect)
                            {
                                integrity.degraded_reasons.push(IntegrityReason::CrcSuspect);
                            }
                            integrity.degraded = true;
                            file_crc_suspect = file_crc_suspect.saturating_add(1);
                        }
                        if integrity.degraded {
                            file_degraded += 1;
                            total_degraded += 1;
                            for r in &integrity.degraded_reasons {
                                tally_reason(&mut degraded_by_reason, *r);
                                tally_reason(&mut file_degraded_by_reason, *r);
                            }
                            if integrity.is_orphaned {
                                total_orphaned += 1;
                            }
                            // Buffer degraded ledger rows (one per reason). Flushed after
                            // poly-class strip so integrity.csv matches keep-set identity.
                            for r in &integrity.degraded_reasons {
                                file_integrity_degraded.push(SkipRecord {
                                    source_path: path_str.clone(),
                                    source_pst: name.clone(),
                                    folder_path: folder_path.clone(),
                                    is_orphaned: integrity.is_orphaned,
                                    nid: msg_nid.0,
                                    reason: *r,
                                    detail: format!("degraded: {}", r.as_str()),
                                    mode: opts.mode,
                                });
                            }
                        }

                        // Map structured TC rows for Tier-2.5 identity (0082).
                        let structured_recips: Vec<dedup_engine::CanonicalRecipient> = props
                            .recipients
                            .iter()
                            .map(dedup_engine::CanonicalRecipient::from_reader)
                            .collect();
                        let strong_in = StrongHashInput {
                            identity: grouping.identity,
                            body_sha256: props.body_sha256.as_ref(),
                            body_char_len: props.body_char_len,
                            display_to: props.display_to.as_deref(),
                            display_cc: props.display_cc.as_deref(),
                            display_bcc: props.display_bcc.as_deref(),
                            recipients: if structured_recips.is_empty() {
                                None
                            } else {
                                Some(structured_recips.as_slice())
                            },
                            ignore_inline_attachments: grouping.ignore_inline_attachments,
                        };
                        let keys = hasher::compute_dedup_keys_ex(
                            props.message_id.as_deref(),
                            props.subject.as_deref(),
                            props.submit_time,
                            props.sender_email.as_deref(),
                            props.body_preview.as_deref(),
                            &attachments,
                            &strong_in,
                        );
                        if keys.preview_bytes_over_budget {
                            index.record_preview_over_budget();
                        }
                        // X.500 telemetry: table-sourced EX keys preferred; else display /O=.
                        let x500 = if !structured_recips.is_empty() {
                            structured_recips.iter().any(|r| r.identity_is_x500())
                        } else {
                            props
                                .display_to
                                .as_deref()
                                .is_some_and(dedup_engine::recipient_has_x500)
                                || props
                                    .display_cc
                                    .as_deref()
                                    .is_some_and(dedup_engine::recipient_has_x500)
                                || props
                                    .display_bcc
                                    .as_deref()
                                    .is_some_and(dedup_engine::recipient_has_x500)
                        };
                        if x500 {
                            index.record_x500_recipient_item();
                        }

                        let msg_ref = MessageRef {
                            pst_index: file_idx,
                            pst_name: name.clone(),
                            folder_path: folder_path.clone(),
                            nid: msg_nid.0,
                            subject: props.subject.clone().unwrap_or_default(),
                            submit_time: props.submit_time,
                            sender: props.sender_email.clone().unwrap_or_default(),
                            size: props.message_size.unwrap_or(0) as u32,
                        };

                        // Presence ≠ non-emptiness: clean empty body still binds (§3.3).
                        let has_body = props.body_preview.is_some();
                        let subject_ne = props
                            .subject
                            .as_deref()
                            .map(|s| !s.trim().is_empty())
                            .unwrap_or(false);
                        let sender_ne = props
                            .sender_email
                            .as_deref()
                            .map(|s| !s.trim().is_empty())
                            .unwrap_or(false);
                        let mut eligible = match hasher::tier2_eligibility(
                            props.body_incomplete,
                            props.body_unavailable,
                            has_body,
                            subject_ne,
                            props.submit_time.is_some(),
                            sender_ne,
                            attachments.len(),
                        ) {
                            Ok(()) => true,
                            Err(Tier2IneligibleReason::UnreadableBody) => {
                                if grouping.enforce_readable_body() {
                                    index.record_tier2_block_unreadable();
                                    false
                                } else {
                                    true
                                }
                            }
                            Err(Tier2IneligibleReason::Degenerate) => {
                                if grouping.enforce_readable_body() {
                                    index.record_tier2_block_degenerate();
                                    false
                                } else {
                                    true
                                }
                            }
                            Err(Tier2IneligibleReason::CrcSuspect) => {
                                // Not produced by tier2_eligibility(); handled below.
                                true
                            }
                        };
                        // 0077: CRC_SUSPECT is Tier-2 ineligible by default (split-only).
                        // Dual-rate poly reclassifies (clears) false-positive taint at
                        // end-of-source so keep-set rebuild binds without an auto-allow.
                        // Sparse corruption keeps taint; operator flag restores Tier-2.
                        if msg_crc_suspect
                            && !grouping.allow_crc_suspect_tier2_for(
                                &dedup_engine::keepset::path_compare_key(Path::new(&path_str)),
                            )
                        {
                            if eligible {
                                index.record_tier2_block_crc_suspect();
                            }
                            eligible = false;
                        }

                        let source_key = {
                            let p = std::path::Path::new(&path_str);
                            let s = p.to_string_lossy();
                            if cfg!(windows) {
                                s.to_lowercase()
                            } else {
                                s.into_owned()
                            }
                        };
                        let result = index.check_and_insert_item(IndexItem {
                            message_id: keys.message_id.clone(),
                            content_hash: keys.content_hash,
                            strong_content_hash: keys.strong_content_hash,
                            tier2_eligible: eligible,
                            source_key,
                            fp_body: keys.fp_body,
                            fp_header: keys.fp_header,
                            fp_recipients: keys.fp_recipients,
                            fp_attachments: keys.fp_attachments,
                            msg_ref: msg_ref.clone(),
                        });

                        if let DedupResult::DuplicateOf { .. } = &result {
                            file_duplicates += 1;
                            total_savings += msg_ref.size as u64;
                        }

                        // Deep attach preflight needs loci even when keep-set candidates
                        // are not otherwise retained (0074).
                        if opts.retain_candidates || opts.deep_attach_preflight {
                            // Display BCC or structured MAPI_BCC row (0082).
                            let has_bcc = props
                                .display_bcc
                                .as_deref()
                                .map(|s| !s.trim().is_empty())
                                .unwrap_or(false)
                                || props.recipients.iter().any(|r| {
                                    matches!(r.recipient_type, pst_reader::RecipientType::Bcc)
                                });
                            candidates.push(RecoverableScanItem {
                                locus: MessageLocus {
                                    source_path: path_str.clone(),
                                    source_pst: name.clone(),
                                    folder_path: folder_path.clone(),
                                    nid: msg_nid.0,
                                    is_orphaned: integrity.is_orphaned,
                                },
                                message_id_norm: keys.message_id.clone(),
                                content_hash: keys.content_hash,
                                size: msg_ref.size,
                                integrity: integrity.clone(),
                                scan_order,
                                submit_time: props.submit_time,
                                delivery_time: props.delivery_time,
                                has_bcc,
                                // Successfully read (including empty); not the same as non-empty.
                                has_body_preview: props.body_preview.is_some(),
                                subject_nonempty: props
                                    .subject
                                    .as_deref()
                                    .map(|s| !s.trim().is_empty())
                                    .unwrap_or(false),
                                sender_nonempty: props
                                    .sender_email
                                    .as_deref()
                                    .map(|s| !s.trim().is_empty())
                                    .unwrap_or(false),
                                attach_count: attachments.len() as u32,
                                body_sha256: props.body_sha256,
                                body_char_len: props.body_char_len,
                                display_to: props.display_to.clone(),
                                display_cc: props.display_cc.clone(),
                                display_bcc: props.display_bcc.clone(),
                                strong_content_hash: keys.strong_content_hash,
                                fp_header: keys.fp_header,
                                fp_body: keys.fp_body,
                                fp_recipients: keys.fp_recipients,
                                fp_attachments: keys.fp_attachments,
                                preview_bytes_over_budget: keys.preview_bytes_over_budget,
                            });
                            scan_order += 1;
                        }

                        let report_row = ReportRow {
                            message: msg_ref,
                            result,
                            integrity,
                        };

                        // Always buffer until end-of-source so poly reclassification
                        // clears false-positive CRC_SUSPECT before CSV write (0077).
                        // Deep-attach path still flushes after probe; otherwise flush
                        // at end of this source (after poly clear).
                        buffered_rows.push(report_row);
                        file_messages += 1;
                    }
                }
            }
        }

        // End-of-source flush so per-source deltas include all TLS counts (0077).
        pst_reader::integrity_telemetry::flush_summary();
        let tel = pst_reader::integrity_telemetry::end_source_delta(&telemetry_before);

        // Poly-class dual-rate gate (D-0077-systematic-poly / DoD-12 aspose):
        // Non-standard poly (aspose) hits **both** page and block CRC at high rate.
        // Dual-rate gate only:
        //   poly_class = (block_crc/block_reads ≥ 0.50) AND (page_crc/page_reads ≥ 0.50)
        //
        // Honest poly handling (final-gate P1):
        // - Dual-rate means computed≠stored is **not** evidence of corrupt bytes.
        // - **Clear** false-positive `CRC_SUSPECT` from candidates / integrity rows
        //   for this source (reclassify) so identity proceeds without taint and
        //   without a Tier-2 auto-allow exception (rule 10).
        // - High block alone (sparse / real data-block corruption) keeps taint and
        //   **blocks** Tier-2.
        // - `crc_suspect_messages` stays the pre-clear hit count (read evidence).
        // - Raw page/block/BID counters and rates are never zeroed.
        // - `export_risk` still sees raw `block_crc_read_rate`.
        // - Streaming unique may under-merge until keep-set rebuild (poly residual).
        let poly_class = is_poly_class_crc(
            tel.page_crc_mismatches,
            tel.page_reads,
            tel.block_crc_mismatches,
            tel.block_reads,
        );
        let crc_suspect_reported = file_crc_suspect;
        if poly_class {
            clear_poly_false_positive_crc_suspect(
                file_idx,
                &path_str,
                &mut candidates,
                &mut all_rows,
                &mut buffered_rows,
                &mut file_integrity_degraded,
                &mut PolyClearTallies {
                    file_degraded: &mut file_degraded,
                    total_degraded: &mut total_degraded,
                    file_degraded_by_reason: &mut file_degraded_by_reason,
                    degraded_by_reason: &mut degraded_by_reason,
                },
            );
        }

        // Flush buffered degraded integrity rows (CRC_SUSPECT filtered when poly).
        if let Some(wtr) = integrity_wtr.as_mut() {
            for row in &file_integrity_degraded {
                wtr.write_degraded(row)
                    .map_err(|source| CliError::CsvWrite {
                        path: integrity_path
                            .clone()
                            .unwrap_or_else(|| PathBuf::from("integrity.csv")),
                        source: Box::new(source),
                    })?;
            }
        }

        // Without deep-attach deferral, flush this source's report rows now
        // (after poly clear) so CSV matches candidates.
        if !defer_dedup_rows {
            let mut keep = Vec::new();
            for row in buffered_rows.drain(..) {
                if row.message.pst_index != file_idx {
                    keep.push(row);
                    continue;
                }
                if let Some(wtr) = dedup_wtr.as_mut() {
                    wtr.write_row(&row).map_err(|source| CliError::CsvWrite {
                        path: opts
                            .csv
                            .clone()
                            .unwrap_or_else(|| PathBuf::from("report.csv")),
                        source,
                    })?;
                }
                if opts.retain_rows {
                    all_rows.push(row);
                }
            }
            buffered_rows = keep;
        }

        // Cancel mid-file still yields Partial so operators see incomplete coverage.
        let status = if cancelled_this_file || file_skipped > 0 || file_degraded > 0 {
            FileScanStatus::Partial
        } else {
            FileScanStatus::Opened
        };

        file_stats.push(FileScanStats {
            path: path_str,
            name,
            status,
            folders: folder_count,
            messages: file_messages,
            recoverable_messages: file_messages,
            duplicates: file_duplicates,
            skipped: file_skipped,
            skipped_by_reason: file_skipped_by_reason,
            degraded_messages: file_degraded,
            degraded_by_reason: file_degraded_by_reason,
            error_code: None,
            error: None,
            page_crc_mismatches: tel.page_crc_mismatches,
            block_crc_mismatches: tel.block_crc_mismatches,
            block_bid_mismatches: tel.block_bid_mismatches,
            distinct_bad_bids: tel.distinct_bad_bids,
            crc_suspect_messages: crc_suspect_reported,
            page_reads: tel.page_reads,
            block_reads: tel.block_reads,
            poly_class_crc: poly_class,
        });

        if cancelled_this_file || scan_cancel_requested(&opts.cancel) {
            break;
        }
    }

    // Flush integrity early; dedup CSV may still receive post-probe reconciled rows.
    if let Some(wtr) = integrity_wtr.as_mut() {
        wtr.flush().map_err(|source| CliError::CsvWrite {
            path: integrity_path
                .clone()
                .unwrap_or_else(|| PathBuf::from("integrity.csv")),
            source: Box::new(source),
        })?;
    }
    if !defer_dedup_rows {
        if let Some(wtr) = dedup_wtr.as_mut() {
            wtr.flush().map_err(|source| CliError::CsvWrite {
                path: opts
                    .csv
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("report.csv")),
                source,
            })?;
        }
    }

    let failed_files = file_stats
        .iter()
        .filter(|f| f.status == FileScanStatus::Failed)
        .count() as u64;
    let mut partial_files = file_stats
        .iter()
        .filter(|f| f.status == FileScanStatus::Partial)
        .count() as u64;
    let mut opened_files = file_stats
        .iter()
        .filter(|f| f.status == FileScanStatus::Opened)
        .count() as u64;

    let mut recoverable_messages = index.total();

    // 0086: surface attach-content budget/cancel truncation on GroupingStats.
    if attach_hash_state.truncated {
        index.record_strong_hash_attach_truncated();
    }
    // 0090: embedded-msg identity honesty counters.
    for _ in 0..attach_hash_state.embedded_parsed {
        index.record_strong_hash_embedded_parsed();
    }
    for _ in 0..attach_hash_state.embedded_depth_limit {
        index.record_strong_hash_embedded_depth_limit();
    }
    for _ in 0..attach_hash_state.embedded_unparsed {
        index.record_strong_hash_embedded_unparsed();
    }

    // ── Deep attach preflight (0074, opt-in) ────────────────────────────────
    // Skipped when parents_only equivalent: include_attachments == false.
    let mut attach_attempted = 0u64;
    let mut attach_failed = 0u64;
    let mut attach_truncated = false;
    let mut attach_cancelled = false;
    let mut peer_probe_capped_groups = 0u64;
    let mut unique_count = index.unique_count;
    let mut duplicate_count = index.duplicate_count;
    let mut tier1_hits = index.tier1_hits;
    let mut tier2_hits = index.tier2_hits;
    let mut grouping_stats = index.stats.clone();
    let attach_probe_wanted = opts.deep_attach_preflight && opts.include_attachments;
    let attach_probe_enabled = attach_probe_wanted && !candidates.is_empty();
    let attach_level = if opts.deep_attach_level.is_empty() {
        "head".to_string()
    } else {
        opts.deep_attach_level.clone()
    };
    let mut probe_completed = false;
    // Strict post-probe rebuilt DedupResult map (P1-1); None for best-effort / no probe.
    let mut strict_rebuilt_results: Option<HashMap<(String, u64), DedupResult>> = None;

    if attach_probe_enabled && !scan_cancel_requested(&opts.cancel) {
        use crate::attach_probe::{probe_scan_items, ProbeBudgets, ProbeLevel, ProbeProgressCb};
        use std::io::Write;
        let level = ProbeLevel::parse(&attach_level).unwrap_or(ProbeLevel::Head);
        let budgets = ProbeBudgets {
            max_attaches: opts.deep_attach_max_attaches,
            max_probe_bytes: opts.deep_attach_max_probe_bytes,
            per_attach_max_bytes: opts.deep_attach_per_attach_max_bytes,
            max_probe_time_ms: opts.deep_attach_max_probe_time_ms,
            max_open_psts: opts.deep_attach_max_open_psts,
            max_peer_probes_per_group: opts.deep_attach_max_peer_probes_per_group,
        };
        // Capture pre-probe degraded reason sets so we only tally *new* probe reasons.
        let pre_degraded: Vec<(bool, HashSet<IntegrityReason>)> = candidates
            .iter()
            .map(|c| {
                (
                    c.integrity.degraded,
                    c.integrity.degraded_reasons.iter().copied().collect(),
                )
            })
            .collect();

        // CLI/library stderr progress sink (0074 P2-A).
        let progress_cb: Option<ProbeProgressCb> = Some(Box::new(move |attempted, bytes, base| {
            if attempted == 1 || attempted.is_multiple_of(500) {
                let _ = writeln!(
                    std::io::stderr(),
                    "scan: deep-attach-preflight: attempted={attempted} bytes={bytes} source={base}"
                );
            }
        }));

        let (probe_summary, _probe_cache) = probe_scan_items(
            &mut candidates,
            budgets,
            level,
            opts.mode,
            opts.cancel.clone(),
            progress_cb,
        );
        probe_completed = true;
        attach_attempted = probe_summary.attempted;
        attach_failed = probe_summary.failed;
        attach_truncated = probe_summary.truncated;
        attach_cancelled = probe_summary.cancelled || scan_cancel_requested(&opts.cancel);
        peer_probe_capped_groups = probe_summary.peer_probe_capped_groups;

        if attach_cancelled {
            // Cancel during probe is not attach corruption; leave tallies as pre-cancel.
            // Coverage incomplete is surfaced via attach_probe.cancelled / truncated.
        } else if opts.mode == ScanMode::Strict {
            // Strict: probe fail → skip (match classify_attach_meta_fail / body strict).
            let mut kept = Vec::with_capacity(candidates.len());
            for (i, c) in candidates.drain(..).enumerate() {
                let pre = pre_degraded
                    .get(i)
                    .map(|(_, s)| s)
                    .cloned()
                    .unwrap_or_default();
                let new_fail = c
                    .integrity
                    .degraded_reasons
                    .iter()
                    .copied()
                    .find(|r| r.is_attach_probe_fail() && !pre.contains(r));
                if let Some(reason) = new_fail {
                    let skip = SkipRecord {
                        source_path: c.locus.source_path.clone(),
                        source_pst: c.locus.source_pst.clone(),
                        folder_path: c.locus.folder_path.clone(),
                        is_orphaned: c.locus.is_orphaned,
                        nid: c.locus.nid,
                        reason,
                        detail: format!("strict deep-attach-preflight skip: {}", reason.as_str()),
                        mode: opts.mode,
                    };
                    total_skipped += 1;
                    tally_reason(&mut skipped_by_reason, reason);
                    if reason == IntegrityReason::CrcMismatch {
                        crc_skips += 1;
                    }
                    // Reconcile file-level skipped/messages/recoverable tallies (0074 P1).
                    if let Some(fs) = file_stats
                        .iter_mut()
                        .find(|f| f.path == c.locus.source_path)
                    {
                        fs.skipped = fs.skipped.saturating_add(1);
                        tally_reason(&mut fs.skipped_by_reason, reason);
                        fs.recoverable_messages = fs.recoverable_messages.saturating_sub(1);
                        fs.messages = fs.messages.saturating_sub(1);
                        if fs.status == FileScanStatus::Opened {
                            fs.status = FileScanStatus::Partial;
                        }
                    }
                    if skip_sample.len() < skip_limit {
                        skip_sample.push(skip.clone());
                    }
                    if let Some(wtr) = integrity_wtr.as_mut() {
                        wtr.write_skip(&skip).map_err(|source| CliError::CsvWrite {
                            path: integrity_path
                                .clone()
                                .unwrap_or_else(|| PathBuf::from("integrity.csv")),
                            source: Box::new(source),
                        })?;
                    }
                    // Drop from candidates (must not remain recoverable under strict).
                } else {
                    kept.push(c);
                }
            }
            candidates = kept;

            // Reconcile summary recoverable + unique/dup from remaining candidates.
            // Rebuild DedupResult map so buffered rows never DuplicateOf a skipped msg (P1-1).
            recoverable_messages = candidates.len() as u64;
            let mut message_refs: HashMap<(String, u64), MessageRef> = HashMap::new();
            if defer_dedup_rows {
                for row in &buffered_rows {
                    message_refs.insert(
                        (row.message.pst_name.clone(), row.message.nid),
                        row.message.clone(),
                    );
                }
            }
            let rebuild = rebuild_dedup_results_with_ctx(&candidates, &message_refs, &grouping);
            unique_count = rebuild.unique_count;
            duplicate_count = rebuild.duplicate_count;
            tier1_hits = rebuild.tier1_hits;
            tier2_hits = rebuild.tier2_hits;
            total_savings = rebuild.total_savings;
            // Post-strict rebuild re-inserts survivors under the same GroupingContext;
            // keep its stats so summary.grouping is not zeroed after deep-attach.
            // Rebuild re-inserts content hashes but does not re-digest attach streams,
            // so attach I/O counters would zero out — preserve scan-path attach stats.
            let pre_attach_unread = grouping_stats.strong_hash_attach_unread;
            let pre_attach_digested = grouping_stats.strong_hash_attach_digested;
            let pre_attach_bytes = grouping_stats.strong_hash_attach_bytes;
            let pre_attach_truncated = grouping_stats.strong_hash_attach_truncated;
            let pre_emb_parsed = grouping_stats.strong_hash_embedded_parsed;
            let pre_emb_depth = grouping_stats.strong_hash_embedded_depth_limit;
            let pre_emb_unparsed = grouping_stats.strong_hash_embedded_unparsed;
            grouping_stats = rebuild.grouping_stats;
            grouping_stats.strong_hash_attach_unread = pre_attach_unread;
            grouping_stats.strong_hash_attach_digested = pre_attach_digested;
            grouping_stats.strong_hash_attach_bytes = pre_attach_bytes;
            grouping_stats.strong_hash_attach_truncated = pre_attach_truncated;
            grouping_stats.strong_hash_embedded_parsed = pre_emb_parsed;
            grouping_stats.strong_hash_embedded_depth_limit = pre_emb_depth;
            grouping_stats.strong_hash_embedded_unparsed = pre_emb_unparsed;
            // Per-file messages already adjusted by apply_strict_probe_skips; rebuild dups.
            recompute_per_file_dup_from_results(&mut file_stats, &rebuild.results);
            // Stash for post-probe row.result rewrite (strict only).
            strict_rebuilt_results = Some(rebuild.results);

            // Recompute file open/partial counts after status flips.
            let (p, o) = recompute_file_status_counts(&file_stats);
            partial_files = p;
            opened_files = o;
        } else {
            // Best-effort: tally only newly added probe reasons; clean→degraded transitions.
            let empty_pre: HashSet<IntegrityReason> = HashSet::new();
            for (i, c) in candidates.iter().enumerate() {
                let (was_degraded, pre) = match pre_degraded.get(i) {
                    Some((d, s)) => (*d, s),
                    None => (false, &empty_pre),
                };
                let mut added_probe_fail = false;
                for r in &c.integrity.degraded_reasons {
                    if r.is_attach_probe_fail() && !pre.contains(r) {
                        tally_reason(&mut degraded_by_reason, *r);
                        // Per-file degraded tallies (0074 P1-B).
                        if let Some(fs) = file_stats
                            .iter_mut()
                            .find(|f| f.path == c.locus.source_path)
                        {
                            tally_reason(&mut fs.degraded_by_reason, *r);
                        }
                        added_probe_fail = true;
                    }
                }
                if added_probe_fail && !was_degraded && c.integrity.degraded {
                    total_degraded += 1;
                    if let Some(fs) = file_stats
                        .iter_mut()
                        .find(|f| f.path == c.locus.source_path)
                    {
                        fs.degraded_messages = fs.degraded_messages.saturating_add(1);
                        if fs.status == FileScanStatus::Opened {
                            fs.status = FileScanStatus::Partial;
                        }
                    }
                }
            }
            // Recompute file open/partial after best-effort status flips.
            partial_files = file_stats
                .iter()
                .filter(|f| f.status == FileScanStatus::Partial)
                .count() as u64;
            opened_files = file_stats
                .iter()
                .filter(|f| f.status == FileScanStatus::Opened)
                .count() as u64;
        }
    } else if attach_probe_wanted && scan_cancel_requested(&opts.cancel) {
        // Cancel before/without completing probe: coverage is incomplete (0074 P1-D).
        attach_cancelled = true;
    }

    // ── Post-probe row reconciliation (0074 P1-B) ───────────────────────────
    if defer_dedup_rows {
        // Key by (source_pst basename, nid) — matches MessageRef identity used in ReportRow.
        let cand_integrity: std::collections::HashMap<(String, u64), RecoverableIntegrity> =
            candidates
                .iter()
                .map(|c| {
                    (
                        (c.locus.source_pst.clone(), c.locus.nid),
                        c.integrity.clone(),
                    )
                })
                .collect();

        for mut row in buffered_rows.drain(..) {
            let key = (row.message.pst_name.clone(), row.message.nid);
            match cand_integrity.get(&key) {
                Some(integ) => {
                    // Best-effort (and kept strict): adopt post-probe integrity.
                    row.integrity = integ.clone();
                    // Strict: also adopt rebuilt DedupResult so no row still says
                    // DuplicateOf a probe-skipped winner (0074 P1-1).
                    if let Some(map) = strict_rebuilt_results.as_ref() {
                        if let Some(rebuilt) = map.get(&key) {
                            row.result = rebuilt.clone();
                        }
                    }
                }
                None if opts.mode == ScanMode::Strict && probe_completed && !attach_cancelled => {
                    // Strict probe skip: integrity CSV already has the skip; omit recoverable row.
                    continue;
                }
                None => {
                    // Cancel mid-probe or probe never ran: keep pre-probe integrity.
                }
            }

            if let Some(wtr) = dedup_wtr.as_mut() {
                wtr.write_row(&row).map_err(|source| CliError::CsvWrite {
                    path: opts
                        .csv
                        .clone()
                        .unwrap_or_else(|| PathBuf::from("report.csv")),
                    source,
                })?;
            }
            if opts.retain_rows {
                all_rows.push(row);
            }
        }
    }

    if let Some(wtr) = dedup_wtr.as_mut() {
        wtr.flush().map_err(|source| CliError::CsvWrite {
            path: opts
                .csv
                .clone()
                .unwrap_or_else(|| PathBuf::from("report.csv")),
            source,
        })?;
    }
    // Integrity may have received post-probe strict skips.
    if let Some(wtr) = integrity_wtr.as_mut() {
        wtr.flush().map_err(|source| CliError::CsvWrite {
            path: integrity_path
                .clone()
                .unwrap_or_else(|| PathBuf::from("integrity.csv")),
            source: Box::new(source),
        })?;
    }

    // Drop probe-only candidates when keep-set retention was not requested.
    if !opts.retain_candidates {
        candidates.clear();
    }

    let preflight = compute_preflight(&PreflightInputs {
        mode: opts.mode,
        recoverable: recoverable_messages,
        skipped: total_skipped,
        crc_skips,
        failed_files,
        input_file_count: paths.len() as u64,
        thresholds: opts.thresholds,
        attach_probe_enabled: attach_probe_wanted,
        attach_probe_level: if attach_probe_wanted {
            attach_level
        } else {
            "off".into()
        },
        attach_attempted,
        attach_failed,
        attach_probe_truncated: attach_truncated,
        peer_probe_capped_groups,
        attach_probe_cancelled: attach_cancelled,
    });

    let page_crc_total: u64 = file_stats.iter().map(|f| f.page_crc_mismatches).sum();
    let block_crc_total: u64 = file_stats.iter().map(|f| f.block_crc_mismatches).sum();
    let block_bid_total: u64 = file_stats.iter().map(|f| f.block_bid_mismatches).sum();
    let page_reads_total: u64 = file_stats.iter().map(|f| f.page_reads).sum();
    let block_reads_total: u64 = file_stats.iter().map(|f| f.block_reads).sum();
    let crc_suspect_total: u64 = file_stats.iter().map(|f| f.crc_suspect_messages).sum();
    let poly_class_crc_sources = file_stats.iter().filter(|f| f.poly_class_crc).count() as u64;
    let end_tel = pst_reader::integrity_telemetry::snapshot();
    let crc_sum = page_crc_total.saturating_add(block_crc_total);
    let block_crc_rate = crc_sum as f64 / (recoverable_messages.max(1) as f64);
    let read_denom = page_reads_total.saturating_add(block_reads_total).max(1) as f64;
    let block_crc_read_rate = (crc_sum as f64 / read_denom).clamp(0.0, 1.0);

    let summary = ScanSummary {
        schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: opts.mode,
        files: file_stats,
        total_messages: recoverable_messages,
        unique: unique_count,
        duplicates: duplicate_count,
        tier1_hits,
        tier2_hits,
        savings_bytes: total_savings,
        skipped: total_skipped,
        skipped_by_reason,
        recoverable_messages,
        degraded_messages: total_degraded,
        degraded_by_reason,
        orphaned_messages: total_orphaned,
        failed_files,
        partial_files,
        opened_files,
        duration_secs: start.elapsed().as_secs_f64(),
        preflight,
        skips: skip_sample,
        integrity_csv: integrity_path.map(|p| p.display().to_string()),
        grouping: grouping_stats,
        page_crc_mismatches: page_crc_total,
        block_crc_mismatches: block_crc_total,
        block_bid_mismatches: block_bid_total,
        distinct_bad_bids: end_tel.distinct_bad_bids,
        distinct_bad_bids_exact: end_tel.distinct_bad_bids_exact,
        crc_suspect_messages: crc_suspect_total,
        page_reads: page_reads_total,
        block_reads: block_reads_total,
        block_crc_rate,
        block_crc_read_rate,
        poly_class_crc_sources,
    };

    Ok(ScanOutcome {
        summary,
        rows: all_rows,
        candidates,
        csv_streamed,
    })
}

/// Poly-class CRC dual-rate gate (D-0077-systematic-poly / DoD-12 aspose).
///
/// Aspose-style non-standard CRC hits **both** page and block trails at high rate
/// (≥ 0.50). Real localized data-block corruption elevates block rate without a
/// matching page rate — those stores keep `CRC_SUSPECT` **and** block Tier-2.
///
/// When dual-rate fires, scan **clears** false-positive `CRC_SUSPECT` identity
/// taint (reclassify) so Tier-2 proceeds without an auto-allow exception. Raw
/// CRC counters remain for reporting and `export_risk`. Residual: true poly
/// fingerprint/allowlist vs dual-rate heuristic.
///
/// ```text
/// poly_class = (block_crc/block_reads ≥ 0.50) AND (page_crc/page_reads ≥ 0.50)
/// ```
pub(crate) fn is_poly_class_crc(
    page_crc: u64,
    page_reads: u64,
    block_crc: u64,
    block_reads: u64,
) -> bool {
    const POLY_RATE: f64 = 0.50;
    let page_rate = page_crc as f64 / (page_reads.max(1) as f64);
    let block_rate = block_crc as f64 / (block_reads.max(1) as f64);
    page_rate >= POLY_RATE && block_rate >= POLY_RATE
}

/// Mutable degraded tallies adjusted when poly clears false-positive CRC_SUSPECT.
struct PolyClearTallies<'a> {
    file_degraded: &'a mut u64,
    total_degraded: &'a mut u64,
    file_degraded_by_reason: &'a mut BTreeMap<String, u64>,
    degraded_by_reason: &'a mut BTreeMap<String, u64>,
}

/// Clear poly false-positive `CRC_SUSPECT` from identity surfaces for one source.
///
/// Non-standard CRC poly: computed≠stored is not evidence of corrupt bytes.
/// Identity proceeds without `CRC_SUSPECT`; raw page/block counters are untouched
/// (caller keeps them on `FileScanStats`). Sparse real corruption (not dual-rate)
/// never calls this helper and still taints / blocks Tier-2.
fn clear_poly_false_positive_crc_suspect(
    file_idx: usize,
    path_str: &str,
    candidates: &mut [RecoverableScanItem],
    all_rows: &mut [ReportRow],
    buffered_rows: &mut [ReportRow],
    file_integrity_degraded: &mut Vec<SkipRecord>,
    tallies: &mut PolyClearTallies<'_>,
) {
    let path_key = dedup_engine::keepset::path_compare_key(Path::new(path_str));
    let matches_source = |source_path: &str| {
        source_path == path_str
            || dedup_engine::keepset::path_compare_key(Path::new(source_path)) == path_key
    };

    fn strip_crc_suspect(integrity: &mut RecoverableIntegrity) -> bool {
        if !integrity
            .degraded_reasons
            .contains(&IntegrityReason::CrcSuspect)
        {
            return false;
        }
        integrity
            .degraded_reasons
            .retain(|r| *r != IntegrityReason::CrcSuspect);
        if integrity.degraded_reasons.is_empty() && !integrity.is_orphaned {
            integrity.degraded = false;
            true // fully cleared
        } else {
            integrity.degraded = !integrity.degraded_reasons.is_empty() || integrity.is_orphaned;
            false
        }
    }

    let mut fully_cleared = 0u64;
    let mut any_candidate = false;
    for c in candidates.iter_mut() {
        if !matches_source(&c.locus.source_path) {
            continue;
        }
        any_candidate = true;
        if strip_crc_suspect(&mut c.integrity) {
            fully_cleared = fully_cleared.saturating_add(1);
        }
    }

    // Report rows: match by pst_index (stable; never basename alone).
    for row in all_rows.iter_mut().chain(buffered_rows.iter_mut()) {
        if row.message.pst_index == file_idx {
            let _ = strip_crc_suspect(&mut row.integrity);
        }
    }

    // integrity.csv buffer: NIDs that had CRC_SUSPECT, then drop those rows.
    let nids_with_crc: HashSet<u64> = file_integrity_degraded
        .iter()
        .filter(|r| r.reason == IntegrityReason::CrcSuspect)
        .map(|r| r.nid)
        .collect();
    let crc_reason_cleared = nids_with_crc.len() as u64;
    file_integrity_degraded.retain(|r| r.reason != IntegrityReason::CrcSuspect);
    let nids_still_degraded: HashSet<u64> = file_integrity_degraded.iter().map(|r| r.nid).collect();
    if !any_candidate {
        // No retained candidates: derive fully-cleared from integrity buffer.
        fully_cleared = nids_with_crc
            .iter()
            .filter(|nid| !nids_still_degraded.contains(nid))
            .count() as u64;
    }

    *tallies.file_degraded = tallies.file_degraded.saturating_sub(fully_cleared);
    *tallies.total_degraded = tallies.total_degraded.saturating_sub(fully_cleared);

    let crc_key = IntegrityReason::CrcSuspect.as_str();
    if let Some(n) = tallies.file_degraded_by_reason.get_mut(crc_key) {
        *n = n.saturating_sub(crc_reason_cleared);
        if *n == 0 {
            tallies.file_degraded_by_reason.remove(crc_key);
        }
    }
    if let Some(n) = tallies.degraded_by_reason.get_mut(crc_key) {
        *n = n.saturating_sub(crc_reason_cleared);
        if *n == 0 {
            tallies.degraded_by_reason.remove(crc_key);
        }
    }
}

/// Mutable tallies updated on each skip (keeps `record_skip` arg count clippy-friendly).
struct SkipAccum<'a> {
    integrity_wtr: &'a mut Option<IntegrityCsvWriter>,
    skip_sample: &'a mut Vec<SkipRecord>,
    skip_limit: usize,
    total_skipped: &'a mut u64,
    file_skipped: &'a mut u64,
    skipped_by_reason: &'a mut BTreeMap<String, u64>,
    file_skipped_by_reason: &'a mut BTreeMap<String, u64>,
    crc_skips: &'a mut u64,
}

fn record_skip(acc: &mut SkipAccum<'_>, row: SkipRecord) -> Result<()> {
    *acc.total_skipped += 1;
    *acc.file_skipped += 1;
    tally_reason(acc.skipped_by_reason, row.reason);
    tally_reason(acc.file_skipped_by_reason, row.reason);
    if row.reason == IntegrityReason::CrcMismatch {
        *acc.crc_skips += 1;
    }
    if let Some(wtr) = acc.integrity_wtr.as_mut() {
        wtr.write_skip(&row).map_err(|source| CliError::CsvWrite {
            path: PathBuf::from("integrity.csv"),
            source: Box::new(source),
        })?;
    }
    if acc.skip_sample.len() < acc.skip_limit {
        acc.skip_sample.push(row);
    }
    Ok(())
}

/// Write CSV report + appended summary section.
///
/// When CSV was already streamed during `run_scan`, only the summary footer is appended.
pub fn write_report(path: &Path, outcome: &ScanOutcome) -> Result<()> {
    if !outcome.csv_streamed {
        dedup_engine::write_csv_report(path, &outcome.rows).map_err(|source| {
            CliError::CsvWrite {
                path: path.to_path_buf(),
                source,
            }
        })?;
    }
    write_summary_report(
        path,
        outcome.summary.total_messages,
        outcome.summary.unique,
        outcome.summary.duplicates,
        outcome.summary.tier1_hits,
        outcome.summary.tier2_hits,
        outcome.summary.savings_bytes,
    )
    .map_err(|source| CliError::CsvWrite {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// Collect duplicate rows (optionally capped).
pub fn collect_dups(outcome: &ScanOutcome, limit: Option<usize>) -> Vec<DupRow> {
    let mut out = Vec::new();
    for row in &outcome.rows {
        if let DedupResult::DuplicateOf { original, tier, .. } = &row.result {
            out.push(DupRow {
                tier: tier.to_string(),
                subject: row.message.subject.clone(),
                sender: row.message.sender.clone(),
                folder: row.message.folder_path.clone(),
                pst: row.message.pst_name.clone(),
                size: row.message.size,
                original_subject: original.subject.clone(),
                original_folder: original.folder_path.clone(),
                original_pst: original.pst_name.clone(),
            });
            if limit.is_some_and(|n| out.len() >= n) {
                break;
            }
        }
    }
    out
}

/// Evaluate exit policy after a completed scan (artifacts already flushed).
///
/// Returns `Ok(())` for success exit, or an error describing why exit should be non-zero.
pub fn evaluate_exit_policy(
    summary: &ScanSummary,
    opts: &ScanOptions,
) -> std::result::Result<(), String> {
    // Strict: any skip OR any partial/failed → non-success.
    if opts.mode == ScanMode::Strict
        && (summary.skipped > 0
            || summary.partial_files > 0
            || summary.failed_files > 0
            || summary.preflight.recommendation
                == dedup_engine::integrity::PreflightRecommendation::NotExportReady)
    {
        return Err(format!(
            "strict integrity failure: skipped={}, partial_files={}, failed_files={}",
            summary.skipped, summary.partial_files, summary.failed_files
        ));
    }

    // failed_files > 0 → non-success unless allow_failed_files and some recoverable.
    if summary.failed_files > 0 && !(opts.allow_failed_files && summary.recoverable_messages > 0) {
        return Err(format!("{} file(s) failed to scan", summary.failed_files));
    }

    // not_export_ready with zero recoverable → non-zero
    if summary.preflight.recommendation
        == dedup_engine::integrity::PreflightRecommendation::NotExportReady
        && summary.recoverable_messages == 0
    {
        return Err("not export ready: zero recoverable messages".into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dedup_engine::integrity::{
        classify_attach_meta_fail, classify_body_flags, classify_orphaned, IntegrityReason,
        MessageClassification, ScanMode,
    };
    use dedup_engine::keepset::MessageLocus;

    /// Dual-rate poly gate: high block alone keeps taint; both high → poly reclassify.
    #[test]
    fn poly_class_requires_dual_high_rate() {
        // Both ≥ 0.50 → poly-class (aspose): clear false-positive CRC_SUSPECT.
        assert!(is_poly_class_crc(50, 100, 50, 100));
        assert!(is_poly_class_crc(1, 1, 1, 1));
        // High block, low page → real data-block corruption; keep CRC_SUSPECT + block Tier-2.
        assert!(!is_poly_class_crc(1, 100, 50, 100));
        assert!(!is_poly_class_crc(0, 100, 100, 100));
        // High page, low block → not poly reclassify.
        assert!(!is_poly_class_crc(50, 100, 1, 100));
        // Both sparse.
        assert!(!is_poly_class_crc(1, 100, 1, 100));
        assert!(!is_poly_class_crc(0, 0, 0, 0));
    }

    /// DoD-4: pre-0077 scan JSON (missing new CRC fields) still deserializes with defaults.
    #[test]
    fn pre_0077_scan_json_deserializes_with_defaults() {
        // Minimal pre-0077 shape: no page_crc_* / block_crc_* / crc_suspect_* fields.
        let json = r#"{
            "schema": "scan_integrity_v1",
            "mode": "best-effort",
            "files": [{
                "path": "C:\\mail\\a.pst",
                "name": "a.pst",
                "status": "opened",
                "folders": 1,
                "messages": 2,
                "recoverable_messages": 2,
                "duplicates": 0,
                "skipped": 0,
                "skipped_by_reason": {},
                "degraded_messages": 0,
                "degraded_by_reason": {},
                "error_code": null,
                "error": null
            }],
            "total_messages": 2,
            "unique": 2,
            "duplicates": 0,
            "tier1_hits": 0,
            "tier2_hits": 0,
            "savings_bytes": 0,
            "skipped": 0,
            "skipped_by_reason": {},
            "recoverable_messages": 2,
            "degraded_messages": 0,
            "degraded_by_reason": {},
            "orphaned_messages": 0,
            "failed_files": 0,
            "partial_files": 0,
            "opened_files": 1,
            "duration_secs": 0.01,
            "preflight": {
                "schema": "preflight_v1",
                "mode": "best-effort",
                "skip_rate": 0.0,
                "crc_skip_rate": 0.0,
                "failed_file_rate": 0.0,
                "recommendation": "ok",
                "reasons": [],
                "thresholds": {
                    "max_skip_rate": 0.05,
                    "max_crc_skip_rate": 0.01,
                    "max_failed_file_rate": 0.0
                }
            }
        }"#;
        let summary: ScanSummary =
            serde_json::from_str(json).expect("pre-0077 JSON should deserialize");
        assert_eq!(summary.total_messages, 2);
        assert_eq!(summary.page_crc_mismatches, 0);
        assert_eq!(summary.block_crc_mismatches, 0);
        assert_eq!(summary.block_bid_mismatches, 0);
        assert_eq!(summary.distinct_bad_bids, 0);
        // #[serde(default)] on bool → false when absent.
        assert!(!summary.distinct_bad_bids_exact);
        assert_eq!(summary.crc_suspect_messages, 0);
        assert_eq!(summary.page_reads, 0);
        assert_eq!(summary.block_reads, 0);
        assert!((summary.block_crc_rate - 0.0).abs() < f64::EPSILON);
        assert!((summary.block_crc_read_rate - 0.0).abs() < f64::EPSILON);
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.files[0].page_crc_mismatches, 0);
        assert_eq!(summary.files[0].distinct_bad_bids, 0);
        assert!(!summary.files[0].poly_class_crc);
        assert_eq!(summary.poly_class_crc_sources, 0);
    }

    /// Winner skipped by strict probe: surviving duplicate must become Unique (P1-1).
    #[test]
    fn rebuild_dedup_promotes_survivor_when_winner_dropped() {
        let winner_ref = MessageRef {
            pst_index: 0,
            pst_name: "a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            subject: "Hello".into(),
            submit_time: None,
            sender: "a@x".into(),
            size: 100,
        };
        let dup_ref = MessageRef {
            pst_index: 0,
            pst_name: "a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 2,
            subject: "Hello".into(),
            submit_time: None,
            sender: "a@x".into(),
            size: 100,
        };
        // Pre-probe: both share message-id; winner first.
        let mut pre = DedupIndex::with_tier2(true);
        assert!(matches!(
            pre.check_and_insert(Some("mid-1"), [9; 32], winner_ref.clone()),
            DedupResult::Unique
        ));
        assert!(matches!(
            pre.check_and_insert(Some("mid-1"), [9; 32], dup_ref.clone()),
            DedupResult::DuplicateOf { .. }
        ));

        // Strict probe drops winner; only survivor remains.
        let survivor = RecoverableScanItem {
            locus: MessageLocus {
                source_path: r"C:\mail\a.pst".into(),
                source_pst: "a.pst".into(),
                folder_path: "Inbox".into(),
                nid: 2,
                is_orphaned: false,
            },
            message_id_norm: Some("mid-1".into()),
            content_hash: [9; 32],
            size: 100,
            integrity: RecoverableIntegrity::clean(),
            scan_order: 1,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
            has_body_preview: true,
            subject_nonempty: true,
            sender_nonempty: true,
            attach_count: 0,
            body_sha256: None,
            body_char_len: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            strong_content_hash: None,
            fp_header: 0,
            fp_body: 0,
            fp_recipients: 0,
            fp_attachments: 0,
            preview_bytes_over_budget: false,
        };
        let mut refs = HashMap::new();
        refs.insert(("a.pst".into(), 2u64), dup_ref);
        let rebuild = rebuild_dedup_results(&[survivor], &refs, true);
        assert_eq!(rebuild.unique_count, 1);
        assert_eq!(rebuild.duplicate_count, 0);
        let result = rebuild
            .results
            .get(&("a.pst".into(), 2u64))
            .expect("survivor result");
        assert!(
            matches!(result, DedupResult::Unique),
            "survivor must not remain DuplicateOf skipped winner: {result:?}"
        );
    }

    /// Two survivors keep first-in-scan-order as Unique (P1-1 pure rebuild).
    #[test]
    fn rebuild_dedup_preserves_scan_order_winner() {
        let c1 = RecoverableScanItem {
            locus: MessageLocus {
                source_path: r"C:\mail\a.pst".into(),
                source_pst: "a.pst".into(),
                folder_path: "Inbox".into(),
                nid: 10,
                is_orphaned: false,
            },
            message_id_norm: Some("same".into()),
            content_hash: [1; 32],
            size: 50,
            integrity: RecoverableIntegrity::clean(),
            scan_order: 0,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
            has_body_preview: true,
            subject_nonempty: true,
            sender_nonempty: true,
            attach_count: 0,
            body_sha256: None,
            body_char_len: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            strong_content_hash: None,
            fp_header: 0,
            fp_body: 0,
            fp_recipients: 0,
            fp_attachments: 0,
            preview_bytes_over_budget: false,
        };
        let c2 = RecoverableScanItem {
            locus: MessageLocus {
                source_path: r"C:\mail\a.pst".into(),
                source_pst: "a.pst".into(),
                folder_path: "Inbox".into(),
                nid: 20,
                is_orphaned: false,
            },
            message_id_norm: Some("same".into()),
            content_hash: [1; 32],
            size: 50,
            integrity: RecoverableIntegrity::clean(),
            scan_order: 1,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
            has_body_preview: true,
            subject_nonempty: true,
            sender_nonempty: true,
            attach_count: 0,
            body_sha256: None,
            body_char_len: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            strong_content_hash: None,
            fp_header: 0,
            fp_body: 0,
            fp_recipients: 0,
            fp_attachments: 0,
            preview_bytes_over_budget: false,
        };
        // Pass in reverse order; scan_order must still make nid=10 the winner.
        let rebuild = rebuild_dedup_results(&[c2, c1], &HashMap::new(), true);
        assert_eq!(rebuild.unique_count, 1);
        assert_eq!(rebuild.duplicate_count, 1);
        assert!(matches!(
            rebuild.results.get(&("a.pst".into(), 10u64)),
            Some(DedupResult::Unique)
        ));
        match rebuild.results.get(&("a.pst".into(), 20u64)) {
            Some(DedupResult::DuplicateOf { original, .. }) => {
                assert_eq!(original.nid, 10);
            }
            other => panic!("expected DuplicateOf winner 10, got {other:?}"),
        }
    }

    /// Per-file tally helper after strict probe skips (P1-2).
    #[test]
    fn apply_strict_probe_skips_updates_file_stats() {
        let mut files = vec![FileScanStats {
            path: r"C:\mail\a.pst".into(),
            name: "a.pst".into(),
            status: FileScanStatus::Opened,
            folders: 1,
            messages: 2,
            recoverable_messages: 2,
            duplicates: 0,
            skipped: 0,
            skipped_by_reason: BTreeMap::new(),
            degraded_messages: 0,
            degraded_by_reason: BTreeMap::new(),
            error_code: None,
            error: None,
            page_crc_mismatches: 0,
            block_crc_mismatches: 0,
            block_bid_mismatches: 0,
            distinct_bad_bids: 0,
            crc_suspect_messages: 0,
            page_reads: 0,
            block_reads: 0,
            poly_class_crc: false,
        }];
        let skips = vec![SkipRecord {
            source_path: r"C:\mail\a.pst".into(),
            source_pst: "a.pst".into(),
            folder_path: "Inbox".into(),
            is_orphaned: false,
            nid: 1,
            reason: IntegrityReason::AttachStreamOpenFailed,
            detail: "strict deep-attach-preflight skip".into(),
            mode: ScanMode::Strict,
        }];
        apply_strict_probe_skips_to_file_stats(&mut files, &skips);
        assert_eq!(files[0].skipped, 1);
        assert_eq!(files[0].recoverable_messages, 1);
        assert_eq!(files[0].messages, 1);
        assert_eq!(files[0].status, FileScanStatus::Partial);
        assert_eq!(
            files[0]
                .skipped_by_reason
                .get(IntegrityReason::AttachStreamOpenFailed.as_str())
                .copied(),
            Some(1)
        );
        let (partial, opened) = recompute_file_status_counts(&files);
        assert_eq!(partial, 1);
        assert_eq!(opened, 0);
    }

    #[test]
    fn recompute_per_file_dup_and_degraded_from_candidates() {
        let mut files = vec![FileScanStats {
            path: r"C:\mail\a.pst".into(),
            name: "a.pst".into(),
            status: FileScanStatus::Opened,
            folders: 1,
            messages: 2,
            recoverable_messages: 2,
            duplicates: 99, // stale pre-probe
            skipped: 0,
            skipped_by_reason: BTreeMap::new(),
            degraded_messages: 0,
            degraded_by_reason: BTreeMap::new(),
            error_code: None,
            error: None,
            page_crc_mismatches: 0,
            block_crc_mismatches: 0,
            block_bid_mismatches: 0,
            distinct_bad_bids: 0,
            crc_suspect_messages: 0,
            page_reads: 0,
            block_reads: 0,
            poly_class_crc: false,
        }];
        let mut results = HashMap::new();
        results.insert(("a.pst".into(), 1u64), DedupResult::Unique);
        results.insert(
            ("a.pst".into(), 2u64),
            DedupResult::DuplicateOf {
                original: MessageRef {
                    pst_index: 0,
                    pst_name: "a.pst".into(),
                    folder_path: "Inbox".into(),
                    nid: 1,
                    subject: String::new(),
                    submit_time: None,
                    sender: String::new(),
                    size: 10,
                },
                tier: dedup_engine::DedupTier::MessageId,
                bound_by: dedup_engine::BoundBy::MessageId,
            },
        );
        recompute_per_file_dup_from_results(&mut files, &results);
        assert_eq!(files[0].duplicates, 1);

        let cand = RecoverableScanItem {
            locus: MessageLocus {
                source_path: r"C:\mail\a.pst".into(),
                source_pst: "a.pst".into(),
                folder_path: "Inbox".into(),
                nid: 2,
                is_orphaned: false,
            },
            message_id_norm: None,
            content_hash: [0u8; 32],
            size: 10,
            integrity: RecoverableIntegrity::with_degraded(
                vec![IntegrityReason::AttachStreamReadFailed],
                false,
            ),
            scan_order: 1,
            submit_time: None,
            delivery_time: None,
            has_bcc: false,
            has_body_preview: true,
            subject_nonempty: true,
            sender_nonempty: true,
            attach_count: 0,
            body_sha256: None,
            body_char_len: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            strong_content_hash: None,
            fp_header: 0,
            fp_body: 0,
            fp_recipients: 0,
            fp_attachments: 0,
            preview_bytes_over_budget: false,
        };
        recompute_per_file_degraded_from_candidates(&mut files, &[cand]);
        assert_eq!(files[0].degraded_messages, 1);
        assert_eq!(files[0].status, FileScanStatus::Partial);
        assert_eq!(
            files[0]
                .degraded_by_reason
                .get(IntegrityReason::AttachStreamReadFailed.as_str())
                .copied(),
            Some(1)
        );
    }

    #[test]
    fn best_effort_attach_is_degraded_keep() {
        let c = classify_attach_meta_fail(ScanMode::BestEffort, "boom");
        match c {
            MessageClassification::Recoverable { integrity } => {
                assert!(integrity.degraded);
                assert!(integrity
                    .degraded_reasons
                    .contains(&IntegrityReason::AttachMetaFailed));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn strict_attach_is_skip() {
        let c = classify_attach_meta_fail(ScanMode::Strict, "boom");
        match c {
            MessageClassification::Skip { reason, .. } => {
                assert_eq!(reason, IntegrityReason::AttachMetaFailed);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn body_flags_mode_matrix() {
        // Intentional preview: clean.
        let clean = classify_body_flags(ScanMode::BestEffort, false, false);
        assert!(matches!(
            clean,
            MessageClassification::Recoverable { integrity } if !integrity.degraded
        ));
        // Truncated best-effort.
        let t = classify_body_flags(ScanMode::BestEffort, true, false);
        assert!(matches!(
            t,
            MessageClassification::Recoverable { integrity }
                if integrity.degraded_reasons.contains(&IntegrityReason::BodyTruncated)
        ));
        // Truncated strict.
        let ts = classify_body_flags(ScanMode::Strict, true, false);
        assert!(matches!(
            ts,
            MessageClassification::Skip {
                reason: IntegrityReason::BodyTruncated,
                ..
            }
        ));
        // Unavailable.
        let u = classify_body_flags(ScanMode::BestEffort, false, true);
        assert!(matches!(
            u,
            MessageClassification::Recoverable { integrity }
                if integrity.degraded_reasons.contains(&IntegrityReason::BodyUnavailable)
        ));
    }

    #[test]
    fn orphan_vs_root_semantics() {
        // Root: empty path + is_orphaned=false is NOT orphan.
        let root = RecoverableIntegrity::clean();
        assert!(!root.is_orphaned);
        // Orphan: explicit flag.
        let o = classify_orphaned(ScanMode::BestEffort);
        match o {
            MessageClassification::Recoverable { integrity } => {
                assert!(integrity.is_orphaned);
                assert!(integrity
                    .degraded_reasons
                    .contains(&IntegrityReason::OrphanedNode));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn exit_policy_allow_failed_files() {
        use dedup_engine::integrity::{
            compute_preflight, PreflightInputs, PreflightRecommendation, SCAN_INTEGRITY_SCHEMA,
        };
        let preflight = compute_preflight(&PreflightInputs::without_attach_probe(
            ScanMode::BestEffort,
            10,
            0,
            0,
            1,
            2,
            IntegrityThresholds::default(),
        ));
        assert_ne!(preflight.recommendation, PreflightRecommendation::Ok);

        let summary = ScanSummary {
            schema: SCAN_INTEGRITY_SCHEMA.to_string(),
            mode: ScanMode::BestEffort,
            files: vec![],
            total_messages: 10,
            unique: 10,
            duplicates: 0,
            tier1_hits: 0,
            tier2_hits: 0,
            savings_bytes: 0,
            skipped: 0,
            skipped_by_reason: BTreeMap::new(),
            recoverable_messages: 10,
            degraded_messages: 0,
            degraded_by_reason: BTreeMap::new(),
            orphaned_messages: 0,
            failed_files: 1,
            partial_files: 0,
            opened_files: 1,
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
        let mut opts = ScanOptions::default();
        assert!(evaluate_exit_policy(&summary, &opts).is_err());
        opts.allow_failed_files = true;
        assert!(evaluate_exit_policy(&summary, &opts).is_ok());
    }

    #[test]
    fn exit_policy_strict_on_skip() {
        use dedup_engine::integrity::{compute_preflight, PreflightInputs, SCAN_INTEGRITY_SCHEMA};
        let preflight = compute_preflight(&PreflightInputs::without_attach_probe(
            ScanMode::Strict,
            10,
            1,
            0,
            0,
            1,
            IntegrityThresholds::default(),
        ));
        let summary = ScanSummary {
            schema: SCAN_INTEGRITY_SCHEMA.to_string(),
            mode: ScanMode::Strict,
            files: vec![],
            total_messages: 10,
            unique: 10,
            duplicates: 0,
            tier1_hits: 0,
            tier2_hits: 0,
            savings_bytes: 0,
            skipped: 1,
            skipped_by_reason: BTreeMap::new(),
            recoverable_messages: 10,
            degraded_messages: 0,
            degraded_by_reason: BTreeMap::new(),
            orphaned_messages: 0,
            failed_files: 0,
            partial_files: 1,
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
        let opts = ScanOptions {
            mode: ScanMode::Strict,
            ..Default::default()
        };
        assert!(evaluate_exit_policy(&summary, &opts).is_err());
    }

    #[test]
    fn run_scan_cancel_before_open_returns_empty_partial() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let sample =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst");
        assert!(
            sample.is_file(),
            "required fixture missing (fail-closed): {}",
            sample.display()
        );
        let cancel = Arc::new(AtomicBool::new(true));
        let opts = ScanOptions {
            cancel: Some(cancel),
            retain_rows: false,
            retain_candidates: true,
            ..Default::default()
        };
        let outcome = run_scan(&[sample], &opts).expect("cancel must return Ok partial");
        // Cancelled before first open: no file stats, no invented candidates.
        assert!(outcome.summary.files.is_empty());
        assert!(outcome.candidates.is_empty());
        assert_eq!(outcome.summary.recoverable_messages, 0);
    }

    /// Cancel with deep_attach_preflight on: attach_probe must report cancelled + incomplete
    /// even when the probe phase never starts (0074 P1-D).
    #[test]
    fn deep_attach_cancel_before_probe_marks_attach_probe_cancelled() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let sample =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst");
        assert!(
            sample.is_file(),
            "required fixture missing (fail-closed): {}",
            sample.display()
        );
        let cancel = Arc::new(AtomicBool::new(true));
        let opts = ScanOptions {
            cancel: Some(cancel),
            deep_attach_preflight: true,
            deep_attach_level: "head".into(),
            retain_rows: false,
            retain_candidates: true,
            include_attachments: true,
            ..Default::default()
        };
        let outcome = run_scan(&[sample], &opts).expect("cancel must return Ok partial");
        assert!(
            outcome.summary.preflight.attach_probe.enabled,
            "deep flag must enable attach_probe block"
        );
        assert!(
            outcome.summary.preflight.attach_probe.cancelled,
            "cancel-before-probe must set attach_probe.cancelled"
        );
        assert_eq!(
            outcome.summary.preflight.attach_probe.attempted, 0,
            "probe never started"
        );
        assert!(
            outcome
                .summary
                .preflight
                .attach_probe
                .coverage_note
                .contains("cancel")
                || outcome.summary.preflight.attach_probe.truncated,
            "coverage must be incomplete: {}",
            outcome.summary.preflight.attach_probe.coverage_note
        );
    }
}
