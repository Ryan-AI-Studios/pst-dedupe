//! Multi-PST scan integrity types, reason mapping, preflight, and streaming skip ledger.
//!
//! Schema id: [`SCAN_INTEGRITY_SCHEMA`] (`scan_integrity_v1`).
//!
//! This module classifies and reports; it never mutates source PST bytes.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Stable JSON schema identifier for scan integrity payloads.
pub const SCAN_INTEGRITY_SCHEMA: &str = "scan_integrity_v1";

/// Scan recoverability mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScanMode {
    /// Default triage mode: keep degraded messages with reason codes.
    #[default]
    BestEffort,
    /// Legal / zero-skip tolerance: skip any degradation; fail closed.
    Strict,
}

impl ScanMode {
    /// CLI / JSON string: `best-effort` or `strict`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BestEffort => "best-effort",
            Self::Strict => "strict",
        }
    }

    /// Parse CLI / JSON mode string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "best-effort" | "best_effort" => Some(Self::BestEffort),
            "strict" => Some(Self::Strict),
            _ => None,
        }
    }
}

impl std::fmt::Display for ScanMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ScanMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ScanMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid scan mode '{s}'; expected best-effort or strict"
            ))
        })
    }
}

/// Per-file open/inventory status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileScanStatus {
    /// Opened and listed with zero skips and zero degradations.
    Opened,
    /// Opened with one or more message-level skips and/or degradations.
    Partial,
    /// Open or folder walk failed (zero inventory).
    Failed,
}

impl FileScanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for FileScanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Stable integrity reason codes (API for 0071 — additive only after v1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntegrityReason {
    OpenFailed,
    AnsiUnsupported,
    UnsupportedCrypt,
    FolderWalkFailed,
    CrcMismatch,
    BlockNotFound,
    NodeNotFound,
    DataTruncated,
    BodyTruncated,
    BodyUnavailable,
    OrphanedNode,
    InvalidStructure,
    PropertyError,
    MessageReadFailed,
    AttachMetaFailed,
    PathNotFound,
    NotPst,
    ReadError,
}

impl IntegrityReason {
    /// Stable string code (e.g. `CRC_MISMATCH`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenFailed => "OPEN_FAILED",
            Self::AnsiUnsupported => "ANSI_UNSUPPORTED",
            Self::UnsupportedCrypt => "UNSUPPORTED_CRYPT",
            Self::FolderWalkFailed => "FOLDER_WALK_FAILED",
            Self::CrcMismatch => "CRC_MISMATCH",
            Self::BlockNotFound => "BLOCK_NOT_FOUND",
            Self::NodeNotFound => "NODE_NOT_FOUND",
            Self::DataTruncated => "DATA_TRUNCATED",
            Self::BodyTruncated => "BODY_TRUNCATED",
            Self::BodyUnavailable => "BODY_UNAVAILABLE",
            Self::OrphanedNode => "ORPHANED_NODE",
            Self::InvalidStructure => "INVALID_STRUCTURE",
            Self::PropertyError => "PROPERTY_ERROR",
            Self::MessageReadFailed => "MESSAGE_READ_FAILED",
            Self::AttachMetaFailed => "ATTACH_META_FAILED",
            Self::PathNotFound => "PATH_NOT_FOUND",
            Self::NotPst => "NOT_PST",
            Self::ReadError => "READ_ERROR",
        }
    }
}

impl std::fmt::Display for IntegrityReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for IntegrityReason {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for IntegrityReason {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "OPEN_FAILED" => Ok(Self::OpenFailed),
            "ANSI_UNSUPPORTED" => Ok(Self::AnsiUnsupported),
            "UNSUPPORTED_CRYPT" => Ok(Self::UnsupportedCrypt),
            "FOLDER_WALK_FAILED" => Ok(Self::FolderWalkFailed),
            "CRC_MISMATCH" => Ok(Self::CrcMismatch),
            "BLOCK_NOT_FOUND" => Ok(Self::BlockNotFound),
            "NODE_NOT_FOUND" => Ok(Self::NodeNotFound),
            "DATA_TRUNCATED" => Ok(Self::DataTruncated),
            "BODY_TRUNCATED" => Ok(Self::BodyTruncated),
            "BODY_UNAVAILABLE" => Ok(Self::BodyUnavailable),
            "ORPHANED_NODE" => Ok(Self::OrphanedNode),
            "INVALID_STRUCTURE" => Ok(Self::InvalidStructure),
            "PROPERTY_ERROR" => Ok(Self::PropertyError),
            "MESSAGE_READ_FAILED" => Ok(Self::MessageReadFailed),
            "ATTACH_META_FAILED" => Ok(Self::AttachMetaFailed),
            "PATH_NOT_FOUND" => Ok(Self::PathNotFound),
            "NOT_PST" => Ok(Self::NotPst),
            "READ_ERROR" => Ok(Self::ReadError),
            other => Err(serde::de::Error::custom(format!(
                "unknown integrity reason '{other}'"
            ))),
        }
    }
}

