//! Source-differential unique-PST QC (track 0080 Tier A).
//!
//! Levels: `off | structure | sample | full`. Default after fixture proof: **sample**.
//! Hard findings (`defect`, `unexplained_loss`) set `verify_ok = false` → existing
//! `VERIFY_FAILED` (no new exit integers). `known_gap` never fails.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::export_oracle::{
    hex_sha256, message_content_detail, structural_digest_pst, MessageContentDetail,
    VolumeStructuralDigest,
};
use crate::fidelity_contract::{FidelityContract, FindingClass, FIDELITY_CONTRACT_VERSION};
use crate::pst_materializer::PstHandleCache;
use crate::qc_attestation::{load_attestation, QcAttestationV1};
use crate::qc_external::{
    run_independent_reader, run_scanpst_auto, ExternalStatus, IndependentReaderResult,
    ScanpstResult, DEFAULT_EXTERNAL_TIMEOUT,
};
use crate::unique_export_report::{ExportMessageRow, VolumeReportRow};

/// Default risk-weighted sample cap (§3.3).
pub const DEFAULT_QC_SAMPLE_MAX: usize = 64;

/// QC depth on `unique-pst` / `qc-pst`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QcLevel {
    Off,
    Structure,
    /// Default after Phase 8 fixture proof (0080).
    #[default]
    Sample,
    Full,
}

impl QcLevel {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "structure" => Ok(Self::Structure),
            "sample" => Ok(Self::Sample),
            "full" => Ok(Self::Full),
            other => Err(format!(
                "invalid --qc-level '{other}' (expected off|structure|sample|full)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Structure => "structure",
            Self::Sample => "sample",
            Self::Full => "full",
        }
    }
}

/// Lightweight candidate metadata for deterministic sampling (pure over export/keep data).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QcSampleCandidate {
    pub export_message_index: u64,
    pub volume_index: u32,
    pub source_path: String,
    pub source_nid: u64,
    pub folder_path: String,
    pub subject: String,
    pub sender: String,
    pub message_id_norm: String,
    pub body_plain_len: usize,
    pub body_html_len: usize,
    pub attach_count: usize,
    pub max_attach_size: u64,
    pub has_zero_byte_attach: bool,
    pub has_embedded: bool,
    /// Sampling stratum only — **never** used to explain unrelated property mismatches.
    pub has_degraded: bool,
    /// Attachment ledger soft-fail on this message (sampling / empty-hash explain only when
    /// filename-specific list is empty). Prefer [`Self::ledger_failed_attach_names`].
    pub has_ledger_fail: bool,
    /// Filenames with attach-ledger Fail events for this message (case-preserving).
    /// Missing output attaches are explained **only** when the filename matches (case-insensitive).
    #[serde(default)]
    pub ledger_failed_attach_names: Vec<String>,
    /// Source `body_unavailable` fidelity flag (explains body loss only, never CC/attaches).
    pub body_unavailable: bool,
    /// Source `body_incomplete` / truncated body flag (body explain only).
    pub body_incomplete: bool,
    /// Source CRC_SUSPECT integrity flag (body/digest explain only — never CC).
    pub crc_suspect: bool,
    pub subject_non_ascii: bool,
    pub display_cc: String,
    pub display_bcc: String,
}

/// One QC finding row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QcFinding {
    pub class: FindingClass,
    pub property: String,
    pub volume_index: u32,
    pub source_path: String,
    pub source_nid: u64,
    pub message_id_norm: String,
    pub detail: String,
}

/// Per-volume QC summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcVolumeResult {
    pub volume_index: u32,
    pub path: String,
    pub open_ok: bool,
    pub folder_tree_match: bool,
    pub message_count_match: bool,
    pub messages_found: u64,
    pub messages_expected: u64,
    pub messages_compared: u64,
    pub attachments_compared: u64,
    pub error: Option<String>,
}

/// Finding counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct QcFindingCounts {
    pub defect: u64,
    pub unexplained_loss: u64,
    pub known_gap: u64,
    pub explained: u64,
    pub skipped_source_unavailable: u64,
}

impl QcFindingCounts {
    pub fn hard_fail(&self) -> bool {
        self.defect > 0 || self.unexplained_loss > 0
    }

    pub fn record(&mut self, class: FindingClass) {
        match class {
            FindingClass::Defect => self.defect = self.defect.saturating_add(1),
            FindingClass::UnexplainedLoss => {
                self.unexplained_loss = self.unexplained_loss.saturating_add(1)
            }
            FindingClass::KnownGap => self.known_gap = self.known_gap.saturating_add(1),
            FindingClass::Explained => self.explained = self.explained.saturating_add(1),
        }
    }
}

/// External block in qc_report_v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcExternalBlock {
    pub independent_reader: IndependentReaderResult,
    pub scanpst: ScanpstResult,
}

/// Top-level `qc_report_v1` artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcReportV1 {
    pub schema: String,
    pub contract: String,
    pub qc_level: String,
    pub source_differential: bool,
    pub content_digest_backed: bool,
    /// True when content digests cover only a sample (or subset) while QC requested broader coverage.
    /// Spec honesty: never claim full content coverage when digests are sample-granularity.
    #[serde(default)]
    pub content_digest_partial: bool,
    pub volumes: Vec<QcVolumeResult>,
    pub messages_compared: u64,
    pub attachments_compared: u64,
    pub findings: QcFindingCounts,
    pub external: QcExternalBlock,
    pub attestation: Option<QcAttestationV1>,
    pub qc_ms: u64,
    /// True when hard findings should fail verification.
    pub hard_fail: bool,
}

/// Inputs for running QC after unique-pst write (or standalone `qc-pst`).
pub struct QcRunInput<'a> {
    pub level: QcLevel,
    pub sample_max: usize,
    pub report_dir: &'a Path,
    pub volumes: &'a [VolumeReportRow],
    pub export_rows: &'a [ExportMessageRow],
    pub candidates: &'a [QcSampleCandidate],
    pub external_reader: Option<&'a Path>,
    pub run_scanpst: bool,
    pub max_open_psts: usize,
    /// When true, sources are re-opened (source-differential).
    pub source_differential: bool,
    /// When true, attachments were omitted by policy (`parents_only` / `--no-attachments`).
    /// Missing attach payloads are then `explained`, not `defect`.
    pub parents_only: bool,
    /// Test / diagnostic hook: when set, classify this property once and record the finding
    /// (exercises `unexplained_loss` → hard_fail wiring beyond unit-only `classify`).
    pub probe_unexplained_property: Option<&'a str>,
}

/// Select risk-weighted sample indices (deterministic pure function of candidates).
///
/// Strata from §3.3; dedupe; cap at `sample_max`. When capping, **stratum
/// representatives are preferred** over naive index-order truncate (so volume-last
/// / extremes are not dropped solely because they sort late).
/// Final order is stable by `export_message_index`.
pub fn select_sample_indices(candidates: &[QcSampleCandidate], sample_max: usize) -> Vec<usize> {
    if candidates.is_empty() || sample_max == 0 {
        return Vec::new();
    }

    // Priority-ordered stratum representatives (first insertion wins when capping).
    let mut priority: Vec<usize> = Vec::new();
    let mut push_prio = |idx: Option<usize>| {
        if let Some(i) = idx {
            if !priority.contains(&i) {
                priority.push(i);
            }
        }
    };

    // Largest body_plain / body_html
    push_prio(
        candidates
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.body_plain_len)
            .map(|(i, _)| i),
    );
    push_prio(
        candidates
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.body_html_len)
            .map(|(i, _)| i),
    );
    // Smallest / zero-byte body_plain floor
    push_prio(
        candidates
            .iter()
            .enumerate()
            .min_by_key(|(_, c)| c.body_plain_len)
            .map(|(i, _)| i),
    );
    // Most attachments; largest single attach; zero-byte attach
    push_prio(
        candidates
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.attach_count)
            .map(|(i, _)| i),
    );
    push_prio(
        candidates
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.max_attach_size)
            .map(|(i, _)| i),
    );
    for (i, c) in candidates.iter().enumerate() {
        if c.has_zero_byte_attach {
            push_prio(Some(i));
            break;
        }
    }
    // Longest subject / sender
    push_prio(
        candidates
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.subject.len())
            .map(|(i, _)| i),
    );
    push_prio(
        candidates
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.sender.len())
            .map(|(i, _)| i),
    );
    // Degraded / ledger
    for (i, c) in candidates.iter().enumerate() {
        if c.has_degraded || c.has_ledger_fail {
            push_prio(Some(i));
        }
    }
    // Non-ASCII subject
    for (i, c) in candidates.iter().enumerate() {
        if c.subject_non_ascii {
            push_prio(Some(i));
            break;
        }
    }
    // Volume first/last (high priority — must survive capping)
    let mut by_vol: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (i, c) in candidates.iter().enumerate() {
        by_vol.entry(c.volume_index).or_default().push(i);
    }
    for idxs in by_vol.values() {
        if let Some(&first) = idxs.first() {
            push_prio(Some(first));
        }
        if let Some(&last) = idxs.last() {
            push_prio(Some(last));
        }
    }
    // One per distinct source
    let mut seen_src: BTreeSet<String> = BTreeSet::new();
    for (i, c) in candidates.iter().enumerate() {
        let key = c.source_path.to_ascii_lowercase();
        if seen_src.insert(key) {
            push_prio(Some(i));
        }
    }
    // Embedded
    for (i, c) in candidates.iter().enumerate() {
        if c.has_embedded {
            push_prio(Some(i));
        }
    }

    // Cap: keep stratum reps first, then fill remaining by export_message_index.
    let mut ordered: Vec<usize> = Vec::with_capacity(sample_max.min(candidates.len()));
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    for i in priority {
        if ordered.len() >= sample_max {
            break;
        }
        if seen.insert(i) {
            ordered.push(i);
        }
    }
    if ordered.len() < sample_max {
        let mut rest: Vec<usize> = (0..candidates.len()).collect();
        rest.sort_by_key(|&i| candidates[i].export_message_index);
        for i in rest {
            if ordered.len() >= sample_max {
                break;
            }
            if seen.insert(i) {
                ordered.push(i);
            }
        }
    }
    ordered.sort_by_key(|&i| candidates[i].export_message_index);
    ordered
}

/// Build candidate list from export rows + optional metadata (lengths etc.).
pub fn candidates_from_export_and_meta(
    export_rows: &[ExportMessageRow],
    meta_by_index: &BTreeMap<u64, QcSampleCandidate>,
) -> Vec<QcSampleCandidate> {
    let mut out = Vec::with_capacity(export_rows.len());
    for row in export_rows {
        if let Some(m) = meta_by_index.get(&row.export_message_index) {
            out.push(m.clone());
        } else {
            out.push(QcSampleCandidate {
                export_message_index: row.export_message_index,
                volume_index: row.volume_index,
                source_path: row.source_path.clone(),
                source_nid: row.nid,
                folder_path: row.folder_path.clone(),
                subject: row.subject.clone(),
                sender: String::new(),
                message_id_norm: row.message_id_norm.clone(),
                body_plain_len: 0,
                body_html_len: 0,
                attach_count: 0,
                max_attach_size: 0,
                has_zero_byte_attach: false,
                has_embedded: false,
                has_degraded: false,
                has_ledger_fail: row.attachments_failed_count > 0,
                ledger_failed_attach_names: Vec::new(),
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                subject_non_ascii: !row.subject.is_ascii(),
                display_cc: String::new(),
                display_bcc: String::new(),
            });
        }
    }
    out
}

/// Inputs for [`candidate_from_write_msg`] (keeps argument count under clippy limits).
pub struct CandidateFromWriteMsg<'a> {
    pub export_message_index: u64,
    pub volume_index: u32,
    pub source_path: &'a str,
    pub source_nid: u64,
    pub folder_path: &'a str,
    pub message_id_norm: &'a str,
    pub subject: &'a str,
    pub write_msg: &'a pst_writer::WriteMessage,
    pub has_degraded: bool,
    pub has_ledger_fail: bool,
    /// Source-side BCC (not written; used for known_gap accounting).
    pub display_bcc: &'a str,
}

/// Build a sample candidate from a write-path `WriteMessage` + export identity.
pub fn candidate_from_write_msg(input: CandidateFromWriteMsg<'_>) -> QcSampleCandidate {
    let CandidateFromWriteMsg {
        export_message_index,
        volume_index,
        source_path,
        source_nid,
        folder_path,
        message_id_norm,
        subject,
        write_msg,
        has_degraded,
        has_ledger_fail,
        display_bcc,
    } = input;
    let body_plain_len = write_msg.body_plain.as_ref().map(|s| s.len()).unwrap_or(0);
    let body_html_len = write_msg.body_html.as_ref().map(|b| b.len()).unwrap_or(0);
    let attach_count = write_msg.attachments.len();
    let max_attach_size = write_msg
        .attachments
        .iter()
        .map(|a| a.size as u64)
        .max()
        .unwrap_or(0);
    let has_zero_byte_attach = write_msg.attachments.iter().any(|a| a.size == 0);
    let has_embedded = write_msg
        .attachments
        .iter()
        .any(|a| a.attach_method == Some(5) || a.embedded_message.is_some());
    QcSampleCandidate {
        export_message_index,
        volume_index,
        source_path: source_path.to_string(),
        source_nid,
        folder_path: folder_path.to_string(),
        subject: subject.to_string(),
        sender: write_msg.sender.clone().unwrap_or_default(),
        message_id_norm: message_id_norm.to_string(),
        body_plain_len,
        body_html_len,
        attach_count,
        max_attach_size,
        has_zero_byte_attach,
        has_embedded,
        has_degraded,
        has_ledger_fail,
        ledger_failed_attach_names: Vec::new(),
        body_unavailable: write_msg.body_unavailable,
        body_incomplete: write_msg.body_incomplete,
        crc_suspect: false, // filled by unique-pst from keep-set integrity when known
        subject_non_ascii: !subject.is_ascii(),
        display_cc: write_msg.display_cc.clone().unwrap_or_default(),
        display_bcc: display_bcc.to_string(),
    }
}

