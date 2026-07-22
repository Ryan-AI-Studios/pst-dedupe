//! CSV report generation for dedup results.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::index::{DedupResult, MessageRef};
use crate::integrity::RecoverableIntegrity;
use crate::util::{filetime_to_unix, format_bytes};

/// A single row in the dedup report.
#[derive(Debug, Clone)]
pub struct ReportRow {
    /// The message being reported.
    pub message: MessageRef,
    /// Dedup result for this message.
    pub result: DedupResult,
    /// Integrity of this recoverable message (always present; clean if not degraded).
    pub integrity: RecoverableIntegrity,
}

const DEDUP_CSV_HEADER: [&str; 14] = [
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
    // Appended for 0065 integrity (backward-compatible: new columns at end).
    "IsOrphaned",
    "Degraded",
    "DegradedReasons",
];

/// Streaming dedup CSV writer — header once, rows incrementally (O(1) memory).
pub struct StreamingCsvReportWriter {
    wtr: csv::Writer<BufWriter<File>>,
    path: PathBuf,
    rows_written: u64,
}

impl StreamingCsvReportWriter {
    /// Create/truncate dedup CSV and write header.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(&path)?;
        let mut wtr = csv::Writer::from_writer(BufWriter::new(file));
        wtr.write_record(DEDUP_CSV_HEADER)?;
        wtr.flush()?;
        Ok(Self {
            wtr,
            path,
            rows_written: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rows_written(&self) -> u64 {
        self.rows_written
    }

    /// Write one recoverable report row immediately.
    pub fn write_row(&mut self, row: &ReportRow) -> Result<(), Box<dyn std::error::Error>> {
        let is_orphaned = if row.integrity.is_orphaned {
            "true"
        } else {
            "false"
        };
        let degraded = if row.integrity.degraded {
            "true"
        } else {
            "false"
        };
        let degraded_reasons = row
            .integrity
            .degraded_reasons
            .iter()
            .map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join(";");
        match &row.result {
            DedupResult::Unique => {
                self.wtr.write_record([
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
                    is_orphaned,
                    degraded,
                    &degraded_reasons,
                ])?;
            }
            DedupResult::DuplicateOf { original, tier } => {
                self.wtr.write_record([
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
                    is_orphaned,
                    degraded,
                    &degraded_reasons,
                ])?;
            }
        }
        self.rows_written += 1;
        self.wtr.flush()?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.wtr.flush()?;
        Ok(())
    }
}

/// Write a CSV dedup report to a file (batch — retained for compatibility).
///
/// Columns: Status, Tier, PST File, Folder, Subject, Date, Sender, Size,
/// Original PST, Original Folder, Original Subject
pub fn write_csv_report(path: &Path, rows: &[ReportRow]) -> Result<(), Box<dyn std::error::Error>> {
    let mut wtr = StreamingCsvReportWriter::create(path)?;
    for row in rows {
        wtr.write_row(row)?;
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

/// Convert FILETIME to a human-readable date string.
pub fn filetime_str(ft: Option<i64>) -> String {
    match ft {
        Some(v) => {
            let unix_secs = filetime_to_unix(v);
            match chrono::DateTime::from_timestamp(unix_secs, 0) {
                Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
                None => v.to_string(),
            }
        }
        None => String::new(),
    }
}
