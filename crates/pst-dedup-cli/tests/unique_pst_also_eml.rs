//! Track 0107: `unique-pst --also-eml` co-export from the same keep-set.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use assert_cmd::cargo::cargo_bin;
use dedup_engine::integrity::{RecoverableIntegrity, ScanMode};
use dedup_engine::keepset::{
    FamilyPolicy, KeepEntry, KeepPolicy, KeepSet, KeepSetStats, MessageLocus, SoftSkipAttachRecord,
};
use pst_dedup_cli::pst_materializer::{PstAttachStreamSource, PstMaterializer};
use pst_dedup_cli::unique_eml_cmd::{write_eml_pack_from_keep_set, WriteEmlPackFromKeepSetInput};
use pst_dedup_cli::unique_export_report::{
    AttachLedgerMode, LedgerPathMode, EXPORT_ATTACHMENTS_CSV_NAME,
};
use pst_dedup_cli::unique_pst_cmd::{
    run_unique_pst_with_options, FolderLayoutArg, UniquePstCliArgs, UniquePstRunOptions,
};
use pst_writer::{write_unicode_pst, WriteAttachment, WriteMessage, WritePstOpts};
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
}

fn base_msg(mid: &str, subject: &str) -> WriteMessage {
    WriteMessage {
        message_id: Some(mid.to_string()),
        subject: subject.to_string(),
        sender: Some("alice@example.com".to_string()),
        display_to: Some("bob@example.com".to_string()),
        submit_time: Some(0x01D5B035EDA780_i64),
        body_plain: Some("body".to_string()),
        source_folder_path: Some("Inbox".into()),
        ..Default::default()
    }
}

/// Copy of `unique_pst_depth::method5_chain` (integration crates cannot import each other).
fn method5_chain(nests: u32) -> WriteMessage {
    let mut leaf = base_msg("<leaf@ex.com>", "Leaf");
    for d in (1..=nests).rev() {
        let mut parent = base_msg(&format!("<d{d}@ex.com>"), &format!("Depth {d}"));
        parent.source_folder_path = Some("Inbox".into());
        parent.attachments.push(WriteAttachment {
            filename: format!("nested{d}.msg"),
            attach_method: Some(5),
            embedded_message: Some(Box::new(leaf)),
            ..Default::default()
        });
        leaf = parent;
    }
    leaf
}

fn write_method5_source(path: &Path, nests: u32) {
    let opts = WritePstOpts {
        max_embedded_depth: 8,
        ..WritePstOpts::default()
    };
    write_unicode_pst(path, vec![method5_chain(nests)], &[], &opts).expect("write source");
}

fn count_eml_files(dir: &Path) -> u64 {
    let mut n = 0u64;
    let Ok(rd) = fs::read_dir(dir) else {
        return 0;
    };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            n = n.saturating_add(count_eml_files(&p));
        } else if p.extension().and_then(|x| x.to_str()) == Some("eml") {
            n = n.saturating_add(1);
        }
    }
    n
}

fn pack_eml_text(out: &Path) -> String {
    let vol = out.join("VOL001");
    let mut combined = String::new();
    let Ok(entries) = fs::read_dir(&vol) else {
        return combined;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("eml") {
            combined.push_str(&fs::read_to_string(&p).expect("read eml"));
            combined.push('\n');
        }
    }
    combined
}

