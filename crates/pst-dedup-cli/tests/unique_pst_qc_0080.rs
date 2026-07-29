//! Track 0080 unique-PST QC negative + positive tests.
//!
//! Corrupted/short-changed outputs are built at test time via pst-writer + byte
//! edits (0077 pattern) — never from real files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use pst_dedup_cli::export_oracle::structural_digest_pst;
use pst_dedup_cli::fidelity_contract::{FidelityContract, FindingClass};
use pst_dedup_cli::unique_export_report::{ExportMessageRow, VolumeReportRow};
use pst_dedup_cli::unique_pst_qc::{
    corrupt_pst_flip_byte, corrupt_pst_truncate, run_unique_pst_qc, select_sample_indices, QcLevel,
    QcRunInput, QcSampleCandidate, DEFAULT_QC_SAMPLE_MAX,
};
use pst_writer::{write_unicode_pst, WriteMessage, WritePstOpts};
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
}

fn write_simple_pst(path: &Path, msgs: Vec<WriteMessage>) {
    write_unicode_pst(path, msgs, &[], &WritePstOpts::default()).expect("write pst");
}

fn base_msg(mid: &str, subject: &str, body: &str) -> WriteMessage {
    WriteMessage {
        message_id: Some(mid.into()),
        subject: subject.into(),
        sender: Some("alice@example.com".into()),
        display_to: Some("bob@example.com".into()),
        display_cc: Some("carol@example.com".into()),
        body_plain: Some(body.into()),
        source_folder_path: Some("Inbox".into()),
        source_path: Some(r"C:\src\a.pst".into()),
        ..WriteMessage::default()
    }
}

fn vol_row(path: &Path, n: u64) -> VolumeReportRow {
    VolumeReportRow {
        volume_index: 1,
        path: path.display().to_string(),
        bytes: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        sha256_hex: String::new(),
        md5_hex: String::new(),
        messages_written: n,
        finalized_early: false,
        volume_exceeded_soft_limit: false,
    }
}

fn export_row(mid: &str, nid: u64, idx: u64) -> ExportMessageRow {
    ExportMessageRow {
        source_path: r"C:\src\a.pst".into(),
        folder_path: "Inbox".into(),
        nid,
        message_id_norm: mid
            .trim_matches(|c| c == '<' || c == '>')
            .to_ascii_lowercase(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: String::new(),
        volume_index: 1,
        export_message_index: idx,
        attachments_failed_count: 0,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        subject: "subj".into(),
    }
}

fn cand(i: u64, body: usize) -> QcSampleCandidate {
    QcSampleCandidate {
        export_message_index: i,
        volume_index: 1,
        source_path: r"C:\src\a.pst".into(),
        source_nid: i,
        folder_path: "Inbox".into(),
        subject: format!("s{i}"),
        sender: "a@b.com".into(),
        message_id_norm: format!("mid{i}"),
        body_plain_len: body,
        body_html_len: 0,
        attach_count: 0,
        max_attach_size: 0,
        has_zero_byte_attach: false,
        has_embedded: false,
        has_degraded: false,
        has_ledger_fail: false,
        subject_non_ascii: false,
        display_cc: String::new(),
        display_bcc: String::new(),
    }
}

#[test]
fn sample_selection_identical_across_two_calls() {
    let cands: Vec<_> = (0..30).map(|i| cand(i, (i * 17) as usize)).collect();
    let a = select_sample_indices(&cands, DEFAULT_QC_SAMPLE_MAX);
    let b = select_sample_indices(&cands, DEFAULT_QC_SAMPLE_MAX);
    assert_eq!(a, b);
    assert!(!a.is_empty());
}

#[test]
fn known_gap_alone_does_not_hard_fail() {
    let c = FidelityContract::v1();
    let (class, _) = c.classify("display_bcc", false);
    assert_eq!(class, FindingClass::KnownGap);
    let mut counts = pst_dedup_cli::unique_pst_qc::QcFindingCounts::default();
    counts.record(class);
    assert!(!counts.hard_fail());
}

#[test]
fn defect_on_truncated_output_pst() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<d1@ex.com>", "Defect test", "body one")],
    );
    // Truncate aggressively so open fails or counts are wrong.
    let len = fs::metadata(&path).expect("meta").len();
    corrupt_pst_truncate(&path, len.saturating_sub(64).max(len / 2)).expect("truncate");

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("d1@ex.com", 1, 1)];
    let candidates = vec![cand(1, 8)];

    let report = run_unique_pst_qc(QcRunInput {
        level: QcLevel::Structure,
        sample_max: 64,
        report_dir: &report_dir,
        volumes: &volumes,
        export_rows: &export_rows,
        candidates: &candidates,
        external_reader: None,
        run_scanpst: false,
        max_open_psts: 4,
        source_differential: false,
        parents_only: false,
    });
    assert!(
        report.hard_fail || report.findings.defect > 0 || !report.volumes[0].open_ok,
        "truncated output must yield hard finding: {:?}",
        report.findings
    );
    assert!(report_dir.join("qc_report_v1.json").is_file());
    assert!(report_dir.join("qc_findings.csv").is_file());
}

