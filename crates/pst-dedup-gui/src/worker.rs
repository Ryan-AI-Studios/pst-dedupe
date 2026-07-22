//! Background worker thread for PST scanning and deduplication.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dedup_engine::{
    exporter::{export_eml, EmlMessage},
    filetime_to_unix,
    hasher::{self, AttachmentInfo},
    report::ReportRow,
    DedupIndex, DedupResult, MessageRef,
};
use pst_reader::PstFile;

use crate::app::DedupConfig;

/// Live progress state, shared between worker and GUI.
#[derive(Debug, Clone, Default)]
pub struct ScanProgress {
    /// Currently processing file name.
    pub current_file: String,
    /// Index of current PST file (0-based).
    pub current_file_index: usize,
    /// Total PST files to process.
    pub total_files: usize,
    /// Messages processed so far (across all files).
    pub messages_processed: u64,
    /// Estimated total messages (may increase as we discover folders).
    pub messages_estimated: u64,
    /// Running unique count.
    pub unique_count: u64,
    /// Running duplicate count.
    pub duplicate_count: u64,
    /// Messages per second throughput.
    pub messages_per_sec: f64,
    /// Whether a cancellation was requested.
    pub cancelled: bool,
    /// Whether the scan is complete.
    pub complete: bool,
    /// Error message if something went wrong.
    pub error: Option<String>,
}

/// Final scan results.
#[derive(Debug, Clone)]
pub struct ScanResult {
    /// All report rows (one per message scanned).
    pub rows: Vec<ReportRow>,
    /// Summary statistics.
    pub total_messages: u64,
    pub unique_count: u64,
    pub duplicate_count: u64,
    pub tier1_hits: u64,
    pub tier2_hits: u64,
    /// Total size of duplicate messages (bytes).
    pub savings_bytes: u64,
    /// Wall clock duration.
    pub duration_secs: f64,
    /// Per-PST stats.
    pub file_stats: Vec<FileStats>,
    /// Number of files that could not be scanned.
    pub failed_files: u64,
    /// Source file paths (preserved for re-export).
    pub source_files: Vec<PathBuf>,
}

/// Per-PST scan outcome.
#[derive(Debug, Clone)]
pub struct FileStats {
    pub name: String,
    pub messages: u64,
    pub duplicates: u64,
    /// Error if the file could not be opened or traversed.
    pub error: Option<String>,
    /// Number of messages skipped due to read errors.
    pub skipped_messages: u64,
}