fn tiny_cli_args(src: PathBuf, out: PathBuf, report: PathBuf) -> UniquePstCliArgs {
    UniquePstCliArgs {
        paths: vec![src],
        out,
        report_dir: Some(report),
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::KeepAttachmentsWithParent,
        prefer_path_contains: vec![],
        prefer_bcc_copy: false,
        prefer_folder_class: false,
        folder_rank: vec![],
        source_rank: vec![],
        rank_folder_class_first: false,
        fidelity_rank: "binary".into(),
        decision_csv: None,
        keep_set_json: None,
        folder_layout: FolderLayoutArg::Preserve,
        max_volume_bytes: None,
        overwrite: false,
        verify_hash: false,
        also_eml: None,
        no_tier2: false,
        no_attachments: false,
        json: false,
        mode: ScanMode::BestEffort,
        max_skip_rate: 0.05,
        max_crc_skip_rate: 0.01,
        max_failed_file_rate: 0.0,
        allow_failed_files: false,
        integrity_csv: None,
        skip_limit: 10_000,
        attach_ledger: AttachLedgerMode::Full,
        attach_ledger_max_rows: 500_000,
        ledger_path_mode: LedgerPathMode::Full,
        deep_attach_preflight: false,
        deep_attach_level: "head".into(),
        deep_attach_max_attaches: 50_000,
        deep_attach_max_probe_bytes: 268_435_456,
        deep_attach_per_attach_max_bytes: 1_048_576,
        deep_attach_max_probe_time_ms: 2000,
        deep_attach_max_open_psts: 32,
        deep_attach_max_peer_probes: 3,
        max_attach_fail_rate: 0.05,
        strong_content_hash: "off".into(),
        strong_hash_attach_max_attaches: 50_000,
        strong_hash_attach_max_bytes: 1_073_741_824,
        strong_hash_attach_per_attach_max_bytes: 536_870_912,
        dedupe_scope: "global".into(),
        tier1_verify: "off".into(),
        tier1_backfill: false,
        identity_ignore_inline_attachments: false,
        allow_cross_mid_tier2: false,
        allow_degenerate_tier2: false,
        allow_crc_suspect_tier2: false,
        crc_log_limit: 10,
        crc_log_interval_secs: 30,
        fail_on_partial_fidelity: true,
        allow_partial_fidelity: false,
        fail_on_export_risk: None,
        max_open_psts: 32,
        qc_level: pst_dedup_cli::unique_pst_qc::QcLevel::Off,
        qc_sample_max: 64,
        qc_external_reader: None,
        qc_scanpst: false,
        include_bcc_recipients: false,
        promote_on_attach_fail: false,
        max_embedded_depth: 3,
    }
}

#[test]
fn aspose_also_eml_count_and_manifest() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let also = dir.path().join("also_eml");

    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--also-eml",
            also.to_str().expect("utf8"),
            "--qc-level",
            "off",
            "--no-attachments",
            "--json",
            "--allow-partial-fidelity",
        ])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stderr.to_ascii_lowercase().contains("not implemented"),
        "unimplemented warning must be gone: {stderr}"
    );
    assert!(result.status.success(), "stderr={stderr} stdout={stdout}");

    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["also_eml_ran"], true);
    assert!(v["also_eml_out"].as_str().is_some());
    let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
    assert!(unique > 0, "expected unique > 0");
    let eml_written = v["also_eml_eml_written"].as_u64().unwrap_or(0);
    assert_eq!(
        eml_written, unique,
        "also_eml_eml_written must equal unique"
    );

    let man_path = also.join("manifest.json");
    assert!(man_path.is_file(), "manifest.json missing");
    let man: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&man_path).expect("man")).expect("man json");
    assert_eq!(man["schema"].as_str(), Some("eml_pack_v1"));
    assert_eq!(count_eml_files(&also), eml_written);
}

#[test]
fn also_eml_parent_of_out_is_usage_error_before_clear() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let also = dir.path().join("case");
    fs::create_dir_all(&also).expect("mkdir also");
    let marker = also.join("must_survive.txt");
    fs::write(&marker, b"keep").expect("seed");
    let out = also.join("unique.pst");
    let report = dir.path().join("report");

    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--also-eml",
            also.to_str().expect("utf8"),
            "--overwrite",
            "--qc-level",
            "off",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(!result.status.success(), "parent also-eml must fail");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        combined.contains("--also-eml"),
        "usage must name --also-eml: {combined}"
    );
    assert!(
        marker.is_file(),
        "overwrite clear must not run when also-eml parents --out"
    );
    assert!(!out.exists(), "no PST write on guard failure");
}

#[test]
fn also_eml_equal_out_is_usage_error() {
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
            "--also-eml",
            out.to_str().expect("utf8"),
            "--qc-level",
            "off",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(!result.status.success(), "overlap must fail before write");
    assert!(
        !out.exists(),
        "PST must not be written on also-eml/--out overlap"
    );
}

