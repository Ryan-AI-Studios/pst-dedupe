//! Unique-export report pack (`unique_export_report_v1`) — tracks 0071 / 0073.
//!
//! Disk layout under `{report-dir}/`:
//! - `summary.json`
//! - `volumes.csv`
//! - `export_messages.csv` (mandatory when ≥1 message written)
//! - `export_attachments.csv` (track 0073; `--attach-ledger=full`)
//! - `decisions.csv` / `keepset.json` / optional `integrity.csv` (orchestrator)

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use pst_writer::{AttachEventSeverity, AttachEventSink, AttachmentFidelityEvent};
use serde::Serialize;

use crate::error::{CliError, Result};

/// Schema id for the unique-export summary JSON.
pub const UNIQUE_EXPORT_REPORT_SCHEMA: &str = "unique_export_report_v1";

/// Fixed header for mandatory `export_messages.csv` (prefix locked; 0073 appends).
pub const EXPORT_MESSAGES_CSV_HEADER: &str = "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count";

/// Fixed header for `volumes.csv`.
pub const VOLUMES_CSV_HEADER: &str =
    "volume_index,path,bytes,sha256,md5,messages_written,finalized_early,volume_exceeded_soft_limit";

/// Fixed header for `export_attachments.csv` (track 0073).
pub const EXPORT_ATTACHMENTS_CSV_HEADER: &str = "source_id,source_path,folder_path,msg_nid,attach_nid,attach_index,filename,size,attach_method,reason_code,severity,volume_path,volume_index,winner_promoted,peer_source_id,peer_msg_nid,message_subject";

/// On-disk name for the attach failure ledger.
pub const EXPORT_ATTACHMENTS_CSV_NAME: &str = "export_attachments.csv";

/// Default CSV row cap (fail + info rows that would be written).
pub const DEFAULT_ATTACH_LEDGER_MAX_ROWS: u64 = 500_000;

/// CLI / report mode for the attach ledger (track 0073).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AttachLedgerMode {
    /// Stream CSV + histogram (default).
    #[default]
    Full,
    /// Histogram only; no CSV file.
    SummaryOnly,
    /// Neither CSV nor histogram fields (counts still honest for exit).
    Off,
}

impl AttachLedgerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::SummaryOnly => "summary-only",
            Self::Off => "off",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "summary-only" | "summary_only" | "summary" => Some(Self::SummaryOnly),
            "off" | "none" | "false" => Some(Self::Off),
            _ => None,
        }
    }
}

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
    /// Fail-severity attach count for this message (0073).
    pub attachments_failed_count: u64,
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
    pub attachments_omitted_by_policy: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments_failed_by_reason: Option<BTreeMap<String, u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_ledger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_ledger_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_ledger_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_ledger_rows_written: Option<u64>,
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

/// Neutralize spreadsheet formula injection, then RFC-style CSV-escape.
///
/// If the field (leading whitespace stripped for the check) starts with
/// `=`, `+`, `-`, or `@`, prefix with a single quote `'` before quoting.
pub fn csv_escape_cell(s: &str) -> String {
    let neutralized = neutralize_csv_formula(s);
    csv_escape_raw(&neutralized)
}

/// Prefix `'` when the cell (leading whitespace stripped) starts with `=+\-@`.
pub fn neutralize_csv_formula(s: &str) -> String {
    let trimmed = s.trim_start();
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@'))
    {
        format!("'{s}")
    } else {
        s.to_string()
    }
}

