//! Unique-export report pack (`unique_export_report_v1`) — tracks 0071 / 0073.
//!
//! Disk layout under `{report-dir}/`:
//! - `summary.json`
//! - `volumes.csv`
//! - `export_messages.csv` (mandatory when ≥1 message written)
//! - `export_attachments.csv` (track 0073; `--attach-ledger=full`)
//! - `decisions.csv` / `keepset.json` / optional `integrity.csv` (orchestrator)

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use dedup_engine::integrity::PreflightRecommendation;
use pst_writer::{AttachEventSeverity, AttachEventSink, AttachmentFidelityEvent};
use serde::{Deserialize, Serialize};

use crate::error::{CliError, Result};
use crate::export_outcome::{ArtifactState, ExportFidelity};

/// Schema id for the unique-export summary JSON.
pub const UNIQUE_EXPORT_REPORT_SCHEMA: &str = "unique_export_report_v1";

/// Fixed header for mandatory `export_messages.csv` (prefix locked; 0073/0075/0081/0082/0085 append).
pub const EXPORT_MESSAGES_CSV_HEADER: &str = "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count,duplicate_source_count,duplicate_sources,source_id,bcc_suppressed,body_cloud_link_count";

/// Pre-0075 export_messages header prefix (10 columns).
pub const EXPORT_MESSAGES_CSV_HEADER_V1: &str = "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count";

/// Fixed header for `volumes.csv`.
pub const VOLUMES_CSV_HEADER: &str =
    "volume_index,path,bytes,sha256,md5,messages_written,finalized_early,volume_exceeded_soft_limit";

/// Fixed header for `export_attachments.csv` (track 0073).
pub const EXPORT_ATTACHMENTS_CSV_HEADER: &str = "source_id,source_path,folder_path,msg_nid,attach_nid,attach_index,filename,size,attach_method,reason_code,severity,volume_path,volume_index,winner_promoted,peer_source_id,peer_msg_nid,message_subject,cloud_provider,cloud_url";

/// On-disk name for the attach failure ledger.
pub const EXPORT_ATTACHMENTS_CSV_NAME: &str = "export_attachments.csv";

/// Fixed header for `export_body_cloud_links.csv` (track 0085).
pub const EXPORT_BODY_CLOUD_LINKS_CSV_HEADER: &str = "source_id,source_path,folder_path,msg_nid,link_index,cloud_url,url_source,truncated,message_subject,reason";

/// On-disk name for the body-inline cloud link hit-list (0085).
pub const EXPORT_BODY_CLOUD_LINKS_CSV_NAME: &str = "export_body_cloud_links.csv";

/// Row kind for a kept document-shaped body cloud URL (0085).
pub const REASON_BODY_CLOUD_LINK: &str = "BODY_CLOUD_LINK";
/// Honesty marker: document-shaped candidate(s) existed past the 100k window (0097).
pub const REASON_BODY_CLOUD_LINK_WINDOW: &str = "BODY_CLOUD_LINK_WINDOW";
/// Honesty marker: additional document-shaped candidates past the 50-link cap (0097).
pub const REASON_BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED: &str = "BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED";
/// Honesty marker: document-shaped URL exceeded 2048 chars; `cloud_url` holds the prefix (0097).
pub const REASON_BODY_CLOUD_LINK_URL_TRUNCATED: &str = "BODY_CLOUD_LINK_URL_TRUNCATED";

/// Pipe-join honesty-marker reason strings (order locked: WINDOW | MAX_LINKS | URL).
pub fn body_cloud_honesty_reason(
    window_dropped: bool,
    max_links_exceeded: bool,
    url_truncated: bool,
) -> String {
    let mut parts = Vec::new();
    if window_dropped {
        parts.push(REASON_BODY_CLOUD_LINK_WINDOW);
    }
    if max_links_exceeded {
        parts.push(REASON_BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED);
    }
    if url_truncated {
        parts.push(REASON_BODY_CLOUD_LINK_URL_TRUNCATED);
    }
    parts.join("|")
}

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

/// How `source_path` columns are written to handoff CSVs (track 0081).
///
/// Default `full` preserves absolute/workstation paths. `basename` strips
/// directory prefixes for handoff copies only — join origin via `source_id`
/// and a non-produced Matter Archive mapping. Does **not** affect in-memory
/// keys (msg fail counts, QC during the same run) or `source_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LedgerPathMode {
    /// Write the full source path as resolved at export time (default).
    #[default]
    Full,
    /// Write only the file basename (e.g. `custodian.pst`).
    Basename,
}

impl LedgerPathMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Basename => "basename",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "basename" | "base" | "file" => Some(Self::Basename),
            _ => None,
        }
    }
}

/// Format a source path for ledger/export CSV path columns.
///
/// - Empty input stays empty in both modes.
/// - Basename mode: `Path::file_name`; when the full path was non-empty the
///   result is never empty (falls back to the full string if `file_name` is
///   missing — e.g. trailing separator edge cases).
pub fn format_ledger_source_path(path: &str, mode: LedgerPathMode) -> String {
    if path.is_empty() {
        return String::new();
    }
    match mode {
        LedgerPathMode::Full => path.to_string(),
        LedgerPathMode::Basename => {
            let base = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty());
            match base {
                Some(b) => b.to_string(),
                None => path.to_string(),
            }
        }
    }
}

/// Resolve 0-based `source_id` for a source path against CLI input order.
///
/// Returns `None` when unmapped — callers write an **empty** field (never invent
/// decimal `0`). Matches exact path first, then case-insensitive equality
/// (same honesty as [`AttachLedgerSink`]).
pub fn resolve_input_source_id(source_path: &str, inputs: &[String]) -> Option<u32> {
    if let Some(i) = inputs.iter().position(|p| p == source_path) {
        return Some(i as u32);
    }
    inputs
        .iter()
        .position(|p| p.eq_ignore_ascii_case(source_path))
        .map(|i| i as u32)
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
    /// Distinct other sources that held a suppressed copy (0075; basename).
    pub duplicate_source_count: u64,
    /// `|`-delimited basenames, capped at 8 (0075).
    pub duplicate_sources: String,
    /// 0-based index into `summary.inputs` as a decimal string; empty when
    /// unmapped (0081 — never invent `"0"`). Join key under `--ledger-path-mode
    /// basename` when multiple sources share a basename.
    pub source_id: String,
    /// True when source had BCC (table Bcc row or non-empty display_bcc) and the
    /// write path omitted them (`include_bcc_recipients == false`) — 0082 rule 7.
    pub bcc_suppressed: bool,
    /// Document-shaped body cloud link hits kept for this message (0085; not attach-incomplete).
    pub body_cloud_link_count: u64,
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
    /// Whether in-process attach event Vec was capped (0077).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_fidelity_events_truncated: Option<bool>,
    /// Total attach events observed (may exceed Vec len when truncated; 0077).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_fidelity_events_total: Option<u64>,
    /// Messages whose recipient TC was budget-truncated (0093 Strategy B).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_tc_truncated_messages: Option<u64>,
    /// Total recipient rows dropped by TC budget cap (0093).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_rows_truncated: Option<u64>,
    /// Whether in-process truncate-event Vec was capped (0093).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_tc_truncated_events_truncated: Option<bool>,
    /// Total truncate events observed (may exceed Vec len; 0093).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_tc_truncated_events_total: Option<u64>,
    /// First-N writer truncate events for clean-room QC (0093).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_tc_truncations: Option<Vec<RecipientTcTruncationRow>>,
    /// Whether BCC rows / `PidTagDisplayBcc` were written (0082 `--include-bcc-recipients`).
    /// Default false. Clean-room `qc-pst` reads this so re-QC matches the export policy.
    #[serde(default)]
    pub include_bcc_recipients: bool,
}

/// Serializable recipient TC truncate row for `summary.json` (0093).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecipientTcTruncationRow {
    pub reason: String,
    pub message_subject: String,
    pub source_path: String,
    pub folder_path: String,
    pub msg_nid: u64,
    pub message_id: String,
    pub source_count: u32,
    pub kept_count: u32,
    pub kept_to: u32,
    pub kept_cc: u32,
    pub kept_bcc: u32,
    pub dropped_to: u32,
    pub dropped_cc: u32,
    pub dropped_bcc: u32,
}

impl RecipientTcTruncationRow {
    pub fn from_writer_event(ev: &pst_writer::RecipientTcTruncatedEvent) -> Self {
        Self {
            reason: ev.reason().to_string(),
            message_subject: ev.message_subject.clone(),
            source_path: ev.source_path.clone(),
            folder_path: ev.folder_path.clone(),
            msg_nid: ev.msg_nid,
            message_id: ev.message_id.clone(),
            source_count: ev.source_count,
            kept_count: ev.kept_count,
            kept_to: ev.kept_to,
            kept_cc: ev.kept_cc,
            kept_bcc: ev.kept_bcc,
            dropped_to: ev.dropped_to,
            dropped_cc: ev.dropped_cc,
            dropped_bcc: ev.dropped_bcc,
        }
    }

    pub fn to_writer_event(&self) -> pst_writer::RecipientTcTruncatedEvent {
        pst_writer::RecipientTcTruncatedEvent {
            message_subject: self.message_subject.clone(),
            source_path: self.source_path.clone(),
            folder_path: self.folder_path.clone(),
            msg_nid: self.msg_nid,
            message_id: self.message_id.clone(),
            source_count: self.source_count,
            kept_count: self.kept_count,
            kept_to: self.kept_to,
            kept_cc: self.kept_cc,
            kept_bcc: self.kept_bcc,
            dropped_to: self.dropped_to,
            dropped_cc: self.dropped_cc,
            dropped_bcc: self.dropped_bcc,
        }
    }
}

/// Per-source CRC class copied 1:1 from [`crate::scan::FileScanStats`] (0099).
///
/// Do not pre-average rates here — [`poly_crc_risk_adjustment`] owns the sums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrcSourceClass {
    pub poly_class_crc: bool,
    pub page_crc_mismatches: u64,
    pub block_crc_mismatches: u64,
    pub page_reads: u64,
    pub block_reads: u64,
}