#[test]
fn also_eml_equal_report_dir_is_usage_error() {
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
            "--also-eml",
            report.to_str().expect("utf8"),
            "--qc-level",
            "off",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(!result.status.success(), "report-dir overlap must fail");
    assert!(!out.exists(), "no PST write on overlap");
}

#[test]
fn also_eml_nonempty_without_overwrite_errors() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let also = dir.path().join("also_eml");
    fs::create_dir_all(&also).expect("mkdir");
    fs::write(also.join("existing.txt"), b"hi").expect("seed");

    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--also-eml",
            also.to_str().expect("utf8"),
            "--qc-level",
            "off",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        !result.status.success(),
        "non-empty also-eml without --overwrite must fail"
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stdout.contains("--also-eml") || stderr.contains("--also-eml"),
        "usage error must name --also-eml, not --out: stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stdout.contains("--out is not empty") && !stderr.contains("--out is not empty"),
        "must not mislabel as --out: stdout={stdout} stderr={stderr}"
    );
    assert!(!out.exists(), "must fail before PST write");
}

#[test]
fn flag_absent_also_eml_null_and_ran_false() {
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
            "--qc-level",
            "off",
            "--no-attachments",
            "--json",
            "--allow-partial-fidelity",
        ])
        .output()
        .expect("run");
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    assert!(
        v.as_object().expect("obj").contains_key("also_eml_out"),
        "also_eml_out key must be present"
    );
    assert!(v["also_eml_out"].is_null(), "flag-absent also_eml_out null");
    assert_eq!(v["also_eml_ran"], false);
    assert_eq!(v["also_eml_exit_code"], 0);
    assert_eq!(v["also_eml_eml_written"], 0);
}

#[test]
fn method5_also_eml_inner_subject_on_rfc822() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    write_method5_source(&src, 2);
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let also = dir.path().join("also_eml");

    let mut args = tiny_cli_args(src, out, report);
    args.also_eml = Some(also.clone());
    args.max_embedded_depth = 3;
    // Do not inject --no-attachments (nested MIME must write).
    let outcome = run_unique_pst_with_options(
        args,
        UniquePstRunOptions {
            cancel: None,
            stderr_progress: false,
            on_progress: None,
            on_log: None,
        },
    )
    .expect("run");
    assert!(
        outcome.ok || outcome.exit.as_u8() == 64,
        "unexpected fail: {:?}",
        outcome.error_message
    );
    assert!(also.join("manifest.json").is_file());
    let text = pack_eml_text(&also);
    assert!(
        text.to_ascii_lowercase()
            .contains("content-type: message/rfc822"),
        "expected rfc822 wrapper: {text}"
    );
    assert!(
        text.contains("Subject: Depth 1") || text.contains("Subject: Leaf"),
        "inner Subject must appear on nested rfc822: {text}"
    );
}