/// Run the full scan pipeline. Called from a background thread.
pub fn run_scan(
    files: Vec<PathBuf>,
    config: DedupConfig,
    progress: Arc<Mutex<ScanProgress>>,
) -> ScanResult {
    let start = Instant::now();
    let mut index = DedupIndex::with_capacity_and_tier2(100_000, config.enable_tier2);
    let mut all_rows = Vec::new();
    let mut file_stats = Vec::new();
    let mut total_savings: u64 = 0;

    // Update total file count
    {
        let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
        p.total_files = files.len();
    }

    for (file_idx, file_path) in files.iter().enumerate() {
        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("file_{}", file_idx));

        // Update progress
        {
            let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
            if p.cancelled {
                break;
            }
            p.current_file = file_name.clone();
            p.current_file_index = file_idx;
        }

        let mut file_messages: u64 = 0;
        let mut file_duplicates: u64 = 0;
        let mut skipped_messages: u64 = 0;

        // Open PST
        let mut pst = match PstFile::open(file_path) {
            Ok(pst) => pst,
            Err(e) => {
                let err_msg = format!("Failed to open {}: {}", file_name, e);
                tracing::error!("{}", err_msg);
                let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                p.error = Some(err_msg.clone());
                file_stats.push(FileStats {
                    name: file_name,
                    messages: 0,
                    duplicates: 0,
                    error: Some(err_msg),
                    skipped_messages: 0,
                });
                continue;
            }
        };

        // Walk folders
        let folders = match pst.folders() {
            Ok(f) => f,
            Err(e) => {
                let err_msg = format!("Failed to read folders in {}: {}", file_name, e);
                tracing::error!("{}", err_msg);
                let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                p.error = Some(err_msg.clone());
                file_stats.push(FileStats {
                    name: file_name,
                    messages: 0,
                    duplicates: 0,
                    error: Some(err_msg),
                    skipped_messages: 0,
                });
                continue;
            }
        };

        // Update estimated total
        {
            let total_msgs: u64 = folders.iter().map(|f| f.message_count as u64).sum();
            let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
            p.messages_estimated += total_msgs;
        }

        // Process each folder's messages
        for folder in &folders {
            for &msg_nid in &folder.message_nids {
                // Check cancellation
                {
                    let p = progress.lock().unwrap_or_else(|e| e.into_inner());
                    if p.cancelled {
                        break;
                    }
                }

                // Read message properties
                let props = match pst.read_message_properties(msg_nid) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("Skipping message 0x{:X}: {}", msg_nid.0, e);
                        skipped_messages += 1;
                        continue;
                    }
                };

                // Read attachment metadata if configured
                let attachments =
                    if config.include_attachments && props.has_attachments.unwrap_or(false) {
                        match pst.read_attachment_metadata(msg_nid) {
                            Ok(atts) => atts
                                .into_iter()
                                .map(|a| AttachmentInfo {
                                    filename: a.filename,
                                    size: a.size,
                                })
                                .collect(),
                            Err(e) => {
                                tracing::warn!(
                                    "Skipping message 0x{:X}: attachment metadata error: {}",
                                    msg_nid.0,
                                    e
                                );
                                skipped_messages += 1;
                                continue;
                            }
                        }
                    } else {
                        Vec::new()
                    };

                // Compute dedup keys
                let keys = hasher::compute_dedup_keys(
                    props.message_id.as_deref(),
                    props.subject.as_deref(),
                    props.submit_time,
                    props.sender_email.as_deref(),
                    props.body_preview.as_deref(),
                    &attachments,
                );

                // Build message reference
                let msg_ref = MessageRef {
                    pst_index: file_idx,
                    pst_name: file_name.clone(),
                    folder_path: folder.path.clone(),
                    nid: msg_nid.0,
                    subject: props.subject.clone().unwrap_or_default(),
                    submit_time: props.submit_time,
                    sender: props.sender_email.clone().unwrap_or_default(),
                    size: props.message_size.unwrap_or(0) as u32,
                };

                // Check against index
                let result = index.check_and_insert(
                    keys.message_id.as_deref(),
                    keys.content_hash,
                    msg_ref.clone(),
                );

                // Track duplicates for savings estimate
                if let DedupResult::DuplicateOf { .. } = &result {
                    file_duplicates += 1;
                    total_savings += msg_ref.size as u64;
                }

                all_rows.push(ReportRow {
                    message: msg_ref,
                    result,
                    integrity: dedup_engine::integrity::RecoverableIntegrity::clean(),
                });

                file_messages += 1;

                // Update progress periodically (every 100 messages)
                if file_messages.is_multiple_of(100) {
                    let elapsed = start.elapsed().as_secs_f64();
                    let total_processed = index.total();
                    let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                    p.messages_processed = total_processed;
                    p.unique_count = index.unique_count;
                    p.duplicate_count = index.duplicate_count;
                    p.messages_per_sec = if elapsed > 0.0 {
                        total_processed as f64 / elapsed
                    } else {
                        0.0
                    };
                }
            }
        }

        file_stats.push(FileStats {
            name: file_name,
            messages: file_messages,
            duplicates: file_duplicates,
            error: None,
            skipped_messages,
        });
    }

    // Mark complete
    {
        let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
        p.complete = true;
        p.messages_processed = index.total();
        p.unique_count = index.unique_count;
        p.duplicate_count = index.duplicate_count;
    }

    let duration_secs = start.elapsed().as_secs_f64();
    let failed_files = file_stats.iter().filter(|fs| fs.error.is_some()).count() as u64;

    ScanResult {
        rows: all_rows,
        total_messages: index.total(),
        unique_count: index.unique_count,
        duplicate_count: index.duplicate_count,
        tier1_hits: index.tier1_hits,
        tier2_hits: index.tier2_hits,
        savings_bytes: total_savings,
        duration_secs,
        file_stats,
        failed_files,
        source_files: files,
    }
}

