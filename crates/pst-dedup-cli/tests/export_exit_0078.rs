//! 0078: unique-export exit codes — process status equals summary.exit_code.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
}

/// Parse JSON from `--json` stdout; **fail closed** when expected JSON is not parseable
/// (missing fixture is the only allowed skip — caller's responsibility before invoke).
fn parse_json_stdout(stdout: &[u8], label: &str) -> serde_json::Value {
    let text = String::from_utf8_lossy(stdout);
    serde_json::from_str(text.trim()).unwrap_or_else(|e| {
        panic!("DoD-9 fail-closed: expected JSON from {label}: {e}; stdout={text}");
    })
}

#[test]
fn clean_fixture_exit_0_matches_summary() {
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
            "--json",
        ])
        .output()
        .expect("run");
    let code = result.status.code().unwrap_or(255);
    let v = parse_json_stdout(&result.stdout, "unique-pst clean");
    assert_eq!(
        v["exit_code"].as_u64().unwrap_or(999) as u32,
        code as u32,
        "DoD-9: summary.exit_code must equal process status; stdout={v}"
    );
    assert_eq!(code, 0, "clean fixture must exit 0");
    assert_eq!(v["fidelity"].as_str(), Some("complete"));
    assert_eq!(v["ok"], true);
    assert_eq!(v["artifact_state"].as_str(), Some("complete"));
    let sp = v["summary_path"].as_str().unwrap_or("");
    assert!(!sp.is_empty(), "summary_path required");
    let summary_file = PathBuf::from(sp);
    assert!(
        summary_file.is_file(),
        "summary_path must exist on disk: {sp}"
    );
    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_file).expect("summary body"))
            .expect("summary json");
    assert_eq!(
        on_disk["exit_code"], v["exit_code"],
        "self-locating summary must carry exit_code"
    );
    assert_eq!(on_disk["fidelity"], v["fidelity"]);
}

/// Process-level attach path: if the fixture yields attach failures, assert 64;
/// if not, still assert DoD-9 and do **not** claim DoD-5 full pass (that is covered
/// by the non-skippable library unit `attach_soft_fail_partial_64` /
/// `unique_eml_style_attach_soft_fail_is_partial_64` in `export_outcome`).
#[test]
fn attach_soft_fail_exit_64_or_clean_dod9() {
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
            "--json",
        ])
        .output()
        .expect("run with attachments");
    let code = result.status.code().unwrap_or(255);
    // Fail closed: JSON required when --json was passed (fixture present).
    let v = parse_json_stdout(&result.stdout, "unique-pst attach");
    let failed = v["export"]["attachments_failed"].as_u64().unwrap_or(0);
    assert_eq!(
        v["exit_code"].as_u64().unwrap_or(999) as u32,
        code as u32,
        "DoD-9 exit_code match"
    );
    if failed == 0 {
        // Fixture has no attach soft-fails — DoD-9 still holds; DoD-5 proven by unit tests.
        assert_eq!(code, 0, "zero attach fails → clean exit when other dims ok");
        assert_eq!(v["fidelity"].as_str(), Some("complete"));
        return;
    }
    // Soft attach-only → 64; if other hard dimensions fire, still non-zero (refinement).
    if v["fidelity"].as_str() == Some("partial") {
        assert_eq!(code, 64, "attach soft-fail → 64; got {code}");
        assert_eq!(v["ok"], false);
        assert_eq!(v["artifact_state"].as_str(), Some("partial_retained"));
        let reasons = v["exit_reason"].as_array().cloned().unwrap_or_default();
        assert!(
            reasons
                .iter()
                .any(|r| r.as_str() == Some("ATTACH_SOFT_FAIL")),
            "reasons={reasons:?}"
        );
        assert!(out.is_file(), "artifact retained on 64");
    } else {
        assert_ne!(code, 0, "attach fails must stay non-zero; code={code}");
        assert!(out.is_file() || code == 1, "hard fail path");
        return;
    }

    // --allow-partial-fidelity → 0 with fidelity still partial
    let dir2 = TempDir::new().expect("tmp2");
    let out2 = dir2.path().join("unique.pst");
    let report2 = dir2.path().join("report");
    let result2 = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out2.to_str().expect("utf8"),
            "--report-dir",
            report2.to_str().expect("utf8"),
            "--allow-partial-fidelity",
            "--json",
        ])
        .output()
        .expect("run allow partial");
    let code2 = result2.status.code().unwrap_or(255);
    let v2 = parse_json_stdout(&result2.stdout, "unique-pst allow-partial");
    let failed2 = v2["export"]["attachments_failed"].as_u64().unwrap_or(0);
    if failed2 > 0 && v2["fidelity"].as_str() == Some("partial") {
        assert_eq!(code2, 0, "allow-partial → 0");
        assert_eq!(v2["fidelity"].as_str(), Some("partial"));
        assert_eq!(v2["ok"], false);
        assert_eq!(
            v2["exit_code"].as_u64().unwrap_or(999) as u32,
            code2 as u32,
            "DoD-9 allow-partial"
        );
    }
}