#[test]
fn mode_a_soft_skip_row_lands_on_also_eml_ledger() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    write_unicode_pst(
        &src,
        vec![base_msg("<mode-a@ex.com>", "Mode A winner")],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write");

    let mut pst = pst_reader::PstFile::open(&src).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid")
        .0;
    let src_display = src.display().to_string();

    let keep_set = KeepSet {
        schema: "keep_set_v1".into(),
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::ParentsOnly,
        created_from: None,
        identity_level: None,
        dedupe_scope: None,
        winners: vec![KeepEntry {
            locus: MessageLocus {
                source_path: src_display.clone(),
                source_pst: src
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                folder_path: "Inbox".into(),
                nid,
                is_orphaned: false,
            },
            message_id_norm: Some("<mode-a@ex.com>".into()),
            content_hash: [0u8; 32],
            edrm_mih_hex: None,
            integrity: RecoverableIntegrity::clean(),
            size: 10,
            promoted_from_failure: true,
            folder_class: None,
            decided_by: None,
            duplicate_source_count: 0,
            duplicate_sources: vec![],
            duplicate_sources_truncated: false,
        }],
        stats: KeepSetStats {
            unique: 1,
            recoverable: 1,
            ..KeepSetStats::default()
        },
    };

    let soft = SoftSkipAttachRecord {
        source_path: r"C:\peer\incomplete.pst".into(),
        source_pst: "incomplete.pst".into(),
        folder_path: "Inbox".into(),
        msg_nid: 0x42,
        attach_nid: Some(99),
        attach_index: 0,
        filename: "missing.bin".into(),
        size: 100,
        attach_method: 1,
        reason_code: "ATTACH_STREAM_OPEN_FAILED".into(),
        peer_source_path: src_display.clone(),
        peer_msg_nid: nid,
        cloud_provider: String::new(),
        cloud_url: String::new(),
    };

    let pack_out = dir.path().join("pack");
    fs::create_dir_all(&pack_out).expect("mkdir");
    let mut mat = PstMaterializer::new(FamilyPolicy::ParentsOnly);
    let mut attach_src = PstAttachStreamSource::new();
    let preflight = dedup_engine::integrity::compute_preflight(
        &dedup_engine::integrity::PreflightInputs::without_attach_probe(
            ScanMode::BestEffort,
            1,
            0,
            0,
            0,
            1,
            dedup_engine::integrity::IntegrityThresholds::default(),
        ),
    );
    let scan = pst_dedup_cli::scan::ScanSummary {
        schema: "scan_integrity_v1".into(),
        mode: ScanMode::BestEffort,
        files: vec![],
        total_messages: 1,
        unique: 1,
        duplicates: 0,
        tier1_hits: 0,
        tier2_hits: 0,
        savings_bytes: 0,
        skipped: 0,
        skipped_by_reason: Default::default(),
        recoverable_messages: 1,
        degraded_messages: 0,
        degraded_by_reason: Default::default(),
        orphaned_messages: 0,
        failed_files: 0,
        partial_files: 0,
        opened_files: 1,
        duration_secs: 0.0,
        preflight,
        skips: vec![],
        integrity_csv: None,
        grouping: Default::default(),
        page_crc_mismatches: 0,
        block_crc_mismatches: 0,
        block_bid_mismatches: 0,
        distinct_bad_bids: 0,
        distinct_bad_bids_exact: true,
        crc_suspect_messages: 0,
        page_reads: 0,
        block_reads: 0,
        block_crc_rate: 0.0,
        block_crc_read_rate: 0.0,
        poly_class_crc_sources: 0,
    };

    let cancel = AtomicBool::new(false);
    let pack = write_eml_pack_from_keep_set(WriteEmlPackFromKeepSetInput {
        keep_set: &keep_set,
        paths: std::slice::from_ref(&src),
        out: &pack_out,
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::ParentsOnly,
        write_opts: dedup_engine::EmlWriteOpts {
            family_policy: FamilyPolicy::ParentsOnly,
            max_embedded_depth: 3,
        },
        files_per_volume: 10_000,
        volume_prefix: "VOL".into(),
        attach_ledger: AttachLedgerMode::Full,
        attach_ledger_max_rows: 500_000,
        ledger_path_mode: LedgerPathMode::Full,
        soft_skip_attach_records: &[soft],
        scan,
        scan_ok: true,
        fail_on_partial_fidelity: false,
        allow_partial_fidelity: true,
        risk_gate: pst_dedup_cli::export_outcome::RiskGate::Off,
        export_risk: dedup_engine::integrity::PreflightRecommendation::Ok,
        cancel: Some(&cancel),
        mat: &mut mat,
        attach_src: &mut attach_src,
        manifest_json: None,
        materialized_count: 1,
    })
    .expect("write pack");
    assert_eq!(pack.eml_written, 1);

    let csv = fs::read_to_string(pack_out.join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
    assert!(
        csv.contains("0x42") || csv.contains(",66,") || csv.lines().any(|l| l.contains("66")),
        "soft-skip msg_nid must appear: {csv}"
    );
    assert!(
        csv.contains("ATTACH_STREAM_OPEN_FAILED"),
        "soft-skip reason_code must appear: {csv}"
    );
}

#[test]
fn helper_hard_fail_writes_summary_json() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    write_unicode_pst(
        &src,
        vec![base_msg("<hf@ex.com>", "Hard fail")],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write");
    let mut pst = pst_reader::PstFile::open(&src).expect("open");
    let nid = pst
        .folders()
        .expect("folders")
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid")
        .0;
    let src_display = src.display().to_string();
    let keep_set = KeepSet {
        schema: "keep_set_v1".into(),
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::ParentsOnly,
        created_from: None,
        identity_level: None,
        dedupe_scope: None,
        winners: vec![KeepEntry {
            locus: MessageLocus {
                source_path: src_display.clone(),
                source_pst: "src.pst".into(),
                folder_path: "Inbox".into(),
                nid,
                is_orphaned: false,
            },
            message_id_norm: Some("<hf@ex.com>".into()),
            content_hash: [0u8; 32],
            edrm_mih_hex: None,
            integrity: RecoverableIntegrity::clean(),
            size: 10,
            promoted_from_failure: false,
            folder_class: None,
            decided_by: None,
            duplicate_source_count: 0,
            duplicate_sources: vec![],
            duplicate_sources_truncated: false,
        }],
        stats: KeepSetStats {
            unique: 1,
            recoverable: 1,
            ..KeepSetStats::default()
        },
    };
    let pack_out = dir.path().join("pack");
    fs::create_dir_all(&pack_out).expect("mkdir");
    // Manifest path that cannot be written as a file (directory collision).
    let bad_manifest = pack_out.join("manifest.json");
    fs::create_dir_all(&bad_manifest).expect("manifest as dir");
    let mut mat = PstMaterializer::new(FamilyPolicy::ParentsOnly);
    let mut attach_src = PstAttachStreamSource::new();
    let preflight = dedup_engine::integrity::compute_preflight(
        &dedup_engine::integrity::PreflightInputs::without_attach_probe(
            ScanMode::BestEffort,
            1,
            0,
            0,
            0,
            1,
            dedup_engine::integrity::IntegrityThresholds::default(),
        ),
    );
    let scan = pst_dedup_cli::scan::ScanSummary {
        schema: "scan_integrity_v1".into(),
        mode: ScanMode::BestEffort,
        files: vec![],
        total_messages: 1,
        unique: 1,
        duplicates: 0,
        tier1_hits: 0,
        tier2_hits: 0,
        savings_bytes: 0,
        skipped: 0,
        skipped_by_reason: Default::default(),
        recoverable_messages: 1,
        degraded_messages: 0,
        degraded_by_reason: Default::default(),
        orphaned_messages: 0,
        failed_files: 0,
        partial_files: 0,
        opened_files: 1,
        duration_secs: 0.0,
        preflight,
        skips: vec![],
        integrity_csv: None,
        grouping: Default::default(),
        page_crc_mismatches: 0,
        block_crc_mismatches: 0,
        block_bid_mismatches: 0,
        distinct_bad_bids: 0,
        distinct_bad_bids_exact: true,
        crc_suspect_messages: 0,
        page_reads: 0,
        block_reads: 0,
        block_crc_rate: 0.0,
        block_crc_read_rate: 0.0,
        poly_class_crc_sources: 0,
    };
    let result = write_eml_pack_from_keep_set(WriteEmlPackFromKeepSetInput {
        keep_set: &keep_set,
        paths: std::slice::from_ref(&src),
        out: &pack_out,
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::ParentsOnly,
        write_opts: dedup_engine::EmlWriteOpts {
            family_policy: FamilyPolicy::ParentsOnly,
            max_embedded_depth: 3,
        },
        files_per_volume: 10_000,
        volume_prefix: "VOL".into(),
        attach_ledger: AttachLedgerMode::Off,
        attach_ledger_max_rows: 500_000,
        ledger_path_mode: LedgerPathMode::Full,
        soft_skip_attach_records: &[],
        scan,
        scan_ok: true,
        fail_on_partial_fidelity: false,
        allow_partial_fidelity: true,
        risk_gate: pst_dedup_cli::export_outcome::RiskGate::Off,
        export_risk: dedup_engine::integrity::PreflightRecommendation::Ok,
        cancel: None,
        mat: &mut mat,
        attach_src: &mut attach_src,
        manifest_json: Some(&bad_manifest),
        materialized_count: 1,
    });
    assert!(result.is_err(), "manifest dir must hard-fail the helper");
    let summary = pack_out.join("summary.json");
    assert!(
        summary.is_file(),
        "helper Err must still write summary.json"
    );
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary).expect("read")).expect("json");
    assert_eq!(v["ok"], false);
    assert_eq!(v["exit_code"].as_u64(), Some(1));
    assert!(
        v["error"]["message"].as_str().is_some(),
        "error.message required: {v}"
    );
}