/// Escape a CSV field (RFC-style double-quote when needed). No formula check.
fn csv_escape_raw(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// One attach ledger CSV row (owned; Send across mpsc).
#[derive(Debug, Clone)]
pub struct AttachLedgerRow {
    /// 0-based index into `summary.inputs` as a decimal string; empty when unmapped.
    pub source_id: String,
    pub source_path: String,
    pub folder_path: String,
    pub msg_nid: u64,
    pub attach_nid: String,
    pub attach_index: u32,
    pub filename: String,
    pub size: String,
    pub attach_method: i32,
    pub reason_code: String,
    pub severity: String,
    pub volume_path: String,
    pub volume_index: String,
    pub winner_promoted: bool,
    pub peer_source_id: String,
    pub peer_msg_nid: String,
    pub message_subject: String,
}

impl AttachLedgerRow {
    fn to_csv_line(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            csv_escape_cell(&self.source_id),
            csv_escape_cell(&self.source_path),
            csv_escape_cell(&self.folder_path),
            self.msg_nid,
            csv_escape_cell(&self.attach_nid),
            self.attach_index,
            csv_escape_cell(&self.filename),
            csv_escape_cell(&self.size),
            self.attach_method,
            csv_escape_cell(&self.reason_code),
            csv_escape_cell(&self.severity),
            csv_escape_cell(&self.volume_path),
            csv_escape_cell(&self.volume_index),
            self.winner_promoted,
            csv_escape_cell(&self.peer_source_id),
            csv_escape_cell(&self.peer_msg_nid),
            csv_escape_cell(&self.message_subject),
        )
    }

    /// Marker row when the CSV row cap is hit.
    pub fn truncated_marker(rows_dropped_estimate: Option<u64>) -> Self {
        Self {
            source_id: String::new(),
            source_path: String::new(),
            folder_path: String::new(),
            msg_nid: 0,
            attach_nid: String::new(),
            attach_index: 0,
            filename: String::new(),
            size: rows_dropped_estimate
                .map(|n| n.to_string())
                .unwrap_or_default(),
            attach_method: -1,
            reason_code: "ATTACH_LEDGER_TRUNCATED".into(),
            severity: "info".into(),
            volume_path: String::new(),
            volume_index: String::new(),
            winner_promoted: false,
            peer_source_id: String::new(),
            peer_msg_nid: String::new(),
            message_subject: String::new(),
        }
    }
}

/// Build a ledger row from a writer fidelity event + CLI enrichment.
///
/// `source_id` is the decimal index into inputs, or empty when unmapped (never a fake `0`).
pub fn ledger_row_from_event(
    event: &AttachmentFidelityEvent,
    source_id: Option<u32>,
    volume_path: &str,
    volume_index: u32,
) -> AttachLedgerRow {
    AttachLedgerRow {
        source_id: source_id.map(|id| id.to_string()).unwrap_or_default(),
        source_path: event.source_path.clone(),
        folder_path: event.folder_path.clone(),
        msg_nid: event.msg_nid,
        attach_nid: event.attach_nid.map(|n| n.to_string()).unwrap_or_default(),
        attach_index: event.attach_index,
        filename: event.attach_filename.clone(),
        size: event.size.map(|n| n.to_string()).unwrap_or_default(),
        attach_method: event.attach_method,
        reason_code: event.kind.as_code().to_string(),
        severity: event.severity.as_str().to_string(),
        volume_path: volume_path.to_string(),
        volume_index: if volume_index == 0 {
            String::new()
        } else {
            volume_index.to_string()
        },
        winner_promoted: false,
        peer_source_id: String::new(),
        peer_msg_nid: String::new(),
        message_subject: event.message_subject.clone(),
    }
}

enum LedgerCmd {
    /// Boxed to keep `Finish` small (clippy `large_enum_variant`).
    Row(Box<AttachLedgerRow>),
    Finish,
}

/// Background batched CSV writer for `export_attachments.csv` (track 0073).
///
/// Critical path only enqueues; the writer thread owns the file + BufWriter.
pub struct AttachLedgerCsvWriter {
    tx: mpsc::Sender<LedgerCmd>,
    join: Option<JoinHandle<std::result::Result<u64, String>>>,
}

