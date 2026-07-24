//! Unique-export report pack (`unique_export_report_v1`) — track 0071.
//!
//! Disk layout under `{report-dir}/`:
//! - `summary.json`
//! - `volumes.csv`
//! - `export_messages.csv` (mandatory when ≥1 message written)
//! - `decisions.csv` / `keepset.json` / optional `integrity.csv` (orchestrator)

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{CliError, Result};

/// Schema id for the unique-export summary JSON.
pub const UNIQUE_EXPORT_REPORT_SCHEMA: &str = "unique_export_report_v1";

/// Fixed header for mandatory `export_messages.csv` (order locked).
pub const EXPORT_MESSAGES_CSV_HEADER: &str = "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index";

/// Fixed header for `volumes.csv`.
pub const VOLUMES_CSV_HEADER: &str =
    "volume_index,path,bytes,sha256,md5,messages_written,finalized_early,volume_exceeded_soft_limit";

/// One completed PST volume row.
#[derive(Debug, Clone, Serialize)]
pub struct VolumeReportRow {
    pub volume_index: u32,
    pub path: String,
    pub bytes: u64,
    pub sha256_hex: String,
    pub md5_hex: String,
    pub messages_written: u64,
    pub finalized_early: bool,
    pub volume_exceeded_soft_limit: bool,
}

/// One written winner → volume cross-reference row.
#[derive(Debug, Clone, Serialize)]
pub struct ExportMessageRow {
    pub source_path: String,
    pub folder_path: String,
    pub nid: u64,
    pub message_id_norm: String,
    pub edrm_mih: String,
    pub content_hash_hex: String,
    pub volume_path: String,
    pub volume_index: u32,
    pub export_message_index: u64,
    /// In-memory only: used for sample verification when MID is empty.
    /// Not written to `export_messages.csv` (header locked).
    #[serde(skip)]
    pub subject: String,
}