#[test]
fn cancel_during_pst_write_skips_also_eml() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let also = dir.path().join("also_eml");
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_c = Arc::clone(&cancel);

    let mut args = tiny_cli_args(sample, out.clone(), report.clone());
    args.also_eml = Some(also.clone());
    args.no_attachments = true;
    args.qc_level = pst_dedup_cli::unique_pst_qc::QcLevel::Off;

    let outcome = run_unique_pst_with_options(
        args,
        UniquePstRunOptions {
            cancel: Some(cancel),
            stderr_progress: false,
            on_progress: Some(Box::new(move |p| {
                if p.stage == "write" {
                    cancel_c.store(true, Ordering::SeqCst);
                }
            })),
            on_log: None,
        },
    )
    .expect("structured outcome");
    assert!(outcome.cancelled, "cancel at write must cancel");
    assert_eq!(outcome.exit.as_u8(), 130);
    let summary = fs::read_to_string(report.join("summary.json")).expect("summary");
    let v: serde_json::Value = serde_json::from_str(&summary).expect("json");
    assert_eq!(v["also_eml_ran"], false);
    assert_eq!(v["also_eml_exit_code"], 0);
    // Prepared empty also-eml dir may remain; must not contain a finished pack.
    assert!(
        !also.join("manifest.json").is_file(),
        "also-eml must be skipped (no pack manifest)"
    );
    let eml_n = count_eml_files(&also);
    assert_eq!(eml_n, 0, "no EML files when also-eml skipped");
}