impl AttachLedgerCsvWriter {
    /// Create CSV (header) and spawn the background writer thread.
    pub fn start(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CliError::Msg(format!(
                    "create export_attachments parent {}: {e}",
                    parent.display()
                ))
            })?;
        }
        let path_buf = path.to_path_buf();
        let f = File::create(path).map_err(|e| CliError::CsvWrite {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
        let mut w = BufWriter::new(f);
        writeln!(w, "{EXPORT_ATTACHMENTS_CSV_HEADER}").map_err(|e| CliError::CsvWrite {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;
        w.flush().map_err(|e| CliError::CsvWrite {
            path: path.to_path_buf(),
            source: Box::new(e),
        })?;

        let (tx, rx) = mpsc::channel::<LedgerCmd>();
        let join = thread::spawn(move || {
            let mut w = w;
            let mut written: u64 = 0;
            let mut batch: u64 = 0;
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    LedgerCmd::Row(row) => {
                        let line = row.to_csv_line();
                        if let Err(e) = writeln!(w, "{line}") {
                            return Err(format!(
                                "export_attachments.csv write {}: {e}",
                                path_buf.display()
                            ));
                        }
                        written = written.saturating_add(1);
                        batch = batch.saturating_add(1);
                        if batch >= 64 {
                            if let Err(e) = w.flush() {
                                return Err(format!(
                                    "export_attachments.csv flush {}: {e}",
                                    path_buf.display()
                                ));
                            }
                            batch = 0;
                        }
                    }
                    LedgerCmd::Finish => break,
                }
            }
            w.flush().map_err(|e| {
                format!(
                    "export_attachments.csv final flush {}: {e}",
                    path_buf.display()
                )
            })?;
            Ok(written)
        });
        Ok(Self {
            tx,
            join: Some(join),
        })
    }

    /// Enqueue a row (non-blocking for the PST write loop until the channel fills).
    pub fn enqueue(&self, row: AttachLedgerRow) -> Result<()> {
        self.tx
            .send(LedgerCmd::Row(Box::new(row)))
            .map_err(|_| CliError::Msg("attach ledger writer thread ended early".into()))
    }

    /// Signal finish, join the writer thread, return rows written by the thread.
    pub fn finish(mut self) -> Result<u64> {
        let _ = self.tx.send(LedgerCmd::Finish);
        match self.join.take() {
            Some(h) => match h.join() {
                Ok(Ok(n)) => Ok(n),
                Ok(Err(e)) => Err(CliError::Msg(e)),
                Err(_) => Err(CliError::Msg("attach ledger writer thread panicked".into())),
            },
            None => Ok(0),
        }
    }
}

/// Live attach accounting sink used during `write_unicode_pst_streaming`.
///
/// Per-message fail counts are maintained in **all** modes (including Off) for
/// honest `export_messages.attachments_failed_count`. Histogram + omit tallies
/// are suppressed in Off; CSV enqueue only when mode=full and under the row cap.
pub struct AttachLedgerSink {
    pub mode: AttachLedgerMode,
    pub max_rows: u64,
    /// Fail-severity histogram (never truncated; not updated when mode=Off).
    pub failed_by_reason: BTreeMap<String, u64>,
    pub omitted_by_policy: u64,
    /// Per (source_path, msg_nid) fail counts for export_messages column (all modes).
    pub msg_fail_counts: HashMap<(String, u64), u64>,
    /// CSV rows accepted for write (including truncation marker).
    pub rows_written: u64,
    pub truncated: bool,
    /// Rows dropped after cap (estimate for marker size field).
    pub rows_dropped: u64,
    csv: Option<AttachLedgerCsvWriter>,
    /// 0-based index lookup: source_path display string → source_id.
    source_ids: HashMap<String, u32>,
    /// Current volume enrichment (set before each volume write).
    pub volume_path: String,
    pub volume_index: u32,
}

impl AttachLedgerSink {
    /// Create sink; opens CSV when mode=full.
    pub fn new(
        mode: AttachLedgerMode,
        max_rows: u64,
        report_dir: &Path,
        input_paths: &[String],
    ) -> Result<Self> {
        let mut source_ids = HashMap::new();
        for (i, p) in input_paths.iter().enumerate() {
            source_ids.insert(p.clone(), i as u32);
        }
        let csv = if mode == AttachLedgerMode::Full {
            let path = report_dir.join(EXPORT_ATTACHMENTS_CSV_NAME);
            Some(AttachLedgerCsvWriter::start(&path)?)
        } else {
            None
        };
        Ok(Self {
            mode,
            max_rows: max_rows.max(1),
            failed_by_reason: BTreeMap::new(),
            omitted_by_policy: 0,
            msg_fail_counts: HashMap::new(),
            rows_written: 0,
            truncated: false,
            rows_dropped: 0,
            csv,
            source_ids,
            volume_path: String::new(),
            volume_index: 0,
        })
    }

    pub fn set_volume(&mut self, volume_path: &str, volume_index: u32) {
        self.volume_path = volume_path.to_string();
        self.volume_index = volume_index;
    }

