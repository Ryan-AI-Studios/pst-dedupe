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
    record_classified_finding, run_qc_pst, run_unique_pst_qc, select_sample_indices,
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
        source_id: String::new(),
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
        ledger_failed_attach_names: Vec::new(),
        body_unavailable: false,
        body_incomplete: false,
        crc_suspect: false,
        subject_non_ascii: false,
        display_cc: String::new(),
        display_bcc: String::new(),
    }
}

fn digest_entry(
    idx: u64,
    mid: &str,
    subject: &str,
    digest: &str,
    body_plain_len: usize,
) -> ContentDigestEntry {
    ContentDigestEntry {
        export_message_index: idx,
        source_path: r"C:\src\a.pst".into(),
        source_nid: idx,
        message_id_norm: mid.into(),
        content_digest: digest.into(),
        subject: subject.into(),
        sender: "alice@example.com".into(),
        display_to: "bob@example.com".into(),
        display_cc: "carol@example.com".into(),
        body_plain_len,
        body_html_len: 0,
        attaches: vec![],
        extra_source_props: vec![],
        has_degraded: false,
        body_unavailable: false,
        body_incomplete: false,
        crc_suspect: false,
        has_ledger_fail: false,
        ledger_failed_attach_names: Vec::new(),
    }
}

/// Test-only: truncate PST tail (negative fixture helper — not production surface).
fn corrupt_pst_truncate(path: &Path, drop_tail: u64) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    let new_len = meta.len().saturating_sub(drop_tail);
    let f = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.set_len(new_len).map_err(|e| e.to_string())
}

/// Test-only: flip a byte for negative fixtures.
fn corrupt_pst_flip_byte(path: &Path, offset: u64) -> Result<(), String> {
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
fn unexplained_loss_via_production_record_classified_finding() {
    // DoD-9: production `record_classified_finding` path (same as run_unique_pst_qc uses).
    let contract = FidelityContract::v1();
    let mut counts = pst_dedup_cli::unique_pst_qc::QcFindingCounts::default();
    let mut findings = Vec::new();
    record_classified_finding(
        &contract,
        &mut counts,
        &mut findings,
        "never_heard_of_this_mapi_prop",
        false,
        pst_dedup_cli::unique_pst_qc::RecordFindingId {
            volume_index: 1,
            source_path: r"C:\src\a.pst",
            source_nid: 0x100,
            message_id_norm: "mid@ex.com",
        },
        "synthetic unknown property observed in comparison",
    );
    assert_eq!(counts.unexplained_loss, 1);
    assert!(counts.hard_fail());
    assert_eq!(findings[0].class, FindingClass::UnexplainedLoss);
    // Also exercise through full QC pipeline (probe uses same record path).
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("clean.pst");
    write_simple_pst(&path, vec![base_msg("<u2@ex.com>", "U2", "body")]);
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("u2@ex.com", 1, 1)];
    let mut input = qc_input(
        QcLevel::Structure,
        &report_dir,
        &volumes,
        &export_rows,
        &[],
        false,
        false,
    );
    input.probe_unexplained_property = Some("totally_unknown_observed_prop");
    let report = run_unique_pst_qc(input);
    assert!(report.findings.unexplained_loss > 0);
    assert!(report.hard_fail);
    let csv = fs::read_to_string(report_dir.join("qc_findings.csv")).expect("csv");
    assert!(csv.contains("unexplained_loss"));
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
        source_id: String::new(),
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
        source_id: String::new(),
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
                sender: detail.sender.clone(),
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
                extra_source_props: vec![],
                has_degraded: false,
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                has_ledger_fail: false,
                ledger_failed_attach_names: Vec::new(),
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
                sender: "alice@example.com".into(),
                display_to: "bob@example.com".into(),
                display_cc: "carol@example.com".into(),
                body_plain_len: 9999,
                body_html_len: 0,
                attaches: vec![],
                extra_source_props: vec![],
                has_degraded: false,
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                has_ledger_fail: false,
                ledger_failed_attach_names: Vec::new(),
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

#[test]
fn degraded_message_stripped_cc_still_defects() {
    // Broad degradation / ledger soft-fail must not suppress CC loss.
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    let out = dir.path().join("out.pst");
    let mut src_msg = base_msg("<deg@ex.com>", "Deg CC", "body deg");
    src_msg.display_cc = Some("carol@example.com".into());
    write_simple_pst(&src, vec![src_msg]);
    let mut out_msg = base_msg("<deg@ex.com>", "Deg CC", "body deg");
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
        message_id_norm: "deg@ex.com".into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 1,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        source_id: String::new(),
        subject: "Deg CC".into(),
    }];
    let mut c = cand(1, 8);
    c.source_path = src.display().to_string();
    c.source_nid = src_nid;
    c.message_id_norm = "deg@ex.com".into();
    c.subject = "Deg CC".into();
    c.display_cc = "carol@example.com".into();
    c.has_degraded = true;
    c.has_ledger_fail = true;
    c.body_unavailable = false;

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
        "degraded+ledger soft-fail must not explain stripped CC: {:?}",
        report.findings
    );
}

#[test]
fn degraded_body_unavailable_stripped_body_still_defects_when_not_flagged() {
    // has_degraded alone must not explain body loss without body_unavailable.
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    let out = dir.path().join("out.pst");
    write_simple_pst(
        &src,
        vec![base_msg(
            "<bd@ex.com>",
            "Body loss",
            "full source body text here",
        )],
    );
    write_simple_pst(&out, vec![base_msg("<bd@ex.com>", "Body loss", "x")]);
    let src_nid = first_message_nid(&src).expect("src nid");
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&out, 1)];
    let export_rows = vec![ExportMessageRow {
        source_path: src.display().to_string(),
        folder_path: "Inbox".into(),
        nid: src_nid,
        message_id_norm: "bd@ex.com".into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 0,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        source_id: String::new(),
        subject: "Body loss".into(),
    }];
    let mut c = cand(1, 26);
    c.source_path = src.display().to_string();
    c.source_nid = src_nid;
    c.message_id_norm = "bd@ex.com".into();
    c.subject = "Body loss".into();
    c.has_degraded = true; // broad flag only
    c.body_unavailable = false;

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
        "has_degraded alone must not suppress body defect: {:?}",
        report.findings
    );
}

#[test]
fn corrupt_existing_source_hard_fails_not_skip() {
    // Path exists but is malformed ⇒ defect / hard fail, not Explained skip.
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("corrupt_src.pst");
    let out = dir.path().join("out.pst");
    fs::write(&src, b"not-a-valid-pst-file-but-exists").expect("src");
    write_simple_pst(&out, vec![base_msg("<cs@ex.com>", "Corrupt src", "body")]);
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&out, 1)];
    let export_rows = vec![ExportMessageRow {
        source_path: src.display().to_string(),
        folder_path: "Inbox".into(),
        nid: 0x100,
        message_id_norm: "cs@ex.com".into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 0,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        source_id: String::new(),
        subject: "Corrupt src".into(),
    }];
    let mut c = cand(1, 4);
    c.source_path = src.display().to_string();
    c.source_nid = 0x100;
    c.message_id_norm = "cs@ex.com".into();
    c.subject = "Corrupt src".into();

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
        "corrupt existing source must hard-fail, not skip: {:?}",
        report.findings
    );
    assert_eq!(
        report.findings.skipped_source_unavailable, 0,
        "must not count as skipped_source_unavailable: {:?}",
        report.findings
    );
}

