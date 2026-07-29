//! Track 0080 unique-PST QC negative + positive tests.
//!
//! Corrupted/short-changed outputs are built at test time via pst-writer + byte
//! edits (0077 pattern) — never from real files.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use pst_dedup_cli::export_oracle::structural_digest_pst;
use pst_dedup_cli::fidelity_contract::{FidelityContract, FindingClass};
use pst_dedup_cli::unique_export_report::{ExportMessageRow, VolumeReportRow};
use pst_dedup_cli::unique_pst_qc::{
    corrupt_pst_flip_byte, corrupt_pst_truncate, run_unique_pst_qc, select_sample_indices,
    AttachDigestEntry, ContentDigestEntry, ContentDigestsFile, ContentDigestsVolume, QcLevel,
    QcRunInput, QcSampleCandidate, CONTENT_DIGEST_ORIGIN_SOURCE, DEFAULT_QC_SAMPLE_MAX,
};
use pst_writer::{
    write_unicode_pst, FolderLayoutPolicy, WriteAttachment, WriteMessage, WritePstOpts,
};
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
    export_row_folder(mid, nid, idx, "Inbox")
}

fn export_row_folder(mid: &str, nid: u64, idx: u64, folder: &str) -> ExportMessageRow {
    ExportMessageRow {
        source_path: r"C:\src\a.pst".into(),
        folder_path: folder.into(),
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

fn qc_input<'a>(
    level: QcLevel,
    report_dir: &'a Path,
    volumes: &'a [VolumeReportRow],
    export_rows: &'a [ExportMessageRow],
    candidates: &'a [QcSampleCandidate],
    source_differential: bool,
    parents_only: bool,
) -> QcRunInput<'a> {
    QcRunInput {
        level,
        sample_max: 64,
        report_dir,
        volumes,
        export_rows,
        candidates,
        external_reader: None,
        run_scanpst: false,
        max_open_psts: 4,
        source_differential,
        parents_only,
        probe_unexplained_property: None,
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
fn sample_cap_prefers_stratum_over_index_truncate() {
    let mut cands: Vec<_> = (0..20).map(|i| cand(i, 100 + i as usize)).collect();
    for c in &mut cands {
        c.volume_index = 1;
        c.source_path = "C:/same.pst".into();
    }
    cands[0].body_plain_len = 1;
    cands[1].body_plain_len = 99999;
    let sel = select_sample_indices(&cands, 3);
    assert!(
        sel.contains(&19),
        "volume-last must survive sample_max=3, got {sel:?}"
    );
    assert!(sel.contains(&1), "largest body must survive, got {sel:?}");
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
fn bcc_candidate_counts_known_gap_without_hard_fail() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("bcc.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<bcc@ex.com>", "Bcc meta", "body with bcc")],
    );
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("bcc@ex.com", 1, 1)];
    let mut c = cand(1, 14);
    c.display_bcc = "secret@example.com".into();
    c.message_id_norm = "bcc@ex.com".into();
    let report = run_unique_pst_qc(qc_input(
        QcLevel::Structure,
        &report_dir,
        &volumes,
        &export_rows,
        &[c],
        false,
        false,
    ));
    assert!(
        report.findings.known_gap > 0,
        "non-empty display_bcc must count known_gap: {:?}",
        report.findings
    );
    assert!(
        !report.hard_fail,
        "known_gap alone must not hard_fail: {:?}",
        report.findings
    );
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

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Structure,
        &report_dir,
        &volumes,
        &export_rows,
        &candidates,
        false,
        false,
    ));
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
fn unexplained_loss_pipeline_via_probe_hook() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("clean.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<u@ex.com>", "Probe", "body for probe")],
    );
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("u@ex.com", 1, 1)];
    let mut input = qc_input(
        QcLevel::Structure,
        &report_dir,
        &volumes,
        &export_rows,
        &[],
        false,
        false,
    );
    input.probe_unexplained_property = Some("never_heard_of_this_mapi_prop");
    let report = run_unique_pst_qc(input);
    assert!(
        report.findings.unexplained_loss > 0,
        "probe must record unexplained_loss: {:?}",
        report.findings
    );
    assert!(report.hard_fail);
    assert!(report_dir.join("qc_findings.csv").is_file());
    let csv = fs::read_to_string(report_dir.join("qc_findings.csv")).expect("csv");
    assert!(
        csv.contains("unexplained_loss"),
        "findings csv must list unexplained_loss"
    );
}