    /// Resolve 0-based input index; `None` when unmapped (never invent `0`).
    fn resolve_source_id(&self, source_path: &str) -> Option<u32> {
        if let Some(id) = self.source_ids.get(source_path) {
            return Some(*id);
        }
        // Case-insensitive / suffix fallback for path encoding drift.
        for (k, id) in &self.source_ids {
            if k.eq_ignore_ascii_case(source_path) {
                return Some(*id);
            }
        }
        None
    }

    /// Ingest one fidelity event (same path as the sink trait).
    pub fn ingest(&mut self, event: &AttachmentFidelityEvent) {
        // Per-message fail counts in all modes (Off still fills export_messages).
        if event.severity == AttachEventSeverity::Fail {
            let key = (event.source_path.clone(), event.msg_nid);
            *self.msg_fail_counts.entry(key).or_insert(0) += 1;
        }

        if self.mode == AttachLedgerMode::Off {
            return;
        }

        match event.severity {
            AttachEventSeverity::Fail => {
                let code = event.kind.as_code().to_string();
                *self.failed_by_reason.entry(code).or_insert(0) += 1;
            }
            AttachEventSeverity::Info => {
                if event.kind.as_code() == "ATTACH_OMITTED_BY_POLICY" {
                    self.omitted_by_policy = self.omitted_by_policy.saturating_add(1);
                }
            }
        }

        if self.mode != AttachLedgerMode::Full {
            return;
        }

        if self.truncated {
            self.rows_dropped = self.rows_dropped.saturating_add(1);
            return;
        }

        // Cap is on CSV rows before the truncation marker; leave one slot for marker.
        if self.rows_written >= self.max_rows.saturating_sub(1) && self.max_rows > 0 {
            // Write final marker then stop.
            let marker = AttachLedgerRow::truncated_marker(None);
            if let Some(csv) = self.csv.as_ref() {
                let _ = csv.enqueue(marker);
            }
            self.rows_written = self.rows_written.saturating_add(1);
            self.truncated = true;
            self.rows_dropped = self.rows_dropped.saturating_add(1);
            return;
        }

        let source_id = self.resolve_source_id(&event.source_path);
        let row = ledger_row_from_event(event, source_id, &self.volume_path, self.volume_index);
        if let Some(csv) = self.csv.as_ref() {
            if csv.enqueue(row).is_ok() {
                self.rows_written = self.rows_written.saturating_add(1);
            }
        }
    }

    /// Fail-count for a message locus (export_messages column).
    pub fn fail_count_for(&self, source_path: &str, msg_nid: u64) -> u64 {
        self.msg_fail_counts
            .get(&(source_path.to_string(), msg_nid))
            .copied()
            .unwrap_or(0)
    }

    /// Flush CSV thread; update rows_written from thread if available.
    pub fn finish(mut self) -> Result<AttachLedgerFinish> {
        let csv_rows = if let Some(csv) = self.csv.take() {
            csv.finish()?
        } else {
            0
        };
        // Prefer local rows_written (includes marker); thread count should match.
        let rows = if self.mode == AttachLedgerMode::Full {
            self.rows_written.max(csv_rows)
        } else {
            0
        };
        Ok(AttachLedgerFinish {
            mode: self.mode,
            failed_by_reason: self.failed_by_reason,
            omitted_by_policy: self.omitted_by_policy,
            msg_fail_counts: self.msg_fail_counts,
            rows_written: rows,
            truncated: self.truncated,
        })
    }
}

/// Volume-local attach event buffer (track 0073 P1-3).
///
/// Collects writer fidelity events for one volume write. On success, drain into
/// the global [`AttachLedgerSink`]; on hard volume failure, drop so histogram /
/// CSV / msg fail counts only reflect committed volumes.
#[derive(Debug, Default)]
pub struct VolumeAttachBuffer {
    events: Vec<AttachmentFidelityEvent>,
}

impl VolumeAttachBuffer {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Commit buffered events into the global ledger (CSV + histogram + counts).
    pub fn commit_into(self, sink: &mut AttachLedgerSink) {
        for event in &self.events {
            sink.ingest(event);
        }
    }