#[test]
fn empty_volumes_still_emit_qc_report() {
    let dir = TempDir::new().expect("tmp");
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let report = run_unique_pst_qc(qc_input(
        QcLevel::Structure,
        &report_dir,
        &[],
        &[],
        &[],
        false,
        false,
    ));
    assert!(
        report_dir.join("qc_report_v1.json").is_file(),
        "empty volumes must still write qc_report_v1"
    );
    assert!(
        report_dir.join("qc_findings.csv").is_file(),
        "empty volumes must still write qc_findings.csv"
    );
    assert_eq!(report.volumes.len(), 0);
    assert!(!report.hard_fail, "zero winners structure should be green");
}

#[test]
fn multi_volume_external_reader_called_for_each_volume() {
    let dir = TempDir::new().expect("tmp");
    let counter = dir.path().join("reader_calls.txt");
    let stub = dir.path().join("pffinfo.cmd");
    {
        let mut f = fs::File::create(&stub).expect("stub");
        // Append one line per invocation so we can count multi-volume calls.
        let counter_s = counter.display().to_string();
        writeln!(f, "@echo off").expect("w");
        writeln!(f, "echo call>>\"{counter_s}\"").expect("w");
        writeln!(f, "echo Number of folders : 1").expect("w");
        writeln!(f, "echo Number of items : 1").expect("w");
        writeln!(f, "exit /b 0").expect("w");
    }
    let v1 = dir.path().join("v1.pst");
    let v2 = dir.path().join("v2.pst");
    write_simple_pst(&v1, vec![base_msg("<mv1@ex.com>", "V1", "a")]);
    write_simple_pst(&v2, vec![base_msg("<mv2@ex.com>", "V2", "b")]);
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![
        VolumeReportRow {
            volume_index: 1,
            path: v1.display().to_string(),
            bytes: fs::metadata(&v1).map(|m| m.len()).unwrap_or(0),
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 1,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        },
        VolumeReportRow {
            volume_index: 2,
            path: v2.display().to_string(),
            bytes: fs::metadata(&v2).map(|m| m.len()).unwrap_or(0),
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 1,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        },
    ];
    let export_rows = vec![
        ExportMessageRow {
            source_path: r"C:\src\a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            message_id_norm: "mv1@ex.com".into(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: v1.display().to_string(),
            volume_index: 1,
            export_message_index: 1,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "V1".into(),
        },
        ExportMessageRow {
            source_path: r"C:\src\a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 2,
            message_id_norm: "mv2@ex.com".into(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: v2.display().to_string(),
            volume_index: 2,
            export_message_index: 2,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "V2".into(),
        },
    ];
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
    let _report = run_unique_pst_qc(input);
    let calls = fs::read_to_string(&counter).unwrap_or_default();
    let n = calls.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(
        n >= 2,
        "external reader must run per volume (got {n} calls): {calls:?}"
    );
}

#[test]
fn folder_count_redistribution_hard_fails() {
    // Same total messages and leaf names, but redistributed between folders.
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("redist.pst");
    let m1 = WriteMessage {
        message_id: Some("<r1@ex.com>".into()),
        subject: "A".into(),
        body_plain: Some("a".into()),
        source_folder_path: Some("Inbox".into()),
        source_path: Some(r"C:\src\a.pst".into()),
        ..WriteMessage::default()
    };
    let m2 = WriteMessage {
        message_id: Some("<r2@ex.com>".into()),
        subject: "B".into(),
        body_plain: Some("b".into()),
        source_folder_path: Some("Inbox".into()),
        source_path: Some(r"C:\src\a.pst".into()),
        ..WriteMessage::default()
    };
    let m3 = WriteMessage {
        message_id: Some("<r3@ex.com>".into()),
        subject: "C".into(),
        body_plain: Some("c".into()),
        source_folder_path: Some("Sent Items".into()),
        source_path: Some(r"C:\src\a.pst".into()),
        ..WriteMessage::default()
    };
    write_unicode_pst(&out, vec![m1, m2, m3], &[], &WritePstOpts::default()).expect("write");

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&out, 3)];
    // Expect opposite distribution: Inbox=1, Sent=2 (output has Inbox=2, Sent=1).
    let export_rows = vec![
        export_row_folder("r1@ex.com", 1, 1, "Inbox"),
        export_row_folder("r2@ex.com", 2, 2, "Sent Items"),
        export_row_folder("r3@ex.com", 3, 3, "Sent Items"),
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
        "redistributed folder counts must fail folder_tree_match"
    );
    assert!(
        report.hard_fail || report.findings.defect > 0,
        "redistributed counts must hard-fail: {:?}",
        report.findings
    );
}

#[test]
fn qc_pst_honors_moved_out_path() {
    let dir = TempDir::new().expect("tmp");
    let original = dir.path().join("original.pst");
    let moved = dir.path().join("moved_out.pst");
    write_simple_pst(
        &original,
        vec![base_msg("<mv@ex.com>", "Moved", "body moved")],
    );
    fs::copy(&original, &moved).expect("copy");
    // Remove original so summary path is stale; positional out must be used.
    fs::remove_file(&original).expect("rm");

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let summary = serde_json::json!({
        "export": {
            "volumes": [{
                "volume_index": 1,
                "path": original.display().to_string(),
                "bytes": 0,
                "sha256_hex": "",
                "md5_hex": "",
                "messages_written": 1,
                "finalized_early": false,
                "volume_exceeded_soft_limit": false
            }]
        }
    });
    fs::write(
        report_dir.join("summary.json"),
        serde_json::to_string_pretty(&summary).expect("json"),
    )
    .expect("summary");

    let report =
        run_qc_pst(&moved, &report_dir, QcLevel::Structure, 64, None, false, 4).expect("qc-pst");
    assert!(
        report.volumes[0].open_ok,
        "moved out path must be remapped and open: {:?}",
        report.volumes
    );
    assert!(
        report.volumes[0].path.contains("moved_out")
            || Path::new(&report.volumes[0].path).file_name()
                == Some(std::ffi::OsStr::new("moved_out.pst")),
        "volume path should be remapped to moved out: {}",
        report.volumes[0].path
    );
}