/// Effective ledger-fail names: candidate (live export) ∪ clean-room digest flags.
fn attach_ledger_explains_effective(
    cand: &QcSampleCandidate,
    digest: Option<&ContentDigestEntry>,
    filename: &str,
) -> bool {
    let target = filename.trim().to_ascii_lowercase();
    if cand
        .ledger_failed_attach_names
        .iter()
        .any(|n| n.trim().to_ascii_lowercase() == target)
    {
        return true;
    }
    digest.is_some_and(|d| {
        d.ledger_failed_attach_names
            .iter()
            .any(|n| n.trim().to_ascii_lowercase() == target)
    })
}

/// True when a body-specific fidelity flag may explain body/digest differences.
/// Never use for CC, subject, or attachment presence.
/// Prefers live candidate flags; falls back to clean-room digest flags (DoD-21).
fn body_loss_explained_with_digest(
    cand: &QcSampleCandidate,
    digest: Option<&ContentDigestEntry>,
) -> bool {
    if cand.body_unavailable || cand.body_incomplete || cand.crc_suspect {
        return true;
    }
    digest.is_some_and(|d| d.body_unavailable || d.body_incomplete || d.crc_suspect)
}

/// Normalize display address strings for comparison (strip quotes / angle brackets).
fn normalize_display_addr(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '\'' || c == '"' || c == '<' || c == '>')
        .trim()
        .to_string()
}

/// Origin of digests in `content_digests.json`. Only `"source"` enables
/// `content_digest_backed` clean-room content compare.
pub const CONTENT_DIGEST_ORIGIN_SOURCE: &str = "source";
/// Output-side digests must never enable defect-capable clean-room path.
pub const CONTENT_DIGEST_ORIGIN_OUTPUT: &str = "output";

