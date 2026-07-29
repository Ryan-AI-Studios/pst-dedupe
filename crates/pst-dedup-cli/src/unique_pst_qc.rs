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
    pub has_degraded: bool,
    pub has_ledger_fail: bool,
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
        subject_non_ascii: !subject.is_ascii(),
        display_cc: write_msg.display_cc.clone().unwrap_or_default(),
        display_bcc: display_bcc.to_string(),
    }
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
    pub attaches: Vec<AttachDigestEntry>,
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

    let content_digests_path = input.report_dir.join("content_digests.json");
    let existing_digests = load_content_digests(&content_digests_path);
    // content_digest_backed only when loaded digests are source-origin (never output).
    let content_digest_backed = !input.source_differential
        && existing_digests
            .as_ref()
            .is_some_and(content_digests_are_source_origin);

    // Output-only without source digests: structural only — cannot emit defect from content.
    let content_capable = input.source_differential || content_digest_backed;

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

        // Expected folder set: distinct folder_path from export rows for this volume.
        let expected_folders: BTreeSet<String> = expected_rows
            .iter()
            .map(|r| r.folder_path.to_ascii_lowercase())
            .collect();

        match structural_digest_pst(&path) {
            Ok(digest) => {
                open_ok = true;
                messages_found = digest.message_count;
                message_count_match = messages_found == expected_count;

                // Folder tree: every message folder should appear under output paths
                // (output may add multi-source prefixes — require non-empty folders when
                // messages exist; compare multiset of per-folder counts loosely via paths).
                folder_tree_match = folder_tree_matches(&digest, &expected_folders, expected_count);

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
                            "folder tree mismatch: out_folders={:?} expected_leaf_set_size={}",
                            digest.folder_paths,
                            expected_folders.len()
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
                    let mut out_by_mid = index_output_by_mid(&path);

                    for &ci in &sample_idxs {
                        let cand = match input.candidates.get(ci) {
                            Some(c) if c.volume_index == vol.volume_index => c,
                            _ => continue,
                        };
                        let compare = compare_one_message(CompareOneArgs {
                            cand,
                            handles: &mut handle_cache,
                            out_by_mid: &mut out_by_mid,
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
                        if input.source_differential {
                            if let Some(entry) = compare.digest_entry {
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

    // Test/diagnostic: force one allowlist-miss classification into the pipeline.
    if let Some(prop) = input.probe_unexplained_property {
        if !prop.is_empty() {
            let (class, _) = contract.classify(prop, false);
            counts.record(class);
            findings_list.push(QcFinding {
                class,
                property: prop.into(),
                volume_index: 0,
                source_path: String::new(),
                source_nid: 0,
                message_id_norm: String::new(),
                detail: format!("probe property '{prop}' classified as {class:?}"),
            });
        }
    }

    // External sidecars (skip-safe).
    let mut independent_reader = if let Some(tool) = input.external_reader {
        if let Some(vol) = input.volumes.first() {
            run_independent_reader(tool, Path::new(&vol.path), DEFAULT_EXTERNAL_TIMEOUT)
        } else {
            IndependentReaderResult::skipped("no volumes")
        }
    } else {
        IndependentReaderResult::skipped("no --qc-external-reader path")
    };

    // When external reader returns Ok counts, compare to expected volume counts.
    if independent_reader.status == ExternalStatus::Ok {
        if let Some(vol) = input.volumes.first() {
            let expected_msgs = vol.messages_written;
            if let Some(reader_msgs) = independent_reader.message_count {
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
                            "independent reader message_count={reader_msgs} expected={expected_msgs}"
                        ),
                    });
                    independent_reader.reason = Some(format!(
                        "message_count mismatch: reader={reader_msgs} expected={expected_msgs}"
                    ));
                }
            }
            let expected_folder_leaves = input
                .export_rows
                .iter()
                .filter(|r| r.volume_index == vol.volume_index)
                .map(|r| r.folder_path.to_ascii_lowercase())
                .collect::<BTreeSet<_>>()
                .len() as u64;
            if expected_folder_leaves > 0 {
                if let Some(reader_folders) = independent_reader.folder_count {
                    // Allow reader folder_count >= expected leaves (IPM hierarchy overhead).
                    // Hard fail only when reader reports fewer folders than distinct export leaves
                    // or an exact expected when both are comparable and clearly wrong (0 vs N).
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
                                "independent reader folder_count=0 expected_leaf_folders>={expected_folder_leaves}"
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
                                "independent reader folder_count={reader_folders} < expected_leaf_folders={expected_folder_leaves}"
                            ),
                        });
                    }
                }
            }
        }
    }

    let mut scanpst = if input.run_scanpst {
        if let Some(vol) = input.volumes.first() {
            run_scanpst_auto(Path::new(&vol.path), DEFAULT_EXTERNAL_TIMEOUT)
        } else {
            ScanpstResult::skipped("no volumes")
        }
    } else {
        ScanpstResult::skipped("scanpst not requested")
    };
    // Hard error from bak ⇒ defect
    if scanpst.hard_error {
        counts.record(FindingClass::Defect);
        findings_list.push(QcFinding {
            class: FindingClass::Defect,
            property: "scanpst".into(),
            volume_index: 0,
            source_path: String::new(),
            source_nid: 0,
            message_id_norm: String::new(),
            detail: scanpst
                .reason
                .clone()
                .unwrap_or_else(|| "scanpst hard error".into()),
        });
        scanpst.status = ExternalStatus::Failed;
    }

    let attestation = load_attestation(&input.report_dir.join("qc_attestation_v1.json"))
        .ok()
        .flatten();

    let qc_ms = t0.elapsed().as_millis() as u64;
    let hard_fail = counts.hard_fail();

    let report = QcReportV1 {
        schema: "qc_report_v1".into(),
        contract: FIDELITY_CONTRACT_VERSION.into(),
        qc_level: input.level.as_str().into(),
        source_differential: input.source_differential,
        content_digest_backed,
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

    // Write artifacts.
    let _ = write_qc_report(input.report_dir, &report);
    let _ = write_qc_findings_csv(input.report_dir, &findings_list);
    // Persist content_digests.json only for source-side digests (export with live sources).
    // Never write output-only digests under this schema — they must not enable
    // content_digest_backed on a later run (DoD-21).
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
        let _ = write_content_digests(input.report_dir, &digests);
    }

    report
}