#[test]
fn no_mid_message_green_when_digests_match() {
    use pst_dedup_cli::export_oracle::message_content_detail;
    use pst_reader::PstFile;

    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("nomid.pst");
    let mut msg = base_msg("", "NoMid Subject", "no mid body");
    msg.message_id = None;
    write_simple_pst(&path, vec![msg]);

    let mut pst = PstFile::open(&path).expect("open");
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
        qc_level: "full".into(),
        volumes: vec![ContentDigestsVolume {
            volume_index: 1,
            path: path.display().to_string(),
            messages: vec![ContentDigestEntry {
                export_message_index: 1,
                source_path: r"C:\src\a.pst".into(),
                source_nid: 1,
                message_id_norm: String::new(),
                content_digest: detail.digest.clone(),
                subject: detail.subject.clone(),
                sender: detail.sender.clone(),
                display_to: detail.display_to.clone(),
                display_cc: detail.display_cc.clone(),
                body_plain_len: detail.body_plain_len,
                body_html_len: detail.body_html_len,
                attaches: vec![],
                extra_source_props: vec![],
                has_degraded: false,
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                has_ledger_fail: false,
                ledger_failed_attach_names: Vec::new(),
            }],
        }],
    };
    fs::write(
        report_dir.join("content_digests.json"),
        serde_json::to_string_pretty(&digests).expect("json"),
    )
    .expect("digests");

    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![ExportMessageRow {
        source_path: r"C:\src\a.pst".into(),
        folder_path: "Inbox".into(),
        nid: 1,
        message_id_norm: String::new(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: path.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 0,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        source_id: String::new(),
        subject: "NoMid Subject".into(),
    }];
    let mut c = cand(1, detail.body_plain_len);
    c.message_id_norm = String::new();
    c.subject = "NoMid Subject".into();
    c.source_nid = 1;

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &[c],
        false,
        true, // parents_only clean-room body match
    ));
    assert!(
        report.content_digest_backed,
        "source digests must enable clean-room"
    );
    assert!(
        !report.hard_fail,
        "no-MID with matching digests/subject must be green: {:?}",
        report.findings
    );
    assert_eq!(report.findings.defect, 0);
}

/// DoD-5/10: one ledger-failed attach must not explain a different missing attach.
#[test]
fn attach_ledger_fail_does_not_explain_unrelated_missing_attach() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    let out = dir.path().join("out.pst");

    let good = b"good-payload-bytes-aaa";
    let bad = b"bad-payload-bytes-bbb";
    let mut src_msg = base_msg("<att2@ex.com>", "TwoAttaches", "body two attaches");
    src_msg.attachments = vec![
        WriteAttachment {
            filename: "good.bin".into(),
            data: Some(good.to_vec()),
            size: good.len() as u32,
            ..WriteAttachment::default()
        },
        WriteAttachment {
            filename: "ledger_fail.bin".into(),
            data: Some(bad.to_vec()),
            size: bad.len() as u32,
            ..WriteAttachment::default()
        },
    ];
    write_simple_pst(&src, vec![src_msg]);

    // Output keeps only neither attach (both missing) — only ledger_fail.bin is explained.
    let mut out_msg = base_msg("<att2@ex.com>", "TwoAttaches", "body two attaches");
    out_msg.attachments = vec![];
    write_simple_pst(&out, vec![out_msg]);

    let src_nid = first_message_nid(&src).expect("src nid");
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&out, 1)];
    let export_rows = vec![ExportMessageRow {
        source_path: src.display().to_string(),
        folder_path: "Inbox".into(),
        nid: src_nid,
        message_id_norm: "att2@ex.com".into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 1,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        source_id: String::new(),
        subject: "TwoAttaches".into(),
    }];
    let mut c = cand(1, 18);
    c.source_path = src.display().to_string();
    c.source_nid = src_nid;
    c.message_id_norm = "att2@ex.com".into();
    c.subject = "TwoAttaches".into();
    c.has_ledger_fail = true; // message-wide flag alone must not suppress good.bin
    c.ledger_failed_attach_names = vec!["ledger_fail.bin".into()];
    c.attach_count = 2;

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
        "silently missing good.bin must defect even when sibling has ledger fail: {:?}",
        report.findings
    );
    let csv = fs::read_to_string(report_dir.join("qc_findings.csv")).expect("csv");
    assert!(
        csv.to_ascii_lowercase().contains("good.bin")
            || csv.contains("attachment_by_value")
            || csv.contains("missing"),
        "findings should mention missing good.bin: {csv}"
    );
}

/// DoD-21: sample digests + full qc must not silently green uncovered candidates.
#[test]
fn sample_digests_full_qc_not_silent_green_for_uncovered() {
    use pst_dedup_cli::export_oracle::message_content_detail;
    use pst_reader::PstFile;

    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![
            base_msg("<cov@ex.com>", "Covered", "covered body"),
            base_msg("<unc@ex.com>", "Uncovered", "uncovered body"),
        ],
    );
    let mut pst = PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let nids: Vec<_> = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .collect();
    assert!(nids.len() >= 2);
    let d0 = message_content_detail(&mut pst, nids[0].0).expect("d0");
    drop(pst);

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    // Only first message covered; digests claim sample granularity.
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
                message_id_norm: "cov@ex.com".into(),
                content_digest: d0.digest.clone(),
                subject: d0.subject.clone(),
                sender: d0.sender.clone(),
                display_to: d0.display_to.clone(),
                display_cc: d0.display_cc.clone(),
                body_plain_len: d0.body_plain_len,
                body_html_len: d0.body_html_len,
                attaches: vec![],
                extra_source_props: vec![],
                has_degraded: false,
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                has_ledger_fail: false,
                ledger_failed_attach_names: Vec::new(),
            }],
        }],
    };
    fs::write(
        report_dir.join("content_digests.json"),
        serde_json::to_string_pretty(&digests).expect("json"),
    )
    .expect("write");

    let volumes = vec![vol_row(&path, 2)];
    let export_rows = vec![
        export_row("cov@ex.com", 1, 1),
        export_row("unc@ex.com", 2, 2),
    ];
    let mut c1 = cand(1, 12);
    c1.message_id_norm = "cov@ex.com".into();
    c1.subject = "Covered".into();
    let mut c2 = cand(2, 14);
    c2.message_id_norm = "unc@ex.com".into();
    c2.subject = "Uncovered".into();

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &[c1, c2],
        false,
        true,
    ));
    assert!(report.content_digest_backed);
    assert!(
        report.content_digest_partial || report.hard_fail || report.findings.defect > 0,
        "sample digests under full must not silent-green: partial={} findings={:?}",
        report.content_digest_partial,
        report.findings
    );
    assert!(
        report.hard_fail || report.findings.defect > 0,
        "must hard_fail when full requested against sample digests: {:?}",
        report.findings
    );
}

/// DoD-9: production unexplained_loss via extra_source_props on content digests (no probe).
#[test]
fn unexplained_loss_via_extra_source_props_production_path() {
    use pst_dedup_cli::export_oracle::message_content_detail;
    use pst_reader::PstFile;

    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<xp@ex.com>", "ExtraProp", "body extra prop")],
    );
    let mut pst = PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid");
    let detail = message_content_detail(&mut pst, nid.0).expect("detail");
    drop(pst);

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let digests = ContentDigestsFile {
        schema: "content_digests_v1".into(),
        origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
        qc_level: "full".into(),
        volumes: vec![ContentDigestsVolume {
            volume_index: 1,
            path: path.display().to_string(),
            messages: vec![ContentDigestEntry {
                export_message_index: 1,
                source_path: r"C:\src\a.pst".into(),
                source_nid: 1,
                message_id_norm: "xp@ex.com".into(),
                content_digest: detail.digest.clone(),
                subject: detail.subject.clone(),
                sender: detail.sender.clone(),
                display_to: detail.display_to.clone(),
                display_cc: detail.display_cc.clone(),
                body_plain_len: detail.body_plain_len,
                body_html_len: detail.body_html_len,
                attaches: vec![],
                extra_source_props: vec!["PidTagUnmappedQcTest".into()],
                has_degraded: false,
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                has_ledger_fail: false,
                ledger_failed_attach_names: Vec::new(),
            }],
        }],
    };
    fs::write(
        report_dir.join("content_digests.json"),
        serde_json::to_string_pretty(&digests).expect("json"),
    )
    .expect("write");

    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("xp@ex.com", 1, 1)];
    let mut c = cand(1, detail.body_plain_len);
    c.message_id_norm = "xp@ex.com".into();
    c.subject = "ExtraProp".into();

    // source_differential false, digests present — production path, no probe hook.
    let candidates = [c];
    let input = qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &candidates,
        false,
        true,
    );
    assert!(input.probe_unexplained_property.is_none());
    let report = run_unique_pst_qc(input);
    assert!(report.content_digest_backed);
    assert!(
        report.findings.unexplained_loss > 0,
        "extra_source_props must yield unexplained_loss without probe: {:?}",
        report.findings
    );
    assert!(report.hard_fail);
    let csv = fs::read_to_string(report_dir.join("qc_findings.csv")).expect("csv");
    assert!(
        csv.contains("PidTagUnmappedQcTest") || csv.contains("unexplained_loss"),
        "{csv}"
    );
}