#[test]
fn mutual_exclusion_fidelity_flags_exit_2() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixture missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--fail-on-partial-fidelity",
            "--allow-partial-fidelity",
            "--json",
        ])
        .output()
        .expect("run");
    assert_eq!(result.status.code(), Some(2));
}

/// unique-eml production path: process exit equals summary.exit_code (DoD-9),
/// and on-disk summary.json carries the same exit contract (DoD-22).
/// Attach→64 is proven by unit `unique_eml_style_attach_soft_fail_is_partial_64`.
#[test]
fn unique_eml_process_exit_matches_summary_and_summary_path() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("pack");
    let result = Command::new(bin())
        .args([
            "unique-eml",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--no-attachments",
            "--json",
        ])
        .output()
        .expect("run unique-eml");
    let code = result.status.code().unwrap_or(255);
    let v = parse_json_stdout(&result.stdout, "unique-eml");
    assert_eq!(
        v["exit_code"].as_u64().unwrap_or(999) as u32,
        code as u32,
        "DoD-9 unique-eml: exit_code must equal process status"
    );
    assert!(v.get("fidelity").is_some(), "unique-eml must emit fidelity");
    assert!(
        v.get("exit_reason").is_some(),
        "unique-eml must emit exit_reason"
    );
    assert!(
        v.get("artifact_state").is_some(),
        "unique-eml must emit artifact_state"
    );
    let sp = v["summary_path"].as_str().unwrap_or("");
    assert!(!sp.is_empty(), "unique-eml summary_path required");
    let summary_file = PathBuf::from(sp);
    assert!(
        summary_file.is_file(),
        "unique-eml summary_path must exist: {sp}"
    );
    // Must not point only at bare manifest without exit fields.
    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_file).expect("summary body"))
            .expect("summary json");
    assert_eq!(
        on_disk["exit_code"], v["exit_code"],
        "on-disk summary must include exit_code (not just manifest.json)"
    );
    assert_eq!(on_disk["fidelity"], v["fidelity"]);
    assert_eq!(on_disk["summary_path"].as_str(), Some(sp));
    // Clean no-attachments path should be complete/0 when fixture is healthy.
    if v["attach_parts_failed"].as_u64().unwrap_or(0) == 0
        && v["fidelity"].as_str() == Some("complete")
    {
        assert_eq!(code, 0);
    }
}

