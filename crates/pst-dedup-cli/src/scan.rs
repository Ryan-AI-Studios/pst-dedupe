//! Shared scan orchestration for CLI commands.

use std::path::{Path, PathBuf};
use std::time::Instant;

use dedup_engine::{
    hasher::{self, AttachmentInfo},
    report::{write_csv_report, write_summary_report, ReportRow},
    DedupIndex, DedupResult, MessageRef,
};
use pst_reader::PstFile;
use serde::Serialize;

use crate::error::{CliError, Result};

/// Options controlling a scan.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub enable_tier2: bool,
    pub include_attachments: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            enable_tier2: true,
            include_attachments: true,
        }
    }
}

/// Per-file outcome.
#[derive(Debug, Clone, Serialize)]
pub struct FileScanStats {
    pub path: String,
    pub name: String,
    pub folders: u64,
    pub messages: u64,
    pub duplicates: u64,
    pub skipped: u64,
    pub error: Option<String>,
}

/// Full scan outcome.
#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub files: Vec<FileScanStats>,
    pub total_messages: u64,
    pub unique: u64,
    pub duplicates: u64,
    pub tier1_hits: u64,
    pub tier2_hits: u64,
    pub savings_bytes: u64,
    pub skipped: u64,
    pub failed_files: u64,
    pub duration_secs: f64,
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
}

/// Validate and normalize input PST paths.
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
        out.push(p.clone());
    }
    Ok(out)
}

/// Scan one or more PST files and build the dedup index result.
pub fn run_scan(paths: &[PathBuf], opts: &ScanOptions) -> Result<ScanOutcome> {
    let start = Instant::now();
    let mut index = DedupIndex::with_capacity_and_tier2(100_000, opts.enable_tier2);
    let mut all_rows: Vec<ReportRow> = Vec::new();
    let mut file_stats: Vec<FileScanStats> = Vec::new();
    let mut total_savings: u64 = 0;
    let mut total_skipped: u64 = 0;

    for (file_idx, path) in paths.iter().enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("file_{file_idx}"));

        let mut pst = match PstFile::open(path) {
            Ok(p) => p,
            Err(source) => {
                file_stats.push(FileScanStats {
                    path: path.display().to_string(),
                    name: name.clone(),
                    folders: 0,
                    messages: 0,
                    duplicates: 0,
                    skipped: 0,
                    error: Some(source.to_string()),
                });
                continue;
            }
        };

        let folders = match pst.folders() {
            Ok(f) => f,
            Err(source) => {
                file_stats.push(FileScanStats {
                    path: path.display().to_string(),
                    name: name.clone(),
                    folders: 0,
                    messages: 0,
                    duplicates: 0,
                    skipped: 0,
                    error: Some(source.to_string()),
                });
                continue;
            }
        };

        let mut file_messages = 0u64;
        let mut file_duplicates = 0u64;
        let mut file_skipped = 0u64;
        let folder_count = folders.len() as u64;

        for folder in &folders {
            for &msg_nid in &folder.message_nids {
                let props = match pst.read_message_properties(msg_nid) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("skip message 0x{:X}: {e}", msg_nid.0);
                        file_skipped += 1;
                        continue;
                    }
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
                                tracing::warn!("skip message 0x{:X} attachments: {e}", msg_nid.0);
                                file_skipped += 1;
                                continue;
                            }
                        }
                    } else {
                        Vec::new()
                    };

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
                    folder_path: folder.path.clone(),
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

                all_rows.push(ReportRow {
                    message: msg_ref,
                    result,
                });
                file_messages += 1;
            }
        }

        total_skipped += file_skipped;
        file_stats.push(FileScanStats {
            path: path.display().to_string(),
            name,
            folders: folder_count,
            messages: file_messages,
            duplicates: file_duplicates,
            skipped: file_skipped,
            error: None,
        });
    }

    let failed_files = file_stats.iter().filter(|f| f.error.is_some()).count() as u64;
    let summary = ScanSummary {
        files: file_stats,
        total_messages: index.total(),
        unique: index.unique_count,
        duplicates: index.duplicate_count,
        tier1_hits: index.tier1_hits,
        tier2_hits: index.tier2_hits,
        savings_bytes: total_savings,
        skipped: total_skipped,
        failed_files,
        duration_secs: start.elapsed().as_secs_f64(),
    };

    Ok(ScanOutcome {
        summary,
        rows: all_rows,
    })
}

/// Write CSV report + appended summary section.
pub fn write_report(path: &Path, outcome: &ScanOutcome) -> Result<()> {
    write_csv_report(path, &outcome.rows).map_err(|source| CliError::CsvWrite {
        path: path.to_path_buf(),
        source,
    })?;
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