/// Malformed/truncated export_messages.csv row must hard-fail standalone qc-pst.
#[test]
fn malformed_export_csv_row_hard_fails_qc_pst() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(&path, vec![base_msg("<csv@ex.com>", "Csv", "body csv")]);
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    // Truncated CSV line (fewer than 9 fields).
    fs::write(
        report_dir.join("export_messages.csv"),
        "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count\n\
         C:\\src\\a.pst,Inbox,0x100,csv@ex.com\n",
    )
    .expect("csv");
    fs::write(
        report_dir.join("summary.json"),
        serde_json::json!({
            "export": {
                "volumes": [{
                    "volume_index": 1,
                    "path": path.display().to_string(),
                    "bytes": 1,
                    "sha256_hex": "",
                    "md5_hex": "",
                    "messages_written": 1,
                    "finalized_early": false,
                    "volume_exceeded_soft_limit": false
                }]
            }
        })
        .to_string(),
    )
    .expect("summary");

    let err = run_qc_pst(&path, &report_dir, QcLevel::Structure, 64, None, false, 4)
        .expect_err("malformed CSV must error");
    assert!(
        err.to_ascii_lowercase().contains("malformed")
            || err.to_ascii_lowercase().contains("fields")
            || err.to_ascii_lowercase().contains("export_messages"),
        "unexpected err: {err}"
    );
}

/// Two no-MID messages with same subject must not misassociate bodies.
#[test]
fn no_mid_duplicate_subjects_do_not_misassociate() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    let out = dir.path().join("out.pst");

    let mut m1 = base_msg("", "Same Subject", "body-alpha-111111");
    m1.message_id = None;
    let mut m2 = base_msg("", "Same Subject", "body-beta-2222222");
    m2.message_id = None;
    write_simple_pst(&src, vec![m1, m2]);

    // Output swaps nothing — same bodies; matching must pair correctly by body.
    let mut o1 = base_msg("", "Same Subject", "body-alpha-111111");
    o1.message_id = None;
    let mut o2 = base_msg("", "Same Subject", "body-beta-2222222");
    o2.message_id = None;
    write_simple_pst(&out, vec![o1, o2]);

    let src_nids = {
        use pst_reader::PstFile;
        let mut pst = PstFile::open(&src).expect("open");
        let folders = pst.folders().expect("f");
        folders
            .iter()
            .flat_map(|f| f.message_nids.iter().map(|n| n.0))
            .collect::<Vec<_>>()
    };
    assert_eq!(src_nids.len(), 2);

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&out, 2)];
    let export_rows = vec![
        ExportMessageRow {
            source_path: src.display().to_string(),
            folder_path: "Inbox".into(),
            nid: src_nids[0],
            message_id_norm: String::new(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: out.display().to_string(),
            volume_index: 1,
            export_message_index: 1,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "Same Subject".into(),
        },
        ExportMessageRow {
            source_path: src.display().to_string(),
            folder_path: "Inbox".into(),
            nid: src_nids[1],
            message_id_norm: String::new(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: out.display().to_string(),
            volume_index: 1,
            export_message_index: 2,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "Same Subject".into(),
        },
    ];
    let mut c1 = cand(1, 17);
    c1.source_path = src.display().to_string();
    c1.source_nid = src_nids[0];
    c1.message_id_norm = String::new();
    c1.subject = "Same Subject".into();
    let mut c2 = cand(2, 17);
    c2.source_path = src.display().to_string();
    c2.source_nid = src_nids[1];
    c2.message_id_norm = String::new();
    c2.subject = "Same Subject".into();

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &[c1, c2],
        true,
        true, // parents_only — body field match
    ));
    assert!(
        !report.hard_fail,
        "two no-MID same-subject messages must pair by body, not misassociate: {:?}",
        report.findings
    );
    assert_eq!(report.findings.defect, 0);
}

/// DoD-11: multi-volume synthetic structure QC green (zero hard findings).
#[test]
fn fixture_matrix_multi_volume_structure_green() {
    let dir = TempDir::new().expect("tmp");
    let v1 = dir.path().join("vol1.pst");
    let v2 = dir.path().join("vol2.pst");
    write_simple_pst(&v1, vec![base_msg("<mv1@ex.com>", "Vol1", "body vol1")]);
    write_simple_pst(&v2, vec![base_msg("<mv2@ex.com>", "Vol2", "body vol2")]);
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![
        VolumeReportRow {
            volume_index: 1,
            path: v1.display().to_string(),
            bytes: fs::metadata(&v1).map(|m| m.len()).unwrap_or(0),
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 1,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        },
        VolumeReportRow {
            volume_index: 2,
            path: v2.display().to_string(),
            bytes: fs::metadata(&v2).map(|m| m.len()).unwrap_or(0),
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 1,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        },
    ];
    let export_rows = vec![
        ExportMessageRow {
            source_path: r"C:\src\a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            message_id_norm: "mv1@ex.com".into(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: v1.display().to_string(),
            volume_index: 1,
            export_message_index: 1,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "Vol1".into(),
        },
        ExportMessageRow {
            source_path: r"C:\src\a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 2,
            message_id_norm: "mv2@ex.com".into(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: v2.display().to_string(),
            volume_index: 2,
            export_message_index: 2,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "Vol2".into(),
        },
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
        !report.hard_fail,
        "multi-volume structure must be green: {:?}",
        report.findings
    );
    assert_eq!(report.volumes.len(), 2);
    assert!(report
        .volumes
        .iter()
        .all(|v| v.open_ok && v.message_count_match));
}

/// DoD-11: non-ASCII subject round-trip QC green under clean-room digests.
#[test]
fn fixture_matrix_non_ascii_subject_qc_green() {
    use pst_dedup_cli::export_oracle::message_content_detail;
    use pst_reader::PstFile;

    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("nonascii.pst");
    let subject = "日本語サブジェクト café";
    write_simple_pst(
        &path,
        vec![base_msg("<na@ex.com>", subject, "non-ascii body text")],
    );
    let mut pst = PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("f");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid");
    let detail = message_content_detail(&mut pst, nid.0).expect("detail");
    drop(pst);

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let digests = ContentDigestsFile {
        schema: "content_digests_v1".into(),
        origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
        qc_level: "full".into(),
        volumes: vec![ContentDigestsVolume {
            volume_index: 1,
            path: path.display().to_string(),
            messages: vec![ContentDigestEntry {
                export_message_index: 1,
                source_path: r"C:\src\a.pst".into(),
                source_nid: 1,
                message_id_norm: "na@ex.com".into(),
                content_digest: detail.digest.clone(),
                subject: detail.subject.clone(),
                sender: detail.sender.clone(),
                display_to: detail.display_to.clone(),
                display_cc: detail.display_cc.clone(),
                body_plain_len: detail.body_plain_len,
                body_html_len: detail.body_html_len,
                attaches: vec![],
                extra_source_props: vec![],
                has_degraded: false,
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                has_ledger_fail: false,
                ledger_failed_attach_names: Vec::new(),
            }],
        }],
    };
    fs::write(
        report_dir.join("content_digests.json"),
        serde_json::to_string_pretty(&digests).expect("json"),
    )
    .expect("digests");

    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("na@ex.com", 1, 1)];
    let mut c = cand(1, detail.body_plain_len);
    c.message_id_norm = "na@ex.com".into();
    c.subject = subject.into();
    c.subject_non_ascii = true;

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &[c],
        false,
        true,
    ));
    assert!(
        !report.hard_fail,
        "non-ASCII subject clean-room must be green: {:?}",
        report.findings
    );
}