    /// Number of buffered events (tests / diagnostics).
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl AttachEventSink for VolumeAttachBuffer {
    fn on_attach_event(&mut self, event: &AttachmentFidelityEvent) {
        self.events.push(event.clone());
    }
}

impl AttachEventSink for AttachLedgerSink {
    fn on_attach_event(&mut self, event: &AttachmentFidelityEvent) {
        self.ingest(event);
    }
}

/// Snapshot after ledger finish (for summary + export_messages fill).
pub struct AttachLedgerFinish {
    pub mode: AttachLedgerMode,
    pub failed_by_reason: BTreeMap<String, u64>,
    pub omitted_by_policy: u64,
    pub msg_fail_counts: HashMap<(String, u64), u64>,
    pub rows_written: u64,
    pub truncated: bool,
}

impl AttachLedgerFinish {
    pub fn fail_count_for(&self, source_path: &str, msg_nid: u64) -> u64 {
        self.msg_fail_counts
            .get(&(source_path.to_string(), msg_nid))
            .copied()
            .unwrap_or(0)
    }

    /// Populate additive export-section fields from this ledger.
    pub fn apply_to_export_section(&self, export: &mut ExportSection) {
        match self.mode {
            AttachLedgerMode::Off => {
                // Neither CSV nor histogram fields. Per-message fail counts still
                // fill export_messages; attachments_failed (writer total) drives exit.
                // Do not overwrite attachments_omitted_by_policy — Off does not tally
                // omits; caller keeps the writer total (parents_only honesty).
                export.attachments_failed_by_reason = None;
                export.attachment_ledger = None;
                export.attachment_ledger_mode = None;
                export.attachment_ledger_truncated = None;
                export.attachment_ledger_rows_written = None;
            }
            AttachLedgerMode::SummaryOnly => {
                export.attachments_failed_by_reason = Some(self.failed_by_reason.clone());
                export.attachment_ledger_mode = Some(self.mode.as_str().to_string());
                export.attachment_ledger = None;
                export.attachment_ledger_truncated = Some(false);
                export.attachment_ledger_rows_written = Some(0);
                export.attachments_omitted_by_policy = Some(self.omitted_by_policy);
            }
            AttachLedgerMode::Full => {
                export.attachments_failed_by_reason = Some(self.failed_by_reason.clone());
                export.attachment_ledger = Some(EXPORT_ATTACHMENTS_CSV_NAME.to_string());
                export.attachment_ledger_mode = Some(self.mode.as_str().to_string());
                export.attachment_ledger_truncated = Some(self.truncated);
                export.attachment_ledger_rows_written = Some(self.rows_written);
                export.attachments_omitted_by_policy = Some(self.omitted_by_policy);
            }
        }
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
            "{},{},{},{},{},{},{},{},{},{}",
            csv_escape_cell(&r.source_path),
            csv_escape_cell(&r.folder_path),
            r.nid,
            csv_escape_cell(&r.message_id_norm),
            csv_escape_cell(&r.edrm_mih),
            csv_escape_cell(&r.content_hash_hex),
            csv_escape_cell(&r.volume_path),
            r.volume_index,
            r.export_message_index,
            r.attachments_failed_count,
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
            csv_escape_cell(&r.path),
            r.bytes,
            csv_escape_cell(&r.sha256_hex),
            csv_escape_cell(&r.md5_hex),
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
    use pst_writer::{AttachEventSeverity, AttachmentFidelityKind};

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
    fn export_messages_header_order_locked_with_attach_fail_column() {
        assert_eq!(
            EXPORT_MESSAGES_CSV_HEADER,
            "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count"
        );
        // Prefix of pre-0073 header remains stable.
        assert!(EXPORT_MESSAGES_CSV_HEADER
            .starts_with("source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index"));
    }

    #[test]
    fn csv_injection_neutralized() {
        for dangerous in ["=cmd|'/c calc'!A0", "+1+1", "@SUM(A1:A2)", "-2+3"] {
            let out = csv_escape_cell(dangerous);
            // After neutralization the visible payload starts with ' (possibly quoted).
            let raw = neutralize_csv_formula(dangerous);
            assert!(
                raw.starts_with('\''),
                "expected leading quote for {dangerous:?}, got {raw:?}"
            );
            assert!(
                out.contains('\''),
                "escaped cell should retain quote: {out}"
            );
        }
        assert_eq!(csv_escape_cell("normal.txt"), "normal.txt");
        assert_eq!(csv_escape_cell("a,b"), "\"a,b\"");
    }

    #[test]
    fn attach_ledger_mode_parse() {
        assert_eq!(
            AttachLedgerMode::parse("full"),
            Some(AttachLedgerMode::Full)
        );
        assert_eq!(
            AttachLedgerMode::parse("summary-only"),
            Some(AttachLedgerMode::SummaryOnly)
        );
        assert_eq!(AttachLedgerMode::parse("off"), Some(AttachLedgerMode::Off));
        assert_eq!(AttachLedgerMode::parse("nope"), None);
    }

    fn synth_event(
        kind: AttachmentFidelityKind,
        severity: AttachEventSeverity,
    ) -> AttachmentFidelityEvent {
        AttachmentFidelityEvent {
            message_subject: "subj".into(),
            attach_filename: "f.bin".into(),
            kind,
            source_path: r"C:\in\a.pst".into(),
            folder_path: "Inbox".into(),
            msg_nid: 42,
            attach_nid: Some(7),
            attach_index: 0,
            size: Some(10),
            attach_method: 1,
            severity,
        }
    }

    #[test]
    fn row_cap_truncation_marker_and_histogram_continues() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string()];
        let mut sink =
            AttachLedgerSink::new(AttachLedgerMode::Full, 2, dir.path(), &inputs).expect("sink");
        sink.set_volume(r"C:\out\u.pst", 1);