/// Map a `PstError` to a stable integrity reason.
pub fn reason_from_pst_error(err: &pst_reader::PstError) -> IntegrityReason {
    use pst_reader::PstError;
    match err {
        PstError::AnsiPstNotSupported(_) => IntegrityReason::AnsiUnsupported,
        PstError::UnsupportedCryptMethod(_) => IntegrityReason::UnsupportedCrypt,
        PstError::CrcMismatch { .. } => IntegrityReason::CrcMismatch,
        PstError::BlockNotFound(_) => IntegrityReason::BlockNotFound,
        PstError::NodeNotFound(_) | PstError::SubnodeNotFound(_) | PstError::NoSubnodeBTree(_) => {
            IntegrityReason::NodeNotFound
        }
        PstError::DataTruncated { .. } => IntegrityReason::DataTruncated,
        PstError::InvalidMagic(_)
        | PstError::InvalidClientMagic(_)
        | PstError::InvalidPageType { .. }
        | PstError::PageTypeMismatch { .. }
        | PstError::InvalidBlockType { .. }
        | PstError::InvalidHnSignature(_)
        | PstError::InvalidBthType(_)
        | PstError::InvalidHid(_)
        | PstError::InvalidUtf16 => IntegrityReason::InvalidStructure,
        PstError::PropertyNotFound(_) | PstError::PropertyTypeMismatch { .. } => {
            IntegrityReason::PropertyError
        }
        // I/O during open or mid-read — open failures use OPEN_FAILED; message path may remap.
        PstError::Io(_) => IntegrityReason::OpenFailed,
    }
}

/// Whether a body property error indicates truncation/CRC corruption (BODY_TRUNCATED path).
pub fn is_body_truncation_or_crc(err: &pst_reader::PstError) -> bool {
    matches!(
        err,
        pst_reader::PstError::DataTruncated { .. } | pst_reader::PstError::CrcMismatch { .. }
    )
}

/// One non-recoverable message attempt (skip ledger row).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkipRecord {
    pub source_path: String,
    pub source_pst: String,
    pub folder_path: String,
    pub is_orphaned: bool,
    pub nid: u64,
    pub reason: IntegrityReason,
    pub detail: String,
    pub mode: ScanMode,
}

/// Integrity fields attached to a recoverable message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoverableIntegrity {
    pub is_orphaned: bool,
    pub degraded: bool,
    pub degraded_reasons: Vec<IntegrityReason>,
}

impl RecoverableIntegrity {
    pub fn clean() -> Self {
        Self::default()
    }

    pub fn with_degraded(reasons: Vec<IntegrityReason>, is_orphaned: bool) -> Self {
        let degraded = !reasons.is_empty() || is_orphaned;
        let mut degraded_reasons = reasons;
        if is_orphaned && !degraded_reasons.contains(&IntegrityReason::OrphanedNode) {
            degraded_reasons.push(IntegrityReason::OrphanedNode);
        }
        Self {
            is_orphaned,
            degraded: degraded || !degraded_reasons.is_empty(),
            degraded_reasons,
        }
    }
}

/// Configurable preflight rate thresholds.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct IntegrityThresholds {
    pub max_skip_rate: f64,
    pub max_crc_skip_rate: f64,
    pub max_failed_file_rate: f64,
}

impl Default for IntegrityThresholds {
    fn default() -> Self {
        Self {
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
        }
    }
}

/// Preflight recommendation for operators / 0066.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightRecommendation {
    Ok,
    ReExportRecommended,
    NotExportReady,
}

impl PreflightRecommendation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::ReExportRecommended => "re_export_recommended",
            Self::NotExportReady => "not_export_ready",
        }
    }
}

impl std::fmt::Display for PreflightRecommendation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Preflight object embedded in scan JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreflightReport {
    pub schema: String,
    pub mode: ScanMode,
    pub skip_rate: f64,
    pub crc_skip_rate: f64,
    pub failed_file_rate: f64,
    pub thresholds: IntegrityThresholds,
    pub recommendation: PreflightRecommendation,
    pub reasons: Vec<String>,
}