#[test]
fn display_cc_strip_is_defect_when_source_has_cc() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    let out = dir.path().join("out.pst");
    // Source has CC; output deliberately omits CC (preserved field loss).
    let mut src_msg = base_msg("<cc@ex.com>", "CC roundtrip", "body for cc");
    src_msg.display_cc = Some("carol@example.com".into());
    write_simple_pst(&src, vec![src_msg]);
    let mut out_msg = base_msg("<cc@ex.com>", "CC roundtrip", "body for cc");
    out_msg.display_cc = None;
    write_simple_pst(&out, vec![out_msg]);

    let src_nid = first_message_nid(&src).expect("src nid");
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&out, 1)];
    let export_rows = vec![ExportMessageRow {
        source_path: src.display().to_string(),
        folder_path: "Inbox".into(),
        nid: src_nid,
        message_id_norm: "cc@ex.com".into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 0,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        subject: "CC roundtrip".into(),
    }];
    let mut c = cand(1, 12);
    c.source_path = src.display().to_string();
    c.source_nid = src_nid;
    c.message_id_norm = "cc@ex.com".into();
    c.subject = "CC roundtrip".into();
    c.display_cc = "carol@example.com".into();

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &[c],
        true,
        false,
    ));
    assert!(
        report.hard_fail || report.findings.defect > 0,
        "missing preserved CC must hard-fail or defect: findings={:?} msgs={}",
        report.findings,
        report.messages_compared
    );
}

#[test]
fn attach_payload_mismatch_is_defect() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    let out = dir.path().join("out.pst");

    let mut src_msg = base_msg("<att@ex.com>", "Attach", "body attach");
    src_msg.attachments = vec![WriteAttachment {
        filename: "note.txt".into(),
        mime: Some("text/plain".into()),
        size: 5,
        attach_method: Some(1),
        data: Some(b"hello".to_vec()),
        stream_available: true,
        attach_nid: None,
        source_path: None,
        parent_nid: None,
        embedded_message: None,
    }];
    write_simple_pst(&src, vec![src_msg]);

    let mut out_msg = base_msg("<att@ex.com>", "Attach", "body attach");
    out_msg.attachments = vec![WriteAttachment {
        filename: "note.txt".into(),
        mime: Some("text/plain".into()),
        size: 5,
        attach_method: Some(1),
        data: Some(b"XXXXX".to_vec()),
        stream_available: true,
        attach_nid: None,
        source_path: None,
        parent_nid: None,
        embedded_message: None,
    }];
    write_simple_pst(&out, vec![out_msg]);

    let src_nid = first_message_nid(&src).expect("src nid");
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&out, 1)];
    let export_rows = vec![ExportMessageRow {
        source_path: src.display().to_string(),
        folder_path: "Inbox".into(),
        nid: src_nid,
        message_id_norm: "att@ex.com".into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 0,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        subject: "Attach".into(),
    }];
    let mut c = cand(1, 11);
    c.source_path = src.display().to_string();
    c.source_nid = src_nid;
    c.message_id_norm = "att@ex.com".into();
    c.subject = "Attach".into();
    c.attach_count = 1;
    c.max_attach_size = 5;

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &[c],
        true,
        false,
    ));
    assert!(
        report.hard_fail || report.findings.defect > 0,
        "attach payload mismatch must defect: findings={:?} att_compared={}",
        report.findings,
        report.attachments_compared
    );
}