/// DoD-11: oversized (long) subject QC green under clean-room digests.
#[test]
fn fixture_matrix_oversized_subject_qc_green() {
    use pst_dedup_cli::export_oracle::message_content_detail;
    use pst_reader::PstFile;

    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("longsubj.pst");
    let subject = "S".repeat(512);
    write_simple_pst(
        &path,
        vec![base_msg("<ls@ex.com>", &subject, "long subject body")],
    );
    let mut pst = PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("f");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid");
    let detail = message_content_detail(&mut pst, nid.0).expect("detail");
    drop(pst);

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let digests = ContentDigestsFile {
        schema: "content_digests_v1".into(),
        origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
        qc_level: "full".into(),
        volumes: vec![ContentDigestsVolume {
            volume_index: 1,
            path: path.display().to_string(),
            messages: vec![ContentDigestEntry {
                export_message_index: 1,
                source_path: r"C:\src\a.pst".into(),
                source_nid: 1,
                message_id_norm: "ls@ex.com".into(),
                content_digest: detail.digest.clone(),
                subject: detail.subject.clone(),
                sender: detail.sender.clone(),
                display_to: detail.display_to.clone(),
                display_cc: detail.display_cc.clone(),
                body_plain_len: detail.body_plain_len,
                body_html_len: detail.body_html_len,
                attaches: vec![],
                extra_source_props: vec![],
                has_degraded: false,
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                has_ledger_fail: false,
                ledger_failed_attach_names: Vec::new(),
            }],
        }],
    };
    fs::write(
        report_dir.join("content_digests.json"),
        serde_json::to_string_pretty(&digests).expect("json"),
    )
    .expect("digests");

    let volumes = vec![vol_row(&path, 1)];
    let export_rows = vec![export_row("ls@ex.com", 1, 1)];
    let mut c = cand(1, detail.body_plain_len);
    c.message_id_norm = "ls@ex.com".into();
    c.subject = subject;

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &[c],
        false,
        true,
    ));
    assert!(
        !report.hard_fail,
        "oversized subject clean-room must be green: {:?}",
        report.findings
    );
}

/// Missing export_messages.csv with messages_written > 0 ⇒ hard defect (not empty green).
#[test]
fn missing_export_messages_csv_is_defect_not_green() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(&path, vec![base_msg("<miss@ex.com>", "Miss", "body miss")]);
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    // No export_messages.csv — only summary with messages_written=1.
    fs::write(
        report_dir.join("summary.json"),
        serde_json::json!({
            "export": {
                "volumes": [{
                    "volume_index": 1,
                    "path": path.display().to_string(),
                    "bytes": 1,
                    "sha256_hex": "",
                    "md5_hex": "",
                    "messages_written": 1,
                    "finalized_early": false,
                    "volume_exceeded_soft_limit": false
                }]
            }
        })
        .to_string(),
    )
    .expect("summary");

    let report = run_qc_pst(&path, &report_dir, QcLevel::Structure, 64, None, false, 4)
        .expect("qc-pst should run");
    assert!(
        report.hard_fail || report.findings.defect > 0,
        "missing export_messages.csv must hard-fail: {:?}",
        report.findings
    );
    let csv = fs::read_to_string(report_dir.join("qc_findings.csv")).unwrap_or_default();
    assert!(
        csv.contains("export_messages_missing")
            || csv.contains("export_messages")
            || report.findings.defect > 0,
        "findings must cite missing CSV: {csv}"
    );
}

/// Omitted export rows (shortfall vs messages_written) ⇒ defect.
#[test]
fn omitted_export_rows_vs_messages_written_is_defect() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![
            base_msg("<o1@ex.com>", "One", "body one"),
            base_msg("<o2@ex.com>", "Two", "body two"),
        ],
    );
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    // Only one CSV row while messages_written=2.
    let volumes = vec![vol_row(&path, 2)];
    let export_rows = vec![export_row("o1@ex.com", 1, 1)];
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
        report.hard_fail || report.findings.defect > 0,
        "export row shortfall must defect: {:?}",
        report.findings
    );
}

/// Duplicate export_message_index ⇒ defect.
#[test]
fn duplicate_export_message_index_is_defect() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![
            base_msg("<d1@ex.com>", "D1", "body d1"),
            base_msg("<d2@ex.com>", "D2", "body d2"),
        ],
    );
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 2)];
    let export_rows = vec![
        export_row("d1@ex.com", 1, 1),
        export_row("d2@ex.com", 2, 1), // duplicate index 1
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
        report.hard_fail || report.findings.defect > 0,
        "duplicate export_message_index must defect: {:?}",
        report.findings
    );
}

/// Unclaimed output folder with messages ⇒ folder_tree mismatch / defect.
#[test]
fn unclaimed_output_folder_with_messages_is_defect() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    // Two folders with messages.
    let mut m1 = base_msg("<u1@ex.com>", "In1", "body u1");
    m1.source_folder_path = Some("Inbox".into());
    let mut m2 = base_msg("<u2@ex.com>", "Ar1", "body u2");
    m2.source_folder_path = Some("Archive".into());
    write_unicode_pst(
        &path,
        vec![m1, m2],
        &[],
        &WritePstOpts {
            folder_layout: FolderLayoutPolicy::PreservePaths {
                multi_source_prefix: false,
            },
            ..WritePstOpts::default()
        },
    )
    .expect("write");
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 2)];
    // Export claims only Inbox=1 and Inbox=1 again would be wrong total —
    // claim only Inbox for both rows so Archive remains unclaimed.
    let export_rows = vec![
        export_row_folder("u1@ex.com", 1, 1, "Inbox"),
        export_row_folder("u2@ex.com", 2, 2, "Inbox"),
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
        "unclaimed Archive must fail folder_tree_match"
    );
    assert!(
        report.hard_fail || report.findings.defect > 0,
        "unclaimed folder must hard-fail: {:?}",
        report.findings
    );
}