        // max_rows=2 → 1 data row + 1 marker, then drop further CSV but keep histogram.
        for _ in 0..5 {
            sink.ingest(&synth_event(
                AttachmentFidelityKind::StreamOpenFailed,
                AttachEventSeverity::Fail,
            ));
        }
        let finish = sink.finish().expect("finish");
        assert!(finish.truncated);
        assert_eq!(
            finish.failed_by_reason.get("ATTACH_STREAM_OPEN_FAILED"),
            Some(&5)
        );
        assert!(finish.rows_written >= 2);
        let csv = fs::read_to_string(dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
        assert!(
            csv.contains("ATTACH_LEDGER_TRUNCATED"),
            "marker row required; csv={csv}"
        );
        // Only one real fail row before marker (max_rows=2).
        let fail_rows = csv
            .lines()
            .filter(|l| l.contains("ATTACH_STREAM_OPEN_FAILED"))
            .count();
        assert!(fail_rows <= 1, "CSV capped; fail_rows={fail_rows}");
    }

    #[test]
    fn summary_only_no_csv_histogram_present() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string()];
        let mut sink =
            AttachLedgerSink::new(AttachLedgerMode::SummaryOnly, 500_000, dir.path(), &inputs)
                .expect("sink");
        sink.ingest(&synth_event(
            AttachmentFidelityKind::MethodUnsupported,
            AttachEventSeverity::Fail,
        ));
        let finish = sink.finish().expect("finish");
        assert!(!dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME).exists());
        assert_eq!(
            finish.failed_by_reason.get("ATTACH_METHOD_UNSUPPORTED"),
            Some(&1)
        );
        let mut export = ExportSection {
            volumes: vec![],
            partial: false,
            messages_written_total: 0,
            attachments_written: 0,
            attachments_failed: 1,
            attachments_omitted_by_policy: None,
            attachments_failed_by_reason: None,
            attachment_ledger: None,
            attachment_ledger_mode: None,
            attachment_ledger_truncated: None,
            attachment_ledger_rows_written: None,
            error: None,
            failed_volume_index: None,
        };
        finish.apply_to_export_section(&mut export);
        assert!(export.attachment_ledger.is_none());
        assert_eq!(
            export.attachment_ledger_mode.as_deref(),
            Some("summary-only")
        );
        assert!(export.attachments_failed_by_reason.is_some());
    }

    #[test]
    fn source_id_from_inputs_order() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\a.pst".into(), r"C:\b.pst".into()];
        let mut sink =
            AttachLedgerSink::new(AttachLedgerMode::Full, 100, dir.path(), &inputs).expect("sink");
        sink.set_volume("out.pst", 1);
        let mut ev = synth_event(
            AttachmentFidelityKind::StreamReadFailed,
            AttachEventSeverity::Fail,
        );
        ev.source_path = r"C:\b.pst".into();
        sink.ingest(&ev);
        let _ = sink.finish().expect("finish");
        let csv = fs::read_to_string(dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
        let data = csv.lines().nth(1).expect("row");
        assert!(
            data.starts_with("1,"),
            "source_id for second input must be 1; row={data}"
        );
    }

    #[test]
    fn unmapped_source_id_is_empty_not_zero() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\a.pst".into()];
        let mut sink =
            AttachLedgerSink::new(AttachLedgerMode::Full, 100, dir.path(), &inputs).expect("sink");
        sink.set_volume("out.pst", 1);
        let mut ev = synth_event(
            AttachmentFidelityKind::StreamOpenFailed,
            AttachEventSeverity::Fail,
        );
        ev.source_path = r"C:\unknown\other.pst".into();
        sink.ingest(&ev);
        let _ = sink.finish().expect("finish");
        let csv = fs::read_to_string(dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
        let data = csv.lines().nth(1).expect("row");
        assert!(
            data.starts_with(','),
            "unmapped source_id must be empty field (not 0); row={data}"
        );
        assert!(
            !data.starts_with("0,"),
            "must not invent source_id 0 for unknown path; row={data}"
        );
    }

    #[test]
    fn off_mode_still_tracks_msg_fail_counts() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string()];
        let mut sink = AttachLedgerSink::new(AttachLedgerMode::Off, 500_000, dir.path(), &inputs)
            .expect("sink");
        sink.ingest(&synth_event(
            AttachmentFidelityKind::StreamOpenFailed,
            AttachEventSeverity::Fail,
        ));
        let finish = sink.finish().expect("finish");
        assert!(finish.failed_by_reason.is_empty(), "Off: no histogram");
        assert_eq!(finish.fail_count_for(r"C:\in\a.pst", 42), 1);
        assert!(!dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME).exists());
        let mut export = ExportSection {
            volumes: vec![],
            partial: false,
            messages_written_total: 0,
            attachments_written: 0,
            attachments_failed: 1,
            attachments_omitted_by_policy: Some(9),
            attachments_failed_by_reason: None,
            attachment_ledger: None,
            attachment_ledger_mode: None,
            attachment_ledger_truncated: None,
            attachment_ledger_rows_written: None,
            error: None,
            failed_volume_index: None,
        };
        finish.apply_to_export_section(&mut export);
        assert!(export.attachments_failed_by_reason.is_none());
        assert_eq!(export.attachments_omitted_by_policy, Some(9));
    }

    #[test]
    fn volume_buffer_discard_does_not_pollute_global() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string()];
        let mut global =
            AttachLedgerSink::new(AttachLedgerMode::Full, 100, dir.path(), &inputs).expect("sink");
        global.set_volume("vol1.pst", 1);

        // Simulated failed volume: buffer then drop.
        let mut failed_vol = VolumeAttachBuffer::new();
        failed_vol.on_attach_event(&synth_event(
            AttachmentFidelityKind::StreamOpenFailed,
            AttachEventSeverity::Fail,
        ));
        assert_eq!(failed_vol.len(), 1);
        drop(failed_vol);

        // Committed volume: buffer then commit.
        let mut ok_vol = VolumeAttachBuffer::new();
        let mut ok_ev = synth_event(
            AttachmentFidelityKind::MethodUnsupported,
            AttachEventSeverity::Fail,
        );
        ok_ev.msg_nid = 99;
        ok_vol.on_attach_event(&ok_ev);
        ok_vol.commit_into(&mut global);

        let finish = global.finish().expect("finish");
        assert_eq!(
            finish.failed_by_reason.get("ATTACH_STREAM_OPEN_FAILED"),
            None,
            "discarded volume fails must not enter histogram"
        );
        assert_eq!(
            finish.failed_by_reason.get("ATTACH_METHOD_UNSUPPORTED"),
            Some(&1)
        );
        assert_eq!(finish.fail_count_for(r"C:\in\a.pst", 42), 0);
        assert_eq!(finish.fail_count_for(r"C:\in\a.pst", 99), 1);

        let csv = fs::read_to_string(dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
        assert!(
            !csv.contains("ATTACH_STREAM_OPEN_FAILED"),
            "discarded volume must not write CSV rows"
        );
        assert!(csv.contains("ATTACH_METHOD_UNSUPPORTED"));
    }
}