struct MsgCompareResult {
    findings: Vec<QcFinding>,
    attachments_compared: u64,
    skipped_source: bool,
    digest_entry: Option<ContentDigestEntry>,
}

fn normalize_mid_key(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_ascii_lowercase()
}

struct CompareOneArgs<'a> {
    cand: &'a QcSampleCandidate,
    handles: &'a mut PstHandleCache,
    out_by_mid: &'a mut BTreeMap<String, MessageContentDetail>,
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
        out_by_mid,
        out_path,
        source_differential,
        existing,
        contract,
        parents_only,
    } = args;
    let mut findings = Vec::new();
    let mut attachments_compared = 0u64;
    let mut skipped_source = false;

    // Resolve source-side detail
    let source_detail: Option<MessageContentDetail> = if source_differential {
        match handles.get_mut(&cand.source_path) {
            Ok(pst) => match message_content_detail(pst, cand.source_nid) {
                Ok(d) => Some(d),
                Err(e) => {
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
                    None
                }
            },
            Err(e) => {
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
            .map(|m| MessageContentDetail {
                digest: m.content_digest.clone(),
                message_id: m.message_id_norm.clone(),
                subject: String::new(),
                display_to: String::new(),
                display_cc: String::new(),
                body_plain_len: 0,
                body_html_len: 0,
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
            })
    } else {
        None
    };

    // Output side: by normalized MID, then subject fallback.
    let mid_key = normalize_mid_key(&cand.message_id_norm);
    let out_detail = {
        let by_mid = if !mid_key.is_empty() {
            out_by_mid.get(&mid_key).cloned().or_else(|| {
                out_by_mid
                    .values()
                    .find(|d| normalize_mid_key(&d.message_id) == mid_key)
                    .cloned()
            })
        } else {
            None
        };
        by_mid.or_else(|| {
            if cand.subject.is_empty() {
                None
            } else {
                out_by_mid
                    .values()
                    .find(|d| d.subject.eq_ignore_ascii_case(&cand.subject))
                    .cloned()
            }
        })
    };

    let Some(src) = source_detail else {
        return MsgCompareResult {
            findings,
            attachments_compared,
            skipped_source,
            digest_entry: None,
        };
    };

    let digest_entry = ContentDigestEntry {
        export_message_index: cand.export_message_index,
        source_path: cand.source_path.clone(),
        source_nid: cand.source_nid,
        message_id_norm: cand.message_id_norm.clone(),
        content_digest: src.digest.clone(),
        attaches: src
            .attaches
            .iter()
            .map(|(f, s, _, h)| AttachDigestEntry {
                filename: f.clone(),
                size: *s,
                payload_sha256: h.clone(),
            })
            .collect(),
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

    // BCC on source: known_gap if present in source extract (display_bcc not in detail —
    // we count from cand.display_bcc in caller). CC comparison:
    if !src.display_cc.is_empty() && src.display_cc != out.display_cc {
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
    } else {
        let out_attach_map: BTreeMap<String, &str> = out
            .attaches
            .iter()
            .map(|(f, _, _, h)| (f.to_ascii_lowercase(), h.as_str()))
            .collect();
        for (fnm, sz, _, ph) in &src.attaches {
            attachments_compared = attachments_compared.saturating_add(1);
            if ph.is_empty() {
                let explained = cand.has_ledger_fail || cand.has_degraded;
                let (class, _) = contract.classify("attachment_stream_soft_fail", explained);
                if class != FindingClass::Explained {
                    findings.push(QcFinding {
                        class,
                        property: "attachment_stream_soft_fail".into(),
                        volume_index: cand.volume_index,
                        source_path: cand.source_path.clone(),
                        source_nid: cand.source_nid,
                        message_id_norm: cand.message_id_norm.clone(),
                        detail: format!("source attach {fnm} has empty payload hash"),
                    });
                } else {
                    findings.push(QcFinding {
                        class: FindingClass::Explained,
                        property: "attachment_stream_soft_fail".into(),
                        volume_index: cand.volume_index,
                        source_path: cand.source_path.clone(),
                        source_nid: cand.source_nid,
                        message_id_norm: cand.message_id_norm.clone(),
                        detail: format!("source attach {fnm} empty hash (ledger/degraded)"),
                    });
                }
                continue;
            }
            match out_attach_map.get(&fnm.to_ascii_lowercase()) {
                Some(out_ph) if *out_ph == ph.as_str() => {}
                Some(out_ph) => {
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
                    let explained = cand.has_ledger_fail;
                    let (class, _) = if explained {
                        contract.classify("attachment_stream_soft_fail", true)
                    } else {
                        contract.classify("attachment_by_value", false)
                    };
                    findings.push(QcFinding {
                        class,
                        property: if explained {
                            "attachment_stream_soft_fail".into()
                        } else {
                            "attachment_by_value".into()
                        },
                        volume_index: cand.volume_index,
                        source_path: cand.source_path.clone(),
                        source_nid: cand.source_nid,
                        message_id_norm: cand.message_id_norm.clone(),
                        detail: format!("attach {fnm} missing in output"),
                    });
                }
            }
        }
    }

    // Body/recipient compare: use field-level checks when parents_only so attach-less
    // digests are not compared apples-to-oranges against source digests that include attaches.
    let body_match = src.body_plain_len == out.body_plain_len
        && src.body_html_len == out.body_html_len
        && src.display_to == out.display_to
        && src.display_cc == out.display_cc
        && src.subject.eq_ignore_ascii_case(&out.subject);
    let digest_match = src.digest == out.digest;
    if parents_only {
        // Compare body lengths + recipients; full digest includes attaches.
        if !body_match && !cand.has_degraded {
            let (class, _) = contract.classify("message_content_digest", false);
            findings.push(QcFinding {
                class,
                property: "message_content_digest".into(),
                volume_index: cand.volume_index,
                source_path: cand.source_path.clone(),
                source_nid: cand.source_nid,
                message_id_norm: cand.message_id_norm.clone(),
                detail: format!(
                    "body/recipient mismatch (parents_only) src_subj={:?} out_subj={:?} src_plain={} out_plain={}",
                    src.subject, out.subject, src.body_plain_len, out.body_plain_len
                ),
            });
        } else if !body_match && cand.has_degraded {
            findings.push(QcFinding {
                class: FindingClass::Explained,
                property: "body_unavailable".into(),
                volume_index: cand.volume_index,
                source_path: cand.source_path.clone(),
                source_nid: cand.source_nid,
                message_id_norm: cand.message_id_norm.clone(),
                detail: "body/recipient differ on degraded winner (explained)".into(),
            });
        }
    } else if !digest_match {
        if cand.has_degraded {
            findings.push(QcFinding {
                class: FindingClass::Explained,
                property: "message_content_digest".into(),
                volume_index: cand.volume_index,
                source_path: cand.source_path.clone(),
                source_nid: cand.source_nid,
                message_id_norm: cand.message_id_norm.clone(),
                detail: format!(
                    "content digest mismatch on degraded winner (explained) src={} out={}",
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

fn index_output_by_mid(path: &Path) -> BTreeMap<String, MessageContentDetail> {
    let mut map = BTreeMap::new();
    let Ok(mut pst) = pst_reader::PstFile::open(path) else {
        return map;
    };
    let Ok(folders) = pst.folders() else {
        return map;
    };
    for folder in &folders {
        for &nid in &folder.message_nids {
            if let Ok(detail) = message_content_detail(&mut pst, nid.0) {
                let key = normalize_mid_key(&detail.message_id);
                if !key.is_empty() {
                    map.insert(key, detail);
                } else {
                    // Key by subject when no MID
                    let sk = format!("subj:{}", detail.subject.to_ascii_lowercase());
                    map.insert(sk, detail);
                }
            }
        }
    }
    map
}

/// Normalize folder path for case-insensitive segment comparison.
fn normalize_folder_key(p: &str) -> String {
    p.trim()
        .trim_matches(|c| c == '/' || c == '\\')
        .replace('\\', "/")
        .to_ascii_lowercase()
}

/// True when `out_path` equals `leaf` or ends with `leaf` as a path suffix/segment chain.
fn folder_leaf_matches(out_path: &str, leaf: &str) -> bool {
    let out = normalize_folder_key(out_path);
    let leaf = normalize_folder_key(leaf);
    if leaf.is_empty() {
        return true;
    }
    if out.is_empty() {
        return false;
    }
    out == leaf || out.ends_with(&format!("/{leaf}")) || out.contains(&format!("/{leaf}/"))
}

/// Residual catch-all folder used by flat / residual routing (not wholesale collapse).
fn is_residual_unique_mail(path: &str) -> bool {
    let n = normalize_folder_key(path);
    n == "unique mail" || n.ends_with("/unique mail")
}

/// Every expected leaf folder must match an output path (suffix or equality,
/// case-insensitive). Missing leaf ⇒ false.
///
/// **Residual Unique Mail allowance** (documented only): an expected path that
/// *is itself* residual Unique Mail may match an output Unique Mail folder.
/// Wholesale collapse (multi-leaf expected → single unrelated residual) fails.
fn folder_tree_matches(
    digest: &VolumeStructuralDigest,
    expected_leaf_folders: &BTreeSet<String>,
    expected_count: u64,
) -> bool {
    if expected_count == 0 {
        return digest.message_count == 0;
    }
    if digest.message_count != expected_count {
        return false;
    }
    if expected_leaf_folders.is_empty() {
        return true;
    }
    let out_lower: Vec<String> = digest
        .folder_paths
        .iter()
        .map(|p| p.to_ascii_lowercase())
        .collect();
    let out_has_residual = out_lower.iter().any(|p| is_residual_unique_mail(p));

    for leaf in expected_leaf_folders {
        if leaf.trim().is_empty() {
            continue;
        }
        if out_lower.iter().any(|p| folder_leaf_matches(p, leaf)) {
            continue;
        }
        // Residual Unique Mail: only when the *expected* path maps to residual,
        // not when any missing leaf is waved through because Unique Mail exists.
        if is_residual_unique_mail(leaf) && out_has_residual {
            continue;
        }
        return false;
    }
    true
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
    // Load volumes from summary or invent single volume.
    let volumes = load_volumes_for_qc(report_dir, out_pst)?;
    let export_rows = load_export_rows_for_qc(report_dir)?;
    let candidates = candidates_from_export_and_meta(&export_rows, &BTreeMap::new());

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

/// Detect parents_only / no-attachments export from summary.json.
fn load_parents_only_for_qc(report_dir: &Path) -> bool {
    let summary_path = report_dir.join("summary.json");
    let Ok(text) = fs::read_to_string(&summary_path) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
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
    false
}

fn load_volumes_for_qc(report_dir: &Path, out_pst: &Path) -> Result<Vec<VolumeReportRow>, String> {
    let summary_path = report_dir.join("summary.json");
    if summary_path.is_file() {
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&summary_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if let Some(arr) = v.pointer("/export/volumes").and_then(|x| x.as_array()) {
            let mut rows = Vec::new();
            for (i, item) in arr.iter().enumerate() {
                rows.push(VolumeReportRow {
                    volume_index: item
                        .get("volume_index")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(i as u64) as u32,
                    path: item
                        .get("path")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string(),
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
                    messages_written: item
                        .get("messages_written")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0),
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
                return Ok(rows);
            }
        }
    }
    // Fallback: single volume = out_pst
    let meta = fs::metadata(out_pst).map_err(|e| e.to_string())?;
    Ok(vec![VolumeReportRow {
        volume_index: 1,
        path: out_pst.display().to_string(),
        bytes: meta.len(),
        sha256_hex: String::new(),
        md5_hex: String::new(),
        messages_written: 0, // unknown — structure will report found
        finalized_early: false,
        volume_exceeded_soft_limit: false,
    }])
}

fn load_export_rows_for_qc(report_dir: &Path) -> Result<Vec<ExportMessageRow>, String> {
    let path = report_dir.join("export_messages.csv");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    let mut lines = text.lines();
    let _header = lines.next();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = split_csv_line(line);
        // source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,...
        if cols.len() < 9 {
            continue;
        }
        rows.push(ExportMessageRow {
            source_path: cols[0].to_string(),
            folder_path: cols[1].to_string(),
            nid: parse_nid(cols[2]),
            message_id_norm: cols[3].to_string(),
            edrm_mih: cols[4].to_string(),
            content_hash_hex: cols[5].to_string(),
            volume_path: cols[6].to_string(),
            volume_index: cols[7].parse().unwrap_or(1),
            export_message_index: cols[8].parse().unwrap_or((i as u64) + 1),
            attachments_failed_count: cols.get(9).and_then(|s| s.parse().ok()).unwrap_or(0),
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            subject: String::new(),
        });
    }
    Ok(rows)
}

fn parse_nid(s: &str) -> u64 {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).unwrap_or(0)
    } else {
        t.parse().unwrap_or(0)
    }
}

fn split_csv_line(line: &str) -> Vec<&str> {
    // Minimal CSV split (no embedded quotes in our export format for these cols).
    line.split(',').collect()
}

/// Hash bytes for negative tests.
pub fn sha256_hex_bytes(bytes: &[u8]) -> String {
    hex_sha256(bytes)
}

/// Deliberately corrupt a PST file (truncate last N bytes) for negative tests.
pub fn corrupt_pst_truncate(path: &Path, drop_tail: u64) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    let new_len = meta.len().saturating_sub(drop_tail);
    let f = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.set_len(new_len).map_err(|e| e.to_string())
}

/// Flip a byte in the middle of the file (payload corruption).
pub fn corrupt_pst_flip_byte(path: &Path, offset: u64) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut b = [0u8; 1];
    f.read_exact(&mut b).map_err(|e| e.to_string())?;
    b[0] ^= 0xFF;
    f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    f.write_all(&b).map_err(|e| e.to_string())
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
            message_digests: vec!["a".into(), "b".into()],
        };
        let expected: BTreeSet<String> = ["inbox".into(), "sent items".into()].into();
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
            message_digests: vec!["a".into()],
        };
        let expected: BTreeSet<String> = ["inbox".into()].into();
        assert!(folder_tree_matches(&digest, &expected, 1));

        let residual = VolumeStructuralDigest {
            message_count: 1,
            folder_paths: vec!["Unique Mail".into()],
            message_digests: vec!["a".into()],
        };
        let expected_res: BTreeSet<String> = ["unique mail".into()].into();
        assert!(folder_tree_matches(&residual, &expected_res, 1));
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
}