#[test]
fn helper_cancel_with_blocked_summary_returns_cancelled_ok() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    write_unicode_pst(
        &src,
        vec![base_msg("<cx@ex.com>", "Cancel")],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write");
    let mut pst = pst_reader::PstFile::open(&src).expect("open");
    let nid = pst
        .folders()
        .expect("folders")
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid")
        .0;
    let src_display = src.display().to_string();
    let keep_set = KeepSet {
        schema: "keep_set_v1".into(),
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::ParentsOnly,
        created_from: None,
        identity_level: None,
        dedupe_scope: None,
        winners: vec![KeepEntry {
            locus: MessageLocus {
                source_path: src_display,
                source_pst: "src.pst".into(),
                folder_path: "Inbox".into(),
                nid,
                is_orphaned: false,
            },
            message_id_norm: Some("<cx@ex.com>".into()),
            content_hash: [0u8; 32],
            edrm_mih_hex: None,
            integrity: RecoverableIntegrity::clean(),
            size: 10,
            promoted_from_failure: false,
            folder_class: None,
            decided_by: None,
            duplicate_source_count: 0,
            duplicate_sources: vec![],
            duplicate_sources_truncated: false,
        }],
        stats: KeepSetStats {
            unique: 1,
            recoverable: 1,
            ..KeepSetStats::default()
        },
    };
    let pack_out = dir.path().join("pack");
    fs::create_dir_all(&pack_out).expect("mkdir");
    fs::create_dir_all(pack_out.join("summary.json")).expect("block summary");
    let cancel = AtomicBool::new(true);
    let mut mat = PstMaterializer::new(FamilyPolicy::ParentsOnly);
    let mut attach_src = PstAttachStreamSource::new();
    let preflight = dedup_engine::integrity::compute_preflight(
        &dedup_engine::integrity::PreflightInputs::without_attach_probe(
            ScanMode::BestEffort,
            1,
            0,
            0,
            0,
            1,
            dedup_engine::integrity::IntegrityThresholds::default(),
        ),
    );
    let scan = pst_dedup_cli::scan::ScanSummary {
        schema: "scan_integrity_v1".into(),
        mode: ScanMode::BestEffort,
        files: vec![],
        total_messages: 1,
        unique: 1,
        duplicates: 0,
        tier1_hits: 0,
        tier2_hits: 0,
        savings_bytes: 0,
        skipped: 0,
        skipped_by_reason: Default::default(),
        recoverable_messages: 1,
        degraded_messages: 0,
        degraded_by_reason: Default::default(),
        orphaned_messages: 0,
        failed_files: 0,
        partial_files: 0,
        opened_files: 1,
        duration_secs: 0.0,
        preflight,
        skips: vec![],
        integrity_csv: None,
        grouping: Default::default(),
        page_crc_mismatches: 0,
        block_crc_mismatches: 0,
        block_bid_mismatches: 0,
        distinct_bad_bids: 0,
        distinct_bad_bids_exact: true,
        crc_suspect_messages: 0,
        page_reads: 0,
        block_reads: 0,
        block_crc_rate: 0.0,
        block_crc_read_rate: 0.0,
        poly_class_crc_sources: 0,
    };
    let pack = write_eml_pack_from_keep_set(WriteEmlPackFromKeepSetInput {
        keep_set: &keep_set,
        paths: std::slice::from_ref(&src),
        out: &pack_out,
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::ParentsOnly,
        write_opts: dedup_engine::EmlWriteOpts {
            family_policy: FamilyPolicy::ParentsOnly,
            max_embedded_depth: 3,
        },
        files_per_volume: 10_000,
        volume_prefix: "VOL".into(),
        attach_ledger: AttachLedgerMode::Off,
        attach_ledger_max_rows: 500_000,
        ledger_path_mode: LedgerPathMode::Full,
        soft_skip_attach_records: &[],
        scan,
        scan_ok: true,
        fail_on_partial_fidelity: false,
        allow_partial_fidelity: true,
        risk_gate: pst_dedup_cli::export_outcome::RiskGate::Off,
        export_risk: dedup_engine::integrity::PreflightRecommendation::Ok,
        cancel: Some(&cancel),
        mat: &mut mat,
        attach_src: &mut attach_src,
        manifest_json: None,
        materialized_count: 1,
    })
    .expect("cancel must not surface as Generic Err");
    assert!(pack.cancelled);
    assert_eq!(pack.exit.as_u8(), 130);
    assert!(
        pack.exit_reasons.iter().any(|r| r == "CANCELLED"),
        "reasons={:?}",
        pack.exit_reasons
    );
}