/// Persist `content_digests.json` for clean-room re-verify (source-side only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDigestsFile {
    pub schema: String,
    /// `"source"` = export-time source-side digests; `"output"` must not enable content_digest_backed.
    #[serde(default)]
    pub origin: String,
    pub qc_level: String,
    pub volumes: Vec<ContentDigestsVolume>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDigestsVolume {
    pub volume_index: u32,
    pub path: String,
    pub messages: Vec<ContentDigestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDigestEntry {
    pub export_message_index: u64,
    pub source_path: String,
    pub source_nid: u64,
    pub message_id_norm: String,
    pub content_digest: String,
    /// Field-level payload for clean-room body/recipient compare under `parents_only`
    /// (full `content_digest` includes attach bytes; reconstructed digests must not zero these).
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub sender: String,
    #[serde(default)]
    pub display_to: String,
    #[serde(default)]
    pub display_cc: String,
    #[serde(default)]
    pub body_plain_len: usize,
    #[serde(default)]
    pub body_html_len: usize,
    pub attaches: Vec<AttachDigestEntry>,
    /// Optional production-path extras observed on source (empty in normal export).
    /// Each prop is classified via the fidelity allowlist; unknown ⇒ `unexplained_loss`.
    #[serde(default)]
    pub extra_source_props: Vec<String>,
    /// Fidelity explanation flags persisted at export so clean-room `qc-pst` can
    /// reclassify soft-fails the same way as the live source-differential path (DoD-21).
    #[serde(default)]
    pub has_degraded: bool,
    #[serde(default)]
    pub body_unavailable: bool,
    #[serde(default)]
    pub body_incomplete: bool,
    #[serde(default)]
    pub crc_suspect: bool,
    #[serde(default)]
    pub has_ledger_fail: bool,
    /// Filenames with attach-ledger Fail events (case-preserving). Explains missing
    /// output attaches only when the filename matches (case-insensitive).
    #[serde(default)]
    pub ledger_failed_attach_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachDigestEntry {
    pub filename: String,
    pub size: u64,
    pub payload_sha256: String,
}

/// Run full QC pipeline and write artifacts under `report_dir`.
pub fn run_unique_pst_qc(input: QcRunInput<'_>) -> QcReportV1 {
    let t0 = Instant::now();
    let contract = FidelityContract::v1();
    let mut findings_list: Vec<QcFinding> = Vec::new();
    let mut counts = QcFindingCounts::default();
    let mut vol_results = Vec::new();
    let mut messages_compared = 0u64;
    let mut attachments_compared = 0u64;
    let mut content_volumes: Vec<ContentDigestsVolume> = Vec::new();

    // Mandatory export_messages.csv coverage (DoD-3/6/21): never green when volumes
    // claim messages but the CSV is missing, short, or has duplicate indexes.
    validate_export_metadata_coverage(
        input.volumes,
        input.export_rows,
        &contract,
        &mut counts,
        &mut findings_list,
    );

    let content_digests_path = input.report_dir.join("content_digests.json");
    let existing_digests = load_content_digests(&content_digests_path);
    // content_digest_backed only when loaded digests are source-origin (never output).
    let content_digest_backed = !input.source_differential
        && existing_digests
            .as_ref()
            .is_some_and(content_digests_are_source_origin);

    // Output-only without source digests: structural only — cannot emit defect from content.
    let content_capable = input.source_differential || content_digest_backed;

    // Digest coverage (DoD-21): sample digests under full QC ⇒ partial, never silent pass.
    let digest_covered_indices: BTreeSet<u64> = existing_digests
        .as_ref()
        .filter(|d| content_digests_are_source_origin(d))
        .map(|d| {
            d.volumes
                .iter()
                .flat_map(|v| v.messages.iter())
                .map(|m| m.export_message_index)
                .collect()
        })
        .unwrap_or_default();
    let digests_qc_level = existing_digests
        .as_ref()
        .map(|d| d.qc_level.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let mut content_digest_partial = false;
    if content_digest_backed {
        let sample_granularity = digests_qc_level == "sample" || digests_qc_level == "structure";
        if input.level == QcLevel::Full && sample_granularity {
            content_digest_partial = true;
            counts.record(FindingClass::Defect);
            findings_list.push(QcFinding {
                class: FindingClass::Defect,
                property: "content_digests_incomplete".into(),
                volume_index: 0,
                source_path: String::new(),
                source_nid: 0,
                message_id_norm: String::new(),
                detail: format!(
                    "content digests incomplete for full (persisted qc_level={digests_qc_level})"
                ),
            });
        } else if input.level == QcLevel::Full {
            let missing: Vec<u64> = input
                .candidates
                .iter()
                .map(|c| c.export_message_index)
                .filter(|i| !digest_covered_indices.contains(i))
                .collect();
            if !missing.is_empty() {
                content_digest_partial = true;
                counts.record(FindingClass::Defect);
                findings_list.push(QcFinding {
                    class: FindingClass::Defect,
                    property: "content_digests_incomplete".into(),
                    volume_index: 0,
                    source_path: String::new(),
                    source_nid: 0,
                    message_id_norm: String::new(),
                    detail: format!(
                        "content digests incomplete for full: {} candidate(s) lack digest coverage (e.g. idx={:?})",
                        missing.len(),
                        missing.iter().take(5).collect::<Vec<_>>()
                    ),
                });
            }
        } else if matches!(input.level, QcLevel::Sample) {
            // Partial when sample set exceeds covered digests (still compare covered).
            let sample_probe = select_sample_indices(input.candidates, input.sample_max);
            let any_uncovered = sample_probe.iter().any(|&ci| {
                input
                    .candidates
                    .get(ci)
                    .is_some_and(|c| !digest_covered_indices.contains(&c.export_message_index))
            });
            if any_uncovered {
                content_digest_partial = true;
            }
        }
    }

    let sample_idxs: Vec<usize> = match input.level {
        QcLevel::Off => Vec::new(),
        QcLevel::Structure => Vec::new(),
        QcLevel::Sample => select_sample_indices(input.candidates, input.sample_max),
        QcLevel::Full => (0..input.candidates.len()).collect(),
    };

    let mut handle_cache = PstHandleCache::new(input.max_open_psts.max(1));

    for vol in input.volumes {
        let path = PathBuf::from(&vol.path);
        let mut open_ok = false;
        let mut folder_tree_match = false;
        let mut message_count_match = false;
        let mut messages_found = 0u64;
        let mut vol_msgs_compared = 0u64;
        let mut vol_att_compared = 0u64;
        let mut error: Option<String> = None;

        let expected_rows: Vec<&ExportMessageRow> = input
            .export_rows
            .iter()
            .filter(|r| r.volume_index == vol.volume_index)
            .collect();
        let expected_count = vol.messages_written;

        // Expected per-folder message counts (normalized path → count).
        let mut expected_folder_counts: BTreeMap<String, u64> = BTreeMap::new();
        for r in &expected_rows {
            let key = normalize_folder_key(&r.folder_path);
            if key.is_empty() {
                continue;
            }
            *expected_folder_counts.entry(key).or_insert(0) += 1;
        }

        match structural_digest_pst(&path) {
            Ok(digest) => {
                open_ok = true;
                messages_found = digest.message_count;
                message_count_match = messages_found == expected_count;

                // Folder tree: every expected leaf must match by exact/suffix path segments
                // with equal per-folder message counts (not presence alone).
                folder_tree_match =
                    folder_tree_matches(&digest, &expected_folder_counts, expected_count);

                if !folder_tree_match {
                    let (class, _) = contract.classify("folder_tree_structure", false);
                    counts.record(class);
                    findings_list.push(QcFinding {
                        class,
                        property: "folder_tree_structure".into(),
                        volume_index: vol.volume_index,
                        source_path: String::new(),
                        source_nid: 0,
                        message_id_norm: String::new(),
                        detail: format!(
                            "folder tree/count mismatch: out_folders={:?} out_counts={:?} expected={:?}",
                            digest.folder_paths,
                            digest.folder_message_counts,
                            expected_folder_counts
                        ),
                    });
                }
                if !message_count_match {
                    let (class, _) = contract.classify("message_content_digest", false);
                    // Count mismatch is a structural defect when we expect equality.
                    counts.record(FindingClass::Defect);
                    findings_list.push(QcFinding {
                        class: FindingClass::Defect,
                        property: "message_count".into(),
                        volume_index: vol.volume_index,
                        source_path: String::new(),
                        source_nid: 0,
                        message_id_norm: String::new(),
                        detail: format!(
                            "messages_found={messages_found} expected={expected_count}"
                        ),
                    });
                    let _ = class;
                }

                // Content comparison for sample/full (source-differential or source digests).
                if matches!(input.level, QcLevel::Sample | QcLevel::Full) && content_capable {
                    let mut digest_entries = Vec::new();
                    let mut out_index = index_output_messages(&path);

                    for &ci in &sample_idxs {
                        let cand = match input.candidates.get(ci) {
                            Some(c) if c.volume_index == vol.volume_index => c,
                            _ => continue,
                        };
                        // Clean-room: never silently pass candidates absent from digests.
                        if content_digest_backed
                            && !input.source_differential
                            && !digest_covered_indices.contains(&cand.export_message_index)
                        {
                            content_digest_partial = true;
                            counts.record(FindingClass::Defect);
                            findings_list.push(QcFinding {
                                class: FindingClass::Defect,
                                property: "content_digest_unavailable".into(),
                                volume_index: cand.volume_index,
                                source_path: cand.source_path.clone(),
                                source_nid: cand.source_nid,
                                message_id_norm: cand.message_id_norm.clone(),
                                detail: format!(
                                    "no source content digest for export_message_index={} (partial/unavailable; not silent pass)",
                                    cand.export_message_index
                                ),
                            });
                            vol_msgs_compared = vol_msgs_compared.saturating_add(1);
                            messages_compared = messages_compared.saturating_add(1);
                            continue;
                        }
                        let compare = compare_one_message(CompareOneArgs {
                            cand,
                            handles: &mut handle_cache,
                            out_index: &mut out_index,
                            out_path: &path,
                            source_differential: input.source_differential,
                            existing: existing_digests
                                .as_ref()
                                .filter(|d| content_digests_are_source_origin(d)),
                            contract: &contract,
                            parents_only: input.parents_only,
                        });
                        vol_msgs_compared = vol_msgs_compared.saturating_add(1);
                        messages_compared = messages_compared.saturating_add(1);
                        attachments_compared =
                            attachments_compared.saturating_add(compare.attachments_compared);
                        vol_att_compared =
                            vol_att_compared.saturating_add(compare.attachments_compared);

                        if compare.skipped_source {
                            counts.skipped_source_unavailable =
                                counts.skipped_source_unavailable.saturating_add(1);
                        }
                        for f in compare.findings {
                            counts.record(f.class);
                            findings_list.push(f);
                        }
                        // Persist digests only from source-side reads (export path).
                        // Never write extra_source_props (empty on export).
                        if input.source_differential {
                            if let Some(mut entry) = compare.digest_entry {
                                entry.extra_source_props.clear();
                                digest_entries.push(entry);
                            }
                        }
                    }
                    if input.source_differential && !digest_entries.is_empty() {
                        content_volumes.push(ContentDigestsVolume {
                            volume_index: vol.volume_index,
                            path: vol.path.clone(),
                            messages: digest_entries,
                        });
                    }
                }
                // Output-only without source digests: structural only — do NOT write
                // output digests as content_digests.json (would falsely enable
                // content_digest_backed on a later qc-pst run).
            }
            Err(e) => {
                error = Some(e);
                counts.record(FindingClass::Defect);
                findings_list.push(QcFinding {
                    class: FindingClass::Defect,
                    property: "volume_open".into(),
                    volume_index: vol.volume_index,
                    source_path: String::new(),
                    source_nid: 0,
                    message_id_norm: String::new(),
                    detail: error.clone().unwrap_or_default(),
                });
            }
        }

        vol_results.push(QcVolumeResult {
            volume_index: vol.volume_index,
            path: vol.path.clone(),
            open_ok,
            folder_tree_match,
            message_count_match,
            messages_found,
            messages_expected: expected_count,
            messages_compared: vol_msgs_compared,
            attachments_compared: vol_att_compared,
            error,
        });
    }

    // BCC known_gap counts from candidates with non-empty display_bcc meta
    // (plumbed from CanonicalMessage; not written to output).
    for c in input.candidates {
        if !c.display_bcc.trim().is_empty() {
            let (class, _) = contract.classify("display_bcc", false);
            if class == FindingClass::KnownGap {
                counts.record(class);
                findings_list.push(QcFinding {
                    class,
                    property: "display_bcc".into(),
                    volume_index: c.volume_index,
                    source_path: c.source_path.clone(),
                    source_nid: c.source_nid,
                    message_id_norm: c.message_id_norm.clone(),
                    detail: "BCC dropped_by_design (disclosure)".into(),
                });
            }
        }
    }

    // Test/diagnostic: force one allowlist-miss classification into the pipeline
    // via the same record path production uses for observed differences.
    if let Some(prop) = input.probe_unexplained_property {
        if !prop.is_empty() {
            record_classified_finding(
                &contract,
                &mut counts,
                &mut findings_list,
                prop,
                false,
                RecordFindingId {
                    volume_index: 0,
                    source_path: "",
                    source_nid: 0,
                    message_id_norm: "",
                },
                format!("probe property '{prop}' via record_classified_finding"),
            );
        }
    }

    // External sidecars (skip-safe) — run for **every** volume and aggregate.
    let mut independent_reader = if input.external_reader.is_none() {
        IndependentReaderResult::skipped("no --qc-external-reader path")
    } else if input.volumes.is_empty() {
        IndependentReaderResult::skipped("no volumes")
    } else {
        IndependentReaderResult::skipped("pending multi-volume aggregate")
    };
    if let Some(tool) = input.external_reader {
        let mut agg: Option<IndependentReaderResult> = None;
        let mut reasons: Vec<String> = Vec::new();
        for vol in input.volumes {
            let r = run_independent_reader(tool, Path::new(&vol.path), DEFAULT_EXTERNAL_TIMEOUT);
            if r.status == ExternalStatus::Ok {
                let expected_msgs = vol.messages_written;
                if let Some(reader_msgs) = r.message_count {
                    if reader_msgs != expected_msgs {
                        counts.record(FindingClass::Defect);
                        findings_list.push(QcFinding {
                            class: FindingClass::Defect,
                            property: "independent_reader_message_count".into(),
                            volume_index: vol.volume_index,
                            source_path: String::new(),
                            source_nid: 0,
                            message_id_norm: String::new(),
                            detail: format!(
                                "independent reader message_count={reader_msgs} expected={expected_msgs} vol={}",
                                vol.volume_index
                            ),
                        });
                        reasons.push(format!(
                            "vol{} message_count mismatch: reader={reader_msgs} expected={expected_msgs}",
                            vol.volume_index
                        ));
                    }
                }
                let expected_folder_leaves = input
                    .export_rows
                    .iter()
                    .filter(|row| row.volume_index == vol.volume_index)
                    .map(|row| normalize_folder_key(&row.folder_path))
                    .filter(|k| !k.is_empty())
                    .collect::<BTreeSet<_>>()
                    .len() as u64;
                if expected_folder_leaves > 0 {
                    if let Some(reader_folders) = r.folder_count {
                        if reader_folders == 0 && expected_folder_leaves > 0 {
                            counts.record(FindingClass::Defect);
                            findings_list.push(QcFinding {
                                class: FindingClass::Defect,
                                property: "independent_reader_folder_count".into(),
                                volume_index: vol.volume_index,
                                source_path: String::new(),
                                source_nid: 0,
                                message_id_norm: String::new(),
                                detail: format!(
                                    "independent reader folder_count=0 expected_leaf_folders>={expected_folder_leaves} vol={}",
                                    vol.volume_index
                                ),
                            });
                        } else if reader_folders < expected_folder_leaves {
                            counts.record(FindingClass::Defect);
                            findings_list.push(QcFinding {
                                class: FindingClass::Defect,
                                property: "independent_reader_folder_count".into(),
                                volume_index: vol.volume_index,
                                source_path: String::new(),
                                source_nid: 0,
                                message_id_norm: String::new(),
                                detail: format!(
                                    "independent reader folder_count={reader_folders} < expected_leaf_folders={expected_folder_leaves} vol={}",
                                    vol.volume_index
                                ),
                            });
                        }
                    }
                }
            }
            if let Some(ref mut a) = agg {
                *a = merge_independent_reader(a, &r);
            } else {
                agg = Some(r);
            }
        }
        if let Some(mut a) = agg {
            if !reasons.is_empty() {
                a.reason = Some(reasons.join("; "));
            }
            independent_reader = a;
        }
    }

    let mut scanpst = if !input.run_scanpst {
        ScanpstResult::skipped("scanpst not requested")
    } else if input.volumes.is_empty() {
        ScanpstResult::skipped("no volumes")
    } else {
        let mut agg: Option<ScanpstResult> = None;
        for vol in input.volumes {
            let r = run_scanpst_auto(Path::new(&vol.path), DEFAULT_EXTERNAL_TIMEOUT);
            if r.hard_error {
                counts.record(FindingClass::Defect);
                findings_list.push(QcFinding {
                    class: FindingClass::Defect,
                    property: "scanpst".into(),
                    volume_index: vol.volume_index,
                    source_path: String::new(),
                    source_nid: 0,
                    message_id_norm: String::new(),
                    detail: r
                        .reason
                        .clone()
                        .unwrap_or_else(|| "scanpst hard error".into()),
                });
            }
            if let Some(ref mut a) = agg {
                *a = merge_scanpst(a, &r);
            } else {
                agg = Some(r);
            }
        }
        agg.unwrap_or_else(|| ScanpstResult::skipped("no volumes"))
    };
    // Ensure hard_error is reflected as Failed status on the aggregate.
    if scanpst.hard_error {
        scanpst.status = ExternalStatus::Failed;
    }

    let attestation = load_attestation(&input.report_dir.join("qc_attestation_v1.json"))
        .ok()
        .flatten();

    let qc_ms = t0.elapsed().as_millis() as u64;

    // Write artifacts first so write failures can force hard_fail / report_ok false.
    let mut artifact_errors: Vec<String> = Vec::new();

    // Findings CSV before final report so we can include artifact errors in report counts.
    // content_digests only for source-side digests (export with live sources).
    if input.source_differential
        && matches!(input.level, QcLevel::Sample | QcLevel::Full)
        && !content_volumes.is_empty()
    {
        let digests = ContentDigestsFile {
            schema: "content_digests_v1".into(),
            origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
            qc_level: input.level.as_str().into(),
            volumes: content_volumes,
        };
        if let Err(e) = write_content_digests(input.report_dir, &digests) {
            artifact_errors.push(format!("content_digests.json: {e}"));
        }
    }

    if !artifact_errors.is_empty() {
        for e in &artifact_errors {
            counts.record(FindingClass::Defect);
            findings_list.push(QcFinding {
                class: FindingClass::Defect,
                property: "qc_artifact_write".into(),
                volume_index: 0,
                source_path: String::new(),
                source_nid: 0,
                message_id_norm: String::new(),
                detail: e.clone(),
            });
        }
    }

    let hard_fail = counts.hard_fail();
    let report = QcReportV1 {
        schema: "qc_report_v1".into(),
        contract: FIDELITY_CONTRACT_VERSION.into(),
        qc_level: input.level.as_str().into(),
        source_differential: input.source_differential,
        content_digest_backed,
        content_digest_partial,
        volumes: vol_results,
        messages_compared,
        attachments_compared,
        findings: counts,
        external: QcExternalBlock {
            independent_reader,
            scanpst,
        },
        attestation,
        qc_ms,
        hard_fail,
    };

    if let Err(e) = write_qc_report(input.report_dir, &report) {
        // Report write failed — force hard_fail on a second write attempt with defect.
        artifact_errors.push(format!("qc_report_v1.json: {e}"));
    }
    if let Err(e) = write_qc_findings_csv(input.report_dir, &findings_list) {
        artifact_errors.push(format!("qc_findings.csv: {e}"));
    }

    if !artifact_errors.is_empty() {
        // Re-emit with hard_fail so callers cannot treat missing artifacts as green.
        let mut counts2 = report.findings.clone();
        let mut findings2 = findings_list.clone();
        for e in &artifact_errors {
            // Avoid double-counting content_digests defects already recorded.
            if e.starts_with("qc_report_v1.json:") || e.starts_with("qc_findings.csv:") {
                counts2.record(FindingClass::Defect);
                findings2.push(QcFinding {
                    class: FindingClass::Defect,
                    property: "qc_artifact_write".into(),
                    volume_index: 0,
                    source_path: String::new(),
                    source_nid: 0,
                    message_id_norm: String::new(),
                    detail: e.clone(),
                });
            }
        }
        let hard2 = counts2.hard_fail();
        let report2 = QcReportV1 {
            findings: counts2,
            hard_fail: hard2,
            ..report
        };
        let _ = write_qc_report(input.report_dir, &report2);
        let _ = write_qc_findings_csv(input.report_dir, &findings2);
        return report2;
    }

    report
}

/// Merge two independent-reader results (worst status wins).
/// Ranking: Failed > Timeout > Skipped > Ok — any Skipped volume prevents aggregate Ok.
fn merge_independent_reader(
    a: &IndependentReaderResult,
    b: &IndependentReaderResult,
) -> IndependentReaderResult {
    let status = worse_external_status(a.status.clone(), b.status.clone());
    let reason = match (&a.reason, &b.reason) {
        (Some(x), Some(y)) if x != y => Some(format!("{x}; {y}")),
        (Some(x), _) => Some(x.clone()),
        (_, Some(y)) => Some(y.clone()),
        _ => None,
    };
    IndependentReaderResult {
        status,
        reason,
        tool: a.tool.clone().or_else(|| b.tool.clone()),
        version: a.version.clone().or_else(|| b.version.clone()),
        // Sum counts across volumes when both Ok; else keep first present.
        message_count: match (a.message_count, b.message_count) {
            (Some(x), Some(y)) => Some(x.saturating_add(y)),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            _ => None,
        },
        folder_count: match (a.folder_count, b.folder_count) {
            (Some(x), Some(y)) => Some(x.saturating_add(y)),
            (Some(x), None) => Some(x),
            (None, Some(y)) => Some(y),
            _ => None,
        },
        exit_code: b.exit_code.or(a.exit_code),
    }
}

fn merge_scanpst(a: &ScanpstResult, b: &ScanpstResult) -> ScanpstResult {
    let status = worse_external_status(a.status.clone(), b.status.clone());
    let reason = match (&a.reason, &b.reason) {
        (Some(x), Some(y)) if x != y => Some(format!("{x}; {y}")),
        (Some(x), _) => Some(x.clone()),
        (_, Some(y)) => Some(y.clone()),
        _ => None,
    };
    ScanpstResult {
        status,
        reason,
        build: a.build.clone().or_else(|| b.build.clone()),
        log_path: b.log_path.clone().or_else(|| a.log_path.clone()),
        bak_present: a.bak_present || b.bak_present,
        log_summary: match (&a.log_summary, &b.log_summary) {
            (Some(x), Some(y)) if x != y => Some(format!("{x}; {y}")),
            (Some(x), _) => Some(x.clone()),
            (_, Some(y)) => Some(y.clone()),
            _ => None,
        },
        hard_error: a.hard_error || b.hard_error,
    }
}

/// Worst-status ranking for multi-volume external aggregates.
///
/// Order: **Failed > Timeout > Skipped > Ok**.
/// A volume that was not checked (`Skipped`) must not green-wash aggregate `Ok`.
fn worse_external_status(a: ExternalStatus, b: ExternalStatus) -> ExternalStatus {
    use ExternalStatus::*;
    let rank = |s: &ExternalStatus| match s {
        Failed => 4,
        Timeout => 3,
        Skipped => 2,
        Ok => 1,
    };
    if rank(&b) > rank(&a) {
        b
    } else {
        a
    }
}

struct MsgCompareResult {
    findings: Vec<QcFinding>,
    attachments_compared: u64,
    skipped_source: bool,
    digest_entry: Option<ContentDigestEntry>,
}

/// How a source attachment matched an output multiset slot.
enum MatchKind {
    Exact,
    HashOnly,
    HashMismatch(String),
}

/// Fail closed when `export_messages.csv` is missing/short/duplicated/orphaned vs volumes.
///
/// Strict membership rules (DoD-3/6/21):
/// - every export row `volume_index` must be in the declared volume set (orphan ⇒ defect)
/// - global `export_message_index` unique
/// - per-volume row count exact-match `messages_written`
/// - per-volume `export_message_index` set unique (also covered by global unique)
fn validate_export_metadata_coverage(
    volumes: &[VolumeReportRow],
    export_rows: &[ExportMessageRow],
    contract: &FidelityContract,
    counts: &mut QcFindingCounts,
    findings_list: &mut Vec<QcFinding>,
) {
    let total_written: u64 = volumes.iter().map(|v| v.messages_written).sum();
    if total_written == 0 && volumes.iter().all(|v| v.messages_written == 0) {
        // Zero-winner / empty export: empty CSV is OK only when no orphan rows either.
        if export_rows.is_empty() {
            return;
        }
        // Rows present while all volumes claim zero messages ⇒ defect (orphan / stale CSV).
        counts.record(FindingClass::Defect);
        findings_list.push(QcFinding {
            class: FindingClass::Defect,
            property: "export_messages_orphan_rows".into(),
            volume_index: 0,
            source_path: String::new(),
            source_nid: 0,
            message_id_norm: String::new(),
            detail: format!(
                "export_messages.csv has {} row(s) but volumes report messages_written_total=0",
                export_rows.len()
            ),
        });
        return;
    }

    if total_written > 0 && export_rows.is_empty() {
        counts.record(FindingClass::Defect);
        findings_list.push(QcFinding {
            class: FindingClass::Defect,
            property: "export_messages_missing".into(),
            volume_index: 0,
            source_path: String::new(),
            source_nid: 0,
            message_id_norm: String::new(),
            detail: format!(
                "export_messages.csv missing or empty while volumes report messages_written_total={total_written}"
            ),
        });
        return;
    }

    let declared_vols: BTreeSet<u32> = volumes.iter().map(|v| v.volume_index).collect();

    // Every export row must reference a declared volume (orphan volume_index ⇒ defect).
    // Without this, wrong-volume-index rows can leave per-volume counts matching while
    // unclaimed messages are never compared in the volume loop (false green).
    let mut orphan_vols: BTreeSet<u32> = BTreeSet::new();
    for r in export_rows {
        if !declared_vols.contains(&r.volume_index) {
            orphan_vols.insert(r.volume_index);
        }
    }
    if !orphan_vols.is_empty() {
        counts.record(FindingClass::Defect);
        findings_list.push(QcFinding {
            class: FindingClass::Defect,
            property: "export_messages_orphan_volume_index".into(),
            volume_index: 0,
            source_path: String::new(),
            source_nid: 0,
            message_id_norm: String::new(),
            detail: format!(
                "export_messages.csv row(s) reference volume_index not in declared volumes {:?}: orphan={:?}",
                declared_vols.iter().collect::<Vec<_>>(),
                orphan_vols.iter().collect::<Vec<_>>()
            ),
        });
    }

    // Duplicate export_message_index ⇒ defect (join key must be unique globally).
    let mut seen_idx: BTreeSet<u64> = BTreeSet::new();
    let mut dups: Vec<u64> = Vec::new();
    for r in export_rows {
        if !seen_idx.insert(r.export_message_index) {
            dups.push(r.export_message_index);
        }
    }
    if !dups.is_empty() {
        counts.record(FindingClass::Defect);
        findings_list.push(QcFinding {
            class: FindingClass::Defect,
            property: "export_message_index_duplicate".into(),
            volume_index: 0,
            source_path: String::new(),
            source_nid: 0,
            message_id_norm: String::new(),
            detail: format!(
                "duplicate export_message_index values in export_messages.csv: {:?}",
                dups.iter().take(8).collect::<Vec<_>>()
            ),
        });
    }

    // Per-volume: exact row count match AND unique export_message_index set within volume.
    for vol in volumes {
        let vol_rows: Vec<&ExportMessageRow> = export_rows
            .iter()
            .filter(|r| r.volume_index == vol.volume_index)
            .collect();
        let row_count = vol_rows.len() as u64;
        if row_count != vol.messages_written {
            let (class, _) = contract.classify("message_content_digest", false);
            let _ = class;
            counts.record(FindingClass::Defect);
            findings_list.push(QcFinding {
                class: FindingClass::Defect,
                property: "export_messages_row_count".into(),
                volume_index: vol.volume_index,
                source_path: String::new(),
                source_nid: 0,
                message_id_norm: String::new(),
                detail: format!(
                    "export_messages.csv rows for volume {} = {row_count}, messages_written = {}",
                    vol.volume_index, vol.messages_written
                ),
            });
        }
        let mut vol_idxs: BTreeSet<u64> = BTreeSet::new();
        let mut vol_dups: Vec<u64> = Vec::new();
        for r in &vol_rows {
            if !vol_idxs.insert(r.export_message_index) {
                vol_dups.push(r.export_message_index);
            }
        }
        if !vol_dups.is_empty() {
            counts.record(FindingClass::Defect);
            findings_list.push(QcFinding {
                class: FindingClass::Defect,
                property: "export_message_index_duplicate_in_volume".into(),
                volume_index: vol.volume_index,
                source_path: String::new(),
                source_nid: 0,
                message_id_norm: String::new(),
                detail: format!(
                    "duplicate export_message_index within volume {}: {:?}",
                    vol.volume_index,
                    vol_dups.iter().take(8).collect::<Vec<_>>()
                ),
            });
        }
    }
}

fn normalize_mid_key(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_ascii_lowercase()
}

struct CompareOneArgs<'a> {
    cand: &'a QcSampleCandidate,
    handles: &'a mut PstHandleCache,
    out_index: &'a mut OutputMessageIndex,
    out_path: &'a Path,
    source_differential: bool,
    existing: Option<&'a ContentDigestsFile>,
    contract: &'a FidelityContract,
    parents_only: bool,
}

fn compare_one_message(args: CompareOneArgs<'_>) -> MsgCompareResult {
    let CompareOneArgs {
        cand,
        handles,
        out_index,
        out_path,
        source_differential,
        existing,
        contract,
        parents_only,
    } = args;
    let mut findings = Vec::new();
    let mut attachments_compared = 0u64;
    let mut skipped_source = false;
    // Clean-room digest entry for this candidate (fidelity flags + field payload).
    let digest_for_cand: Option<&ContentDigestEntry> = existing.and_then(|digests| {
        digests
            .volumes
            .iter()
            .flat_map(|v| v.messages.iter())
            .find(|m| m.export_message_index == cand.export_message_index)
    });

    // Resolve source-side detail
    let source_detail: Option<MessageContentDetail> = if source_differential {
        let src_path = Path::new(&cand.source_path);
        let path_missing = !src_path.is_file();
        match handles.get_mut(&cand.source_path) {
            Ok(pst) => match message_content_detail(pst, cand.source_nid) {
                Ok(d) => Some(d),
                Err(e) => {
                    // Path openable but message read/parse failed ⇒ hard fail (not Explained skip).
                    let (class, _) = contract.classify("message_content_digest", false);
                    findings.push(QcFinding {
                        class,
                        property: "source_read".into(),
                        volume_index: cand.volume_index,
                        source_path: cand.source_path.clone(),
                        source_nid: cand.source_nid,
                        message_id_norm: cand.message_id_norm.clone(),
                        detail: format!("source_read_failed: {e}"),
                    });
                    None
                }
            },
            Err(e) => {
                if path_missing {
                    // Missing source path ⇒ explained skip (sources gone).
                    skipped_source = true;
                    findings.push(QcFinding {
                        class: FindingClass::Explained,
                        property: "source_open".into(),
                        volume_index: cand.volume_index,
                        source_path: cand.source_path.clone(),
                        source_nid: cand.source_nid,
                        message_id_norm: cand.message_id_norm.clone(),
                        detail: format!("skipped_source_unavailable: {e}"),
                    });
                } else {
                    // Path exists but open/parse failed ⇒ hard finding.
                    findings.push(QcFinding {
                        class: FindingClass::Defect,
                        property: "source_open".into(),
                        volume_index: cand.volume_index,
                        source_path: cand.source_path.clone(),
                        source_nid: cand.source_nid,
                        message_id_norm: cand.message_id_norm.clone(),
                        detail: format!("source_open_failed: {e}"),
                    });
                }
                None
            }
        }
    } else if let Some(file) = existing {
        // Content-digest-backed clean room
        file.volumes
            .iter()
            .flat_map(|v| v.messages.iter())
            .find(|m| {
                m.export_message_index == cand.export_message_index
                    || (!cand.message_id_norm.is_empty()
                        && m.message_id_norm == cand.message_id_norm)
            })
            .map(|m| {
                // Production unexplained_loss path (DoD-9): extras on digest entry.
                for prop in &m.extra_source_props {
                    if prop.trim().is_empty() {
                        continue;
                    }
                    let (class, _) = contract.classify(prop, false);
                    findings.push(QcFinding {
                        class,
                        property: prop.clone(),
                        volume_index: cand.volume_index,
                        source_path: cand.source_path.clone(),
                        source_nid: cand.source_nid,
                        message_id_norm: cand.message_id_norm.clone(),
                        detail: format!(
                            "extra_source_prop '{prop}' classified via content digest production path"
                        ),
                    });
                }
                MessageContentDetail {
                    digest: m.content_digest.clone(),
                    message_id: m.message_id_norm.clone(),
                    // Prefer persisted field-level data so clean-room under parents_only
                    // can body-match (DoD-21); never silently zero these when present.
                    subject: m.subject.clone(),
                    sender: m.sender.clone(),
                    display_to: m.display_to.clone(),
                    display_cc: m.display_cc.clone(),
                    body_plain_len: m.body_plain_len,
                    body_html_len: m.body_html_len,
                    attaches: m
                        .attaches
                        .iter()
                        .map(|a| {
                            (
                                a.filename.clone(),
                                a.size,
                                String::new(),
                                a.payload_sha256.clone(),
                            )
                        })
                        .collect(),
                    attach_list_error: None,
                }
            })
    } else {
        None
    };

    let Some(src) = source_detail else {
        // Still try to consume an output slot? No — without source we cannot compare.
        return MsgCompareResult {
            findings,
            attachments_compared,
            skipped_source,
            digest_entry: None,
        };
    };

    // Output side: MID first, then no-MID multimap scored by subject/body/digest (consume).
    let out_detail = out_index.take_match(
        &cand.message_id_norm,
        &cand.subject,
        cand.source_nid,
        Some(src.digest.as_str()),
        Some(src.body_plain_len),
    );

    let digest_entry = ContentDigestEntry {
        export_message_index: cand.export_message_index,
        source_path: cand.source_path.clone(),
        source_nid: cand.source_nid,
        message_id_norm: cand.message_id_norm.clone(),
        content_digest: src.digest.clone(),
        subject: src.subject.clone(),
        sender: src.sender.clone(),
        display_to: src.display_to.clone(),
        display_cc: src.display_cc.clone(),
        body_plain_len: src.body_plain_len,
        body_html_len: src.body_html_len,
        attaches: src
            .attaches
            .iter()
            .map(|(f, s, _, h)| AttachDigestEntry {
                filename: f.clone(),
                size: *s,
                payload_sha256: h.clone(),
            })
            .collect(),
        // Export path always empty; production extras only via crafted digests / tests.
        extra_source_props: Vec::new(),
        // Persist fidelity flags so clean-room qc-pst can explain soft-fails (DoD-21).
        has_degraded: cand.has_degraded,
        body_unavailable: cand.body_unavailable,
        body_incomplete: cand.body_incomplete,
        crc_suspect: cand.crc_suspect,
        has_ledger_fail: cand.has_ledger_fail,
        ledger_failed_attach_names: cand.ledger_failed_attach_names.clone(),
    };

    let Some(out) = out_detail else {
        let (class, _) = contract.classify("message_content_digest", false);
        findings.push(QcFinding {
            class,
            property: "message_content_digest".into(),
            volume_index: cand.volume_index,
            source_path: cand.source_path.clone(),
            source_nid: cand.source_nid,
            message_id_norm: cand.message_id_norm.clone(),
            detail: format!("message missing in output (mid={})", cand.message_id_norm),
        });
        return MsgCompareResult {
            findings,
            attachments_compared,
            skipped_source,
            digest_entry: Some(digest_entry),
        };
    };

    // CC is preserved: never explained by attach soft-fail, body flags, or generic degradation.
    if !src.display_cc.is_empty()
        && normalize_display_addr(&src.display_cc) != normalize_display_addr(&out.display_cc)
    {
        let (class, _) = contract.classify("PidTagDisplayCc", false);
        findings.push(QcFinding {
            class,
            property: "PidTagDisplayCc".into(),
            volume_index: cand.volume_index,
            source_path: cand.source_path.clone(),
            source_nid: cand.source_nid,
            message_id_norm: cand.message_id_norm.clone(),
            detail: format!("cc src={:?} out={:?}", src.display_cc, out.display_cc),
        });
    }

    // Attachment list failure must not look like empty attaches.
    if let Some(ref err) = src.attach_list_error {
        findings.push(QcFinding {
            class: FindingClass::Defect,
            property: "attachment_list".into(),
            volume_index: cand.volume_index,
            source_path: cand.source_path.clone(),
            source_nid: cand.source_nid,
            message_id_norm: cand.message_id_norm.clone(),
            detail: format!("source attachment list failed: {err}"),
        });
    }

    // Attachment payload hashes (skip when parents_only / policy omit).
    if parents_only {
        if !src.attaches.is_empty() {
            // Policy omit is explained — count as explained when attaches exist on source.
            findings.push(QcFinding {
                class: FindingClass::Explained,
                property: "attachment_omitted_by_policy".into(),
                volume_index: cand.volume_index,
                source_path: cand.source_path.clone(),
                source_nid: cand.source_nid,
                message_id_norm: cand.message_id_norm.clone(),
                detail: format!(
                    "parents_only: {} source attach(es) not written (policy)",
                    src.attaches.len()
                ),
            });
        }
    } else if src.attach_list_error.is_none() {
        // Multiset of output attaches keyed by lowercase filename. Duplicate names
        // must not collapse: each source attach consumes exactly one output slot
        // (prefer filename+size+hash, then filename+hash, then filename+size).
        let mut out_attach_pool: BTreeMap<String, Vec<(u64, String, bool)>> = BTreeMap::new();
        for (f, sz, _, h) in &out.attaches {
            out_attach_pool
                .entry(f.to_ascii_lowercase())
                .or_default()
                .push((*sz, h.clone(), false));
        }
        for (fnm, sz, _, ph) in &src.attaches {
            attachments_compared = attachments_compared.saturating_add(1);
            let fnm_key = fnm.to_ascii_lowercase();
            if ph.is_empty() {
                // Empty source hash: only filename-specific ledger fail explains.
                let explained = attach_ledger_explains_effective(cand, digest_for_cand, fnm);
                let (class, _) = contract.classify("attachment_stream_soft_fail", explained);
                findings.push(QcFinding {
                    class,
                    property: "attachment_stream_soft_fail".into(),
                    volume_index: cand.volume_index,
                    source_path: cand.source_path.clone(),
                    source_nid: cand.source_nid,
                    message_id_norm: cand.message_id_norm.clone(),
                    detail: if explained {
                        format!("source attach {fnm} empty hash (ledger soft-fail)")
                    } else {
                        format!("source attach {fnm} has empty payload hash")
                    },
                });
                continue;
            }
            let pool = out_attach_pool.get_mut(&fnm_key);
            let consumed = pool.and_then(|slots| {
                // Prefer exact size+hash, then hash-only, then size-only (hash mismatch).
                let exact = slots.iter().position(|(osz, oh, used)| {
                    !*used && *osz == *sz && oh.as_str() == ph.as_str()
                });
                if let Some(i) = exact {
                    slots[i].2 = true;
                    return Some(MatchKind::Exact);
                }
                let hash_only = slots
                    .iter()
                    .position(|(_, oh, used)| !*used && oh.as_str() == ph.as_str());
                if let Some(i) = hash_only {
                    slots[i].2 = true;
                    return Some(MatchKind::HashOnly);
                }
                let size_only = slots
                    .iter()
                    .position(|(osz, _, used)| !*used && *osz == *sz);
                if let Some(i) = size_only {
                    let out_ph = slots[i].1.clone();
                    slots[i].2 = true;
                    return Some(MatchKind::HashMismatch(out_ph));
                }
                let any = slots.iter().position(|(_, _, used)| !*used);
                if let Some(i) = any {
                    let out_ph = slots[i].1.clone();
                    slots[i].2 = true;
                    return Some(MatchKind::HashMismatch(out_ph));
                }
                None
            });
            match consumed {
                Some(MatchKind::Exact | MatchKind::HashOnly) => {}
                Some(MatchKind::HashMismatch(out_ph)) => {
                    let (class, _) = contract.classify("attachment_payload_sha256", false);
                    findings.push(QcFinding {
                        class,
                        property: "attachment_payload_sha256".into(),
                        volume_index: cand.volume_index,
                        source_path: cand.source_path.clone(),
                        source_nid: cand.source_nid,
                        message_id_norm: cand.message_id_norm.clone(),
                        detail: format!("attach {fnm} size={sz} src_sha={ph} out_sha={out_ph}"),
                    });
                }
                None => {
                    // Missing in output: explain only when **this filename** is in the
                    // attach-ledger fail set (never message-wide has_ledger_fail alone).
                    // Clean-room: also honor ledger names persisted on content digests.
                    if attach_ledger_explains_effective(cand, digest_for_cand, fnm) {
                        let (class, _) = contract.classify("attachment_stream_soft_fail", true);
                        findings.push(QcFinding {
                            class,
                            property: "attachment_stream_soft_fail".into(),
                            volume_index: cand.volume_index,
                            source_path: cand.source_path.clone(),
                            source_nid: cand.source_nid,
                            message_id_norm: cand.message_id_norm.clone(),
                            detail: format!(
                                "attach {fnm} missing in output (ledger soft-fail explained)"
                            ),
                        });
                    } else {
                        let (class, _) = contract.classify("attachment_by_value", false);
                        findings.push(QcFinding {
                            class,
                            property: "attachment_by_value".into(),
                            volume_index: cand.volume_index,
                            source_path: cand.source_path.clone(),
                            source_nid: cand.source_nid,
                            message_id_norm: cand.message_id_norm.clone(),
                            detail: format!("attach {fnm} missing in output (multiset exhausted)"),
                        });
                    }
                }
            }
        }
        // Unexpected / unconsumed output attaches (extra multiset entries).
        for (f, sz, _, h) in &out.attaches {
            let key = f.to_ascii_lowercase();
            let still_free = out_attach_pool
                .get(&key)
                .map(|slots| {
                    slots
                        .iter()
                        .any(|(osz, oh, used)| !*used && *osz == *sz && oh.as_str() == h.as_str())
                })
                .unwrap_or(false);
            if still_free {
                // Mark one free slot consumed for reporting so we don't double-count.
                if let Some(slots) = out_attach_pool.get_mut(&key) {
                    if let Some(slot) = slots
                        .iter_mut()
                        .find(|(osz, oh, used)| !*used && *osz == *sz && oh.as_str() == h.as_str())
                    {
                        slot.2 = true;
                    }
                }
                let (class, _) = contract.classify("attachment_by_value", false);
                findings.push(QcFinding {
                    class,
                    property: "attachment_unexpected_output".into(),
                    volume_index: cand.volume_index,
                    source_path: cand.source_path.clone(),
                    source_nid: cand.source_nid,
                    message_id_norm: cand.message_id_norm.clone(),
                    detail: format!("unexpected/unconsumed output attach {f}"),
                });
            }
        }
    }

    // Field-level body/recipient checks.
    // Body explain flags: body_unavailable | body_incomplete | crc_suspect only.
    // Never: generic has_degraded alone, attach ledger, or any of the above for CC.
    let subject_match = src.subject.eq_ignore_ascii_case(&out.subject);
    let sender_match = normalize_display_addr(&src.sender) == normalize_display_addr(&out.sender);
    let to_match =
        normalize_display_addr(&src.display_to) == normalize_display_addr(&out.display_to);
    let cc_match =
        normalize_display_addr(&src.display_cc) == normalize_display_addr(&out.display_cc);
    let body_len_match =
        src.body_plain_len == out.body_plain_len && src.body_html_len == out.body_html_len;
    let digest_match = src.digest == out.digest;

    if !subject_match {
        let (class, _) = contract.classify("PidTagSubject", false);
        findings.push(QcFinding {
            class,
            property: "PidTagSubject".into(),
            volume_index: cand.volume_index,
            source_path: cand.source_path.clone(),
            source_nid: cand.source_nid,
            message_id_norm: cand.message_id_norm.clone(),
            detail: format!("subject src={:?} out={:?}", src.subject, out.subject),
        });
    }
    if !sender_match && !src.sender.is_empty() {
        let (class, _) = contract.classify("PidTagSenderEmailAddress", false);
        findings.push(QcFinding {
            class,
            property: "PidTagSenderEmailAddress".into(),
            volume_index: cand.volume_index,
            source_path: cand.source_path.clone(),
            source_nid: cand.source_nid,
            message_id_norm: cand.message_id_norm.clone(),
            detail: format!("sender src={:?} out={:?}", src.sender, out.sender),
        });
    }
    if !to_match && !src.display_to.is_empty() {
        // Empty-vs-nonempty or material address change only (quotes already normalized).
        let (class, _) = contract.classify("PidTagDisplayTo", false);
        findings.push(QcFinding {
            class,
            property: "PidTagDisplayTo".into(),
            volume_index: cand.volume_index,
            source_path: cand.source_path.clone(),
            source_nid: cand.source_nid,
            message_id_norm: cand.message_id_norm.clone(),
            detail: format!("to src={:?} out={:?}", src.display_to, out.display_to),
        });
    }
    let body_explained = body_loss_explained_with_digest(cand, digest_for_cand);
    let body_unavail_flag =
        cand.body_unavailable || digest_for_cand.is_some_and(|d| d.body_unavailable);
    let body_incompl_flag =
        cand.body_incomplete || digest_for_cand.is_some_and(|d| d.body_incomplete);
    let crc_flag = cand.crc_suspect || digest_for_cand.is_some_and(|d| d.crc_suspect);

    if !body_len_match {
        if body_explained {
            let prop = if body_unavail_flag {
                "body_unavailable"
            } else {
                "body_plain"
            };
            let (class, _) = contract.classify(
                if body_unavail_flag {
                    "body_unavailable"
                } else {
                    // Best-effort path: treat as explained soft body under incomplete/CRC.
                    "body_unavailable"
                },
                true,
            );
            findings.push(QcFinding {
                class,
                property: prop.into(),
                volume_index: cand.volume_index,
                source_path: cand.source_path.clone(),
                source_nid: cand.source_nid,
                message_id_norm: cand.message_id_norm.clone(),
                detail: format!(
                    "body lengths differ under body fidelity flag (explained) src_plain={} out_plain={} flags=bu:{} bi:{} crc:{}",
                    src.body_plain_len,
                    out.body_plain_len,
                    body_unavail_flag,
                    body_incompl_flag,
                    crc_flag
                ),
            });
        } else {
            let (class, _) = contract.classify("body_plain", false);
            findings.push(QcFinding {
                class,
                property: "body_plain".into(),
                volume_index: cand.volume_index,
                source_path: cand.source_path.clone(),
                source_nid: cand.source_nid,
                message_id_norm: cand.message_id_norm.clone(),
                detail: format!(
                    "body length mismatch src_plain={} out_plain={} src_html={} out_html={}",
                    src.body_plain_len, out.body_plain_len, src.body_html_len, out.body_html_len
                ),
            });
        }
    }

    if parents_only {
        // Full digest includes attaches; field checks above cover body/recipients.
    } else if !digest_match {
        // Digest mismatch: explain only with body-specific flags when non-body fields match.
        let non_body_ok = subject_match && to_match && cc_match;
        if body_explained && non_body_ok {
            let (class, _) = contract.classify("body_unavailable", true);
            findings.push(QcFinding {
                class,
                property: "body_unavailable".into(),
                volume_index: cand.volume_index,
                source_path: cand.source_path.clone(),
                source_nid: cand.source_nid,
                message_id_norm: cand.message_id_norm.clone(),
                detail: format!(
                    "content digest mismatch under body fidelity flag (explained) src={} out={}",
                    src.digest, out.digest
                ),
            });
        } else {
            let (class, _) = contract.classify("message_content_digest", false);
            findings.push(QcFinding {
                class,
                property: "message_content_digest".into(),
                volume_index: cand.volume_index,
                source_path: cand.source_path.clone(),
                source_nid: cand.source_nid,
                message_id_norm: cand.message_id_norm.clone(),
                detail: format!(
                    "content digest mismatch src={} out={}",
                    src.digest, out.digest
                ),
            });
        }
    }

    let _ = out_path;
    MsgCompareResult {
        findings,
        attachments_compared,
        skipped_source,
        digest_entry: Some(digest_entry),
    }
}

/// Indexed output messages for QC matching (MID map + no-MID multimap).
///
/// No-MID messages with duplicate subjects must not overwrite one another (DoD-6).
/// Matching consumes an entry so two same-subject messages cannot share one output.
struct OutputMessageIndex {
    by_mid: BTreeMap<String, MessageContentDetail>,
    /// Remaining unmatched no-MID messages: (output_nid, detail).
    no_mid: Vec<(u64, MessageContentDetail)>,
}

impl OutputMessageIndex {
    fn take_match(
        &mut self,
        message_id_norm: &str,
        subject: &str,
        source_nid: u64,
        prefer_digest: Option<&str>,
        prefer_body_plain: Option<usize>,
    ) -> Option<MessageContentDetail> {
        let mid_key = normalize_mid_key(message_id_norm);
        if !mid_key.is_empty() {
            if let Some(d) = self.by_mid.remove(&mid_key) {
                return Some(d);
            }
            let hit_key = self
                .by_mid
                .iter()
                .find(|(_, d)| normalize_mid_key(&d.message_id) == mid_key)
                .map(|(k, _)| k.clone());
            if let Some(k) = hit_key {
                return self.by_mid.remove(&k);
            }
        }

        // No-MID (or MID miss): score remaining no_mid entries.
        if self.no_mid.is_empty() {
            return None;
        }
        let subj_l = subject.to_ascii_lowercase();
        let mut best_i: Option<usize> = None;
        let mut best_score: i32 = i32::MIN;
        for (i, (out_nid, d)) in self.no_mid.iter().enumerate() {
            let mut score = 0i32;
            if !subj_l.is_empty() && d.subject.eq_ignore_ascii_case(subject) {
                score += 100;
            } else if !subj_l.is_empty() {
                continue; // subject required when provided
            }
            if source_nid != 0 && *out_nid == source_nid {
                score += 50;
            }
            if let Some(dig) = prefer_digest {
                if !dig.is_empty() && d.digest == dig {
                    score += 40;
                }
            }
            if let Some(bp) = prefer_body_plain {
                if d.body_plain_len == bp {
                    score += 20;
                }
            }
            if score > best_score {
                best_score = score;
                best_i = Some(i);
            }
        }
        // Require at least subject match (or empty subject + nid/digest).
        if best_score < 50 && !subject.is_empty() {
            // subject-only is enough (score 100); below 50 means no subject hit
            if best_score < 100 {
                return None;
            }
        }
        best_i.map(|i| self.no_mid.remove(i).1)
    }
}

fn index_output_messages(path: &Path) -> OutputMessageIndex {
    let mut by_mid = BTreeMap::new();
    let mut no_mid = Vec::new();
    let Ok(mut pst) = pst_reader::PstFile::open(path) else {
        return OutputMessageIndex { by_mid, no_mid };
    };
    let Ok(folders) = pst.folders() else {
        return OutputMessageIndex { by_mid, no_mid };
    };
    for folder in &folders {
        for &nid in &folder.message_nids {
            if let Ok(detail) = message_content_detail(&mut pst, nid.0) {
                let key = normalize_mid_key(&detail.message_id);
                if !key.is_empty() {
                    by_mid.insert(key, detail);
                } else {
                    // Multimap: unique by output nid so duplicate subjects never overwrite.
                    no_mid.push((nid.0, detail));
                }
            }
        }
    }
    OutputMessageIndex { by_mid, no_mid }
}

/// Normalize folder path for case-insensitive segment comparison.
fn normalize_folder_key(p: &str) -> String {
    p.trim()
        .trim_matches(|c| c == '/' || c == '\\')
        .replace('\\', "/")
        .to_ascii_lowercase()
}

/// True when `out_path` equals `leaf` or ends with `leaf` as a full path-segment suffix.
///
/// Rejects ancestor-only / intermediate-segment false positives from
/// `contains("/leaf/")` (e.g. expected `Inbox/Sub` must not match bare `Inbox`).
fn folder_leaf_matches(out_path: &str, leaf: &str) -> bool {
    let out = normalize_folder_key(out_path);
    let leaf = normalize_folder_key(leaf);
    if leaf.is_empty() {
        return true;
    }
    if out.is_empty() {
        return false;
    }
    out == leaf || out.ends_with(&format!("/{leaf}"))
}

/// Residual catch-all folder used by flat / residual routing (not wholesale collapse).
fn is_residual_unique_mail(path: &str) -> bool {
    let n = normalize_folder_key(path);
    n == "unique mail" || n.ends_with("/unique mail")
}

/// Every expected leaf folder must match an output path (exact or path-segment
/// suffix, case-insensitive) **with equal per-folder message counts**.
/// Presence alone is not enough: same leaves with redistributed counts fail.
///
/// After expected leaves claim output slots, any **remaining** non-system output
/// folder that still has messages is unclaimed → fail (DoD-4: no silent extra
/// folders with mail).
///
/// **Residual Unique Mail allowance** (documented only): an expected path that
/// *is itself* residual Unique Mail may match an output Unique Mail folder when
/// counts agree. Wholesale collapse (multi-leaf expected → single residual) fails.
fn folder_tree_matches(
    digest: &VolumeStructuralDigest,
    expected_folder_counts: &BTreeMap<String, u64>,
    expected_count: u64,
) -> bool {
    if expected_count == 0 {
        return digest.message_count == 0;
    }
    if digest.message_count != expected_count {
        return false;
    }
    if expected_folder_counts.is_empty() {
        // No expected folder rows (metadata incomplete) — do not green-pass
        // multi-folder output as matched; message_count alone already checked.
        // Unclaimed check below still applies when expected is empty: any
        // message-bearing output folder is unclaimed.
        let any_mail_folder = digest
            .folder_paths
            .iter()
            .zip(
                digest
                    .folder_message_counts
                    .iter()
                    .copied()
                    .chain(std::iter::repeat(0)),
            )
            .any(|(p, c)| {
                let n = normalize_folder_key(p);
                c > 0 && !n.is_empty() && !is_system_folder_path(&n)
            });
        return !any_mail_folder;
    }

    // Available output folders with message counts.
    // Multi-source prefixes may split one logical leaf across several out paths;
    // sum counts across all path-segment suffix matches (exclusive claim).
    let mut out_slots: Vec<(String, u64)> = digest
        .folder_paths
        .iter()
        .zip(
            digest
                .folder_message_counts
                .iter()
                .copied()
                .chain(std::iter::repeat(0)),
        )
        .map(|(p, c)| (normalize_folder_key(p), c))
        .filter(|(p, c)| !p.is_empty() && *c > 0 && !is_system_folder_path(p))
        .collect();

    // Longest expected leaves first so "Inbox/Sub" claims before "Inbox".
    let mut expected_ordered: Vec<(&String, u64)> = expected_folder_counts
        .iter()
        .map(|(k, v)| (k, *v))
        .collect();
    expected_ordered.sort_by_key(|(leaf, _)| std::cmp::Reverse(leaf.len()));

    for (leaf, exp_count) in expected_ordered {
        if leaf.trim().is_empty() {
            continue;
        }
        let mut matched_total = 0u64;
        let mut claimed: Vec<usize> = Vec::new();
        for (i, (p, c)) in out_slots.iter().enumerate() {
            let path_ok = folder_leaf_matches(p, leaf)
                || (is_residual_unique_mail(leaf) && is_residual_unique_mail(p));
            if path_ok {
                matched_total = matched_total.saturating_add(*c);
                claimed.push(i);
            }
        }
        if matched_total != exp_count {
            return false;
        }
        // Remove claimed slots (highest index first) so they cannot satisfy another leaf.
        for i in claimed.into_iter().rev() {
            out_slots.remove(i);
        }
    }
    // Unclaimed message-bearing output folders ⇒ tree mismatch.
    out_slots.is_empty()
}

/// System / non-content folders that may appear with zero user expectation.
fn is_system_folder_path(normalized: &str) -> bool {
    let n = normalized.trim_matches('/');
    matches!(
        n,
        "" | "ipm_subtree"
            | "top of personal folders"
            | "to-do search"
            | "search root"
            | "deleted items"
            | "finder"
    ) || n.ends_with("/deleted items")
        || n.ends_with("/search root")
}

/// Source-origin digests only enable content_digest_backed (DoD-21).
fn content_digests_are_source_origin(file: &ContentDigestsFile) -> bool {
    let origin = file.origin.trim().to_ascii_lowercase();
    if origin == CONTENT_DIGEST_ORIGIN_OUTPUT {
        return false;
    }
    if origin == CONTENT_DIGEST_ORIGIN_SOURCE {
        return true;
    }
    // Legacy files without origin: accept only when entries carry source_path
    // (export-time source digests). Output-only stubs had empty source_path.
    file.volumes
        .iter()
        .flat_map(|v| v.messages.iter())
        .any(|m| !m.source_path.trim().is_empty())
}

fn load_content_digests(path: &Path) -> Option<ContentDigestsFile> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_qc_report(report_dir: &Path, report: &QcReportV1) -> Result<(), String> {
    fs::create_dir_all(report_dir).map_err(|e| e.to_string())?;
    let path = report_dir.join("qc_report_v1.json");
    let json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

fn write_qc_findings_csv(report_dir: &Path, findings: &[QcFinding]) -> Result<(), String> {
    fs::create_dir_all(report_dir).map_err(|e| e.to_string())?;
    let path = report_dir.join("qc_findings.csv");
    let mut f = fs::File::create(&path).map_err(|e| e.to_string())?;
    writeln!(
        f,
        "class,property,volume_index,source_path,source_nid,message_id_norm,detail"
    )
    .map_err(|e| e.to_string())?;
    for row in findings {
        let class = match row.class {
            FindingClass::Defect => "defect",
            FindingClass::UnexplainedLoss => "unexplained_loss",
            FindingClass::KnownGap => "known_gap",
            FindingClass::Explained => "explained",
        };
        writeln!(
            f,
            "{class},{},{},{},{:#x},{},{}",
            csv_escape(&row.property),
            row.volume_index,
            csv_escape(&row.source_path),
            row.source_nid,
            csv_escape(&row.message_id_norm),
            csv_escape(&row.detail),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_content_digests(report_dir: &Path, digests: &ContentDigestsFile) -> Result<(), String> {
    let path = report_dir.join("content_digests.json");
    let json = serde_json::to_string_pretty(digests).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Standalone `qc-pst` entry: re-run QC on an existing pack.
pub fn run_qc_pst(
    out_pst: &Path,
    report_dir: &Path,
    level: QcLevel,
    sample_max: usize,
    external_reader: Option<&Path>,
    run_scanpst: bool,
    max_open_psts: usize,
) -> Result<QcReportV1, String> {
    if level == QcLevel::Off {
        return Err("qc-pst requires --qc-level other than off".into());
    }
    // Load volumes from summary or invent single volume; honor positional out.pst.
    let volumes = load_volumes_for_qc(report_dir, out_pst)?;
    let mut export_rows = load_export_rows_for_qc(report_dir)?;
    // After basename handoff, basenamed `source_path` may not open; prefer
    // `summary.inputs[source_id]` when the CSV path is missing (0081 P2-B).
    resolve_export_source_paths_from_summary(report_dir, &mut export_rows);
    // Hydrate subjects from content_digests (export CSV has no subject column).
    hydrate_export_subjects_from_digests(report_dir, &mut export_rows);
    let mut candidates = candidates_from_export_and_meta(&export_rows, &BTreeMap::new());
    hydrate_candidates_from_digests(report_dir, &mut candidates);

    // Source differential if any source path still exists.
    let source_differential = export_rows
        .iter()
        .any(|r| Path::new(&r.source_path).is_file());

    // Honor parents_only when summary/digests indicate no-attachments export.
    let parents_only = load_parents_only_for_qc(report_dir);

    Ok(run_unique_pst_qc(QcRunInput {
        level,
        sample_max,
        report_dir,
        volumes: &volumes,
        export_rows: &export_rows,
        candidates: &candidates,
        external_reader,
        run_scanpst,
        max_open_psts,
        source_differential,
        parents_only,
        probe_unexplained_property: None,
    }))
}

/// Fill empty export-row subjects from source-origin content digests.
fn hydrate_export_subjects_from_digests(report_dir: &Path, rows: &mut [ExportMessageRow]) {
    let Some(digests) = load_content_digests(&report_dir.join("content_digests.json")) else {
        return;
    };
    if !content_digests_are_source_origin(&digests) {
        return;
    }
    let by_idx: BTreeMap<u64, &ContentDigestEntry> = digests
        .volumes
        .iter()
        .flat_map(|v| v.messages.iter())
        .map(|m| (m.export_message_index, m))
        .collect();
    let by_mid: BTreeMap<String, &ContentDigestEntry> = digests
        .volumes
        .iter()
        .flat_map(|v| v.messages.iter())
        .filter(|m| !m.message_id_norm.is_empty())
        .map(|m| (normalize_mid_key(&m.message_id_norm), m))
        .collect();
    for row in rows.iter_mut() {
        if !row.subject.is_empty() {
            continue;
        }
        if let Some(m) = by_idx.get(&row.export_message_index) {
            if !m.subject.is_empty() {
                row.subject = m.subject.clone();
                continue;
            }
        }
        let mid = normalize_mid_key(&row.message_id_norm);
        if !mid.is_empty() {
            if let Some(m) = by_mid.get(&mid) {
                if !m.subject.is_empty() {
                    row.subject = m.subject.clone();
                }
            }
        }
    }
}

/// Hydrate candidate subjects / body lens / fidelity flags from digests for clean-room QC.
fn hydrate_candidates_from_digests(report_dir: &Path, candidates: &mut [QcSampleCandidate]) {
    let Some(digests) = load_content_digests(&report_dir.join("content_digests.json")) else {
        return;
    };
    if !content_digests_are_source_origin(&digests) {
        return;
    }
    let by_idx: BTreeMap<u64, &ContentDigestEntry> = digests
        .volumes
        .iter()
        .flat_map(|v| v.messages.iter())
        .map(|m| (m.export_message_index, m))
        .collect();
    for c in candidates.iter_mut() {
        let Some(m) = by_idx.get(&c.export_message_index) else {
            continue;
        };
        if c.subject.is_empty() && !m.subject.is_empty() {
            c.subject = m.subject.clone();
        }
        if c.body_plain_len == 0 && m.body_plain_len > 0 {
            c.body_plain_len = m.body_plain_len;
        }
        if c.body_html_len == 0 && m.body_html_len > 0 {
            c.body_html_len = m.body_html_len;
        }
        if c.display_cc.is_empty() && !m.display_cc.is_empty() {
            c.display_cc = m.display_cc.clone();
        }
        if c.attach_count == 0 && !m.attaches.is_empty() {
            c.attach_count = m.attaches.len();
            c.max_attach_size = m.attaches.iter().map(|a| a.size).max().unwrap_or(0);
        }
        // Reconstruct fidelity explanation flags from persisted digests (DoD-21).
        if m.has_degraded {
            c.has_degraded = true;
        }
        if m.body_unavailable {
            c.body_unavailable = true;
        }
        if m.body_incomplete {
            c.body_incomplete = true;
        }
        if m.crc_suspect {
            c.crc_suspect = true;
        }
        if m.has_ledger_fail || !m.ledger_failed_attach_names.is_empty() {
            c.has_ledger_fail = true;
        }
        if c.ledger_failed_attach_names.is_empty() && !m.ledger_failed_attach_names.is_empty() {
            c.ledger_failed_attach_names = m.ledger_failed_attach_names.clone();
        }
    }
}

/// Detect parents_only / no-attachments export from summary.json.
///
/// Fail-closed when the export section is missing: return false so missing attaches
/// surface as defects rather than silently explained by a guessed policy.
fn load_parents_only_for_qc(report_dir: &Path) -> bool {
    let summary_path = report_dir.join("summary.json");
    let Ok(text) = fs::read_to_string(&summary_path) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    // Missing export section ⇒ cannot infer policy; fail closed (false).
    if v.get("export").is_none() && v.get("family_policy").is_none() {
        return false;
    }
    let family = v
        .get("family_policy")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if family == "parents_only" {
        return true;
    }
    if let Some(n) = v
        .pointer("/export/attachments_omitted_by_policy")
        .and_then(|x| x.as_u64())
    {
        if n > 0 {
            return true;
        }
    }
    // Explicit no_attachments flag on summary when present.
    if v.pointer("/export/no_attachments")
        .and_then(|x| x.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    false
}

fn load_volumes_for_qc(report_dir: &Path, out_pst: &Path) -> Result<Vec<VolumeReportRow>, String> {
    let summary_path = report_dir.join("summary.json");
    if summary_path.is_file() {
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&summary_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        // When summary exists but has no export/volumes, fall through carefully —
        // do not invent messages_written from partial fields (fail closed via structure).
        if let Some(arr) = v.pointer("/export/volumes").and_then(|x| x.as_array()) {
            let mut rows = Vec::new();
            for (i, item) in arr.iter().enumerate() {
                // Require path and messages_written when a volume object is present;
                // silent default-to-0 for messages_written would green-wash missing metadata.
                let path = item
                    .get("path")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let messages_written = match item.get("messages_written").and_then(|x| x.as_u64()) {
                    Some(n) => n,
                    None => {
                        return Err(format!(
                            "summary.json export/volumes[{i}] missing messages_written (strict metadata)"
                        ));
                    }
                };
                rows.push(VolumeReportRow {
                    volume_index: item
                        .get("volume_index")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(i as u64 + 1) as u32,
                    path,
                    bytes: item.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0),
                    sha256_hex: item
                        .get("sha256_hex")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    md5_hex: item
                        .get("md5_hex")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
                    messages_written,
                    finalized_early: item
                        .get("finalized_early")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                    volume_exceeded_soft_limit: item
                        .get("volume_exceeded_soft_limit")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                });
            }
            if !rows.is_empty() {
                // Honor positional out.pst over summary paths when provided and present.
                // Remap first volume (or sole volume) to the operator-supplied output.
                if out_pst.is_file() {
                    let out_str = out_pst.display().to_string();
                    let out_meta = fs::metadata(out_pst).map_err(|e| e.to_string())?;
                    // Prefer remapping volume whose path is missing, else first volume.
                    let remap_idx = rows
                        .iter()
                        .position(|r| !Path::new(&r.path).is_file())
                        .unwrap_or(0);
                    rows[remap_idx].path = out_str;
                    rows[remap_idx].bytes = out_meta.len();
                }
                return Ok(rows);
            }
        }
    }
    // Fallback: single volume = out_pst. messages_written=0 is intentional —
    // structure compare will defect if the PST has messages and CSV is empty
    // (validate_export_metadata_coverage) or count mismatches after open.
    let meta = fs::metadata(out_pst).map_err(|e| e.to_string())?;
    Ok(vec![VolumeReportRow {
        volume_index: 1,
        path: out_pst.display().to_string(),
        bytes: meta.len(),
        sha256_hex: String::new(),
        md5_hex: String::new(),
        messages_written: 0,
        finalized_early: false,
        volume_exceeded_soft_limit: false,
    }])
}

fn load_export_rows_for_qc(report_dir: &Path) -> Result<Vec<ExportMessageRow>, String> {
    let path = report_dir.join("export_messages.csv");
    if !path.is_file() {
        // Missing file: return empty; `validate_export_metadata_coverage` turns
        // this into a hard defect when any volume has messages_written > 0.
        return Ok(Vec::new());
    }
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(&path)
        .map_err(|e| e.to_string())?;
    let headers = rdr.headers().map_err(|e| e.to_string())?.clone();
    let col = |name: &str| headers.iter().position(|h| h == name);
    // Prefer header names; fall back to locked prefix positions for older packs.
    let i_source_path = col("source_path").unwrap_or(0);
    let i_folder_path = col("folder_path").unwrap_or(1);
    let i_nid = col("nid").unwrap_or(2);
    let i_mid = col("message_id_norm").unwrap_or(3);
    let i_edrm = col("edrm_mih").unwrap_or(4);
    let i_hash = col("content_hash_hex").unwrap_or(5);
    let i_vol_path = col("volume_path").unwrap_or(6);
    let i_vol_idx = col("volume_index").unwrap_or(7);
    let i_export_idx = col("export_message_index").unwrap_or(8);
    let i_attach_fails = col("attachments_failed_count");
    let i_dup_count = col("duplicate_source_count");
    let i_dup_sources = col("duplicate_sources");
    // 0081: optional trailing source_id (by name, else last column when header includes it).
    let i_source_id = col("source_id").or_else(|| {
        if headers.iter().any(|h| h == "source_id") {
            Some(headers.len().saturating_sub(1))
        } else {
            None
        }
    });
    let mut rows = Vec::new();
    let mut seen_idx: BTreeSet<u64> = BTreeSet::new();
    for (i, rec) in rdr.records().enumerate() {
        let rec = rec.map_err(|e| format!("export_messages.csv row {}: {e}", i + 1))?;
        // source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,...
        if rec.len() < 9 {
            return Err(format!(
                "export_messages.csv row {} malformed: expected ≥9 fields, got {} (truncated/corrupt row)",
                i + 1,
                rec.len()
            ));
        }
        let nid_raw = rec.get(i_nid).unwrap_or("").trim();
        let nid = parse_nid_strict(nid_raw).map_err(|e| {
            format!(
                "export_messages.csv row {}: invalid nid '{nid_raw}': {e}",
                i + 1
            )
        })?;
        let volume_index = rec
            .get(i_vol_idx)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                format!(
                    "export_messages.csv row {}: invalid volume_index {:?}",
                    i + 1,
                    rec.get(i_vol_idx)
                )
            })?;
        let export_message_index = rec
            .get(i_export_idx)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                format!(
                    "export_messages.csv row {}: invalid export_message_index {:?}",
                    i + 1,
                    rec.get(i_export_idx)
                )
            })?;
        if !seen_idx.insert(export_message_index) {
            return Err(format!(
                "export_messages.csv row {}: duplicate export_message_index {export_message_index}",
                i + 1
            ));
        }
        // source_id: present only when column exists; empty when blank — never invent "0".
        let source_id = i_source_id
            .and_then(|idx| rec.get(idx))
            .unwrap_or("")
            .trim()
            .to_string();
        rows.push(ExportMessageRow {
            source_path: rec.get(i_source_path).unwrap_or("").to_string(),
            folder_path: rec.get(i_folder_path).unwrap_or("").to_string(),
            nid,
            message_id_norm: rec.get(i_mid).unwrap_or("").to_string(),
            edrm_mih: rec.get(i_edrm).unwrap_or("").to_string(),
            content_hash_hex: rec.get(i_hash).unwrap_or("").to_string(),
            volume_path: rec.get(i_vol_path).unwrap_or("").to_string(),
            volume_index,
            export_message_index,
            attachments_failed_count: i_attach_fails
                .and_then(|idx| rec.get(idx))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            duplicate_source_count: i_dup_count
                .and_then(|idx| rec.get(idx))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            duplicate_sources: i_dup_sources
                .and_then(|idx| rec.get(idx))
                .unwrap_or("")
                .to_string(),
            source_id,
            subject: String::new(),
        });
    }
    Ok(rows)
}

/// Load `summary.json` → `inputs` array (full paths at export time).
fn load_summary_inputs(report_dir: &Path) -> Option<Vec<String>> {
    let summary_path = report_dir.join("summary.json");
    let text = fs::read_to_string(&summary_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let arr = v.get("inputs")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
    )
}

/// For standalone `qc-pst` after basename handoff: when CSV `source_path` is not an
/// existing file and `source_id` is present and in range of `summary.inputs`, open
/// via the full path from `inputs[source_id]` when that file exists.
///
/// - Does **not** invent `source_id` 0 when the column is missing/empty.
/// - Leaves `source_path` unchanged when the CSV path already exists (CWD-relative
///   basename that is a real file still wins — operator can re-map via Matter Archive).
/// - On successful resolve, replaces in-memory `source_path` so source differential
///   opens the correct PST; the on-disk CSV remains basenamed (honest handoff copy).
pub fn resolve_export_source_paths_from_summary(report_dir: &Path, rows: &mut [ExportMessageRow]) {
    let Some(inputs) = load_summary_inputs(report_dir) else {
        return;
    };
    for row in rows.iter_mut() {
        if Path::new(&row.source_path).is_file() {
            continue;
        }
        if row.source_id.is_empty() {
            // Do not invent source_id 0 when missing.
            continue;
        }
        let Ok(id) = row.source_id.parse::<usize>() else {
            continue;
        };
        let Some(full) = inputs.get(id) else {
            continue;
        };
        if Path::new(full).is_file() {
            row.source_path = full.clone();
        }
    }
}

fn parse_nid_strict(s: &str) -> Result<u64, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("empty nid".into());
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        t.parse().map_err(|e| format!("{e}"))
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn parse_nid(s: &str) -> u64 {
    parse_nid_strict(s).unwrap_or(0)
}

/// Hash bytes for negative tests / digests.
pub fn sha256_hex_bytes(bytes: &[u8]) -> String {
    hex_sha256(bytes)
}

/// Identity fields for [`record_classified_finding`] (keeps arg count under clippy limits).
pub struct RecordFindingId<'a> {
    pub volume_index: u32,
    pub source_path: &'a str,
    pub source_nid: u64,
    pub message_id_norm: &'a str,
}

/// Record a classified finding through the same path production uses (DoD-9).
///
/// Unknown properties map to `unexplained_loss` via the contract allowlist.
pub fn record_classified_finding(
    contract: &FidelityContract,
    counts: &mut QcFindingCounts,
    findings: &mut Vec<QcFinding>,
    property: &str,
    explained: bool,
    id: RecordFindingId<'_>,
    detail: impl Into<String>,
) {
    let (class, _) = contract.classify(property, explained);
    counts.record(class);
    findings.push(QcFinding {
        class,
        property: property.into(),
        volume_index: id.volume_index,
        source_path: id.source_path.into(),
        source_nid: id.source_nid,
        message_id_norm: id.message_id_norm.into(),
        detail: detail.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(i: u64, body: usize, att: usize, subj: &str) -> QcSampleCandidate {
        QcSampleCandidate {
            export_message_index: i,
            volume_index: 1,
            source_path: format!("C:/s{i}.pst"),
            source_nid: i,
            folder_path: "Inbox".into(),
            subject: subj.into(),
            sender: "a@b.com".into(),
            message_id_norm: format!("mid{i}"),
            body_plain_len: body,
            body_html_len: 0,
            attach_count: att,
            max_attach_size: att as u64 * 10,
            has_zero_byte_attach: false,
            has_embedded: false,
            has_degraded: false,
            has_ledger_fail: false,
            ledger_failed_attach_names: Vec::new(),
            body_unavailable: false,
            body_incomplete: false,
            crc_suspect: false,
            subject_non_ascii: !subj.is_ascii(),
            display_cc: String::new(),
            display_bcc: String::new(),
        }
    }

    #[test]
    fn sample_selection_deterministic() {
        let cands: Vec<_> = (0..20)
            .map(|i| cand(i, (i * 100) as usize, (i % 5) as usize, &format!("s{i}")))
            .collect();
        let a = select_sample_indices(&cands, 64);
        let b = select_sample_indices(&cands, 64);
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn sample_includes_extremes() {
        let mut cands = vec![
            cand(1, 10, 0, "short"),
            cand(2, 9999, 0, "longbody"),
            cand(3, 0, 0, "emptybody"),
            cand(4, 50, 9, "manyatt"),
        ];
        cands[1].body_plain_len = 9999;
        cands[2].body_plain_len = 0;
        cands[3].attach_count = 9;
        let sel = select_sample_indices(&cands, 64);
        // Must include largest body (idx 1), smallest (idx 2), most attach (idx 3)
        assert!(sel.contains(&1));
        assert!(sel.contains(&2));
        assert!(sel.contains(&3));
    }

    #[test]
    fn sample_cap_preserves_volume_last_stratum() {
        // Many candidates: volume-last has high export index and would be dropped
        // by naive sort-then-truncate; stratum preference must keep it.
        let mut cands: Vec<_> = (0..20)
            .map(|i| cand(i, 100 + i as usize, 0, &format!("s{i}")))
            .collect();
        // Make index 19 the volume-last (already is) and not win any other extreme
        // except volume-last / source uniqueness.
        for c in &mut cands {
            c.volume_index = 1;
            c.source_path = "C:/same.pst".into();
        }
        cands[0].body_plain_len = 1; // smallest body
        cands[1].body_plain_len = 99999; // largest body
        let sel = select_sample_indices(&cands, 3);
        assert!(
            sel.contains(&19) || sel.contains(&0) && sel.contains(&1),
            "cap must prefer stratum reps; got {sel:?}"
        );
        // With cap 3: largest body, smallest body, volume first/last — last must survive.
        let sel3 = select_sample_indices(&cands, 3);
        assert!(
            sel3.contains(&19),
            "volume-last (idx 19) must survive sample_max=3, got {sel3:?}"
        );
    }

    #[test]
    fn folder_tree_rejects_collapsed_multi_leaf() {
        let digest = VolumeStructuralDigest {
            message_count: 2,
            folder_paths: vec!["Unique Mail".into()],
            folder_message_counts: vec![2],
            message_digests: vec!["a".into(), "b".into()],
        };
        let mut expected = BTreeMap::new();
        expected.insert("inbox".into(), 1);
        expected.insert("sent items".into(), 1);
        assert!(
            !folder_tree_matches(&digest, &expected, 2),
            "collapsed tree must not match multi-leaf expected"
        );
    }

    #[test]
    fn folder_tree_accepts_suffix_and_residual_self() {
        let digest = VolumeStructuralDigest {
            message_count: 1,
            folder_paths: vec!["IPM_SUBTREE/Inbox".into()],
            folder_message_counts: vec![1],
            message_digests: vec!["a".into()],
        };
        let mut expected = BTreeMap::new();
        expected.insert("inbox".into(), 1);
        assert!(folder_tree_matches(&digest, &expected, 1));

        let residual = VolumeStructuralDigest {
            message_count: 1,
            folder_paths: vec!["Unique Mail".into()],
            folder_message_counts: vec![1],
            message_digests: vec!["a".into()],
        };
        let mut expected_res = BTreeMap::new();
        expected_res.insert("unique mail".into(), 1);
        assert!(folder_tree_matches(&residual, &expected_res, 1));
    }

    #[test]
    fn folder_tree_rejects_same_leaves_different_counts() {
        // Both Inbox and Sent exist with total 3, but counts redistributed.
        let digest = VolumeStructuralDigest {
            message_count: 3,
            folder_paths: vec!["IPM_SUBTREE/Inbox".into(), "IPM_SUBTREE/Sent Items".into()],
            folder_message_counts: vec![1, 2],
            message_digests: vec!["a".into(), "b".into(), "c".into()],
        };
        let mut expected = BTreeMap::new();
        expected.insert("inbox".into(), 2);
        expected.insert("sent items".into(), 1);
        assert!(
            !folder_tree_matches(&digest, &expected, 3),
            "same leaves with different per-folder counts must hard-fail match"
        );
    }

    #[test]
    fn folder_tree_rejects_unclaimed_output_folder_with_messages() {
        // Expected only Inbox=1 but output also has Archive=1 (total still 2 if both counted).
        let digest = VolumeStructuralDigest {
            message_count: 2,
            folder_paths: vec!["IPM_SUBTREE/Inbox".into(), "IPM_SUBTREE/Archive".into()],
            folder_message_counts: vec![1, 1],
            message_digests: vec!["a".into(), "b".into()],
        };
        let mut expected = BTreeMap::new();
        expected.insert("inbox".into(), 1);
        // Message count mismatch first... use expected_count=2 with only inbox claimed partial.
        // Claim inbox=1, archive remains unclaimed ⇒ fail even if we lie about expected total.
        assert!(
            !folder_tree_matches(&digest, &expected, 2),
            "unclaimed Archive with messages must fail folder_tree_matches"
        );
    }

    #[test]
    fn export_metadata_missing_csv_is_defect() {
        let volumes = vec![VolumeReportRow {
            volume_index: 1,
            path: "out.pst".into(),
            bytes: 1,
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 2,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        }];
        let contract = FidelityContract::v1();
        let mut counts = QcFindingCounts::default();
        let mut findings = Vec::new();
        validate_export_metadata_coverage(&volumes, &[], &contract, &mut counts, &mut findings);
        assert!(counts.hard_fail());
        assert!(findings
            .iter()
            .any(|f| f.property == "export_messages_missing"));
    }

    #[test]
    fn export_metadata_row_shortfall_is_defect() {
        let volumes = vec![VolumeReportRow {
            volume_index: 1,
            path: "out.pst".into(),
            bytes: 1,
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 2,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        }];
        let rows = vec![ExportMessageRow {
            source_path: "a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            message_id_norm: "m1".into(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: "out.pst".into(),
            volume_index: 1,
            export_message_index: 1,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "s".into(),
        }];
        let contract = FidelityContract::v1();
        let mut counts = QcFindingCounts::default();
        let mut findings = Vec::new();
        validate_export_metadata_coverage(&volumes, &rows, &contract, &mut counts, &mut findings);
        assert!(counts.hard_fail());
        assert!(findings
            .iter()
            .any(|f| f.property == "export_messages_row_count"));
    }

    /// Wrong-volume-index orphan rows: declared vol1 count can still match while an
    /// extra row points at volume 99 — must hard_fail (membership strictness).
    #[test]
    fn export_metadata_orphan_volume_index_is_defect() {
        let volumes = vec![VolumeReportRow {
            volume_index: 1,
            path: "out.pst".into(),
            bytes: 1,
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 1,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        }];
        let rows = vec![
            ExportMessageRow {
                source_path: "a.pst".into(),
                folder_path: "Inbox".into(),
                nid: 1,
                message_id_norm: "m1".into(),
                edrm_mih: String::new(),
                content_hash_hex: String::new(),
                volume_path: "out.pst".into(),
                volume_index: 1,
                export_message_index: 1,
                attachments_failed_count: 0,
                duplicate_source_count: 0,
                duplicate_sources: String::new(),
                source_id: String::new(),
                subject: "s1".into(),
            },
            // Orphan: volume_index 99 not declared — per-vol1 count still matches (=1).
            ExportMessageRow {
                source_path: "a.pst".into(),
                folder_path: "Inbox".into(),
                nid: 2,
                message_id_norm: "m2".into(),
                edrm_mih: String::new(),
                content_hash_hex: String::new(),
                volume_path: "ghost.pst".into(),
                volume_index: 99,
                export_message_index: 2,
                attachments_failed_count: 0,
                duplicate_source_count: 0,
                duplicate_sources: String::new(),
                source_id: String::new(),
                subject: "s2".into(),
            },
        ];
        let contract = FidelityContract::v1();
        let mut counts = QcFindingCounts::default();
        let mut findings = Vec::new();
        validate_export_metadata_coverage(&volumes, &rows, &contract, &mut counts, &mut findings);
        assert!(
            counts.hard_fail(),
            "orphan volume_index with matching declared count must hard_fail: {findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|f| f.property == "export_messages_orphan_volume_index"),
            "expected orphan_volume_index finding: {findings:?}"
        );
    }

    #[test]
    fn worse_external_status_skipped_beats_ok() {
        use crate::qc_external::ExternalStatus;
        assert_eq!(
            worse_external_status(ExternalStatus::Ok, ExternalStatus::Skipped),
            ExternalStatus::Skipped
        );
        assert_eq!(
            worse_external_status(ExternalStatus::Skipped, ExternalStatus::Ok),
            ExternalStatus::Skipped
        );
        assert_eq!(
            worse_external_status(ExternalStatus::Skipped, ExternalStatus::Failed),
            ExternalStatus::Failed
        );
        assert_eq!(
            worse_external_status(ExternalStatus::Ok, ExternalStatus::Failed),
            ExternalStatus::Failed
        );
        assert_eq!(
            worse_external_status(ExternalStatus::Timeout, ExternalStatus::Skipped),
            ExternalStatus::Timeout
        );
    }

    #[test]
    fn folder_leaf_rejects_ancestor_contains_false_positive() {
        // Intermediate-segment contains("/leaf/") must not match.
        assert!(
            !folder_leaf_matches("archive/inbox/2020", "inbox"),
            "intermediate segment must not match leaf"
        );
        assert!(
            !folder_leaf_matches("Inbox", "Inbox/Sub"),
            "ancestor-only must not match longer expected leaf"
        );
        assert!(folder_leaf_matches("IPM_SUBTREE/Inbox", "inbox"));
        assert!(folder_leaf_matches("IPM_SUBTREE/Inbox/Sub", "Inbox/Sub"));
    }

    #[test]
    fn record_classified_finding_unexplained_is_hard_fail() {
        let contract = FidelityContract::v1();
        let mut counts = QcFindingCounts::default();
        let mut findings = Vec::new();
        record_classified_finding(
            &contract,
            &mut counts,
            &mut findings,
            "never_heard_of_this_mapi_prop",
            false,
            RecordFindingId {
                volume_index: 0,
                source_path: "",
                source_nid: 0,
                message_id_norm: "",
            },
            "synthetic unknown property via production record path",
        );
        assert_eq!(counts.unexplained_loss, 1);
        assert!(counts.hard_fail());
        assert_eq!(findings[0].class, FindingClass::UnexplainedLoss);
    }

    #[test]
    fn content_digests_origin_guards() {
        let src = ContentDigestsFile {
            schema: "content_digests_v1".into(),
            origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
            qc_level: "sample".into(),
            volumes: vec![],
        };
        assert!(content_digests_are_source_origin(&src));
        let out = ContentDigestsFile {
            origin: CONTENT_DIGEST_ORIGIN_OUTPUT.into(),
            ..src.clone()
        };
        assert!(!content_digests_are_source_origin(&out));
    }

    #[test]
    fn known_gap_never_hard_fail() {
        let mut c = QcFindingCounts::default();
        c.record(FindingClass::KnownGap);
        c.record(FindingClass::Explained);
        assert!(!c.hard_fail());
        c.record(FindingClass::Defect);
        assert!(c.hard_fail());
    }

    #[test]
    fn qc_level_parse() {
        assert_eq!(QcLevel::parse("sample").unwrap(), QcLevel::Sample);
        assert_eq!(QcLevel::parse("FULL").unwrap(), QcLevel::Full);
        assert!(QcLevel::parse("nope").is_err());
    }

    /// Basename-mode CSV + summary.inputs: distinct source_id resolves to full paths
    /// when basenamed source_path is not an existing file.
    #[test]
    fn resolve_export_source_paths_from_summary_basename_source_id() {
        let dir = tempfile::tempdir().expect("tmp");
        let report = dir.path().join("report");
        fs::create_dir_all(&report).expect("report");
        // Two real files with different directories, same basename.
        let a_dir = dir.path().join("custA");
        let b_dir = dir.path().join("custB");
        fs::create_dir_all(&a_dir).expect("a");
        fs::create_dir_all(&b_dir).expect("b");
        let path_a = a_dir.join("mailbox.pst");
        let path_b = b_dir.join("mailbox.pst");
        fs::write(&path_a, b"a").expect("touch a");
        fs::write(&path_b, b"b").expect("touch b");
        let path_a_s = path_a.display().to_string();
        let path_b_s = path_b.display().to_string();

        fs::write(
            report.join("summary.json"),
            serde_json::json!({
                "inputs": [path_a_s, path_b_s],
            })
            .to_string(),
        )
        .expect("summary");

        // CSV as basename handoff would write it.
        let header = crate::unique_export_report::EXPORT_MESSAGES_CSV_HEADER;
        let csv = format!(
            "{header}\n\
             mailbox.pst,Inbox,8193,a@x,,,out.pst,1,1,0,0,,0\n\
             mailbox.pst,Inbox,8194,b@x,,,out.pst,1,2,0,0,,1\n"
        );
        fs::write(report.join("export_messages.csv"), csv).expect("csv");

        let mut rows = load_export_rows_for_qc(&report).expect("load");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].source_path, "mailbox.pst");
        assert_eq!(rows[0].source_id, "0");
        assert_eq!(rows[1].source_id, "1");
        // Before resolve, basenames are not absolute files (unless CWD collision).
        // Resolve via summary.inputs.
        resolve_export_source_paths_from_summary(&report, &mut rows);
        assert_eq!(rows[0].source_path, path_a_s, "source_id 0 → inputs[0]");
        assert_eq!(rows[1].source_path, path_b_s, "source_id 1 → inputs[1]");
        assert!(Path::new(&rows[0].source_path).is_file());
        assert!(Path::new(&rows[1].source_path).is_file());
    }

    /// Missing source_id must not invent 0 even when summary.inputs is present.
    #[test]
    fn resolve_export_source_paths_does_not_invent_source_id_zero() {
        let dir = tempfile::tempdir().expect("tmp");
        let report = dir.path().join("report");
        fs::create_dir_all(&report).expect("report");
        let real = dir.path().join("real.pst");
        fs::write(&real, b"x").expect("touch");
        let real_s = real.display().to_string();
        fs::write(
            report.join("summary.json"),
            serde_json::json!({ "inputs": [real_s] }).to_string(),
        )
        .expect("summary");
        // Pre-0081 header: no source_id column.
        let csv = "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count\n\
                   mailbox.pst,Inbox,8193,a@x,,,out.pst,1,1,0\n";
        fs::write(report.join("export_messages.csv"), csv).expect("csv");
        let mut rows = load_export_rows_for_qc(&report).expect("load");
        assert_eq!(rows[0].source_id, "", "missing column → empty, not 0");
        resolve_export_source_paths_from_summary(&report, &mut rows);
        assert_eq!(
            rows[0].source_path, "mailbox.pst",
            "must not invent source_id 0 resolve when column missing"
        );
    }
}