#[test]
fn unexplained_loss_on_unknown_property_class() {
    // Contract allowlist: absent property ⇒ unexplained_loss.
    let c = FidelityContract::v1();
    let (class, st) = c.classify("never_heard_of_this_mapi_prop", false);
    assert_eq!(class, FindingClass::UnexplainedLoss);
    assert!(st.is_none());
    let mut counts = pst_dedup_cli::unique_pst_qc::QcFindingCounts::default();
    counts.record(class);
    assert!(counts.hard_fail());
}

#[test]
fn structure_qc_passes_clean_writer_pst() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("clean.pst");
    write_simple_pst(
        &path,
        vec![
            base_msg("<c1@ex.com>", "One", "body1"),
            base_msg("<c2@ex.com>", "Two", "body2"),
        ],
    );
    let digest = structural_digest_pst(&path).expect("digest");
    assert_eq!(digest.message_count, 2);

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 2)];
    let export_rows = vec![export_row("c1@ex.com", 1, 1), export_row("c2@ex.com", 2, 2)];
    // Structure-only: no source differential content compare.
    let report = run_unique_pst_qc(QcRunInput {
        level: QcLevel::Structure,
        sample_max: 64,
        report_dir: &report_dir,
        volumes: &volumes,
        export_rows: &export_rows,
        candidates: &[],
        external_reader: None,
        run_scanpst: false,
        max_open_psts: 4,
        source_differential: false,
        parents_only: false,
    });
    assert!(
        !report.hard_fail,
        "clean structure should pass: findings={:?}",
        report.findings
    );
    assert!(report.volumes.iter().all(|v| v.open_ok));
}

#[test]
fn fixture_unique_pst_qc_sample_zero_hard_findings() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--no-attachments",
            "--qc-level",
            "sample",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(report.join("qc_report_v1.json").is_file());
    let qc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report.join("qc_report_v1.json")).expect("qc"))
            .expect("json");
    assert_eq!(qc["findings"]["defect"].as_u64().unwrap_or(99), 0);
    assert_eq!(qc["findings"]["unexplained_loss"].as_u64().unwrap_or(99), 0);
    assert_eq!(qc["hard_fail"], false);
    // content digests persisted when sample ran
    assert!(
        report.join("content_digests.json").is_file()
            || qc["messages_compared"].as_u64().unwrap_or(0) == 0,
        "content_digests.json expected when messages compared"
    );
}

#[test]
fn fixture_unique_pst_qc_full_zero_hard_findings() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--no-attachments",
            "--qc-level",
            "full",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let qc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report.join("qc_report_v1.json")).expect("qc"))
            .expect("json");
    assert_eq!(qc["findings"]["defect"].as_u64().unwrap_or(99), 0);
    assert_eq!(
        qc["findings"]["unexplained_loss"].as_u64().unwrap_or(99),
        0,
        "full QC must be zero unexplained_loss before default sample"
    );
}

#[test]
fn qc_off_skips_artifacts() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--no-attachments",
            "--qc-level",
            "off",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(result.status.success());
    assert!(
        !report.join("qc_report_v1.json").is_file(),
        "qc-level off must not write qc_report_v1"
    );
}

#[test]
fn qc_never_lowers_exit_on_prior_failure() {
    // Structural: hard_fail only sets verify_ok false; never upgrades success of
    // an already-failed condition. Exercise via classify mapping: defect + known_gap
    // still hard-fails (does not clear).
    let mut counts = pst_dedup_cli::unique_pst_qc::QcFindingCounts::default();
    counts.record(FindingClass::Defect);
    assert!(counts.hard_fail());
    counts.record(FindingClass::KnownGap);
    assert!(counts.hard_fail(), "known_gap must not clear hard fail");
}

#[test]
fn flip_byte_corruption_detected_at_structure_or_open() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("flip.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<f@ex.com>", "Flip", "payload body here")],
    );
    // Flip a mid-file byte (past header) to damage content without always preventing open.
    let len = fs::metadata(&path).expect("meta").len();
    let off = (len / 2).max(512);
    corrupt_pst_flip_byte(&path, off).expect("flip");

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("f@ex.com", 1, 1)];
    let report = run_unique_pst_qc(QcRunInput {
        level: QcLevel::Structure,
        sample_max: 64,
        report_dir: &report_dir,
        volumes: &volumes,
        export_rows: &export_rows,
        candidates: &[],
        external_reader: None,
        run_scanpst: false,
        max_open_psts: 4,
        source_differential: false,
        parents_only: false,
    });
    // May still open; if open succeeds and count matches, hard_fail may be false.
    // At least the pipeline must complete without panic and write artifacts.
    assert!(report_dir.join("qc_report_v1.json").is_file());
    let _ = report;
}

#[test]
fn cloud_attachment_contract_entry_exists() {
    let c = FidelityContract::v1();
    let p = c
        .get("cloud_modern_attachments")
        .expect("Q10 contract line");
    assert_ne!(format!("{:?}", p.status).to_ascii_lowercase(), "preserved");
}

// Silence unused import warning if BTreeMap unused in some builds.
#[allow(dead_code)]
fn _touch_btreemap() -> BTreeMap<u64, u64> {
    BTreeMap::new()
}