#[test]
fn summary_write_failure_returns_err() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src.pst");
    write_unicode_pst(
        &src,
        vec![base_msg("<sw@ex.com>", "Summary write")],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write");
    let mut pst = pst_reader::PstFile::open(&src).expect("open");
    let nid = pst
        .folders()
        .expect("folders")
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid")
        .0;
    let src_display = src.display().to_string();
    let keep_set = KeepSet {
        schema: "keep_set_v1".into(),
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::ParentsOnly,
        created_from: None,
        identity_level: None,
        dedupe_scope: None,
        winners: vec![KeepEntry {
            locus: MessageLocus {
                source_path: src_display,
                source_pst: "src.pst".into(),
                folder_path: "Inbox".into(),
                nid,
                is_orphaned: false,
            },
            message_id_norm: Some("<sw@ex.com>".into()),
            content_hash: [0u8; 32],
            edrm_mih_hex: None,
            integrity: RecoverableIntegrity::clean(),
            size: 10,
            promoted_from_failure: false,
            folder_class: None,
            decided_by: None,
            duplicate_source_count: 0,
            duplicate_sources: vec![],
            duplicate_sources_truncated: false,
        }],
        stats: KeepSetStats {
            unique: 1,
            recoverable: 1,
            ..KeepSetStats::default()
        },
    };
    let pack_out = dir.path().join("pack");
    fs::create_dir_all(&pack_out).expect("mkdir");
    // Block summary.json as a directory so the final write fails closed.
    fs::create_dir_all(pack_out.join("summary.json")).expect("summary as dir");
    let mut mat = PstMaterializer::new(FamilyPolicy::ParentsOnly);
    let mut attach_src = PstAttachStreamSource::new();
    let preflight = dedup_engine::integrity::compute_preflight(
        &dedup_engine::integrity::PreflightInputs::without_attach_probe(
            ScanMode::BestEffort,
            1,
            0,
            0,
            0,
            1,
            dedup_engine::integrity::IntegrityThresholds::default(),
        ),
    );
    let scan = pst_dedup_cli::scan::ScanSummary {
        schema: "scan_integrity_v1".into(),
        mode: ScanMode::BestEffort,
        files: vec![],
        total_messages: 1,
        unique: 1,
        duplicates: 0,
        tier1_hits: 0,
        tier2_hits: 0,
        savings_bytes: 0,
        skipped: 0,
        skipped_by_reason: Default::default(),
        recoverable_messages: 1,
        degraded_messages: 0,
        degraded_by_reason: Default::default(),
        orphaned_messages: 0,
        failed_files: 0,
        partial_files: 0,
        opened_files: 1,
        duration_secs: 0.0,
        preflight,
        skips: vec![],
        integrity_csv: None,
        grouping: Default::default(),
        page_crc_mismatches: 0,
        block_crc_mismatches: 0,
        block_bid_mismatches: 0,
        distinct_bad_bids: 0,
        distinct_bad_bids_exact: true,
        crc_suspect_messages: 0,
        page_reads: 0,
        block_reads: 0,
        block_crc_rate: 0.0,
        block_crc_read_rate: 0.0,
        poly_class_crc_sources: 0,
    };
    let result = write_eml_pack_from_keep_set(WriteEmlPackFromKeepSetInput {
        keep_set: &keep_set,
        paths: std::slice::from_ref(&src),
        out: &pack_out,
        policy: KeepPolicy::FirstSeen,
        family_policy: FamilyPolicy::ParentsOnly,
        write_opts: dedup_engine::EmlWriteOpts {
            family_policy: FamilyPolicy::ParentsOnly,
            max_embedded_depth: 3,
        },
        files_per_volume: 10_000,
        volume_prefix: "VOL".into(),
        attach_ledger: AttachLedgerMode::Off,
        attach_ledger_max_rows: 500_000,
        ledger_path_mode: LedgerPathMode::Full,
        soft_skip_attach_records: &[],
        scan,
        scan_ok: true,
        fail_on_partial_fidelity: false,
        allow_partial_fidelity: true,
        risk_gate: pst_dedup_cli::export_outcome::RiskGate::Off,
        export_risk: dedup_engine::integrity::PreflightRecommendation::Ok,
        cancel: None,
        mat: &mut mat,
        attach_src: &mut attach_src,
        manifest_json: None,
        materialized_count: 1,
    });
    assert!(result.is_err(), "summary write failure must return Err");
}