/// Post-export CRC adjustment: thresholds key on **effective** (non-poly) rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolyCrcRiskAdjustment {
    /// Non-poly CRC sum / non-poly reads. `None` if per-source stats missing (fail closed).
    pub effective_block_crc_read_rate: Option<f64>,
    /// True when ≥1 poly-class source was excluded from the rate used for thresholds.
    pub poly_class_crc_discounted: bool,
    /// True when attach-stream CRC can only be poly noise (no CRC-noisy non-poly source).
    pub discount_attach_stream_crc: bool,
    pub poly_class_crc_sources: u64,
    pub non_poly_crc_noisy_sources: u64,
}

fn crc_noisy(s: &CrcSourceClass) -> bool {
    s.page_crc_mismatches.saturating_add(s.block_crc_mismatches) > 0
}

/// Map scan `files[]` to CRC class rows (raw counters; no pre-averaged rates).
pub fn crc_source_classes_from_files(files: &[crate::scan::FileScanStats]) -> Vec<CrcSourceClass> {
    files
        .iter()
        .map(|f| CrcSourceClass {
            poly_class_crc: f.poly_class_crc,
            page_crc_mismatches: f.page_crc_mismatches,
            block_crc_mismatches: f.block_crc_mismatches,
            page_reads: f.page_reads,
            block_reads: f.block_reads,
        })
        .collect()
}

/// Effective (non-poly) CRC rate and attach-CRC discount flags (0099).
///
/// Empty `sources` → fail closed (`effective = None`, discount flags false).
pub fn poly_crc_risk_adjustment(sources: &[CrcSourceClass]) -> PolyCrcRiskAdjustment {
    if sources.is_empty() {
        return PolyCrcRiskAdjustment {
            effective_block_crc_read_rate: None,
            poly_class_crc_discounted: false,
            discount_attach_stream_crc: false,
            poly_class_crc_sources: 0,
            non_poly_crc_noisy_sources: 0,
        };
    }

    let poly_class_crc_sources = sources.iter().filter(|s| s.poly_class_crc).count() as u64;
    let non_poly_crc_noisy_sources = sources
        .iter()
        .filter(|s| crc_noisy(s) && !s.poly_class_crc)
        .count() as u64;

    let mut crc_sum = 0u64;
    let mut reads = 0u64;
    for s in sources.iter().filter(|s| !s.poly_class_crc) {
        crc_sum =
            crc_sum.saturating_add(s.page_crc_mismatches.saturating_add(s.block_crc_mismatches));
        reads = reads.saturating_add(s.page_reads.saturating_add(s.block_reads));
    }
    let effective = if reads == 0 {
        0.0
    } else {
        (crc_sum as f64 / reads as f64).clamp(0.0, 1.0)
    };

    // Spec §3.1 `!any(crc_noisy && !poly)` plus §3.4 all-clean (flag false):
    // attach CRC is "only poly noise" when a poly source was actually excluded.
    let discount_attach_stream_crc = poly_class_crc_sources >= 1 && non_poly_crc_noisy_sources == 0;
    let poly_class_crc_discounted = poly_class_crc_sources >= 1;

    PolyCrcRiskAdjustment {
        effective_block_crc_read_rate: Some(effective),
        poly_class_crc_discounted,
        discount_attach_stream_crc,
        poly_class_crc_sources,
        non_poly_crc_noisy_sources,
    }
}

/// Inputs for post-export risk evaluation (0077 / 0099).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportRiskInputs {
    pub attach_fail_rate: f64,
    pub block_crc_rate: f64,
    pub block_crc_read_rate: f64,
    pub degraded_winner_rate: f64,
    pub partial: bool,
    pub failed_volume_index: Option<u32>,
    pub scan_recommendation: PreflightRecommendation,
    /// Count of final-write `ATTACH_STREAM_CRC` Info events (0077; default 0).
    /// Warning-only: does not raise `attachments_failed` / attach_fail_rate.
    #[serde(default)]
    pub attach_stream_crc_events: u64,
    /// Rate thresholds use when `Some` (non-poly sources). `None` → raw (fail closed).
    #[serde(default)]
    pub effective_block_crc_read_rate: Option<f64>,
    /// Attest: ≥1 poly-class source was excluded from the keyed rate.
    #[serde(default)]
    pub poly_class_crc_discounted: bool,
    /// Skip `attach_stream_crc_events>0` advisory (poly-only CRC noise).
    #[serde(default)]
    pub discount_attach_stream_crc: bool,
    /// Telemetry copy of scan `poly_class_crc_sources`.
    #[serde(default)]
    pub poly_class_crc_sources: u64,
}

impl Default for ExportRiskInputs {
    fn default() -> Self {
        Self {
            attach_fail_rate: 0.0,
            block_crc_rate: 0.0,
            block_crc_read_rate: 0.0,
            degraded_winner_rate: 0.0,
            partial: false,
            failed_volume_index: None,
            scan_recommendation: PreflightRecommendation::Ok,
            attach_stream_crc_events: 0,
            effective_block_crc_read_rate: None,
            poly_class_crc_discounted: false,
            discount_attach_stream_crc: false,
            poly_class_crc_sources: 0,
        }
    }
}

/// Visible thresholds for `export_risk` (serde-defaulted; 0077).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportRiskThresholds {
    pub max_attach_fail_rate: f64,
    pub max_block_crc_read_rate: f64,
    pub max_degraded_winner_rate: f64,
    pub catastrophic_block_crc_read_rate: f64,
    pub catastrophic_attach_fail_rate: f64,
}

impl Default for ExportRiskThresholds {
    fn default() -> Self {
        Self {
            max_attach_fail_rate: 0.05,
            max_block_crc_read_rate: 0.01,
            max_degraded_winner_rate: 0.02,
            catastrophic_block_crc_read_rate: 0.15,
            catastrophic_attach_fail_rate: 0.50,
        }
    }
}

/// Post-export risk on the **existing** preflight vocabulary (no second enum).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportRisk {
    pub level: PreflightRecommendation,
    /// Closed vocabulary, sorted (e.g. `attach_fail_rate=0.098>0.05`).
    pub reasons: Vec<String>,
    pub inputs: ExportRiskInputs,
    pub thresholds: ExportRiskThresholds,
}

/// Compute `export_risk`: max(scan preflight, post-export evaluation).
///
/// Advisory thresholds → `re_export_recommended`. Catastrophic rates or hard
/// failures → `not_export_ready`. Export never lowers scan risk.
pub fn compute_export_risk(
    scan_recommendation: &PreflightRecommendation,
    inputs: &ExportRiskInputs,
) -> ExportRisk {
    compute_export_risk_with_thresholds(
        scan_recommendation,
        inputs,
        &ExportRiskThresholds::default(),
    )
}

/// Same as [`compute_export_risk`] with explicit thresholds (tests / overrides).
pub fn compute_export_risk_with_thresholds(
    scan_recommendation: &PreflightRecommendation,
    inputs: &ExportRiskInputs,
    thresholds: &ExportRiskThresholds,
) -> ExportRisk {
    let mut reasons: Vec<String> = Vec::new();
    let mut post = PreflightRecommendation::Ok;

    // Hard conditions → not_export_ready.
    if inputs.failed_volume_index.is_some() {
        post = PreflightRecommendation::NotExportReady;
        reasons.push(format!(
            "failed_volume_index={}",
            inputs.failed_volume_index.unwrap_or(0)
        ));
    }
    if inputs.partial && inputs.failed_volume_index.is_some() {
        // already not_export_ready; keep reason specific
        if !reasons
            .iter()
            .any(|r| r.starts_with("partial+failed_volume"))
        {
            reasons.push("partial+failed_volume".into());
        }
    }
    if *scan_recommendation == PreflightRecommendation::NotExportReady {
        // Carried forward via max(); still name it.
        reasons.push("scan_recommendation=not_export_ready".into());
    }
    if *scan_recommendation == PreflightRecommendation::ReExportRecommended {
        // When scan alone elevates and post is ok, reasons must not be empty.
        reasons.push("scan_preflight=re_export_recommended".into());
    }

    // 0099: thresholds key on effective (non-poly) rate when present; else raw.
    let keyed_block_crc_read_rate = inputs
        .effective_block_crc_read_rate
        .unwrap_or(inputs.block_crc_read_rate);
    let using_effective = inputs.effective_block_crc_read_rate.is_some();
    let crc_rate_reason = |rate: f64, threshold: f64| -> String {
        if using_effective {
            format!("effective_block_crc_read_rate={rate:.3}>{threshold}")
        } else {
            format!("block_crc_read_rate={rate:.3}>{threshold}")
        }
    };
    let is_crc_rate_reason = |r: &str| {
        r.starts_with("block_crc_read_rate=") || r.starts_with("effective_block_crc_read_rate=")
    };

    // Catastrophic rates → not_export_ready (may fire without failed volume).
    if keyed_block_crc_read_rate > thresholds.catastrophic_block_crc_read_rate {
        post = PreflightRecommendation::NotExportReady;
        reasons.push(crc_rate_reason(
            keyed_block_crc_read_rate,
            thresholds.catastrophic_block_crc_read_rate,
        ));
    }
    if inputs.attach_fail_rate > thresholds.catastrophic_attach_fail_rate {
        post = PreflightRecommendation::NotExportReady;
        reasons.push(format!(
            "attach_fail_rate={:.3}>{}",
            inputs.attach_fail_rate, thresholds.catastrophic_attach_fail_rate
        ));
    }

    // Advisory thresholds → re_export_recommended (cannot alone reach not_export_ready).
    if post == PreflightRecommendation::Ok {
        if inputs.attach_fail_rate > thresholds.max_attach_fail_rate {
            post = PreflightRecommendation::ReExportRecommended;
            reasons.push(format!(
                "attach_fail_rate={:.3}>{}",
                inputs.attach_fail_rate, thresholds.max_attach_fail_rate
            ));
        }
        if keyed_block_crc_read_rate > thresholds.max_block_crc_read_rate {
            post = PreflightRecommendation::ReExportRecommended;
            reasons.push(crc_rate_reason(
                keyed_block_crc_read_rate,
                thresholds.max_block_crc_read_rate,
            ));
        }
        if inputs.degraded_winner_rate > thresholds.max_degraded_winner_rate {
            post = PreflightRecommendation::ReExportRecommended;
            reasons.push(format!(
                "degraded_winner_rate={:.3}>{}",
                inputs.degraded_winner_rate, thresholds.max_degraded_winner_rate
            ));
        }
        // Final attach stream CRC is warning-only (not attach_fail_rate) but still
        // elevates export_risk so operators re-export rather than trust the bytes.
        // 0099: skip when attach CRC can only be poly-class noise.
        if inputs.attach_stream_crc_events > 0 && !inputs.discount_attach_stream_crc {
            post = PreflightRecommendation::ReExportRecommended;
            reasons.push(format!(
                "attach_stream_crc_events={}>0",
                inputs.attach_stream_crc_events
            ));
        }
    } else {
        // Still surface advisory crossings for operator detail when already catastrophic.
        if inputs.attach_fail_rate > thresholds.max_attach_fail_rate
            && !reasons.iter().any(|r| r.starts_with("attach_fail_rate="))
        {
            reasons.push(format!(
                "attach_fail_rate={:.3}>{}",
                inputs.attach_fail_rate, thresholds.max_attach_fail_rate
            ));
        }
        if keyed_block_crc_read_rate > thresholds.max_block_crc_read_rate
            && !reasons.iter().any(|r| is_crc_rate_reason(r))
        {
            reasons.push(crc_rate_reason(
                keyed_block_crc_read_rate,
                thresholds.max_block_crc_read_rate,
            ));
        }
        if inputs.attach_stream_crc_events > 0
            && !inputs.discount_attach_stream_crc
            && !reasons
                .iter()
                .any(|r| r.starts_with("attach_stream_crc_events="))
        {
            reasons.push(format!(
                "attach_stream_crc_events={}>0",
                inputs.attach_stream_crc_events
            ));
        }
    }

    if inputs.poly_class_crc_discounted {
        reasons.push("poly_class_crc_discounted".into());
    }

    reasons.sort();
    reasons.dedup();

    let level = scan_recommendation.max(post);
    let mut inputs_out = inputs.clone();
    inputs_out.scan_recommendation = *scan_recommendation;

    ExportRisk {
        level,
        reasons,
        inputs: inputs_out,
        thresholds: thresholds.clone(),
    }
}

