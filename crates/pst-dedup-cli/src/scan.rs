//! Shared scan orchestration for CLI commands (track 0065 integrity).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use dedup_engine::{
    hasher::{self, AttachmentInfo},
    integrity::{
        classify_attach_meta_fail, classify_body_flags, classify_orphaned, compute_preflight,
        integrity_sidecar_path, merge_recoverable, reason_from_pst_error, tally_reason,
        FileScanStatus, IntegrityCsvWriter, IntegrityLedgerWriter, IntegrityReason,
        IntegrityThresholds, MessageClassification, PreflightInputs, PreflightReport,
        RecoverableIntegrity, ScanMode, SkipRecord, SCAN_INTEGRITY_SCHEMA,
    },
    keepset::{MessageLocus, RecoverableScanItem},
    report::{write_summary_report, ReportRow, StreamingCsvReportWriter},
    DedupIndex, DedupResult, MessageRef,
};
use pst_reader::PstFile;
use serde::Serialize;

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
        }
    }
}

/// Per-file outcome with integrity status.
#[derive(Debug, Clone, Serialize)]
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
}

/// Full scan outcome (schema `scan_integrity_v1`).
#[derive(Debug, Clone, Serialize)]
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skips: Vec<SkipRecord>,
    /// Path of streaming integrity CSV if written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity_csv: Option<String>,
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
    let mut index = DedupIndex::with_capacity_and_tier2(100_000, opts.enable_tier2);
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

    for (file_idx, path) in paths.iter().enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("file_{file_idx}"));
        let path_str = path.display().to_string();

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
                });
                continue;
            }
        };

        let folders = match pst.folders() {
            Ok(f) => f,
            Err(source) => {
                let code = IntegrityReason::FolderWalkFailed;
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
                });
                continue;
            }
        };

        let mut file_messages = 0u64;
        let mut file_duplicates = 0u64;
        let mut file_skipped = 0u64;
        let mut file_degraded = 0u64;
        let mut file_skipped_by_reason: BTreeMap<String, u64> = BTreeMap::new();
        let mut file_degraded_by_reason: BTreeMap<String, u64> = BTreeMap::new();
        let folder_count = folders.len() as u64;
        // Progress: emit at most every N messages or once per folder (§3.11).
        const PROGRESS_EVERY_MSGS: u64 = 500;
        let mut msgs_seen_file = 0u64;

        for folder in &folders {
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

                let props = match pst.read_message_properties(msg_nid) {
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

                // Attachments.
                let mut attach_cls = MessageClassification::Recoverable {
                    integrity: RecoverableIntegrity::clean(),
                };
                let attachments =
                    if opts.include_attachments && props.has_attachments.unwrap_or(false) {
                        match pst.read_attachment_metadata(msg_nid) {
                            Ok(atts) => atts
                                .into_iter()
                                .map(|a| AttachmentInfo {
                                    filename: a.filename,
                                    size: a.size,
                                })
                                .collect(),
                            Err(e) => {
                                attach_cls = classify_attach_meta_fail(opts.mode, e.to_string());
                                Vec::new()
                            }
                        }
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
                    MessageClassification::Recoverable { integrity } => {
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
                            // Stream degraded ledger rows (one per reason for operator clarity).
                            if let Some(wtr) = integrity_wtr.as_mut() {
                                for r in &integrity.degraded_reasons {
                                    let row = SkipRecord {
                                        source_path: path_str.clone(),
                                        source_pst: name.clone(),
                                        folder_path: folder_path.clone(),
                                        is_orphaned: integrity.is_orphaned,
                                        nid: msg_nid.0,
                                        reason: *r,
                                        detail: format!("degraded: {}", r.as_str()),
                                        mode: opts.mode,
                                    };
                                    wtr.write_degraded(&row).map_err(|source| {
                                        CliError::CsvWrite {
                                            path: integrity_path
                                                .clone()
                                                .unwrap_or_else(|| PathBuf::from("integrity.csv")),
                                            source: Box::new(source),
                                        }
                                    })?;
                                }
                            }
                        }

                        let keys = hasher::compute_dedup_keys(
                            props.message_id.as_deref(),
                            props.subject.as_deref(),
                            props.submit_time,
                            props.sender_email.as_deref(),
                            props.body_preview.as_deref(),
                            &attachments,
                        );

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

                        let result = index.check_and_insert(
                            keys.message_id.as_deref(),
                            keys.content_hash,
                            msg_ref.clone(),
                        );

                        if let DedupResult::DuplicateOf { .. } = &result {
                            file_duplicates += 1;
                            total_savings += msg_ref.size as u64;
                        }

                        if opts.retain_candidates {
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
                            });
                            scan_order += 1;
                        }

                        let report_row = ReportRow {
                            message: msg_ref,
                            result,
                            integrity,
                        };

                        if let Some(wtr) = dedup_wtr.as_mut() {
                            wtr.write_row(&report_row)
                                .map_err(|source| CliError::CsvWrite {
                                    path: opts
                                        .csv
                                        .clone()
                                        .unwrap_or_else(|| PathBuf::from("report.csv")),
                                    source,
                                })?;
                        }

                        if opts.retain_rows {
                            all_rows.push(report_row);
                        }
                        file_messages += 1;
                    }
                }
            }
        }

        let status = if file_skipped > 0 || file_degraded > 0 {
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
        });
    }

    // Always flush writers before return (including integrity failure paths).
    if let Some(wtr) = integrity_wtr.as_mut() {
        wtr.flush().map_err(|source| CliError::CsvWrite {
            path: integrity_path
                .clone()
                .unwrap_or_else(|| PathBuf::from("integrity.csv")),
            source: Box::new(source),
        })?;
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

    let failed_files = file_stats
        .iter()
        .filter(|f| f.status == FileScanStatus::Failed)
        .count() as u64;
    let partial_files = file_stats
        .iter()
        .filter(|f| f.status == FileScanStatus::Partial)
        .count() as u64;
    let opened_files = file_stats
        .iter()
        .filter(|f| f.status == FileScanStatus::Opened)
        .count() as u64;

    let recoverable_messages = index.total();
    let preflight = compute_preflight(&PreflightInputs {
        mode: opts.mode,
        recoverable: recoverable_messages,
        skipped: total_skipped,
        crc_skips,
        failed_files,
        input_file_count: paths.len() as u64,
        thresholds: opts.thresholds,
    });

    let summary = ScanSummary {
        schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: opts.mode,
        files: file_stats,
        total_messages: recoverable_messages,
        unique: index.unique_count,
        duplicates: index.duplicate_count,
        tier1_hits: index.tier1_hits,
        tier2_hits: index.tier2_hits,
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
    };

    Ok(ScanOutcome {
        summary,
        rows: all_rows,
        candidates,
        csv_streamed,
    })
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
        if let DedupResult::DuplicateOf { original, tier } = &row.result {
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
        let preflight = compute_preflight(&PreflightInputs {
            mode: ScanMode::BestEffort,
            recoverable: 10,
            skipped: 0,
            crc_skips: 0,
            failed_files: 1,
            input_file_count: 2,
            thresholds: IntegrityThresholds::default(),
        });
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
        };
        let mut opts = ScanOptions::default();
        assert!(evaluate_exit_policy(&summary, &opts).is_err());
        opts.allow_failed_files = true;
        assert!(evaluate_exit_policy(&summary, &opts).is_ok());
    }

    #[test]
    fn exit_policy_strict_on_skip() {
        use dedup_engine::integrity::{compute_preflight, PreflightInputs, SCAN_INTEGRITY_SCHEMA};
        let preflight = compute_preflight(&PreflightInputs {
            mode: ScanMode::Strict,
            recoverable: 10,
            skipped: 1,
            crc_skips: 0,
            failed_files: 0,
            input_file_count: 1,
            thresholds: IntegrityThresholds::default(),
        });
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
        };
        let opts = ScanOptions {
            mode: ScanMode::Strict,
            ..Default::default()
        };
        assert!(evaluate_exit_policy(&summary, &opts).is_err());
    }
}