/// Inputs for pure preflight computation.
#[derive(Clone, Debug)]
pub struct PreflightInputs {
    pub mode: ScanMode,
    pub recoverable: u64,
    pub skipped: u64,
    pub crc_skips: u64,
    pub failed_files: u64,
    pub input_file_count: u64,
    pub thresholds: IntegrityThresholds,
}

/// Compute preflight recommendation from scan tallies (pure).
pub fn compute_preflight(input: &PreflightInputs) -> PreflightReport {
    let denom = (input.recoverable + input.skipped).max(1) as f64;
    let skip_rate = input.skipped as f64 / denom;
    let crc_skip_rate = input.crc_skips as f64 / denom;
    let failed_file_rate = input.failed_files as f64 / (input.input_file_count.max(1) as f64);

    let mut reasons: Vec<String> = Vec::new();
    let mut recommendation = PreflightRecommendation::Ok;

    // not_export_ready: strict integrity failure OR all files failed OR zero recoverable with skip/fail
    let all_files_failed =
        input.input_file_count > 0 && input.failed_files >= input.input_file_count;
    let zero_recoverable_with_problems =
        input.recoverable == 0 && (input.skipped > 0 || input.failed_files > 0);
    let strict_fail = input.mode == ScanMode::Strict
        && (input.skipped > 0 || input.failed_files > 0 || skip_rate > 0.0);

    if all_files_failed || zero_recoverable_with_problems || strict_fail {
        recommendation = PreflightRecommendation::NotExportReady;
        if all_files_failed {
            reasons.push("all_files_failed".into());
        }
        if zero_recoverable_with_problems {
            reasons.push("zero_recoverable".into());
        }
        if strict_fail {
            reasons.push("strict_integrity_failure".into());
        }
    } else {
        if skip_rate > input.thresholds.max_skip_rate {
            reasons.push("skip_rate_exceeded".into());
        }
        if crc_skip_rate > input.thresholds.max_crc_skip_rate {
            reasons.push("crc_skip_rate_exceeded".into());
        }
        if failed_file_rate > input.thresholds.max_failed_file_rate {
            reasons.push("failed_file_rate_exceeded".into());
        }
        if !reasons.is_empty() {
            recommendation = PreflightRecommendation::ReExportRecommended;
        }
    }

    PreflightReport {
        schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: input.mode,
        skip_rate,
        crc_skip_rate,
        failed_file_rate,
        thresholds: input.thresholds,
        recommendation,
        reasons,
    }
}

/// Increment a reason tally map.
pub fn tally_reason(map: &mut BTreeMap<String, u64>, reason: IntegrityReason) {
    *map.entry(reason.as_str().to_string()).or_insert(0) += 1;
}

/// Classification decision for a message candidate under a scan mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageClassification {
    /// Hard failure — skip with reason.
    Skip {
        reason: IntegrityReason,
        detail: String,
    },
    /// Recoverable (optionally degraded).
    Recoverable { integrity: RecoverableIntegrity },
}

/// Classify body integrity flags under the given mode.
///
/// Intentional 4KB preview must leave both flags false and never produce BODY_TRUNCATED.
pub fn classify_body_flags(
    mode: ScanMode,
    body_incomplete: bool,
    body_unavailable: bool,
) -> MessageClassification {
    if body_incomplete {
        match mode {
            ScanMode::BestEffort => MessageClassification::Recoverable {
                integrity: RecoverableIntegrity::with_degraded(
                    vec![IntegrityReason::BodyTruncated],
                    false,
                ),
            },
            ScanMode::Strict => MessageClassification::Skip {
                reason: IntegrityReason::BodyTruncated,
                detail: "body truncated due to corruption".into(),
            },
        }
    } else if body_unavailable {
        match mode {
            ScanMode::BestEffort => MessageClassification::Recoverable {
                integrity: RecoverableIntegrity::with_degraded(
                    vec![IntegrityReason::BodyUnavailable],
                    false,
                ),
            },
            ScanMode::Strict => MessageClassification::Skip {
                reason: IntegrityReason::BodyUnavailable,
                detail: "body property unreadable".into(),
            },
        }
    } else {
        MessageClassification::Recoverable {
            integrity: RecoverableIntegrity::clean(),
        }
    }
}