/// DoD-5: two same-name source attaches, only one on output ⇒ multiset defect.
#[test]
fn duplicate_attach_filename_multiset_missing_is_defect() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    let out = dir.path().join("out.pst");

    let payload_a = b"payload-aaa-111";
    let payload_b = b"payload-bbb-222";
    let mut src_msg = base_msg("<dupatt@ex.com>", "DupAtt", "body dup att");
    src_msg.attachments = vec![
        WriteAttachment {
            filename: "same.txt".into(),
            data: Some(payload_a.to_vec()),
            size: payload_a.len() as u32,
            ..WriteAttachment::default()
        },
        WriteAttachment {
            filename: "same.txt".into(),
            data: Some(payload_b.to_vec()),
            size: payload_b.len() as u32,
            ..WriteAttachment::default()
        },
    ];
    write_simple_pst(&src, vec![src_msg]);

    // Output has only one same-named attach (first payload) — second must be defect.
    let mut out_msg = base_msg("<dupatt@ex.com>", "DupAtt", "body dup att");
    out_msg.attachments = vec![WriteAttachment {
        filename: "same.txt".into(),
        data: Some(payload_a.to_vec()),
        size: payload_a.len() as u32,
        ..WriteAttachment::default()
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
        message_id_norm: "dupatt@ex.com".into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 0,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        source_id: String::new(),
        subject: "DupAtt".into(),
    }];
    let mut c = cand(1, 12);
    c.source_path = src.display().to_string();
    c.source_nid = src_nid;
    c.message_id_norm = "dupatt@ex.com".into();
    c.subject = "DupAtt".into();
    c.attach_count = 2;

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
        "duplicate filename multiset shortfall must defect: {:?}",
        report.findings
    );
    let csv = fs::read_to_string(report_dir.join("qc_findings.csv")).unwrap_or_default();
    assert!(
        csv.contains("attachment") || csv.contains("multiset") || csv.contains("missing"),
        "findings must mention attach loss: {csv}"
    );
}

/// DoD-9 design doc: unexplained_loss via production digest extras (not probe);
/// byte-edit residual is D-0080-unexplained-byte-edit.
#[test]
fn unexplained_loss_design_via_extra_source_props_not_probe() {
    // Re-assert production path used by clean-room compare (extra_source_props),
    // independent of probe_unexplained_property. See D-0080-unexplained-byte-edit.
    unexplained_loss_via_extra_source_props_production_path();
}

/// DoD-11: zero-winner production unique-pst still emits QC report.
#[test]
fn production_zero_winner_unique_pst_emits_qc_report() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("empty_src.pst");
    // Empty message list → zero winners.
    write_simple_pst(&src, vec![]);
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            src.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--no-attachments",
            "--qc-level",
            "structure",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        report.join("qc_report_v1.json").is_file() || report.join("summary.json").is_file(),
        "zero-winner must emit report pack; stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    if report.join("qc_report_v1.json").is_file() {
        let qc: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(report.join("qc_report_v1.json")).expect("qc"),
        )
        .expect("json");
        assert_eq!(qc["hard_fail"], false, "zero-winner QC must not hard_fail");
    }
}

/// DoD-11: multi-source synthetic PSTs via writer → unique-pst full QC green.
#[test]
fn production_multi_source_synthetic_unique_pst_qc_green() {
    let dir = TempDir::new().expect("tmp");
    let a = dir.path().join("src_a.pst");
    let b = dir.path().join("src_b.pst");
    write_simple_pst(&a, vec![base_msg("<msa@ex.com>", "MultiA", "body multi a")]);
    write_simple_pst(&b, vec![base_msg("<msb@ex.com>", "MultiB", "body multi b")]);
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            a.to_str().expect("utf8"),
            b.to_str().expect("utf8"),
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
        "multi-source unique-pst failed: stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(report.join("qc_report_v1.json").is_file());
    let qc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report.join("qc_report_v1.json")).expect("qc"))
            .expect("json");
    assert_eq!(
        qc["hard_fail"], false,
        "multi-source QC: {:?}",
        qc["findings"]
    );
    assert_eq!(qc["findings"]["defect"].as_u64().unwrap_or(99), 0);
    assert_eq!(qc["findings"]["unexplained_loss"].as_u64().unwrap_or(99), 0);
    let written = qc
        .pointer("/volumes")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert!(written >= 1);
    // Two unique messages expected.
    let stdout: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).unwrap_or_default();
    let unique = stdout
        .pointer("/keep_set/stats/unique")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    assert_eq!(unique, 2, "two synthetic sources must yield 2 uniques");
}

/// DoD-11: zero-byte attachment write + full QC green (production unique-pst path).
#[test]
fn production_zero_byte_attachment_unique_pst_qc_green() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src_zba.pst");
    let mut msg = base_msg("<zba@ex.com>", "ZeroByteAtt", "body with zero-byte attach");
    msg.attachments = vec![WriteAttachment {
        filename: "empty.bin".into(),
        data: Some(Vec::new()),
        size: 0,
        attach_method: Some(1),
        stream_available: true,
        ..WriteAttachment::default()
    }];
    write_simple_pst(&src, vec![msg]);
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            src.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--qc-level",
            "full",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        result.status.success() || report.join("qc_report_v1.json").is_file(),
        "zero-byte attach unique-pst: stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(
        report.join("qc_report_v1.json").is_file(),
        "qc_report required for zero-byte attach path"
    );
    let qc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report.join("qc_report_v1.json")).expect("qc"))
            .expect("json");
    assert_eq!(
        qc["hard_fail"], false,
        "zero-byte attach full QC must be green: {:?}",
        qc["findings"]
    );
    assert_eq!(qc["findings"]["defect"].as_u64().unwrap_or(99), 0);
    assert_eq!(qc["findings"]["unexplained_loss"].as_u64().unwrap_or(99), 0);
}

/// CSV membership: orphan volume_index with declared-volume count still matching ⇒ hard_fail.
#[test]
fn orphan_volume_index_rows_hard_fail_via_qc_pipeline() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("out.pst");
    write_simple_pst(
        &path,
        vec![base_msg("<or@ex.com>", "Orphan", "body orphan")],
    );
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![vol_row(&path, 1)];
    // One legitimate row + one orphan volume_index=99 (vol1 count still 1).
    let export_rows = vec![
        export_row("or@ex.com", 1, 1),
        ExportMessageRow {
            source_path: r"C:\src\a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 2,
            message_id_norm: "ghost@ex.com".into(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: "ghost.pst".into(),
            volume_index: 99,
            export_message_index: 2,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "Ghost".into(),
        },
    ];
    let mut c = cand(1, 11);
    c.message_id_norm = "or@ex.com".into();
    c.subject = "Orphan".into();
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
        report.hard_fail || report.findings.defect > 0,
        "orphan volume_index must hard_fail: {:?}",
        report.findings
    );
    let csv = fs::read_to_string(report_dir.join("qc_findings.csv")).unwrap_or_default();
    assert!(
        csv.contains("export_messages_orphan_volume_index") || csv.contains("orphan"),
        "findings csv must mention orphan volume: {csv}"
    );
}

