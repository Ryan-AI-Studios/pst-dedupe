//! CSV report generation for dedup results.

use std::io::Write;
use std::path::Path;

use crate::index::{DedupResult, MessageRef};

/// A single row in the dedup report.
#[derive(Debug, Clone)]
pub struct ReportRow {
    /// The message being reported.
    pub message: MessageRef,
    /// Dedup result for this message.
    pub result: DedupResult,
}

/// Write a CSV dedup report to a file.
///
/// Columns: Status, Tier, PST File, Folder, Subject, Date, Sender, Size,
/// Original PST, Original Folder, Original Subject
pub fn write_csv_report(path: &Path, rows: &[ReportRow]) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(path)?;
    let mut wtr = csv::Writer::from_writer(file);

    // Header
    wtr.write_record(&[
        "Status",
        "Match Tier",
        "PST File",
        "Folder",
        "Subject",
        "Date (FILETIME)",
        "Sender",
        "Size (bytes)",
        "Original PST",
        "Original Folder",
        "Original Subject",
    ])?;

    for row in rows {
        match &row.result {
            DedupResult::Unique => {
                wtr.write_record(&[
                    "Unique",
                    "",
                    &row.message.pst_name,
                    &row.message.folder_path,
                    &row.message.subject,
                    &filetime_str(row.message.submit_time),
                    &row.message.sender,
                    &row.message.size.to_string(),
                    "",
                    "",
                    "",
                ])?;
            }
            DedupResult::DuplicateOf { original, tier } => {
                wtr.write_record(&[
                    "Duplicate",
                    &tier.to_string(),
                    &row.message.pst_name,
                    &row.message.folder_path,
                    &row.message.subject,
                    &filetime_str(row.message.submit_time),
                    &row.message.sender,
                    &row.message.size.to_string(),
                    &original.pst_name,
                    &original.folder_path,
                    &original.subject,
                ])?;
            }
        }
    }

    wtr.flush()?;
    Ok(())
}

/// Write a summary section at the end of the report.
pub fn write_summary_report(
    path: &Path,
    total: u64,
    unique: u64,
    duplicates: u64,
    tier1_hits: u64,
    tier2_hits: u64,
    savings_bytes: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;

    writeln!(file)?;
    writeln!(file, "--- SUMMARY ---")?;
    writeln!(file, "Total messages scanned: {}", total)?;
    writeln!(file, "Unique messages: {}", unique)?;
    writeln!(file, "Duplicates found: {}", duplicates)?;
    writeln!(file, "  Tier 1 (Message-ID): {}", tier1_hits)?;
    writeln!(file, "  Tier 2 (Content Hash): {}", tier2_hits)?;
    writeln!(file, "Estimated savings: {}", format_bytes(savings_bytes))?;

    Ok(())
}

fn filetime_str(ft: Option<i64>) -> String {
    match ft {
        Some(v) => {
            // Convert FILETIME to rough ISO 8601
            // FILETIME = 100ns intervals since 1601-01-01
            // Unix epoch offset: 11644473600 seconds
            let unix_secs = (v / 10_000_000) - 11_644_473_600;
            match chrono::DateTime::from_timestamp(unix_secs, 0) {
                Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
                None => v.to_string(),
            }
        }
        None => String::new(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.2} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}