/// Per-phase wall-clock timings for unique-pst (track 0079).
///
/// All fields are additive and `#[serde(default)]` so older summaries remain
/// readable. `unaccounted_ms = total_ms − Σ(phases)` is **computed**, never
/// forced to zero — a non-zero value means instrumentation gap, not noise.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PhaseTimings {
    pub scan_ms: u64,
    pub deep_attach_preflight_ms: u64,
    pub resolve_ms: u64,
    pub materialize_ms: u64,
    pub prepare_ms: u64,
    pub write_ms: u64,
    pub report_ms: u64,
    pub verify_ms: u64,
    /// Source-differential / external QC wall time (0080).
    pub qc_ms: u64,
    pub quarantine_ms: u64,
    /// `total_ms − Σ(phases)`. Non-zero is a gap in instrumentation, not noise.
    pub unaccounted_ms: u64,
    pub total_ms: u64,
}

impl PhaseTimings {
    /// Sum of all accounted phase fields (excludes `unaccounted_ms` / `total_ms`).
    pub fn accounted_ms(self) -> u64 {
        self.scan_ms
            .saturating_add(self.deep_attach_preflight_ms)
            .saturating_add(self.resolve_ms)
            .saturating_add(self.materialize_ms)
            .saturating_add(self.prepare_ms)
            .saturating_add(self.write_ms)
            .saturating_add(self.report_ms)
            .saturating_add(self.verify_ms)
            .saturating_add(self.qc_ms)
            .saturating_add(self.quarantine_ms)
    }

    /// Fill `total_ms` and `unaccounted_ms` from wall start elapsed.
    pub fn finalize(&mut self, total_ms: u64) {
        self.total_ms = total_ms;
        self.unaccounted_ms = total_ms.saturating_sub(self.accounted_ms());
    }
}

/// Soft warning threshold for retained prepared winner bytes (body + buffered
/// attach payloads). Above this, unique-pst emits a soft warning (0079 §3.9).
/// Documented default: **1 GiB**.
pub const PREPARED_BYTES_PEAK_WARN_THRESHOLD: u64 = 1_073_741_824;

/// Top-level `summary.json` payload (`unique_export_report_v1`).
#[derive(Debug, Clone, Serialize)]
pub struct UniqueExportSummary {
    pub schema: String,
    pub ok: bool,
    /// Terminal fidelity (0078): `complete` | `partial` | `failed`.
    #[serde(default)]
    pub fidelity: ExportFidelity,
    /// Process exit code that must equal the real process status (0078 DoD-9).
    #[serde(default)]
    pub exit_code: u8,
    /// Closed-vocabulary reason codes, worst-first (0078).
    #[serde(default)]
    pub exit_reason: Vec<String>,
    /// On-disk artifact disposition (0078 closed vocabulary).
    #[serde(default)]
    pub artifact_state: ArtifactState,
    /// Absolute path of this summary file (self-locating; 0078).
    #[serde(default)]
    pub summary_path: String,
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
    /// Per-phase timings (0079). Always present; defaults to zeros for old readers.
    #[serde(default)]
    pub phase_timings: PhaseTimings,
    /// Source PST open count across materializer/attach-stream cache (0079).
    #[serde(default)]
    pub source_pst_opens: u64,
    /// Times `materialize` returned a winner message (0079; must equal `unique` after D1).
    #[serde(default)]
    pub messages_materialized: u64,
    /// Sum of completed volume `bytes` (0079).
    #[serde(default)]
    pub bytes_written_total: u64,
    /// Peak retained body + buffered-attach bytes in `prepared` (0079 §3.9).
    #[serde(default)]
    pub prepared_bytes_peak: u64,
    /// Final-hash wall time across volumes (SHA-256+MD5 of temp), when measured (0079).
    #[serde(default)]
    pub hash_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_volume_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision_csv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_set_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SummaryError>,
    /// Post-export risk (0077); same vocabulary as preflight.
    pub export_risk: ExportRisk,
    /// Messages where source BCC was omitted from the written PST by policy (0082).
    #[serde(default)]
    pub bcc_suppressed_message_count: u64,
    /// Empty recipient table on a non-draft (MSGFLAG_UNSENT clear) message (0082 rule 8).
    /// Telemetry only — does **not** invent a new `export_risk` value.
    #[serde(default)]
    pub sent_message_with_no_recipients_count: u64,
    /// Whether automation may retry this run (0082 D-0078-retryable).
    /// `true` only for clearly transient IO / cancel-retry classes; permanent
    /// failures (risk gate, fidelity, schema, passphrase, audit) stay `false`.
    #[serde(default)]
    pub retryable: bool,
    /// Whether Mode A `--promote-on-attach-fail` was enabled (0083). Default false.
    #[serde(default)]
    pub promote_on_attach_fail: bool,
    /// Mode A successful soft-attach promote count (0083).
    #[serde(default)]
    pub promoted_after_attach_incomplete_count: u64,
    /// Mode A all-peers-incomplete Mode C fallback count (0083).
    #[serde(default)]
    pub mode_c_fallback_all_peers_incomplete_count: u64,
    /// Messages with ≥1 kept body-inline document-shaped cloud link (0085).
    #[serde(default)]
    pub messages_with_body_cloud_links: u64,
    /// Total kept body-inline cloud link hits across written winners (0085).
    #[serde(default)]
    pub body_cloud_links_total: u64,
    /// Messages where document-shaped candidates were actually dropped
    /// (window tail / max-links / url-len).
    #[serde(default)]
    pub body_cloud_link_truncated_messages: u64,
    /// Messages whose HTML or plain body exceeded the 100k scan window (0097).
    /// Independent of whether any document-shaped candidate was dropped.
    #[serde(default)]
    pub body_scan_window_capped_messages: u64,
    /// Store RecordKey mode for this export (0087). Default `"deterministic"`.
    /// Values: `"deterministic"` | `"ephemeral"`.
    /// Always set by unique-pst writers; `default` reserved if Deserialize is added.
    #[serde(default)]
    pub store_record_key_mode: String,
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
    /// Cloud provider (0084 append); empty when not CloudLink.
    pub cloud_provider: String,
    /// Cloud URL (0084 append); empty when unknown / not CloudLink. Formula-neutralized.
    pub cloud_url: String,
}

impl AttachLedgerRow {
    /// Format one CSV data line (public for unit tests of column append / injection).
    pub fn to_csv_line(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
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
            csv_escape_cell(&self.cloud_provider),
            csv_escape_cell(&self.cloud_url),
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
            cloud_provider: String::new(),
            cloud_url: String::new(),
        }
    }
}

/// Build a ledger row from a writer fidelity event + CLI enrichment.
///
/// `source_id` is the decimal index into inputs, or empty when unmapped (never a fake `0`).
/// `path_mode` formats the CSV `source_path` column only; resolution of `source_id`
/// must use the full `event.source_path` before this call.
///
/// `winner_promoted` / peer locus: set when Mode A promoted away from this locus (0083).
pub fn ledger_row_from_event(
    event: &AttachmentFidelityEvent,
    source_id: Option<u32>,
    volume_path: &str,
    volume_index: u32,
    path_mode: LedgerPathMode,
) -> AttachLedgerRow {
    ledger_row_from_event_ex(
        event,
        source_id,
        volume_path,
        volume_index,
        path_mode,
        &LedgerPromoteContext::default(),
    )
}