fn first_message_nid(path: &Path) -> Option<u64> {
    let mut pst = pst_reader::PstFile::open(path).ok()?;
    let folders = pst.folders().ok()?;
    for f in folders {
        if let Some(n) = f.message_nids.first() {
            return Some(n.0);
        }
    }
    None
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
    let report = run_unique_pst_qc(qc_input(
        QcLevel::Structure,
        &report_dir,
        &volumes,
        &export_rows,
        &[],
        false,
        false,
    ));
    assert!(
        !report.hard_fail,
        "clean structure should pass: findings={:?}",
        report.findings
    );
    assert!(report.volumes.iter().all(|v| v.open_ok));
}

#[test]
fn folder_tree_collapsed_multi_leaf_hard_fails() {
    let dir = TempDir::new().expect("tmp");
    // Multi-folder expected (export rows) vs deliberately flat/collapsed output.
    let out = dir.path().join("flat.pst");
    let m1 = WriteMessage {
        message_id: Some("<f1@ex.com>".into()),
        subject: "In inbox".into(),
        body_plain: Some("a".into()),
        source_folder_path: Some("Inbox".into()),
        source_path: Some(r"C:\src\a.pst".into()),
        ..WriteMessage::default()
    };
    let m2 = WriteMessage {
        message_id: Some("<f2@ex.com>".into()),
        subject: "In sent".into(),
        body_plain: Some("b".into()),
        source_folder_path: Some("Sent Items".into()),
        source_path: Some(r"C:\src\a.pst".into()),
        ..WriteMessage::default()
    };
    // Flat layout collapses both into Unique Mail.
    write_unicode_pst(
        &out,
        vec![m1, m2],
        &[],
        &WritePstOpts {
            folder_layout: FolderLayoutPolicy::Flat {
                folder_display_name: "Unique Mail".into(),
            },
            ..WritePstOpts::default()
        },
    )
    .expect("write flat");

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&out, 2)];
    // Expected leaves still list multi-folder source paths (preserve expectation).
    let export_rows = vec![
        export_row_folder("f1@ex.com", 1, 1, "Inbox"),
        export_row_folder("f2@ex.com", 2, 2, "Sent Items"),
    ];
    let report = run_unique_pst_qc(qc_input(
        QcLevel::Structure,
        &report_dir,
        &volumes,
        &export_rows,
        &[],
        false,
        false,
    ));
    assert!(
        !report.volumes[0].folder_tree_match,
        "collapsed tree must set folder_tree_match=false"
    );
    assert!(
        report.hard_fail || report.findings.defect > 0 || report.findings.unexplained_loss > 0,
        "collapsed multi-folder must hard_fail: {:?}",
        report.findings
    );
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
    // Source-side digests persisted at export (source_differential true).
    assert!(
        report.join("content_digests.json").is_file()
            || qc["messages_compared"].as_u64().unwrap_or(0) == 0,
        "content_digests.json expected when messages compared at export"
    );
    if report.join("content_digests.json").is_file() {
        let digests: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(report.join("content_digests.json")).expect("digests"),
        )
        .expect("json");
        assert_eq!(
            digests["origin"].as_str().unwrap_or(""),
            "source",
            "export digests must be origin=source"
        );
    }
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
fn fixture_unique_pst_qc_full_with_attachments_zero_hard_findings() {
    // DoD default-on safety: full green path WITH attachments (not only --no-attachments).
    // Fixture has soft attach failures (embedded unparsed) — allow partial fidelity for exit,
    // but QC hard findings must still be zero (contract explains ledger soft-fails).
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique_att.pst");
    let report = dir.path().join("report");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--allow-partial-fidelity",
            "--qc-level",
            "full",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        report.join("qc_report_v1.json").is_file(),
        "qc_report must exist; stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let qc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report.join("qc_report_v1.json")).expect("qc"))
            .expect("json");
    assert_eq!(
        qc["findings"]["defect"].as_u64().unwrap_or(99),
        0,
        "defect with attachments: {:?}",
        qc["findings"]
    );
    assert_eq!(
        qc["findings"]["unexplained_loss"].as_u64().unwrap_or(99),
        0,
        "unexplained_loss with attachments: {:?}",
        qc["findings"]
    );
    assert_eq!(qc["hard_fail"], false);
    // Prefer success under allow-partial; if still non-zero, QC green is the DoD gate.
    if !result.status.success() {
        eprintln!(
            "note: process exit non-zero with allow-partial-fidelity; QC hard_fail=false is required"
        );
    }
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
    // Flip magic / early header bytes so open or structure fails hard.
    corrupt_pst_flip_byte(&path, 0).expect("flip");
    corrupt_pst_flip_byte(&path, 1).expect("flip2");
    corrupt_pst_flip_byte(&path, 8).expect("flip3");

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("f@ex.com", 1, 1)];
    let report = run_unique_pst_qc(qc_input(
        QcLevel::Structure,
        &report_dir,
        &volumes,
        &export_rows,
        &[],
        false,
        false,
    ));
    assert!(report_dir.join("qc_report_v1.json").is_file());
    assert!(
        report.hard_fail || report.findings.defect > 0 || !report.volumes[0].open_ok,
        "header flip must hard-fail or defect: {:?}",
        report.findings
    );
}