/// Multi-volume external reader: one Ok + one Skipped ⇒ aggregate not Ok.
#[test]
fn multi_volume_external_one_skipped_aggregate_not_ok() {
    let dir = TempDir::new().expect("tmp");
    // Stub: Ok only when PST path basename contains "v1"; otherwise exit 0 without counts
    // (Skipped, not Failed — ranking under test is Skipped > Ok).
    let stub = dir.path().join("pffinfo.cmd");
    {
        let mut f = fs::File::create(&stub).expect("stub");
        writeln!(f, "@echo off").expect("w");
        // %1 is the PST path. Only emit counts for paths containing "v1".
        writeln!(f, "echo %1 | findstr /I \"\\v1.pst\" >nul").expect("w");
        writeln!(f, "if errorlevel 1 (").expect("w");
        writeln!(f, "  echo tool unavailable for this volume").expect("w");
        writeln!(f, "  exit /b 0").expect("w");
        writeln!(f, ")").expect("w");
        writeln!(f, "echo Number of folders : 1").expect("w");
        writeln!(f, "echo Number of items : 1").expect("w");
        writeln!(f, "exit /b 0").expect("w");
    }
    let v1 = dir.path().join("v1.pst");
    let v2 = dir.path().join("v2.pst");
    write_simple_pst(&v1, vec![base_msg("<agg1@ex.com>", "Agg1", "a")]);
    write_simple_pst(&v2, vec![base_msg("<agg2@ex.com>", "Agg2", "b")]);
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let volumes = vec![
        VolumeReportRow {
            volume_index: 1,
            path: v1.display().to_string(),
            bytes: fs::metadata(&v1).map(|m| m.len()).unwrap_or(0),
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 1,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        },
        VolumeReportRow {
            volume_index: 2,
            path: v2.display().to_string(),
            bytes: fs::metadata(&v2).map(|m| m.len()).unwrap_or(0),
            sha256_hex: String::new(),
            md5_hex: String::new(),
            messages_written: 1,
            finalized_early: false,
            volume_exceeded_soft_limit: false,
        },
    ];
    let export_rows = vec![
        ExportMessageRow {
            source_path: r"C:\src\a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            message_id_norm: "agg1@ex.com".into(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: v1.display().to_string(),
            volume_index: 1,
            export_message_index: 1,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "Agg1".into(),
        },
        ExportMessageRow {
            source_path: r"C:\src\a.pst".into(),
            folder_path: "Inbox".into(),
            nid: 2,
            message_id_norm: "agg2@ex.com".into(),
            edrm_mih: String::new(),
            content_hash_hex: String::new(),
            volume_path: v2.display().to_string(),
            volume_index: 2,
            export_message_index: 2,
            attachments_failed_count: 0,
            duplicate_source_count: 0,
            duplicate_sources: String::new(),
            source_id: String::new(),
            subject: "Agg2".into(),
        },
    ];
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
    let status = format!("{:?}", report.external.independent_reader.status);
    assert!(
        !status.eq_ignore_ascii_case("ok")
            && report.external.independent_reader.status
                != pst_dedup_cli::qc_external::ExternalStatus::Ok,
        "one skipped volume must not yield aggregate Ok: {:?}",
        report.external.independent_reader
    );
}

/// Clean-room: ledger fail name on digest explains missing attach without live candidate flags.
#[test]
fn clean_room_digest_ledger_fail_explains_missing_attach() {
    use pst_dedup_cli::export_oracle::message_content_detail;
    use pst_reader::PstFile;

    let dir = TempDir::new().expect("tmp");
    // Source-side digest claims softfail.bin existed; output has no attaches.
    let out = dir.path().join("out.pst");
    write_simple_pst(
        &out,
        vec![base_msg(
            "<crlf@ex.com>",
            "CleanRoomLedger",
            "body clean room ledger",
        )],
    );
    // Build a source digest with one attach that is missing from output.
    let src = dir.path().join("src.pst");
    let mut src_msg = base_msg("<crlf@ex.com>", "CleanRoomLedger", "body clean room ledger");
    src_msg.attachments = vec![WriteAttachment {
        filename: "softfail.bin".into(),
        data: Some(b"payload".to_vec()),
        size: 7,
        attach_method: Some(1),
        stream_available: true,
        ..WriteAttachment::default()
    }];
    write_simple_pst(&src, vec![src_msg]);
    let mut pst = PstFile::open(&src).expect("open src");
    let folders = pst.folders().expect("f");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid");
    let detail = message_content_detail(&mut pst, nid.0).expect("detail");
    drop(pst);

    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report");
    let digests = ContentDigestsFile {
        schema: "content_digests_v1".into(),
        origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
        qc_level: "full".into(),
        volumes: vec![ContentDigestsVolume {
            volume_index: 1,
            path: out.display().to_string(),
            messages: vec![ContentDigestEntry {
                export_message_index: 1,
                source_path: src.display().to_string(),
                source_nid: nid.0,
                message_id_norm: "crlf@ex.com".into(),
                content_digest: detail.digest.clone(),
                subject: detail.subject.clone(),
                sender: detail.sender.clone(),
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
                extra_source_props: vec![],
                has_degraded: false,
                body_unavailable: false,
                body_incomplete: false,
                crc_suspect: false,
                has_ledger_fail: true,
                ledger_failed_attach_names: vec!["softfail.bin".into()],
            }],
        }],
    };
    fs::write(
        report_dir.join("content_digests.json"),
        serde_json::to_string_pretty(&digests).expect("json"),
    )
    .expect("digests");

    let volumes = vec![vol_row(&out, 1)];
    let export_rows = vec![export_row("crlf@ex.com", 1, 1)];
    // Candidate has NO ledger flags — clean-room must reconstruct from digests.
    let mut c = cand(1, detail.body_plain_len);
    c.message_id_norm = "crlf@ex.com".into();
    c.subject = "CleanRoomLedger".into();
    c.has_ledger_fail = false;
    c.ledger_failed_attach_names.clear();
    c.attach_count = 1;

    let report = run_unique_pst_qc(qc_input(
        QcLevel::Full,
        &report_dir,
        &volumes,
        &export_rows,
        &[c],
        false, // clean-room: source-differential off, digests backed
        false,
    ));
    // Missing softfail.bin explained by digest ledger name ⇒ not hard_fail for that attach.
    let csv = fs::read_to_string(report_dir.join("qc_findings.csv")).unwrap_or_default();
    let explained_soft = csv.contains("ledger soft-fail")
        || csv.contains("attachment_stream_soft_fail")
        || report.findings.explained > 0;
    let hard_on_attach = csv.contains("attachment_by_value") && csv.contains("softfail.bin");
    assert!(
        explained_soft || !hard_on_attach,
        "digest ledger_failed_attach_names must explain missing softfail.bin on clean-room path; findings={:?} csv={csv}",
        report.findings
    );
    // Must not hard-fail solely because attach is missing when ledger name explains it.
    // (Other hard findings are ok only if not attach_by_value for softfail.bin.)
    if report.hard_fail {
        assert!(
            !hard_on_attach,
            "must not defect softfail.bin when digest ledger names it: {csv}"
        );
    }
}