/// Classify attach-meta failure under mode.
pub fn classify_attach_meta_fail(
    mode: ScanMode,
    detail: impl Into<String>,
) -> MessageClassification {
    let detail = detail.into();
    match mode {
        ScanMode::BestEffort => MessageClassification::Recoverable {
            integrity: RecoverableIntegrity::with_degraded(
                vec![IntegrityReason::AttachMetaFailed],
                false,
            ),
        },
        ScanMode::Strict => MessageClassification::Skip {
            reason: IntegrityReason::AttachMetaFailed,
            detail,
        },
    }
}

/// Classify orphaned hierarchy under mode.
pub fn classify_orphaned(mode: ScanMode) -> MessageClassification {
    match mode {
        ScanMode::BestEffort => MessageClassification::Recoverable {
            integrity: RecoverableIntegrity::with_degraded(
                vec![IntegrityReason::OrphanedNode],
                true,
            ),
        },
        ScanMode::Strict => MessageClassification::Skip {
            reason: IntegrityReason::OrphanedNode,
            detail: "message hierarchy path unresolved".into(),
        },
    }
}

/// Merge multiple recoverable classifications (body + attach + orphan).
///
/// If any part is Skip, returns that skip (strict). Otherwise unions degraded reasons.
pub fn merge_recoverable(
    parts: impl IntoIterator<Item = MessageClassification>,
) -> MessageClassification {
    let mut reasons: Vec<IntegrityReason> = Vec::new();
    let mut is_orphaned = false;
    for part in parts {
        match part {
            MessageClassification::Skip { reason, detail } => {
                return MessageClassification::Skip { reason, detail };
            }
            MessageClassification::Recoverable { integrity } => {
                is_orphaned |= integrity.is_orphaned;
                for r in integrity.degraded_reasons {
                    if !reasons.contains(&r) {
                        reasons.push(r);
                    }
                }
            }
        }
    }
    MessageClassification::Recoverable {
        integrity: RecoverableIntegrity::with_degraded(reasons, is_orphaned),
    }
}

/// Streaming ledger sink for skip / degraded rows.
pub trait IntegrityLedgerWriter {
    fn write_skip(&mut self, row: &SkipRecord) -> std::io::Result<()>;
    /// Optional degraded recoverable row (same columns + Class=degraded).
    fn write_degraded(&mut self, row: &SkipRecord) -> std::io::Result<()>;
    fn flush(&mut self) -> std::io::Result<()>;
}

/// Streaming integrity CSV: `SourcePath,SourcePst,Folder,IsOrphaned,NID,Reason,Detail,Mode,Class`.
pub struct IntegrityCsvWriter {
    wtr: csv::Writer<BufWriter<File>>,
    path: PathBuf,
    rows_written: u64,
}

impl IntegrityCsvWriter {
    /// Create/truncate integrity CSV and write header.
    pub fn create(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(&path)?;
        let mut wtr = csv::Writer::from_writer(BufWriter::new(file));
        wtr.write_record([
            "SourcePath",
            "SourcePst",
            "Folder",
            "IsOrphaned",
            "NID",
            "Reason",
            "Detail",
            "Mode",
            "Class",
        ])
        .map_err(csv_err_to_io)?;
        wtr.flush().map_err(io_err_identity)?;
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

    fn write_row(&mut self, row: &SkipRecord, class: &str) -> std::io::Result<()> {
        self.wtr
            .write_record([
                row.source_path.as_str(),
                row.source_pst.as_str(),
                row.folder_path.as_str(),
                if row.is_orphaned { "true" } else { "false" },
                &row.nid.to_string(),
                row.reason.as_str(),
                row.detail.as_str(),
                row.mode.as_str(),
                class,
            ])
            .map_err(csv_err_to_io)?;
        self.rows_written += 1;
        // Flush every row for crash resilience on multi-GB skip storms.
        self.wtr.flush().map_err(io_err_identity)
    }
}

impl IntegrityLedgerWriter for IntegrityCsvWriter {
    fn write_skip(&mut self, row: &SkipRecord) -> std::io::Result<()> {
        self.write_row(row, "skip")
    }