/// Mode A promote honesty fields for attach ledger rows (0083).
#[derive(Debug, Clone, Default)]
pub struct LedgerPromoteContext {
    pub winner_promoted: bool,
    pub peer_source_id: String,
    pub peer_msg_nid: String,
}

/// Extended ledger row builder with Mode A promote honesty fields (0083).
pub fn ledger_row_from_event_ex(
    event: &AttachmentFidelityEvent,
    source_id: Option<u32>,
    volume_path: &str,
    volume_index: u32,
    path_mode: LedgerPathMode,
    promote: &LedgerPromoteContext,
) -> AttachLedgerRow {
    AttachLedgerRow {
        source_id: source_id.map(|id| id.to_string()).unwrap_or_default(),
        source_path: format_ledger_source_path(&event.source_path, path_mode),
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
        winner_promoted: promote.winner_promoted,
        peer_source_id: promote.peer_source_id.clone(),
        peer_msg_nid: promote.peer_msg_nid.clone(),
        message_subject: event.message_subject.clone(),
        cloud_provider: event.cloud_provider.clone(),
        cloud_url: event.cloud_url.clone(),
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
    /// How `source_path` is written to the CSV (0081); internal keys stay full.
    pub path_mode: LedgerPathMode,
    pub max_rows: u64,
    /// Fail-severity histogram (never truncated; not updated when mode=Off).
    pub failed_by_reason: BTreeMap<String, u64>,
    pub omitted_by_policy: u64,
    /// Per (source_path, msg_nid) fail counts for export_messages column (all modes).
    /// Keys use the **full** event source_path (never basenamed).
    pub msg_fail_counts: HashMap<(String, u64), u64>,
    /// Per (source_path, msg_nid) failed attachment filenames (case-preserving; match case-insensitive).
    pub msg_fail_filenames: HashMap<(String, u64), BTreeSet<String>>,
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
    /// Export winners that were selected via promote (hard or Mode A soft; 0083).
    /// Write-time fail rows for these loci get `winner_promoted=true`.
    pub promoted_winner_loci: BTreeSet<(String, u64)>,
}

impl AttachLedgerSink {
    /// Create sink; opens CSV when mode=full.
    pub fn new(
        mode: AttachLedgerMode,
        max_rows: u64,
        report_dir: &Path,
        input_paths: &[String],
        path_mode: LedgerPathMode,
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
            path_mode,
            max_rows: max_rows.max(1),
            failed_by_reason: BTreeMap::new(),
            omitted_by_policy: 0,
            msg_fail_counts: HashMap::new(),
            msg_fail_filenames: HashMap::new(),
            rows_written: 0,
            truncated: false,
            rows_dropped: 0,
            csv,
            source_ids,
            volume_path: String::new(),
            volume_index: 0,
            promoted_winner_loci: BTreeSet::new(),
        })
    }

    /// Mark an export winner locus as promoted (Mode A / hard materialize promote).
    pub fn mark_promoted_winner(&mut self, source_path: &str, msg_nid: u64) {
        self.promoted_winner_loci
            .insert((source_path.to_string(), msg_nid));
    }

    /// Enqueue a soft-skipped incomplete attach row (0083 Mode A honesty).
    ///
    /// Does not increment `attachments_failed` writer totals (those are write-path);
    /// still records fail severity in the ledger histogram so operators see the skip.
    pub fn enqueue_soft_skip_row(&mut self, row: AttachLedgerRow) {
        if self.mode == AttachLedgerMode::Off {
            return;
        }
        if row.severity == "fail" {
            *self
                .failed_by_reason
                .entry(row.reason_code.clone())
                .or_insert(0) += 1;
        }
        if self.mode != AttachLedgerMode::Full {
            return;
        }
        if self.truncated {
            self.rows_dropped = self.rows_dropped.saturating_add(1);
            return;
        }
        if self.rows_written >= self.max_rows.saturating_sub(1) && self.max_rows > 0 {
            let marker = AttachLedgerRow::truncated_marker(None);
            if let Some(csv) = self.csv.as_ref() {
                let _ = csv.enqueue(marker);
            }
            self.rows_written = self.rows_written.saturating_add(1);
            self.truncated = true;
            self.rows_dropped = self.rows_dropped.saturating_add(1);
            return;
        }
        if let Some(csv) = self.csv.as_ref() {
            if csv.enqueue(row).is_ok() {
                self.rows_written = self.rows_written.saturating_add(1);
            }
        }
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
        // Per-message fail counts / filenames in all modes (Off still fills export_messages).
        if event.severity == AttachEventSeverity::Fail {
            let key = (event.source_path.clone(), event.msg_nid);
            *self.msg_fail_counts.entry(key.clone()).or_insert(0) += 1;
            // Track empty filenames too (embedded MSG often has blank long-filename).
            self.msg_fail_filenames
                .entry(key)
                .or_default()
                .insert(event.attach_filename.clone());
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

        // Resolve source_id from full path; basename only the CSV path column.
        let source_id = self.resolve_source_id(&event.source_path);
        let winner_promoted = event.severity == AttachEventSeverity::Fail
            && self
                .promoted_winner_loci
                .contains(&(event.source_path.clone(), event.msg_nid));
        let row = ledger_row_from_event_ex(
            event,
            source_id,
            &self.volume_path,
            self.volume_index,
            self.path_mode,
            &LedgerPromoteContext {
                winner_promoted,
                peer_source_id: String::new(),
                peer_msg_nid: String::new(),
            },
        );
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

    /// Failed attachment filenames for a message locus (0080 attach-specific explain).
    pub fn fail_filenames_for(&self, source_path: &str, msg_nid: u64) -> Vec<String> {
        self.msg_fail_filenames
            .get(&(source_path.to_string(), msg_nid))
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
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
            msg_fail_filenames: self.msg_fail_filenames,
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
    pub msg_fail_filenames: HashMap<(String, u64), BTreeSet<String>>,
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

    pub fn fail_filenames_for(&self, source_path: &str, msg_nid: u64) -> Vec<String> {
        self.msg_fail_filenames
            .get(&(source_path.to_string(), msg_nid))
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
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

/// One body-inline cloud link ledger row (0085).
#[derive(Debug, Clone)]
pub struct BodyCloudLinkRow {
    pub source_id: String,
    pub source_path: String,
    pub folder_path: String,
    pub msg_nid: u64,
    pub link_index: u32,
    pub cloud_url: String,
    pub url_source: String,
    pub truncated: bool,
    pub message_subject: String,
    pub reason: String,
}

impl BodyCloudLinkRow {
    /// Format one CSV data line.
    pub fn to_csv_line(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{}",
            csv_escape_cell(&self.source_id),
            csv_escape_cell(&self.source_path),
            csv_escape_cell(&self.folder_path),
            self.msg_nid,
            self.link_index,
            csv_escape_cell(&self.cloud_url),
            csv_escape_cell(&self.url_source),
            if self.truncated { "true" } else { "false" },
            csv_escape_cell(&self.message_subject),
            csv_escape_cell(&self.reason),
        )
    }

    /// ≤1 honesty marker per message when document-shaped candidates were dropped.
    ///
    /// `link_index` is `u32::MAX` so it cannot collide with a real `link_index: 0`.
    /// `cloud_url` is empty except URL-over-length (2048-char prefix). `url_source` empty.
    pub fn honesty_marker(
        source_id: String,
        source_path: String,
        folder_path: String,
        msg_nid: u64,
        message_subject: String,
        reason: String,
        cloud_url: String,
    ) -> Self {
        Self {
            source_id,
            source_path,
            folder_path,
            msg_nid,
            link_index: u32::MAX,
            cloud_url,
            url_source: String::new(),
            truncated: true,
            message_subject,
            reason,
        }
    }
}

/// Write `export_body_cloud_links.csv` (always when report pack is written; 0085).
///
/// `path_mode` formats the `source_path` column only.
pub fn write_body_cloud_links_csv(
    path: &Path,
    rows: &[BodyCloudLinkRow],
    path_mode: LedgerPathMode,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CliError::Msg(format!(
                "create export_body_cloud_links parent {}: {e}",
                parent.display()
            ))
        })?;
    }
    let f = File::create(path).map_err(|e| CliError::CsvWrite {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    let mut w = BufWriter::new(f);
    writeln!(w, "{EXPORT_BODY_CLOUD_LINKS_CSV_HEADER}").map_err(|e| CliError::CsvWrite {
        path: path.to_path_buf(),
        source: Box::new(e),
    })?;
    for r in rows {
        let source_path = format_ledger_source_path(&r.source_path, path_mode);
        let line = format!(
            "{},{},{},{},{},{},{},{},{},{}",
            csv_escape_cell(&r.source_id),
            csv_escape_cell(&source_path),
            csv_escape_cell(&r.folder_path),
            r.msg_nid,
            r.link_index,
            csv_escape_cell(&r.cloud_url),
            csv_escape_cell(&r.url_source),
            if r.truncated { "true" } else { "false" },
            csv_escape_cell(&r.message_subject),
            csv_escape_cell(&r.reason),
        );
        writeln!(w, "{line}").map_err(|e| CliError::CsvWrite {
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

/// Write mandatory `export_messages.csv`.
///
/// `path_mode` formats the `source_path` column only at serialization time.
/// Callers that keep full paths in-memory (QC / fail-count join) pass
/// [`LedgerPathMode::Full`] for verification, or the operator mode for handoff.
pub fn write_export_messages_csv(
    path: &Path,
    rows: &[ExportMessageRow],
    path_mode: LedgerPathMode,
) -> Result<()> {
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
        let source_path = format_ledger_source_path(&r.source_path, path_mode);
        writeln!(
            w,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            csv_escape_cell(&source_path),
            csv_escape_cell(&r.folder_path),
            r.nid,
            csv_escape_cell(&r.message_id_norm),
            csv_escape_cell(&r.edrm_mih),
            csv_escape_cell(&r.content_hash_hex),
            csv_escape_cell(&r.volume_path),
            r.volume_index,
            r.export_message_index,
            r.attachments_failed_count,
            r.duplicate_source_count,
            csv_escape_cell(&r.duplicate_sources),
            csv_escape_cell(&r.source_id),
            if r.bcc_suppressed { "true" } else { "false" },
            r.body_cloud_link_count,
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
            EXPORT_MESSAGES_CSV_HEADER_V1,
            "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count"
        );
        // Append-only: full header starts with pre-0075 prefix.
        assert!(
            EXPORT_MESSAGES_CSV_HEADER.starts_with(EXPORT_MESSAGES_CSV_HEADER_V1),
            "export_messages header must keep pre-0075 columns as prefix"
        );
        assert!(EXPORT_MESSAGES_CSV_HEADER.contains("duplicate_source_count"));
        assert!(EXPORT_MESSAGES_CSV_HEADER.contains("duplicate_sources"));
        // 0081: source_id append; 0082: bcc_suppressed trailing append.
        assert!(
            EXPORT_MESSAGES_CSV_HEADER.contains(",source_id,"),
            "source_id must remain after locked prefix; got {EXPORT_MESSAGES_CSV_HEADER}"
        );
        assert!(
            EXPORT_MESSAGES_CSV_HEADER.ends_with(",body_cloud_link_count"),
            "body_cloud_link_count must be trailing append; got {EXPORT_MESSAGES_CSV_HEADER}"
        );
        assert!(
            EXPORT_MESSAGES_CSV_HEADER.contains(",bcc_suppressed,body_cloud_link_count"),
            "0085 append must follow bcc_suppressed: {EXPORT_MESSAGES_CSV_HEADER}"
        );
    }

    #[test]
    fn export_messages_all_custodians_fields_match_keep_entry_fill() {
        // Same fill pattern as unique_pst_cmd: KeepEntry aggregate → ExportMessageRow.
        let dup_count = 3u64;
        let joined = ["cust0.pst", "cust1.pst", "cust2.pst"].join("|");
        let row = ExportMessageRow {
            source_path: r"C:\mail\winner.pst".into(),
            folder_path: "Inbox".into(),
            nid: 0x2001,
            message_id_norm: "<m@x>".into(),
            edrm_mih: String::new(),
            content_hash_hex: "ab".repeat(32),
            volume_path: r"C:\out\unique.pst".into(),
            volume_index: 1,
            export_message_index: 0,
            attachments_failed_count: 0,
            duplicate_source_count: dup_count,
            duplicate_sources: joined.clone(),
            source_id: "0".into(),
            bcc_suppressed: false,
            body_cloud_link_count: 0,
            subject: String::new(),
        };
        assert_eq!(row.duplicate_source_count, dup_count);
        assert_eq!(row.duplicate_sources, joined);

        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("export_messages.csv");
        write_export_messages_csv(&path, &[row], LedgerPathMode::Full).expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        let mut lines = text.lines();
        let header = lines.next().expect("header");
        assert_eq!(header, EXPORT_MESSAGES_CSV_HEADER);
        let data = lines.next().expect("data");
        assert!(
            data.ends_with(&format!(",{dup_count},{joined},0"))
                || data.contains(&format!(",{dup_count},\"{joined}\",0"))
                || data.contains(&format!(",{dup_count},{joined},0")),
            "export row must carry All-Custodians columns + source_id; got {data}"
        );
        assert!(data.contains("cust0.pst") && data.contains("cust2.pst"));
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
    fn export_attachments_header_appends_cloud_columns() {
        assert!(
            EXPORT_ATTACHMENTS_CSV_HEADER.ends_with(",cloud_provider,cloud_url"),
            "0084 append-only columns must be rightmost: {EXPORT_ATTACHMENTS_CSV_HEADER}"
        );
        // Existing columns stay left of the append (schema discipline).
        assert!(EXPORT_ATTACHMENTS_CSV_HEADER.contains("message_subject,cloud_provider"));
    }

    #[test]
    fn cloud_link_ledger_row_populates_provider_url() {
        let mut ev = synth_event(AttachmentFidelityKind::CloudLink, AttachEventSeverity::Fail);
        ev.cloud_provider = "OneDrivePro".into();
        ev.cloud_url = "https://contoso.sharepoint.com/x".into();
        let row = ledger_row_from_event(&ev, Some(0), r"C:\out\u.pst", 1, LedgerPathMode::Full);
        assert_eq!(row.reason_code, "ATTACH_CLOUD_LINK");
        assert_eq!(row.cloud_provider, "OneDrivePro");
        assert_eq!(row.cloud_url, "https://contoso.sharepoint.com/x");
        let line = row.to_csv_line();
        assert!(line.contains("OneDrivePro"));
        assert!(line.contains("sharepoint"));
        // Injection neutralization on cloud_url
        ev.cloud_url = "=HYPERLINK(\"http://evil\")".into();
        let row2 = ledger_row_from_event(&ev, Some(0), "", 0, LedgerPathMode::Full);
        let line2 = row2.to_csv_line();
        assert!(
            line2.contains("'=HYPERLINK") || line2.contains("''=HYPERLINK"),
            "cloud_url must neutralize formula injection: {line2}"
        );
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

    #[test]
    fn ledger_path_mode_parse() {
        assert_eq!(LedgerPathMode::parse("full"), Some(LedgerPathMode::Full));
        assert_eq!(
            LedgerPathMode::parse("basename"),
            Some(LedgerPathMode::Basename)
        );
        assert_eq!(
            LedgerPathMode::parse("BASE"),
            Some(LedgerPathMode::Basename)
        );
        assert_eq!(LedgerPathMode::parse("nope"), None);
        assert_eq!(LedgerPathMode::Full.as_str(), "full");
        assert_eq!(LedgerPathMode::Basename.as_str(), "basename");
    }

    #[test]
    fn format_ledger_source_path_full_vs_basename() {
        let multi_a = r"C:\evidence\matter1\custodian_a.pst";
        let multi_b = r"D:\other\folder\custodian_b.pst";
        assert_eq!(
            format_ledger_source_path(multi_a, LedgerPathMode::Full),
            multi_a
        );
        assert_eq!(
            format_ledger_source_path(multi_b, LedgerPathMode::Full),
            multi_b
        );
        assert_eq!(
            format_ledger_source_path(multi_a, LedgerPathMode::Basename),
            "custodian_a.pst"
        );
        assert_eq!(
            format_ledger_source_path(multi_b, LedgerPathMode::Basename),
            "custodian_b.pst"
        );
        // Empty stays empty.
        assert_eq!(format_ledger_source_path("", LedgerPathMode::Full), "");
        assert_eq!(format_ledger_source_path("", LedgerPathMode::Basename), "");
        // Basename non-empty when full had a non-empty path.
        let full = r"C:\mail\inbox.pst";
        let base = format_ledger_source_path(full, LedgerPathMode::Basename);
        assert!(
            !base.is_empty(),
            "basename must be non-empty when full had path"
        );
        assert_eq!(base, "inbox.pst");
    }

    #[test]
    fn export_messages_csv_basename_source_path_column() {
        let row = ExportMessageRow {
            source_path: r"C:\evidence\custA\mailbox.pst".into(),
            folder_path: "Inbox".into(),
            nid: 0x2001,
            message_id_norm: "<m@x>".into(),
            edrm_mih: String::new(),
            content_hash_hex: "ab".repeat(32),
            volume_path: r"C:\out\unique.pst".into(),
            volume_index: 1,
            export_message_index: 1,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: "0".into(),
            bcc_suppressed: false,
            body_cloud_link_count: 0,
            subject: String::new(),
        };
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("export_messages.csv");
        write_export_messages_csv(&path, std::slice::from_ref(&row), LedgerPathMode::Basename)
            .expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        let data = text.lines().nth(1).expect("data");
        assert!(
            data.starts_with("mailbox.pst,"),
            "basename mode must write basename only; row={data}"
        );
        assert!(
            !data.contains(r"C:\evidence"),
            "absolute path must not appear in CSV under basename mode; row={data}"
        );
        assert!(
            data.contains(",0,false,0") || data.ends_with(",0,false,0"),
            "source_id + bcc + body_cloud_link_count columns must be present; row={data}"
        );
        // Full mode retains absolute path.
        write_export_messages_csv(&path, std::slice::from_ref(&row), LedgerPathMode::Full)
            .expect("write full");
        let full_text = std::fs::read_to_string(&path).expect("read full");
        let full_data = full_text.lines().nth(1).expect("data");
        assert!(
            full_data.contains(r"C:\evidence\custA\mailbox.pst"),
            "full mode keeps absolute path; row={full_data}"
        );
    }

    /// Basename mode with two distinct full paths that share a basename must keep
    /// distinct `source_id` while writing the same basenamed `source_path`.
    #[test]
    fn export_messages_basename_same_basename_distinct_source_id() {
        let row_a = ExportMessageRow {
            source_path: r"C:\evidence\custA\mailbox.pst".into(),
            folder_path: "Inbox".into(),
            nid: 0x2001,
            message_id_norm: "<a@x>".into(),
            edrm_mih: String::new(),
            content_hash_hex: "aa".repeat(32),
            volume_path: r"C:\out\unique.pst".into(),
            volume_index: 1,
            export_message_index: 1,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: "0".into(),
            bcc_suppressed: false,
            body_cloud_link_count: 0,
            subject: String::new(),
        };
        let row_b = ExportMessageRow {
            source_path: r"D:\other\custB\mailbox.pst".into(),
            folder_path: "Inbox".into(),
            nid: 0x2002,
            message_id_norm: "<b@x>".into(),
            edrm_mih: String::new(),
            content_hash_hex: "bb".repeat(32),
            volume_path: r"C:\out\unique.pst".into(),
            volume_index: 1,
            export_message_index: 2,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: "1".into(),
            bcc_suppressed: false,
            body_cloud_link_count: 0,
            subject: String::new(),
        };
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("export_messages.csv");
        write_export_messages_csv(&path, &[row_a, row_b], LedgerPathMode::Basename).expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        let mut lines = text.lines();
        let header = lines.next().expect("header");
        assert_eq!(header, EXPORT_MESSAGES_CSV_HEADER);
        assert!(header.ends_with(",body_cloud_link_count"));
        let data_a = lines.next().expect("row a");
        let data_b = lines.next().expect("row b");
        assert!(
            data_a.starts_with("mailbox.pst,"),
            "row a basenamed; got {data_a}"
        );
        assert!(
            data_b.starts_with("mailbox.pst,"),
            "row b basenamed; got {data_b}"
        );
        assert!(
            !data_a.contains(r"C:\evidence") && !data_b.contains(r"D:\other"),
            "absolute paths must not appear under basename; a={data_a} b={data_b}"
        );
        // source_id disambiguates same basename (before bcc_suppressed + body_cloud_link_count).
        assert!(
            data_a.contains(",0,false,0") || data_a.ends_with(",0,false,0"),
            "row a source_id=0; got {data_a}"
        );
        assert!(
            data_b.contains(",1,false,0") || data_b.ends_with(",1,false,0"),
            "row b source_id=1; got {data_b}"
        );
        // Resolve helper matches AttachLedgerSink honesty.
        let inputs = vec![
            r"C:\evidence\custA\mailbox.pst".to_string(),
            r"D:\other\custB\mailbox.pst".to_string(),
        ];
        assert_eq!(
            resolve_input_source_id(r"C:\evidence\custA\mailbox.pst", &inputs),
            Some(0)
        );
        assert_eq!(
            resolve_input_source_id(r"D:\other\custB\mailbox.pst", &inputs),
            Some(1)
        );
        assert_eq!(
            resolve_input_source_id(r"Z:\missing\mailbox.pst", &inputs),
            None,
            "unmapped must not invent 0"
        );
    }

    #[test]
    fn attach_ledger_csv_basename_source_path_column() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![
            r"C:\evidence\matter\a.pst".to_string(),
            r"D:\other\b.pst".to_string(),
        ];
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            100,
            dir.path(),
            &inputs,
            LedgerPathMode::Basename,
        )
        .expect("sink");
        sink.set_volume(r"C:\out\u.pst", 1);
        let mut ev = synth_event(
            AttachmentFidelityKind::StreamOpenFailed,
            AttachEventSeverity::Fail,
        );
        ev.source_path = r"D:\other\b.pst".into();
        sink.ingest(&ev);
        // Internal keys still full-path for fail_count join.
        assert_eq!(sink.fail_count_for(r"D:\other\b.pst", 42), 1);
        let _ = sink.finish().expect("finish");
        let csv = fs::read_to_string(dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
        let data = csv.lines().nth(1).expect("row");
        // source_id for second input is 1; source_path basenamed.
        assert!(
            data.starts_with("1,b.pst,"),
            "basename ledger: source_id=1, path=b.pst; row={data}"
        );
        assert!(
            !data.contains(r"D:\other"),
            "absolute path must not appear under basename mode; row={data}"
        );
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
            cloud_provider: String::new(),
            cloud_url: String::new(),
        }
    }

    #[test]
    fn row_cap_truncation_marker_and_histogram_continues() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string()];
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            2,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
        .expect("sink");
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
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::SummaryOnly,
            500_000,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
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
            attachment_fidelity_events_truncated: None,
            attachment_fidelity_events_total: None,
            recipient_tc_truncated_messages: None,
            recipient_rows_truncated: None,
            recipient_tc_truncated_events_truncated: None,
            recipient_tc_truncated_events_total: None,
            recipient_tc_truncations: None,
            include_bcc_recipients: false,
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
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            100,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
        .expect("sink");
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
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            100,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
        .expect("sink");
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
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Off,
            500_000,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
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
            attachment_fidelity_events_truncated: None,
            attachment_fidelity_events_total: None,
            recipient_tc_truncated_messages: None,
            recipient_rows_truncated: None,
            recipient_tc_truncated_events_truncated: None,
            recipient_tc_truncated_events_total: None,
            recipient_tc_truncations: None,
            include_bcc_recipients: false,
        };
        finish.apply_to_export_section(&mut export);
        assert!(export.attachments_failed_by_reason.is_none());
        assert_eq!(export.attachments_omitted_by_policy, Some(9));
    }

    /// 0083: soft-skip incomplete rows + write-time fails on promoted winners set
    /// `winner_promoted=true`.
    #[test]
    fn mode_a_winner_promoted_ledger_honesty() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string(), r"C:\in\b.pst".to_string()];
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            500_000,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
        .expect("sink");
        // Soft-skipped incomplete peer A; final winner B.
        sink.enqueue_soft_skip_row(AttachLedgerRow {
            source_id: "0".into(),
            source_path: r"C:\in\a.pst".into(),
            folder_path: "Inbox".into(),
            msg_nid: 10,
            attach_nid: "99".into(),
            attach_index: 0,
            filename: "missing.bin".into(),
            size: "100".into(),
            attach_method: 1,
            reason_code: "ATTACH_STREAM_OPEN_FAILED".into(),
            severity: "fail".into(),
            volume_path: String::new(),
            volume_index: String::new(),
            winner_promoted: true,
            peer_source_id: "1".into(),
            peer_msg_nid: "20".into(),
            message_subject: String::new(),
            cloud_provider: String::new(),
            cloud_url: String::new(),
        });
        // Mode C fallback incomplete winner that was promoted: write-time fail.
        sink.mark_promoted_winner(r"C:\in\b.pst", 20);
        sink.set_volume(r"C:\out\u.pst", 1);
        let mut ev = synth_event(
            AttachmentFidelityKind::StreamOpenFailed,
            AttachEventSeverity::Fail,
        );
        ev.source_path = r"C:\in\b.pst".into();
        ev.msg_nid = 20;
        sink.ingest(&ev);
        let _ = sink.finish().expect("finish");
        let csv = fs::read_to_string(dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
        let rows: Vec<&str> = csv.lines().skip(1).collect();
        assert!(rows.len() >= 2, "soft-skip + write fail rows; csv={csv}");
        assert!(
            rows.iter()
                .any(|r| r.contains("true") && r.contains("missing.bin")),
            "soft-skip row must have winner_promoted=true: {csv}"
        );
        assert!(
            rows.iter()
                .any(|r| r.contains(r"C:\in\b.pst") && r.contains(",true,")),
            "write-time fail on promoted winner: {csv}"
        );
    }

    #[test]
    fn export_risk_monotone_composition() {
        let inputs = ExportRiskInputs {
            scan_recommendation: PreflightRecommendation::ReExportRecommended,
            ..Default::default()
        };
        let risk = compute_export_risk(&PreflightRecommendation::ReExportRecommended, &inputs);
        assert_eq!(risk.level, PreflightRecommendation::ReExportRecommended);
        assert!(risk.level.rank() >= PreflightRecommendation::ReExportRecommended.rank());
        // F7: scan-only elevation must name the scan preflight in reasons.
        assert!(
            risk.reasons
                .iter()
                .any(|r| r == "scan_preflight=re_export_recommended"),
            "reasons={:?}",
            risk.reasons
        );
    }

    #[test]
    fn export_risk_advisory_cannot_reach_not_export_ready() {
        let inputs = ExportRiskInputs {
            attach_fail_rate: 0.06, // above 0.05 advisory, well below 0.50 catastrophic
            ..Default::default()
        };
        let risk = compute_export_risk(&PreflightRecommendation::Ok, &inputs);
        assert_eq!(risk.level, PreflightRecommendation::ReExportRecommended);
        assert!(
            risk.reasons
                .iter()
                .any(|r| r.starts_with("attach_fail_rate=")),
            "reasons={:?}",
            risk.reasons
        );
    }

    #[test]
    fn export_risk_catastrophic_read_rate_without_failed_volume() {
        let inputs = ExportRiskInputs {
            block_crc_rate: 1.0,
            block_crc_read_rate: 0.20,
            ..Default::default()
        };
        let risk = compute_export_risk(&PreflightRecommendation::Ok, &inputs);
        assert_eq!(risk.level, PreflightRecommendation::NotExportReady);
        assert!(
            risk.reasons
                .iter()
                .any(|r| r.contains("block_crc_read_rate=") && r.contains("0.15")),
            "reasons={:?}",
            risk.reasons
        );
    }

    /// 0077 P1-2: final attach stream CRC Info events elevate export_risk only.
    #[test]
    fn export_risk_attach_stream_crc_events_recommend_reexport() {
        let inputs = ExportRiskInputs {
            attach_stream_crc_events: 1,
            ..Default::default()
        };
        let risk = compute_export_risk(&PreflightRecommendation::Ok, &inputs);
        assert_eq!(risk.level, PreflightRecommendation::ReExportRecommended);
        assert!(
            risk.reasons
                .iter()
                .any(|r| r == "attach_stream_crc_events=1>0"),
            "reasons={:?}",
            risk.reasons
        );
        // Warning-only: must not look like attach_fail_rate elevation.
        assert!(
            !risk
                .reasons
                .iter()
                .any(|r| r.starts_with("attach_fail_rate=")),
            "reasons={:?}",
            risk.reasons
        );
    }

    fn poly_class(
        page_crc: u64,
        page_reads: u64,
        block_crc: u64,
        block_reads: u64,
    ) -> CrcSourceClass {
        CrcSourceClass {
            poly_class_crc: true,
            page_crc_mismatches: page_crc,
            block_crc_mismatches: block_crc,
            page_reads,
            block_reads,
        }
    }

    fn localized(
        page_crc: u64,
        page_reads: u64,
        block_crc: u64,
        block_reads: u64,
    ) -> CrcSourceClass {
        CrcSourceClass {
            poly_class_crc: false,
            page_crc_mismatches: page_crc,
            block_crc_mismatches: block_crc,
            page_reads,
            block_reads,
        }
    }

    fn file_stats(
        poly: bool,
        page_crc: u64,
        page_reads: u64,
        block_crc: u64,
        block_reads: u64,
    ) -> crate::scan::FileScanStats {
        crate::scan::FileScanStats {
            path: "x.pst".into(),
            name: "x.pst".into(),
            status: dedup_engine::integrity::FileScanStatus::Opened,
            folders: 0,
            messages: 0,
            recoverable_messages: 0,
            duplicates: 0,
            skipped: 0,
            skipped_by_reason: BTreeMap::new(),
            degraded_messages: 0,
            degraded_by_reason: BTreeMap::new(),
            error_code: None,
            error: None,
            page_crc_mismatches: page_crc,
            block_crc_mismatches: block_crc,
            block_bid_mismatches: 0,
            distinct_bad_bids: 0,
            crc_suspect_messages: 0,
            page_reads,
            block_reads,
            poly_class_crc: poly,
        }
    }

    fn inputs_from_sources(
        raw_block: f64,
        attach_crc: u64,
        sources: &[CrcSourceClass],
    ) -> ExportRiskInputs {
        let adj = poly_crc_risk_adjustment(sources);
        ExportRiskInputs {
            block_crc_read_rate: raw_block,
            attach_stream_crc_events: attach_crc,
            effective_block_crc_read_rate: adj.effective_block_crc_read_rate,
            poly_class_crc_discounted: adj.poly_class_crc_discounted,
            discount_attach_stream_crc: adj.discount_attach_stream_crc,
            poly_class_crc_sources: adj.poly_class_crc_sources,
            ..Default::default()
        }
    }

    #[test]
    fn poly_crc_all_poly_zero_effective() {
        let sources = [poly_class(100, 100, 100, 100), poly_class(80, 80, 80, 80)];
        let adj = poly_crc_risk_adjustment(&sources);
        assert_eq!(adj.effective_block_crc_read_rate, Some(0.0));
        assert!(adj.poly_class_crc_discounted);
        assert!(adj.discount_attach_stream_crc);
        assert_eq!(adj.poly_class_crc_sources, 2);
        assert_eq!(adj.non_poly_crc_noisy_sources, 0);
    }

    #[test]
    fn poly_crc_localized_only_passthrough() {
        // High block, low page — not dual-rate. Combined rate = 85/200 = 0.425.
        let sources = [localized(5, 100, 80, 100)];
        let adj = poly_crc_risk_adjustment(&sources);
        assert_eq!(adj.effective_block_crc_read_rate, Some(85.0 / 200.0));
        assert!(!adj.poly_class_crc_discounted);
        assert!(!adj.discount_attach_stream_crc);
        assert_eq!(adj.non_poly_crc_noisy_sources, 1);
    }

    #[test]
    fn poly_crc_mixed_poly_plus_localized() {
        let sources = [
            poly_class(100, 100, 100, 100),
            localized(0, 0, 20, 100), // 20/100 = 0.20
        ];
        let adj = poly_crc_risk_adjustment(&sources);
        assert_eq!(adj.effective_block_crc_read_rate, Some(0.20));
        assert!(adj.poly_class_crc_discounted);
        assert!(!adj.discount_attach_stream_crc);
        assert_eq!(adj.non_poly_crc_noisy_sources, 1);
    }

    #[test]
    fn poly_crc_mixed_poly_plus_clean() {
        let sources = [poly_class(100, 100, 100, 100), localized(0, 100, 0, 100)];
        let adj = poly_crc_risk_adjustment(&sources);
        assert_eq!(adj.effective_block_crc_read_rate, Some(0.0));
        assert!(adj.poly_class_crc_discounted);
        assert!(adj.discount_attach_stream_crc);
        assert_eq!(adj.non_poly_crc_noisy_sources, 0);
    }

    #[test]
    fn poly_crc_empty_fail_closed() {
        let adj = poly_crc_risk_adjustment(&[]);
        assert_eq!(adj.effective_block_crc_read_rate, None);
        assert!(!adj.poly_class_crc_discounted);
        assert!(!adj.discount_attach_stream_crc);
        assert_eq!(adj.poly_class_crc_sources, 0);
        assert_eq!(adj.non_poly_crc_noisy_sources, 0);
    }

    #[test]
    fn export_risk_all_poly_inc_like_ok() {
        let sources = [
            poly_class(100, 100, 100, 100),
            poly_class(100, 100, 100, 100),
        ];
        let inputs = inputs_from_sources(1.0, 6014, &sources);
        let risk = compute_export_risk(&PreflightRecommendation::Ok, &inputs);
        assert_eq!(risk.level, PreflightRecommendation::Ok);
        assert!(
            risk.reasons
                .iter()
                .any(|r| r == "poly_class_crc_discounted"),
            "reasons={:?}",
            risk.reasons
        );
        assert!(
            !risk
                .reasons
                .iter()
                .any(|r| r.starts_with("block_crc_read_rate=")),
            "must not emit raw block_crc_read_rate lie; reasons={:?}",
            risk.reasons
        );
        assert!(
            !risk
                .reasons
                .iter()
                .any(|r| r.starts_with("attach_stream_crc_events=")),
            "reasons={:?}",
            risk.reasons
        );
        assert_eq!(risk.inputs.block_crc_read_rate, 1.0);
        assert_eq!(risk.inputs.attach_stream_crc_events, 6014);
        assert_eq!(risk.inputs.effective_block_crc_read_rate, Some(0.0));
    }

    #[test]
    fn export_risk_poly_does_not_lower_scan_not_export_ready() {
        let sources = [poly_class(100, 100, 100, 100)];
        let inputs = inputs_from_sources(1.0, 0, &sources);
        let risk = compute_export_risk(&PreflightRecommendation::NotExportReady, &inputs);
        assert_eq!(risk.level, PreflightRecommendation::NotExportReady);
        assert!(
            risk.reasons
                .iter()
                .any(|r| r == "scan_recommendation=not_export_ready"),
            "reasons={:?}",
            risk.reasons
        );
        assert!(
            risk.reasons
                .iter()
                .any(|r| r == "poly_class_crc_discounted"),
            "reasons={:?}",
            risk.reasons
        );
    }

    #[test]
    fn export_risk_poly_plus_attach_fail_still_advisory() {
        let sources = [poly_class(100, 100, 100, 100)];
        let mut inputs = inputs_from_sources(1.0, 10, &sources);
        inputs.attach_fail_rate = 0.06;
        let risk = compute_export_risk(&PreflightRecommendation::Ok, &inputs);
        assert_eq!(risk.level, PreflightRecommendation::ReExportRecommended);
        assert!(
            risk.reasons
                .iter()
                .any(|r| r.starts_with("attach_fail_rate=")),
            "reasons={:?}",
            risk.reasons
        );
        assert!(
            !risk
                .reasons
                .iter()
                .any(|r| r.starts_with("attach_stream_crc_events=")),
            "reasons={:?}",
            risk.reasons
        );
    }

    #[test]
    fn export_risk_mixed_localized_still_catastrophic() {
        let sources = [poly_class(100, 100, 100, 100), localized(0, 0, 20, 100)];
        let inputs = inputs_from_sources(1.0, 5, &sources);
        let risk = compute_export_risk(&PreflightRecommendation::Ok, &inputs);
        assert_eq!(risk.level, PreflightRecommendation::NotExportReady);
        assert!(
            risk.reasons
                .iter()
                .any(|r| r.contains("effective_block_crc_read_rate=0.200>0.15")),
            "reasons={:?}",
            risk.reasons
        );
        assert!(
            risk.reasons
                .iter()
                .any(|r| r == "attach_stream_crc_events=5>0"),
            "mixed job must not discount attach CRC; reasons={:?}",
            risk.reasons
        );
        assert!(
            !risk
                .reasons
                .iter()
                .any(|r| r.starts_with("block_crc_read_rate=")),
            "reasons={:?}",
            risk.reasons
        );
    }

    #[test]
    fn crc_source_classes_from_files_maps_raw_counters() {
        let files = vec![
            file_stats(true, 100, 100, 100, 100),
            file_stats(false, 0, 0, 20, 100),
        ];
        let classes = crc_source_classes_from_files(&files);
        assert_eq!(classes.len(), 2);
        assert!(classes[0].poly_class_crc);
        assert_eq!(classes[0].page_crc_mismatches, 100);
        assert_eq!(classes[0].block_crc_mismatches, 100);
        assert_eq!(classes[0].page_reads, 100);
        assert_eq!(classes[0].block_reads, 100);
        assert!(!classes[1].poly_class_crc);
        assert_eq!(classes[1].block_crc_mismatches, 20);
        assert_eq!(classes[1].block_reads, 100);
        // Mapper must not pre-average — no rate fields on CrcSourceClass.
        let adj = poly_crc_risk_adjustment(&classes);
        assert_eq!(adj.effective_block_crc_read_rate, Some(0.20));
    }

    #[test]
    fn export_risk_matrix_table_driven() {
        struct Row {
            name: &'static str,
            sources: Vec<CrcSourceClass>,
            raw_block: f64,
            attach_crc: u64,
            attach_fail: f64,
            failed_volume: Option<u32>,
            scan: PreflightRecommendation,
            expect_level: PreflightRecommendation,
            require: &'static [&'static str],
            forbid: &'static [&'static str],
        }
        let poly = poly_class(100, 100, 100, 100);
        let loc = localized(0, 0, 20, 100);
        let clean = localized(0, 100, 0, 100);
        let rows = [
            Row {
                name: "all_poly",
                sources: vec![poly, poly],
                raw_block: 1.0,
                attach_crc: 6014,
                attach_fail: 0.0,
                failed_volume: None,
                scan: PreflightRecommendation::Ok,
                expect_level: PreflightRecommendation::Ok,
                require: &["poly_class_crc_discounted"],
                forbid: &["block_crc_read_rate=", "attach_stream_crc_events="],
            },
            Row {
                name: "all_clean",
                sources: vec![clean],
                raw_block: 0.0,
                attach_crc: 0,
                attach_fail: 0.0,
                failed_volume: None,
                scan: PreflightRecommendation::Ok,
                expect_level: PreflightRecommendation::Ok,
                require: &[],
                forbid: &["poly_class_crc_discounted"],
            },
            Row {
                name: "localized_only",
                sources: vec![loc],
                raw_block: 0.20,
                attach_crc: 0,
                attach_fail: 0.0,
                failed_volume: None,
                scan: PreflightRecommendation::Ok,
                expect_level: PreflightRecommendation::NotExportReady,
                require: &["effective_block_crc_read_rate=0.200>0.15"],
                forbid: &["poly_class_crc_discounted"],
            },
            Row {
                name: "poly_plus_clean",
                sources: vec![poly, clean],
                raw_block: 1.0,
                attach_crc: 10,
                attach_fail: 0.0,
                failed_volume: None,
                scan: PreflightRecommendation::Ok,
                expect_level: PreflightRecommendation::Ok,
                require: &["poly_class_crc_discounted"],
                forbid: &["attach_stream_crc_events="],
            },
            Row {
                name: "poly_plus_localized",
                sources: vec![poly, loc],
                raw_block: 1.0,
                attach_crc: 3,
                attach_fail: 0.0,
                failed_volume: None,
                scan: PreflightRecommendation::Ok,
                expect_level: PreflightRecommendation::NotExportReady,
                require: &[
                    "effective_block_crc_read_rate=0.200>0.15",
                    "attach_stream_crc_events=3>0",
                    "poly_class_crc_discounted",
                ],
                forbid: &["block_crc_read_rate="],
            },
            Row {
                name: "all_poly_attach_fail",
                sources: vec![poly],
                raw_block: 1.0,
                attach_crc: 0,
                attach_fail: 0.06,
                failed_volume: None,
                scan: PreflightRecommendation::Ok,
                expect_level: PreflightRecommendation::ReExportRecommended,
                require: &["attach_fail_rate=", "poly_class_crc_discounted"],
                forbid: &[],
            },
            Row {
                name: "all_poly_failed_volume",
                sources: vec![poly],
                raw_block: 1.0,
                attach_crc: 0,
                attach_fail: 0.0,
                failed_volume: Some(1),
                scan: PreflightRecommendation::Ok,
                expect_level: PreflightRecommendation::NotExportReady,
                require: &["failed_volume_index=", "poly_class_crc_discounted"],
                forbid: &["block_crc_read_rate="],
            },
            Row {
                name: "scan_ner_all_poly",
                sources: vec![poly],
                raw_block: 1.0,
                attach_crc: 0,
                attach_fail: 0.0,
                failed_volume: None,
                scan: PreflightRecommendation::NotExportReady,
                expect_level: PreflightRecommendation::NotExportReady,
                require: &[
                    "scan_recommendation=not_export_ready",
                    "poly_class_crc_discounted",
                ],
                forbid: &[],
            },
            Row {
                name: "scan_rer_all_poly",
                sources: vec![poly],
                raw_block: 1.0,
                attach_crc: 0,
                attach_fail: 0.0,
                failed_volume: None,
                scan: PreflightRecommendation::ReExportRecommended,
                expect_level: PreflightRecommendation::ReExportRecommended,
                require: &[
                    "scan_preflight=re_export_recommended",
                    "poly_class_crc_discounted",
                ],
                forbid: &[],
            },
        ];
        for row in rows {
            let adj = poly_crc_risk_adjustment(&row.sources);
            let inputs = ExportRiskInputs {
                attach_fail_rate: row.attach_fail,
                block_crc_read_rate: row.raw_block,
                failed_volume_index: row.failed_volume,
                scan_recommendation: row.scan,
                attach_stream_crc_events: row.attach_crc,
                effective_block_crc_read_rate: adj.effective_block_crc_read_rate,
                poly_class_crc_discounted: adj.poly_class_crc_discounted,
                discount_attach_stream_crc: adj.discount_attach_stream_crc,
                poly_class_crc_sources: adj.poly_class_crc_sources,
                ..Default::default()
            };
            let risk = compute_export_risk(&row.scan, &inputs);
            assert_eq!(
                risk.level, row.expect_level,
                "{}: level {:?} reasons={:?}",
                row.name, risk.level, risk.reasons
            );
            for needle in row.require {
                assert!(
                    risk.reasons.iter().any(|r| r.contains(needle)),
                    "{}: missing {needle:?} in {:?}",
                    row.name,
                    risk.reasons
                );
            }
            for needle in row.forbid {
                assert!(
                    !risk.reasons.iter().any(|r| r.starts_with(needle)),
                    "{}: unexpected {needle:?} in {:?}",
                    row.name,
                    risk.reasons
                );
            }
        }
    }

    #[test]
    fn no_competing_risk_enum_vocabulary() {
        // DoD-6: export_risk reuses PreflightRecommendation; no low|elevated|high.
        let level = PreflightRecommendation::Ok;
        assert_eq!(level.as_str(), "ok");
        assert_eq!(
            PreflightRecommendation::ReExportRecommended.as_str(),
            "re_export_recommended"
        );
        assert_eq!(
            PreflightRecommendation::NotExportReady.as_str(),
            "not_export_ready"
        );
    }

    #[test]
    fn volume_buffer_discard_does_not_pollute_global() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string()];
        let mut global = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            100,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
        .expect("sink");
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

    /// 0082 DoD-9: bcc_suppressed true/false column.
    #[test]
    fn bcc_suppressed_column_true_and_false() {
        let row_true = ExportMessageRow {
            source_path: r"C:\a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            message_id_norm: "a@x".into(),
            edrm_mih: String::new(),
            content_hash_hex: "aa".repeat(32),
            volume_path: r"C:\out.pst".into(),
            volume_index: 1,
            export_message_index: 1,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: "0".into(),
            bcc_suppressed: true,
            body_cloud_link_count: 0,
            subject: String::new(),
        };
        let row_false = ExportMessageRow {
            bcc_suppressed: false,
            export_message_index: 2,
            message_id_norm: "b@x".into(),
            ..row_true.clone()
        };
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("export_messages.csv");
        write_export_messages_csv(&path, &[row_true, row_false], LedgerPathMode::Full)
            .expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text
            .lines()
            .next()
            .unwrap()
            .ends_with(",body_cloud_link_count"));
        let lines: Vec<_> = text.lines().skip(1).collect();
        assert!(lines[0].ends_with(",true,0"), "row0={}", lines[0]);
        assert!(lines[1].ends_with(",false,0"), "row1={}", lines[1]);
    }

    #[test]
    fn export_body_cloud_links_header_locked() {
        assert!(
            EXPORT_BODY_CLOUD_LINKS_CSV_HEADER.starts_with("source_id,source_path,"),
            "{EXPORT_BODY_CLOUD_LINKS_CSV_HEADER}"
        );
        assert!(EXPORT_BODY_CLOUD_LINKS_CSV_HEADER.contains("cloud_url"));
        assert!(EXPORT_BODY_CLOUD_LINKS_CSV_HEADER.contains("url_source"));
        assert!(EXPORT_BODY_CLOUD_LINKS_CSV_HEADER.ends_with(",reason"));
    }

    #[test]
    fn body_cloud_link_csv_injection_neutralized_without_rewrite() {
        let dangerous = "https://contoso.sharepoint.com/:x:/s/L/=cmd.xlsx?d=1";
        // Formula-dangerous leading char is not typical for https URLs; test + prefix case.
        let formula_url = "+https://contoso.sharepoint.com/:x:/s/L/a.xlsx";
        let row = BodyCloudLinkRow {
            source_id: "0".into(),
            source_path: r"C:\a.pst".into(),
            folder_path: "Inbox".into(),
            msg_nid: 1,
            link_index: 0,
            cloud_url: formula_url.into(),
            url_source: "html_href".into(),
            truncated: false,
            message_subject: "subj".into(),
            reason: REASON_BODY_CLOUD_LINK.into(),
        };
        let line = row.to_csv_line();
        assert!(
            line.contains("'+https://") || line.contains("\"'+https://"),
            "formula-leading URL must be neutralized: {line}"
        );
        // Structure of the URL path/query must remain after the leading quote.
        assert!(line.contains("sharepoint.com/:x:/s/L/a.xlsx"));
        let _ = dangerous;
    }

    #[test]
    fn body_cloud_honesty_marker_discriminator_and_link_index() {
        let row = BodyCloudLinkRow::honesty_marker(
            "0".into(),
            r"C:\a.pst".into(),
            "Inbox".into(),
            1,
            "subj".into(),
            REASON_BODY_CLOUD_LINK_WINDOW.into(),
            String::new(),
        );
        assert_eq!(row.link_index, u32::MAX);
        assert!(row.truncated);
        assert!(row.cloud_url.is_empty());
        assert!(row.url_source.is_empty());
        assert_eq!(row.reason, REASON_BODY_CLOUD_LINK_WINDOW);
        let line = row.to_csv_line();
        assert!(line.contains(&u32::MAX.to_string()));
        assert!(line.contains(",true,"));
        assert!(line.contains(REASON_BODY_CLOUD_LINK_WINDOW));
        assert!(!line.contains("BODY_CLOUD_LINK_TRUNCATED"));
    }

    #[test]
    fn body_cloud_honesty_reason_pipe_join_order() {
        assert_eq!(
            body_cloud_honesty_reason(true, true, true),
            "BODY_CLOUD_LINK_WINDOW|BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED|BODY_CLOUD_LINK_URL_TRUNCATED"
        );
        assert_eq!(
            body_cloud_honesty_reason(false, true, false),
            REASON_BODY_CLOUD_LINK_MAX_LINKS_EXCEEDED
        );
        assert!(body_cloud_honesty_reason(false, false, false).is_empty());
    }
}