/// keep-set with `--keep-set-json` writes self-locating summary with exit contract.
#[test]
fn keep_set_summary_path_self_locating() {
    let sample = fixture_sample();
    if !sample.exists() {
        panic!("fixtures/aspose_outlook.pst missing — required for 0078 DoD-22");
    }
    let dir = TempDir::new().expect("tmp");
    let ks = dir.path().join("keepset.json");
    let result = Command::new(bin())
        .args([
            "keep-set",
            sample.to_str().expect("utf8"),
            "--keep-set-json",
            ks.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run keep-set");
    let code = result.status.code().unwrap_or(255);
    let v = parse_json_stdout(&result.stdout, "keep-set");
    assert_eq!(
        v["exit_code"].as_u64().unwrap_or(999) as u32,
        code as u32,
        "DoD-9 keep-set"
    );
    assert!(v.get("fidelity").is_some());
    assert!(v.get("artifact_state").is_some());
    let sp = v["summary_path"].as_str().unwrap_or("");
    assert!(
        !sp.is_empty(),
        "keep-set with --keep-set-json must emit non-empty summary_path"
    );
    let summary_file = PathBuf::from(sp);
    assert!(
        summary_file.is_file(),
        "keep_set summary must be on disk: {sp}"
    );
    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_file).expect("body")).expect("json");
    assert_eq!(on_disk["exit_code"], v["exit_code"]);
    assert_eq!(on_disk["fidelity"], v["fidelity"]);
    assert_eq!(on_disk["summary_path"].as_str(), Some(sp));
    assert!(
        summary_file.file_name().and_then(|n| n.to_str()) == Some("keep_set_summary.json"),
        "expected keep_set_summary.json, got {sp}"
    );
}

/// Pure stdout keep-set (`--json` only) still writes absolute summary_path (DoD-22).
#[test]
fn keep_set_stdout_only_still_self_locating() {
    let sample = fixture_sample();
    if !sample.exists() {
        panic!("fixtures/aspose_outlook.pst missing — required for 0078 DoD-22");
    }
    // Copy fixture so we can write keep_set_summary.json next to it without polluting fixtures/.
    let dir = TempDir::new().expect("tmp");
    let local_pst = dir.path().join("sample.pst");
    fs::copy(&sample, &local_pst).expect("copy fixture");
    let result = Command::new(bin())
        .args(["keep-set", local_pst.to_str().expect("utf8"), "--json"])
        .output()
        .expect("run keep-set stdout-only");
    let code = result.status.code().unwrap_or(255);
    let v = parse_json_stdout(&result.stdout, "keep-set stdout-only");
    assert_eq!(
        v["exit_code"].as_u64().unwrap_or(999) as u32,
        code as u32,
        "DoD-9 keep-set stdout-only"
    );
    let sp = v["summary_path"].as_str().unwrap_or("");
    assert!(
        !sp.is_empty(),
        "stdout-only keep-set must still set summary_path"
    );
    let summary_file = PathBuf::from(sp);
    assert!(
        summary_file.is_file(),
        "stdout-only keep-set must write summary on disk: {sp}"
    );
    let on_disk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_file).expect("body")).expect("json");
    assert_eq!(on_disk["exit_code"], v["exit_code"]);
    assert_eq!(on_disk["summary_path"].as_str(), Some(sp));
}