/// Per-volume verification result.
#[derive(Debug, Clone, Serialize)]
pub struct VolumeVerification {
    pub volume_index: u32,
    pub path: String,
    pub open_ok: bool,
    pub message_count_match: bool,
    pub messages_found: u64,
    pub messages_expected: u64,
    pub sample_mid_ok: bool,
    /// Present only when `--verify-hash` ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_match: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Aggregate verification section.
#[derive(Debug, Clone, Serialize)]
pub struct VerificationReport {
    pub ok: bool,
    pub volumes: Vec<VolumeVerification>,
    pub rehash_ran: bool,
}

/// Export section of the summary.
#[derive(Debug, Clone, Serialize)]
pub struct ExportSection {
    pub volumes: Vec<VolumeReportRow>,
    pub partial: bool,
    pub messages_written_total: u64,
    pub attachments_written: u64,
    pub attachments_failed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_volume_index: Option<u32>,
}

/// Top-level `summary.json` payload (`unique_export_report_v1`).
#[derive(Debug, Clone, Serialize)]
pub struct UniqueExportSummary {
    pub schema: String,
    pub ok: bool,
    pub inputs: Vec<String>,
    pub policy: String,
    pub family_policy: String,
    pub mode: String,
    pub folder_layout: String,
    pub out: String,
    pub report_dir: String,
    pub keep_set: dedup_engine::KeepSet,
    pub scan: crate::scan::ScanSummary,
    pub export: ExportSection,
    pub verification: VerificationReport,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_volume_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_csv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_set_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SummaryError>,
}

/// Structured error on the summary / JSON stdout.
#[derive(Debug, Clone, Serialize)]
pub struct SummaryError {
    pub code: String,
    pub message: String,
}

/// Escape a CSV field (RFC-style double-quote when needed).
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Write mandatory `export_messages.csv`.
pub fn write_export_messages_csv(path: &Path, rows: &[ExportMessageRow]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CliError::Msg(format!(
                "create export_messages parent {}: {e}",
                parent.display()
            ))
        })?;
    }
    let f = File::create(path).map_err(|e| CliError::CsvWrite {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    let mut w = BufWriter::new(f);
    writeln!(w, "{EXPORT_MESSAGES_CSV_HEADER}").map_err(|e| CliError::CsvWrite {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    for r in rows {
        writeln!(
            w,
            "{},{},{},{},{},{},{},{},{}",
            csv_escape(&r.source_path),
            csv_escape(&r.folder_path),
            r.nid,
            csv_escape(&r.message_id_norm),
            csv_escape(&r.edrm_mih),
            csv_escape(&r.content_hash_hex),
            csv_escape(&r.volume_path),
            r.volume_index,
            r.export_message_index,
        )
        .map_err(|e| CliError::CsvWrite {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
    }
    w.flush().map_err(|e| CliError::CsvWrite {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    Ok(())
}

/// Write `volumes.csv` (one row per completed volume).
pub fn write_volumes_csv(path: &Path, rows: &[VolumeReportRow]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CliError::Msg(format!(
                "create volumes.csv parent {}: {e}",
                parent.display()
            ))
        })?;
    }
    let f = File::create(path).map_err(|e| CliError::CsvWrite {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    let mut w = BufWriter::new(f);
    writeln!(w, "{VOLUMES_CSV_HEADER}").map_err(|e| CliError::CsvWrite {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    for r in rows {
        writeln!(
            w,
            "{},{},{},{},{},{},{},{}",
            r.volume_index,
            csv_escape(&r.path),
            r.bytes,
            csv_escape(&r.sha256_hex),
            csv_escape(&r.md5_hex),
            r.messages_written,
            r.finalized_early,
            r.volume_exceeded_soft_limit,
        )
        .map_err(|e| CliError::CsvWrite {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
    }
    w.flush().map_err(|e| CliError::CsvWrite {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    Ok(())
}

/// Write `summary.json`.
pub fn write_summary_json(path: &Path, summary: &UniqueExportSummary) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CliError::Msg(format!(
                "create summary.json parent {}: {e}",
                parent.display()
            ))
        })?;
    }
    let json = serde_json::to_string_pretty(summary)?;
    fs::write(path, json)
        .map_err(|e| CliError::Msg(format!("write summary.json {}: {e}", path.display())))?;
    Ok(())
}

/// Default report-dir: sibling of `--out` stem + `_report`.
///
/// Example: `C:\export\unique.pst` → `C:\export\unique_report`.
pub fn default_report_dir(out: &Path) -> PathBuf {
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unique".to_string());
    parent.join(format!("{stem}_report"))
}

/// Multi-volume path for 1-based volume index.
///
/// Volume 1 is `out`. Volume n≥2 is `{stem}_vol{NNN}.pst` next to `out`
/// (e.g. `unique.pst` → `unique_vol002.pst`).
pub fn volume_path_for(out: &Path, volume_index: u32) -> PathBuf {
    if volume_index <= 1 {
        return out.to_path_buf();
    }
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unique".to_string());
    let ext = out
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pst".to_string());
    parent.join(format!("{stem}_vol{volume_index:03}.{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_naming_primary_and_secondary() {
        let out = PathBuf::from(r"C:\export\unique.pst");
        assert_eq!(volume_path_for(&out, 1), out);
        assert_eq!(
            volume_path_for(&out, 2),
            PathBuf::from(r"C:\export\unique_vol002.pst")
        );
        assert_eq!(
            volume_path_for(&out, 12),
            PathBuf::from(r"C:\export\unique_vol012.pst")
        );
    }

    #[test]
    fn default_report_dir_sibling() {
        let out = PathBuf::from(r"C:\export\unique.pst");
        assert_eq!(
            default_report_dir(&out),
            PathBuf::from(r"C:\export\unique_report")
        );
    }

    #[test]
    fn export_messages_header_order_locked() {
        assert_eq!(
            EXPORT_MESSAGES_CSV_HEADER,
            "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index"
        );
    }
}
