//! Background worker thread for PST scanning and deduplication.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use pst_reader::PstFile;
use dedup_engine::{
    DedupIndex, DedupResult, MessageRef,
    hasher::{self, AttachmentInfo},
    report::{self, ReportRow},
};

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
#[derive(Debug)]
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
}

#[derive(Debug)]
pub struct FileStats {
    pub name: String,
    pub messages: u64,
    pub duplicates: u64,
}

/// Run the full scan pipeline. Called from a background thread.
pub fn run_scan(
    files: Vec<PathBuf>,
    config: DedupConfig,
    progress: Arc<Mutex<ScanProgress>>,
) -> ScanResult {
    let start = Instant::now();
    let mut index = DedupIndex::with_capacity(100_000);
    let mut all_rows = Vec::new();
    let mut file_stats = Vec::new();
    let mut total_savings: u64 = 0;

    // Update total file count
    {
        let mut p = progress.lock().unwrap();
        p.total_files = files.len();
    }

    for (file_idx, file_path) in files.iter().enumerate() {
        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("file_{}", file_idx));

        // Update progress
        {
            let mut p = progress.lock().unwrap();
            if p.cancelled {
                break;
            }
            p.current_file = file_name.clone();
            p.current_file_index = file_idx;
        }

        let mut file_messages: u64 = 0;
        let mut file_duplicates: u64 = 0;

        // Open PST
        let mut pst = match PstFile::open(file_path) {
            Ok(pst) => pst,
            Err(e) => {
                tracing::error!("Failed to open {}: {}", file_name, e);
                let mut p = progress.lock().unwrap();
                p.error = Some(format!("Failed to open {}: {}", file_name, e));
                continue;
            }
        };

        // Walk folders
        let folders = match pst.folders() {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Failed to read folders in {}: {}", file_name, e);
                continue;
            }
        };

        // Update estimated total
        {
            let total_msgs: u64 = folders.iter().map(|f| f.message_count as u64).sum();
            let mut p = progress.lock().unwrap();
            p.messages_estimated += total_msgs;
        }

        // Process each folder's messages
        for folder in &folders {
            for &msg_nid in &folder.message_nids {
                // Check cancellation
                {
                    let p = progress.lock().unwrap();
                    if p.cancelled {
                        break;
                    }
                }

                // Read message properties
                let props = match pst.read_message_properties(msg_nid) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("Skipping message 0x{:X}: {}", msg_nid.0, e);
                        continue;
                    }
                };

                // Read attachment metadata if configured
                let attachments = if config.include_attachments
                    && props.has_attachments.unwrap_or(false)
                {
                    match pst.read_attachment_metadata(msg_nid) {
                        Ok(atts) => atts
                            .into_iter()
                            .map(|a| AttachmentInfo {
                                filename: a.filename,
                                size: a.size,
                            })
                            .collect(),
                        Err(_) => Vec::new(),
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
                let result = if config.enable_tier2 {
                    index.check_and_insert(
                        keys.message_id.as_deref(),
                        keys.content_hash,
                        msg_ref.clone(),
                    )
                } else {
                    // Tier 1 only — skip content hash check
                    index.check_and_insert(
                        keys.message_id.as_deref(),
                        [0; 32], // dummy hash, won't match
                        msg_ref.clone(),
                    )
                };

                // Track duplicates for savings estimate
                if let DedupResult::DuplicateOf { .. } = &result {
                    file_duplicates += 1;
                    total_savings += msg_ref.size as u64;
                }

                all_rows.push(ReportRow {
                    message: msg_ref,
                    result,
                });

                file_messages += 1;

                // Update progress periodically (every 100 messages)
                if file_messages % 100 == 0 {
                    let elapsed = start.elapsed().as_secs_f64();
                    let total_processed = index.total();
                    let mut p = progress.lock().unwrap();
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
        });
    }

    // Mark complete
    {
        let mut p = progress.lock().unwrap();
        p.complete = true;
        p.messages_processed = index.total();
        p.unique_count = index.unique_count;
        p.duplicate_count = index.duplicate_count;
    }

    let duration_secs = start.elapsed().as_secs_f64();

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
    }
}