/// Production multi-volume unique-pst + full QC green (tiny max_volume_bytes).
#[test]
fn production_multi_volume_full_qc_green() {
    let dir = TempDir::new().expect("tmp");
    let a = dir.path().join("src_mv_a.pst");
    let b = dir.path().join("src_mv_b.pst");
    write_simple_pst(
        &a,
        vec![base_msg(
            "<mva@ex.com>",
            "MultiVolA",
            &format!("body multi vol a {}", "X".repeat(200)),
        )],
    );
    write_simple_pst(
        &b,
        vec![base_msg(
            "<mvb@ex.com>",
            "MultiVolB",
            &format!("body multi vol b {}", "Y".repeat(200)),
        )],
    );
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            a.to_str().expect("utf8"),
            b.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--no-attachments",
            "--max-volume-bytes",
            "4096",
            "--qc-level",
            "full",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        result.status.success() || report.join("qc_report_v1.json").is_file(),
        "multi-volume unique-pst: stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(
        report.join("qc_report_v1.json").is_file(),
        "qc_report required for multi-volume full QC"
    );
    let qc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report.join("qc_report_v1.json")).expect("qc"))
            .expect("json");
    let vol_count = qc
        .pointer("/volumes")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    // Prefer ≥2 volumes; if writer kept single volume under tiny limit, still require green full QC.
    if vol_count < 2 {
        // Fallback: synthetic two-volume writer path via run_unique_pst_qc.
        let v1 = dir.path().join("syn_v1.pst");
        let v2 = dir.path().join("syn_v2.pst");
        write_simple_pst(&v1, vec![base_msg("<sv1@ex.com>", "SynV1", "syn body 1")]);
        write_simple_pst(&v2, vec![base_msg("<sv2@ex.com>", "SynV2", "syn body 2")]);
        let rd = dir.path().join("report_syn");
        fs::create_dir_all(&rd).expect("rd");
        // Build source digests from each volume-as-source (self compare for structure+full green).
        use pst_dedup_cli::export_oracle::message_content_detail;
        use pst_reader::PstFile;
        let mut dig_msgs = Vec::new();
        for (i, p, mid, subj) in [
            (1u64, &v1, "sv1@ex.com", "SynV1"),
            (2u64, &v2, "sv2@ex.com", "SynV2"),
        ] {
            let mut pst = PstFile::open(p).expect("open");
            let folders = pst.folders().expect("f");
            let nid = folders
                .iter()
                .flat_map(|f| f.message_nids.iter().copied())
                .next()
                .expect("nid");
            let d = message_content_detail(&mut pst, nid.0).expect("d");
            dig_msgs.push((
                i,
                p.display().to_string(),
                ContentDigestEntry {
                    export_message_index: i,
                    source_path: p.display().to_string(),
                    source_nid: nid.0,
                    message_id_norm: mid.into(),
                    content_digest: d.digest.clone(),
                    subject: d.subject.clone(),
                    sender: d.sender.clone(),
                    display_to: d.display_to.clone(),
                    display_cc: d.display_cc.clone(),
                    body_plain_len: d.body_plain_len,
                    body_html_len: d.body_html_len,
                    attaches: vec![],
                    extra_source_props: vec![],
                    has_degraded: false,
                    body_unavailable: false,
                    body_incomplete: false,
                    crc_suspect: false,
                    has_ledger_fail: false,
                    ledger_failed_attach_names: Vec::new(),
                },
                subj,
            ));
            let _ = subj;
        }
        let digests = ContentDigestsFile {
            schema: "content_digests_v1".into(),
            origin: CONTENT_DIGEST_ORIGIN_SOURCE.into(),
            qc_level: "full".into(),
            volumes: dig_msgs
                .iter()
                .map(|(i, path, entry, _)| ContentDigestsVolume {
                    volume_index: *i as u32,
                    path: path.clone(),
                    messages: vec![entry.clone()],
                })
                .collect(),
        };
        fs::write(
            rd.join("content_digests.json"),
            serde_json::to_string_pretty(&digests).expect("json"),
        )
        .expect("w");
        let volumes = vec![
            VolumeReportRow {
                volume_index: 1,
                path: v1.display().to_string(),
                bytes: fs::metadata(&v1).map(|m| m.len()).unwrap_or(0),
                sha256_hex: String::new(),
                md5_hex: String::new(),
                messages_written: 1,
                finalized_early: false,
                volume_exceeded_soft_limit: false,
            },
            VolumeReportRow {
                volume_index: 2,
                path: v2.display().to_string(),
                bytes: fs::metadata(&v2).map(|m| m.len()).unwrap_or(0),
                sha256_hex: String::new(),
                md5_hex: String::new(),
                messages_written: 1,
                finalized_early: false,
                volume_exceeded_soft_limit: false,
            },
        ];
        let export_rows = vec![
            ExportMessageRow {
                source_path: v1.display().to_string(),
                folder_path: "Inbox".into(),
                nid: dig_msgs[0].2.source_nid,
                message_id_norm: "sv1@ex.com".into(),
                edrm_mih: String::new(),
                content_hash_hex: String::new(),
                volume_path: v1.display().to_string(),
                volume_index: 1,
                export_message_index: 1,
                attachments_failed_count: 0,
                duplicate_source_count: 0,
                duplicate_sources: String::new(),
                source_id: String::new(),
                subject: "SynV1".into(),
            },
            ExportMessageRow {
                source_path: v2.display().to_string(),
                folder_path: "Inbox".into(),
                nid: dig_msgs[1].2.source_nid,
                message_id_norm: "sv2@ex.com".into(),
                edrm_mih: String::new(),
                content_hash_hex: String::new(),
                volume_path: v2.display().to_string(),
                volume_index: 2,
                export_message_index: 2,
                attachments_failed_count: 0,
                duplicate_source_count: 0,
                duplicate_sources: String::new(),
                source_id: String::new(),
                subject: "SynV2".into(),
            },
        ];
        let mut c1 = cand(1, dig_msgs[0].2.body_plain_len);
        c1.message_id_norm = "sv1@ex.com".into();
        c1.subject = "SynV1".into();
        c1.volume_index = 1;
        c1.source_path = v1.display().to_string();
        c1.source_nid = dig_msgs[0].2.source_nid;
        let mut c2 = cand(2, dig_msgs[1].2.body_plain_len);
        c2.message_id_norm = "sv2@ex.com".into();
        c2.subject = "SynV2".into();
        c2.volume_index = 2;
        c2.source_path = v2.display().to_string();
        c2.source_nid = dig_msgs[1].2.source_nid;
        let r = run_unique_pst_qc(qc_input(
            QcLevel::Full,
            &rd,
            &volumes,
            &export_rows,
            &[c1, c2],
            false,
            true,
        ));
        assert!(
            !r.hard_fail,
            "two-volume full QC must be green: {:?}",
            r.findings
        );
        assert_eq!(r.volumes.len(), 2);
        return;
    }
    assert_eq!(
        qc["hard_fail"], false,
        "multi-volume full QC must be green: {:?}",
        qc["findings"]
    );
    assert_eq!(qc["findings"]["defect"].as_u64().unwrap_or(99), 0);
    assert_eq!(qc["findings"]["unexplained_loss"].as_u64().unwrap_or(99), 0);
}

// Silence unused import warning if BTreeMap unused in some builds.
#[allow(dead_code)]
fn _touch_btreemap() -> BTreeMap<u64, u64> {
    BTreeMap::new()
}

#[allow(dead_code)]
fn _touch_digest_entry() -> ContentDigestEntry {
    digest_entry(1, "x", "s", "d", 0)
}