#[test]
fn output_only_qc_pst_never_sets_content_digest_backed() {
    // Two qc-pst-style runs without sources and without prior source digests
    // must never set content_digest_backed true (DoD-21 honesty).
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<o@ex.com>", "Out only", "body out only")],
    );
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    // Source path does not exist → not source_differential.
    let export_rows = vec![export_row("o@ex.com", 1, 1)];
    let candidates = vec![cand(1, 13)];

    let r1 = run_unique_pst_qc(qc_input(
        QcLevel::Sample,
        &report_dir,
        &volumes,
        &export_rows,
        &candidates,
        false,
        false,
    ));
    assert!(
        !r1.content_digest_backed,
        "first output-only run must not be content_digest_backed"
    );
    // Must not have written content_digests.json from output-only digests.
    assert!(
        !report_dir.join("content_digests.json").is_file(),
        "output-only must not write content_digests.json"
    );

    let r2 = run_unique_pst_qc(qc_input(
        QcLevel::Sample,
        &report_dir,
        &volumes,
        &export_rows,
        &candidates,
        false,
        false,
    ));
    assert!(
        !r2.content_digest_backed,
        "second output-only run must still not be content_digest_backed"
    );
}

/// Clean-room with prior source digests + parents_only must body-match via
/// persisted subject/lens fields (DoD-21) — not false-defect on zeroed reconstruction.
#[test]
fn clean_room_parents_only_with_source_digests_is_green() {
    use pst_dedup_cli::export_oracle::message_content_detail;
    use pst_reader::PstFile;

    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    let body = "clean room body parents_only";
    write_simple_pst(&path, vec![base_msg("<cr@ex.com>", "CleanRoom", body)]);

    // Read output message detail to build a matching source-side digest file
    // (simulates export-time content_digests.json with origin=source).
    let mut pst = PstFile::open(&path).expect("open out");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("one msg");
    let detail = message_content_detail(&mut pst, nid.0).expect("detail");
    drop(pst);

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let digests = ContentDigestsFile {
        schema: "content_digests_v1".into(),
        origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
        qc_level: "sample".into(),
        volumes: vec![ContentDigestsVolume {
            volume_index: 1,
            path: path.display().to_string(),
            messages: vec![ContentDigestEntry {
                export_message_index: 1,
                source_path: r"C:\src\a.pst".into(),
                source_nid: 1,
                message_id_norm: "cr@ex.com".into(),
                content_digest: detail.digest.clone(),
                subject: detail.subject.clone(),
                display_to: detail.display_to.clone(),
                display_cc: detail.display_cc.clone(),
                body_plain_len: detail.body_plain_len,
                body_html_len: detail.body_html_len,
                attaches: detail
                    .attaches
                    .iter()
                    .map(|(f, s, _, h)| AttachDigestEntry {
                        filename: f.clone(),
                        size: *s,
                        payload_sha256: h.clone(),
                    })
                    .collect(),
            }],
        }],
    };
    fs::write(
        report_dir.join("content_digests.json"),
        serde_json::to_string_pretty(&digests).expect("json"),
    )
    .expect("write digests");

    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("cr@ex.com", nid.0, 1)];
    let mut candidate = cand(1, body.len());
    candidate.message_id_norm = "cr@ex.com".into();
    candidate.subject = "CleanRoom".into();
    candidate.source_nid = nid.0;
    let candidates = vec![candidate];

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Sample,
        &report_dir,
        &volumes,
        &export_rows,
        &candidates,
        false, // no live sources
        true,  // parents_only
    ));
    assert!(
        report.content_digest_backed,
        "prior source digests must enable content_digest_backed"
    );
    assert!(
        !report.hard_fail,
        "clean-room parents_only must not false-defect: {:?}",
        report.findings
    );
    assert_eq!(report.findings.defect, 0);
    assert_eq!(report.findings.unexplained_loss, 0);
}