/// Production-path unique-eml attach soft-fail: writer counters → classify → exit 64.
///
/// Uses the real public `write_canonical_eml` + `NullAttachStreamSource` soft-skip path
/// and the same `ExportOkInput` construction as `unique_eml_cmd`.
#[test]
fn unique_eml_production_writer_attach_soft_fail_classifies_64() {
    use dedup_engine::eml_pack::{write_canonical_eml, EmlWriteOpts, NullAttachStreamSource};
    use dedup_engine::integrity::RecoverableIntegrity;
    use dedup_engine::keepset::{CanonicalAttachment, CanonicalMessage, MessageLocus};
    use pst_dedup_cli::export_outcome::{classify_export, ExportFidelity, ExportOkInput, RiskGate};

    let msg = CanonicalMessage {
        locus: MessageLocus {
            source_path: "synth.pst".into(),
            source_pst: "synth.pst".into(),
            folder_path: "Inbox".into(),
            nid: 0x100,
            is_orphaned: false,
        },
        message_id: Some("<0078-attach@test>".into()),
        subject: Some("soft fail attach".into()),
        sender: Some("a@b.c".into()),
        display_to: None,
        display_cc: None,
        display_bcc: None,
        submit_time: None,
        size: Some(10),
        message_class: None,
        body_plain: Some("plain body".into()),
        body_html: None,
        attachments: vec![CanonicalAttachment {
            filename: "missing.bin".into(),
            size: 10,
            mime: Some("application/octet-stream".into()),
            data: None,
            stream_available: true,
            attach_nid: Some(999),
            attach_method: Some(1),
        }],
        fidelity: RecoverableIntegrity::clean(),
        message_id_norm: Some("0078-attach@test".into()),
        content_hash: [0u8; 32],
        edrm_mih_hex: None,
        body_incomplete: false,
        body_unavailable: false,
    };
    let mut src = NullAttachStreamSource;
    let dir = TempDir::new().expect("tmp");
    let eml_path = dir.path().join("msg.eml");
    let res = write_canonical_eml(&eml_path, &msg, &mut src, &EmlWriteOpts::default())
        .expect("write_canonical_eml is production path");
    assert_eq!(
        res.attachments_failed, 1,
        "production writer must soft-fail missing attach stream"
    );

    // Same mapping unique_eml_cmd uses for classify (attach soft alone → partial 64).
    let input = ExportOkInput {
        scan_ok: true,
        verify_ok: true,
        export_err_absent: true,
        export_partial: false,
        messages_written_total: 1,
        unique: 1,
        attach_failed_total: res.attachments_failed,
        body_soft_fail_total: 0,
        report_ok: true,
    };
    let o = classify_export(
        input,
        dedup_engine::integrity::PreflightRecommendation::Ok,
        RiskGate::Off,
        true,
        false,
    );
    assert_eq!(o.fidelity, ExportFidelity::Partial);
    assert_eq!(o.exit.as_u8(), 64);
    assert!(o.reasons.contains(&"ATTACH_SOFT_FAIL"));
}

#[test]
fn cancel_exit_130_and_summary() {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixture missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let cancel = Arc::new(AtomicBool::new(true));
    let args = pst_dedup_cli::UniquePstCliArgs {
        paths: vec![sample],
        out: out.clone(),
        report_dir: Some(report.clone()),
        policy: dedup_engine::keepset::KeepPolicy::FirstSeen,
        family_policy: dedup_engine::keepset::FamilyPolicy::KeepAttachmentsWithParent,
        prefer_path_contains: vec![],
        prefer_bcc_copy: false,
        prefer_folder_class: false,
        folder_rank: vec![],
        source_rank: vec![],
        rank_folder_class_first: false,
        fidelity_rank: "binary".into(),
        decision_csv: None,
        keep_set_json: None,
        folder_layout: pst_dedup_cli::FolderLayoutArg::Preserve,
        max_volume_bytes: None,
        overwrite: false,
        verify_hash: false,
        also_eml: None,
        no_tier2: false,
        no_attachments: true,
        json: false,
        mode: dedup_engine::integrity::ScanMode::BestEffort,
        max_skip_rate: 0.05,
        max_crc_skip_rate: 0.01,
        max_failed_file_rate: 0.0,
        allow_failed_files: false,
        integrity_csv: None,
        skip_limit: 10_000,
        attach_ledger: pst_dedup_cli::unique_export_report::AttachLedgerMode::Off,
        attach_ledger_max_rows: 500_000,
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
    };
    let outcome = pst_dedup_cli::run_unique_pst_with_options(
        args,
        pst_dedup_cli::UniquePstRunOptions {
            cancel: Some(cancel),
            stderr_progress: false,
            on_progress: None,
            on_log: None,
        },
    )
    .expect("outcome");
    assert!(outcome.cancelled);
    assert_eq!(outcome.exit.as_u8(), 130);
    assert_eq!(outcome.exit_reasons, vec!["CANCELLED"]);
    assert!(outcome.summary_path.is_file());
    let body = fs::read_to_string(&outcome.summary_path).expect("summary");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["exit_code"].as_u64(), Some(130));
    assert_eq!(v["exit_reason"], serde_json::json!(["CANCELLED"]));
}