#[test]
fn rewrite_quarantined_summary_sets_partial_quarantined() {
    use pst_dedup_cli::export_outcome::{ArtifactState, QuarantineResult};
    use pst_dedup_cli::unique_eml_cmd::rewrite_quarantined_eml_summary;

    let dir = TempDir::new().expect("tmp");
    let dest = dir.path().join("also_eml.cancelled-1.partial");
    fs::create_dir_all(&dest).expect("mkdir");
    let summary_path = dest.join("summary.json");
    fs::write(
        &summary_path,
        r#"{
            "ok": false,
            "out": "C:\\old\\also_eml",
            "summary_path": "C:\\old\\also_eml\\summary.json",
            "artifact_state": "invalid_in_place",
            "exit_code": 130
        }"#,
    )
    .expect("seed");
    rewrite_quarantined_eml_summary(&dest, QuarantineResult::Succeeded).expect("rewrite");
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_path).expect("read")).expect("json");
    assert_eq!(
        v["artifact_state"].as_str(),
        Some(ArtifactState::PartialQuarantined.as_str())
    );
    let out = v["out"].as_str().expect("out");
    assert!(
        out.contains("cancelled") && out.contains("partial"),
        "out must point at quarantine dest: {out}"
    );
    let sp = v["summary_path"].as_str().expect("summary_path");
    assert!(
        sp.contains("summary.json"),
        "summary_path must be rewritten: {sp}"
    );
}