/// Export unique messages from the scan result as EML files.
///
/// Re-opens source PSTs and writes one `.eml` per unique message.
/// Returns `(exported_count, failed_count)`.
pub fn export_unique_eml(
    result: &ScanResult,
    output_dir: &Path,
    source_files: &[PathBuf],
) -> (u64, u64, Option<String>) {
    let mut exported: u64 = 0;
    let mut failed: u64 = 0;
    let mut last_error: Option<String> = None;

    // Collect unique rows and group by source file for efficient re-opening
    let unique_rows: Vec<&ReportRow> = result
        .rows
        .iter()
        .filter(|r| matches!(r.result, DedupResult::Unique))
        .collect();

    // Group by pst_index to open each file once
    let mut by_file: std::collections::HashMap<usize, Vec<&ReportRow>> =
        std::collections::HashMap::new();
    for row in &unique_rows {
        by_file.entry(row.message.pst_index).or_default().push(row);
    }

    for (file_idx, rows) in by_file {
        let path = match source_files.get(file_idx) {
            Some(p) => p,
            None => {
                failed += rows.len() as u64;
                last_error = Some(format!("Source file index {} not found", file_idx));
                continue;
            }
        };

        let mut pst = match PstFile::open(path) {
            Ok(p) => p,
            Err(e) => {
                failed += rows.len() as u64;
                last_error = Some(format!("Failed to open {}: {}", path.display(), e));
                continue;
            }
        };

        for row in rows {
            let nid = pst_reader::ndb::nid::NodeId(row.message.nid);
            let props = match pst.read_message_properties(nid) {
                Ok(p) => p,
                Err(e) => {
                    failed += 1;
                    last_error = Some(format!(
                        "Failed to read message 0x{:X} from {}: {}",
                        row.message.nid,
                        path.display(),
                        e
                    ));
                    continue;
                }
            };

            let eml = EmlMessage {
                message_id: props.message_id.clone(),
                subject: props.subject.clone().unwrap_or_default(),
                sender: props.sender_email.clone().unwrap_or_default(),
                recipients: props.display_to.clone().unwrap_or_default(),
                date: props
                    .submit_time
                    .map(|ft| format_rfc2822(filetime_to_unix(ft))),
                body: props.body_preview.clone().unwrap_or_default(),
                filename: dedup_engine::exporter::make_eml_filename(
                    &props.subject.clone().unwrap_or("email".into()),
                    exported + 1,
                ),
            };

            if let Err(e) = export_eml(output_dir, &eml) {
                failed += 1;
                last_error = Some(format!(
                    "Failed to write EML for message 0x{:X}: {}",
                    row.message.nid, e
                ));
                continue;
            }

            exported += 1;
        }
    }

    (exported, failed, last_error)
}

/// Simple RFC 2822 date formatter (Mon, 15 Jan 2026 14:30:00 +0000).
fn format_rfc2822(unix_secs: i64) -> String {
    // Naive formatting without chrono dependency.
    const DAYS_IN_MONTH: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

    let mut days = unix_secs / 86_400;
    let rem_secs = unix_secs % 86_400;
    let hour = (rem_secs / 3600) as i32;
    let min = ((rem_secs % 3600) / 60) as i32;
    let sec = (rem_secs % 60) as i32;

    // 1970-01-01 is Thursday = day 4
    let weekday = ((4 + days) % 7) as usize;

    let mut year = 1970;
    loop {
        let leap = is_leap(year);
        let year_days = if leap { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days -= year_days;
        year += 1;
    }

    let mut month = 0;
    let mut day_of_year = days as i32;
    while month < 12 {
        let dim = DAYS_IN_MONTH[month] + if month == 1 && is_leap(year) { 1 } else { 0 };
        if day_of_year < dim {
            break;
        }
        day_of_year -= dim;
        month += 1;
    }

    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} +0000",
        DAY_NAMES[weekday],
        day_of_year + 1,
        MONTH_NAMES[month],
        year,
        hour,
        min,
        sec
    )
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