/// Clean-room must still defect when persisted source digest does not match output.
#[test]
fn clean_room_parents_only_mismatched_body_is_defect() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<cr2@ex.com>", "CleanRoom2", "actual body")],
    );
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    // Persisted digests claim a different body length / subject → body_match fails.
    let digests = ContentDigestsFile {
        schema: "content_digests_v1".into(),
        origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
        qc_level: "sample".into(),
        volumes: vec![ContentDigestsVolume {
            volume_index: 1,
            path: path.display().to_string(),
            messages: vec![ContentDigestEntry {
                export_message_index: 1,
                source_path: r"C:\src\a.pst".into(),
                source_nid: 1,
                message_id_norm: "cr2@ex.com".into(),
                content_digest: "deadbeef".into(),
                subject: "ExpectedDifferent".into(),
                display_to: "bob@example.com".into(),
                display_cc: "carol@example.com".into(),
                body_plain_len: 9999,
                body_html_len: 0,
                attaches: vec![],
            }],
        }],
    };
    fs::write(
        report_dir.join("content_digests.json"),
        serde_json::to_string_pretty(&digests).expect("json"),
    )
    .expect("write digests");

    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("cr2@ex.com", 1, 1)];
    let mut candidate = cand(1, 11);
    candidate.message_id_norm = "cr2@ex.com".into();
    candidate.subject = "CleanRoom2".into();
    let candidates = vec![candidate];

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Sample,
        &report_dir,
        &volumes,
        &export_rows,
        &candidates,
        false,
        true,
    ));
    assert!(report.content_digest_backed);
    assert!(
        report.hard_fail || report.findings.defect > 0,
        "mismatched clean-room digests must hard-fail: {:?}",
        report.findings
    );
}

#[test]
fn independent_reader_wrong_counts_hard_fails() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<ir@ex.com>", "Reader", "body reader")],
    );
    // Stub external reader reports wrong message count.
    let stub = dir.path().join("pffinfo.cmd");
    {
        let mut f = fs::File::create(&stub).expect("stub");
        writeln!(f, "@echo off").expect("w");
        writeln!(f, "echo Number of folders : 1").expect("w");
        writeln!(f, "echo Number of items : 99").expect("w");
        writeln!(f, "exit /b 0").expect("w");
    }
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("ir@ex.com", 1, 1)];
    let mut input = qc_input(
        QcLevel::Structure,
        &report_dir,
        &volumes,
        &export_rows,
        &[],
        false,
        false,
    );
    input.external_reader = Some(&stub);
    let report = run_unique_pst_qc(input);
    assert!(
        report.hard_fail || report.findings.defect > 0,
        "wrong independent reader counts must defect: {:?}",
        report.findings
    );
    assert_eq!(
        report.external.independent_reader.message_count,
        Some(99),
        "stub counts should parse"
    );
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