    fn write_degraded(&mut self, row: &SkipRecord) -> std::io::Result<()> {
        self.write_row(row, "degraded")
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.wtr.flush().map_err(io_err_identity)
    }
}

/// Derive auto-sidecar integrity path from a dedup CSV path: `report.csv` → `report.integrity.csv`.
pub fn integrity_sidecar_path(csv_path: &Path) -> PathBuf {
    let parent = csv_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = csv_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("report");
    parent.join(format!("{stem}.integrity.csv"))
}

fn csv_err_to_io(e: csv::Error) -> std::io::Error {
    std::io::Error::other(e)
}

fn io_err_identity(e: std::io::Error) -> std::io::Error {
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use pst_reader::PstError;
    use std::io::Read;
    use tempfile::NamedTempFile;

    #[test]
    fn reason_mapping_table() {
        let cases: Vec<(PstError, IntegrityReason)> = vec![
            (
                PstError::AnsiPstNotSupported(14),
                IntegrityReason::AnsiUnsupported,
            ),
            (
                PstError::UnsupportedCryptMethod(3),
                IntegrityReason::UnsupportedCrypt,
            ),
            (
                PstError::CrcMismatch {
                    computed: 1,
                    stored: 2,
                },
                IntegrityReason::CrcMismatch,
            ),
            (
                PstError::BlockNotFound(0x10),
                IntegrityReason::BlockNotFound,
            ),
            (PstError::NodeNotFound(0x20), IntegrityReason::NodeNotFound),
            (
                PstError::SubnodeNotFound(0x21),
                IntegrityReason::NodeNotFound,
            ),
            (
                PstError::NoSubnodeBTree(0x22),
                IntegrityReason::NodeNotFound,
            ),
            (
                PstError::DataTruncated {
                    needed: 10,
                    available: 2,
                },
                IntegrityReason::DataTruncated,
            ),
            (PstError::InvalidMagic(0), IntegrityReason::InvalidStructure),
            (
                PstError::InvalidPageType {
                    expected: 1,
                    actual: 2,
                },
                IntegrityReason::InvalidStructure,
            ),
            (
                PstError::PropertyNotFound(0x1000),
                IntegrityReason::PropertyError,
            ),
            (
                PstError::PropertyTypeMismatch {
                    tag: 1,
                    expected: "str",
                    actual: 2,
                },
                IntegrityReason::PropertyError,
            ),
            (
                PstError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "x")),
                IntegrityReason::OpenFailed,
            ),
            (PstError::InvalidUtf16, IntegrityReason::InvalidStructure),
        ];
        for (err, expected) in cases {
            assert_eq!(
                reason_from_pst_error(&err),
                expected,
                "err={err:?} → {}",
                expected.as_str()
            );
        }
    }

    #[test]
    fn reason_codes_are_stable_strings() {
        assert_eq!(IntegrityReason::CrcMismatch.as_str(), "CRC_MISMATCH");
        assert_eq!(IntegrityReason::BodyTruncated.as_str(), "BODY_TRUNCATED");
        assert_eq!(
            IntegrityReason::BodyUnavailable.as_str(),
            "BODY_UNAVAILABLE"
        );
        assert_eq!(IntegrityReason::OrphanedNode.as_str(), "ORPHANED_NODE");
        assert_eq!(
            IntegrityReason::AttachMetaFailed.as_str(),
            "ATTACH_META_FAILED"
        );
    }

    #[test]
    fn scan_mode_serde_cli_strings() {
        assert_eq!(
            serde_json::to_string(&ScanMode::BestEffort).unwrap(),
            "\"best-effort\""
        );
        assert_eq!(
            serde_json::to_string(&ScanMode::Strict).unwrap(),
            "\"strict\""
        );
        assert_eq!(
            serde_json::from_str::<ScanMode>("\"best-effort\"").unwrap(),
            ScanMode::BestEffort
        );
    }

    #[test]
    fn preflight_ok_when_rates_low() {
        let report = compute_preflight(&PreflightInputs {
            mode: ScanMode::BestEffort,
            recoverable: 100,
            skipped: 1,
            crc_skips: 0,
            failed_files: 0,
            input_file_count: 1,
            thresholds: IntegrityThresholds::default(),
        });
        assert_eq!(report.recommendation, PreflightRecommendation::Ok);
        assert_eq!(report.schema, SCAN_INTEGRITY_SCHEMA);
        assert!((report.skip_rate - 1.0 / 101.0).abs() < 1e-9);
    }

    #[test]
    fn preflight_re_export_on_high_skip_rate() {
        let report = compute_preflight(&PreflightInputs {
            mode: ScanMode::BestEffort,
            recoverable: 90,
            skipped: 10, // 10%
            crc_skips: 0,
            failed_files: 0,
            input_file_count: 1,
            thresholds: IntegrityThresholds::default(),
        });
        assert_eq!(
            report.recommendation,
            PreflightRecommendation::ReExportRecommended
        );
        assert!(report.reasons.iter().any(|r| r == "skip_rate_exceeded"));
    }

    #[test]
    fn preflight_not_export_ready_zero_recoverable() {
        let report = compute_preflight(&PreflightInputs {
            mode: ScanMode::BestEffort,
            recoverable: 0,
            skipped: 5,
            crc_skips: 0,
            failed_files: 0,
            input_file_count: 1,
            thresholds: IntegrityThresholds::default(),
        });
        assert_eq!(
            report.recommendation,
            PreflightRecommendation::NotExportReady
        );
    }

    #[test]
    fn preflight_strict_skip_not_export_ready() {
        let report = compute_preflight(&PreflightInputs {
            mode: ScanMode::Strict,
            recoverable: 100,
            skipped: 1,
            crc_skips: 0,
            failed_files: 0,
            input_file_count: 1,
            thresholds: IntegrityThresholds::default(),
        });
        assert_eq!(
            report.recommendation,
            PreflightRecommendation::NotExportReady
        );
        assert!(report
            .reasons
            .iter()
            .any(|r| r == "strict_integrity_failure"));
    }

    #[test]
    fn classify_body_best_effort_degraded() {
        let c = classify_body_flags(ScanMode::BestEffort, true, false);
        match c {
            MessageClassification::Recoverable { integrity } => {
                assert!(integrity.degraded);
                assert!(integrity
                    .degraded_reasons
                    .contains(&IntegrityReason::BodyTruncated));
            }
            other => panic!("expected recoverable, got {other:?}"),
        }
    }

    #[test]
    fn classify_body_strict_skips() {
        let c = classify_body_flags(ScanMode::Strict, true, false);
        match c {
            MessageClassification::Skip { reason, .. } => {
                assert_eq!(reason, IntegrityReason::BodyTruncated);
            }
            other => panic!("expected skip, got {other:?}"),
        }
    }

    #[test]
    fn intentional_preview_not_truncated() {
        // Both flags false ⇒ clean recoverable (4KB preview path).
        let c = classify_body_flags(ScanMode::BestEffort, false, false);
        assert_eq!(
            c,
            MessageClassification::Recoverable {
                integrity: RecoverableIntegrity::clean()
            }
        );
    }

    #[test]
    fn classify_attach_and_orphan() {
        let a = classify_attach_meta_fail(ScanMode::BestEffort, "x");
        match a {
            MessageClassification::Recoverable { integrity } => {
                assert!(integrity
                    .degraded_reasons
                    .contains(&IntegrityReason::AttachMetaFailed));
            }
            other => panic!("{other:?}"),
        }
        let o = classify_orphaned(ScanMode::Strict);
        match o {
            MessageClassification::Skip { reason, .. } => {
                assert_eq!(reason, IntegrityReason::OrphanedNode);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn orphan_flag_not_from_empty_path_alone() {
        // Empty folder_path alone must not imply orphan — is_orphaned is explicit.
        let root = RecoverableIntegrity::clean();
        assert!(!root.is_orphaned);
        assert!(!root.degraded);
        let orphan = RecoverableIntegrity::with_degraded(vec![IntegrityReason::OrphanedNode], true);
        assert!(orphan.is_orphaned);
        assert!(orphan.degraded);
    }

    #[test]
    fn streaming_writer_n_skips_without_vec() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().with_extension("integrity.csv");
        let mut wtr = IntegrityCsvWriter::create(&path).unwrap();
        let n = 250u64;
        // Write loop only — no Vec of SkipRecord retained.
        for i in 0..n {
            let row = SkipRecord {
                source_path: "/tmp/x.pst".to_string(),
                source_pst: "x.pst".into(),
                folder_path: "Inbox".into(),
                is_orphaned: false,
                nid: i,
                reason: IntegrityReason::CrcMismatch,
                detail: format!("row {i}"),
                mode: ScanMode::Strict,
            };
            wtr.write_skip(&row).unwrap();
        }
        wtr.flush().unwrap();
        assert_eq!(wtr.rows_written(), n);

        let mut file = File::open(&path).unwrap();
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        // header + n data rows
        assert_eq!(lines.len() as u64, n + 1);
        assert!(lines[0].contains("IsOrphaned"));
        assert!(lines[1].contains("CRC_MISMATCH"));
        assert!(lines[1].contains("skip"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sidecar_path_rule() {
        let p = Path::new("output/report.csv");
        assert_eq!(
            integrity_sidecar_path(p),
            PathBuf::from("output/report.integrity.csv")
        );
    }
}
