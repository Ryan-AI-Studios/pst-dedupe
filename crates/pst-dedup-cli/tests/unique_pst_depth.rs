//! Track 0101: unique-pst `--max-embedded-depth` (do **not** use `unique_pst.rs`
//! `run_unique_pst`, which injects `--no-attachments`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use assert_cmd::cargo::cargo_bin;
use dedup_engine::integrity::ScanMode;
use dedup_engine::keepset::{FamilyPolicy, KeepPolicy};
use pst_dedup_cli::unique_export_report::{AttachLedgerMode, LedgerPathMode};
use pst_dedup_cli::unique_pst_cmd::{
    run_unique_pst_with_options, FolderLayoutArg, UniquePstCliArgs, UniquePstRunOptions,
};
use pst_reader::messaging::embedded::EmbeddedExportFields;
use pst_writer::{write_unicode_pst, WriteAttachment, WriteMessage, WritePstOpts};
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
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

/// Top-level winner with `nests` method-5 levels underneath.
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

fn write_source(path: &Path, nests: u32) {
    let opts = WritePstOpts {
        max_embedded_depth: 8,
        ..WritePstOpts::default()
    };
    write_unicode_pst(path, vec![method5_chain(nests)], &[], &opts).expect("write source");
}

fn run_unique_pst(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("run unique-pst")
}

fn method5_nest_depth(path: &Path) -> u32 {
    let mut pst = pst_reader::PstFile::open(path).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid");
    let root = pst.message_node_from_nbt(nid).expect("root");
    let fields = pst
        .read_export_from_message_node(&root, 8, 32 * 1024 * 1024)
        .expect("export");
    fn walk(f: &EmbeddedExportFields) -> u32 {
        f.attachments
            .iter()
            .filter_map(|a| a.embedded.as_ref())
            .map(|e| 1 + walk(e))
            .max()
            .unwrap_or(0)
    }
    walk(&fields)
}

fn depth_limit_count(v: &serde_json::Value) -> u64 {
    v["export"]["attachments_failed_by_reason"]["ATTACH_DEPTH_LIMIT"]
        .as_u64()
        .or_else(|| {
            v["export"]["attachments_failed_by_reason"]
                .as_object()
                .and_then(|m| m.get("ATTACH_DEPTH_LIMIT"))
                .and_then(|x| x.as_u64())
        })
        .unwrap_or(0)
}

fn tiny_cli_args(src: PathBuf, out: PathBuf, report: PathBuf, depth: u32) -> UniquePstCliArgs {
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
        overwrite: true,
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
        fail_on_partial_fidelity: false,
        allow_partial_fidelity: true,
        fail_on_export_risk: None,
        max_open_psts: 32,
        qc_level: pst_dedup_cli::unique_pst_qc::QcLevel::Off,
        qc_sample_max: 64,
        qc_external_reader: None,
        qc_scanpst: false,
        include_bcc_recipients: false,
        promote_on_attach_fail: false,
        max_embedded_depth: depth,
    }
}

#[test]
fn default_depth_3_fails_fourth_nest() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src4.pst");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    write_source(&src, 4);

    let result = run_unique_pst(&[
        "unique-pst",
        src.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
        "--qc-level",
        "off",
        "--allow-partial-fidelity",
    ]);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|_| {
        panic!(
            "json stdout; exit={:?} stderr={stderr} stdout={stdout}",
            result.status.code()
        )
    });
    assert_eq!(
        v["export"]["max_embedded_depth"].as_u64(),
        Some(3),
        "default depth; stdout={stdout}"
    );
    assert!(
        depth_limit_count(&v) >= 1,
        "expected ATTACH_DEPTH_LIMIT at default 3; stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("--max-embedded-depth=3"),
        "0127 hint must name configured cap 3; stderr={stderr}"
    );
    assert!(
        method5_nest_depth(&out) < 4,
        "4th nest must be absent at default 3; depth={}",
        method5_nest_depth(&out)
    );
}

#[test]
fn depth_4_recovers_fourth_nest() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src4.pst");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    write_source(&src, 4);

    let result = run_unique_pst(&[
        "unique-pst",
        src.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
        "--qc-level",
        "off",
        "--max-embedded-depth",
        "4",
    ]);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "exit={:?} stderr={stderr} stdout={stdout}",
        result.status.code()
    );
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["export"]["max_embedded_depth"].as_u64(), Some(4));
    assert_eq!(
        depth_limit_count(&v),
        0,
        "no ATTACH_DEPTH_LIMIT at 4; stdout={stdout}"
    );
    assert_eq!(
        method5_nest_depth(&out),
        4,
        "4th nest must be present; stdout={stdout}"
    );
}

#[test]
fn ceiling_8_fails_at_7_succeeds_at_8() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src8.pst");
    write_source(&src, 8);

    let out7 = dir.path().join("unique7.pst");
    let report7 = dir.path().join("report7");
    let fail = run_unique_pst(&[
        "unique-pst",
        src.to_str().expect("utf8"),
        "--out",
        out7.to_str().expect("utf8"),
        "--report-dir",
        report7.to_str().expect("utf8"),
        "--json",
        "--qc-level",
        "off",
        "--allow-partial-fidelity",
        "--max-embedded-depth",
        "7",
    ]);
    let stdout7 = String::from_utf8_lossy(&fail.stdout);
    let stderr7 = String::from_utf8_lossy(&fail.stderr);
    let v7: serde_json::Value = serde_json::from_str(&stdout7).unwrap_or_else(|_| {
        panic!(
            "json @7; exit={:?} stderr={stderr7} stdout={stdout7}",
            fail.status.code()
        )
    });
    assert_eq!(v7["export"]["max_embedded_depth"].as_u64(), Some(7));
    assert!(
        depth_limit_count(&v7) >= 1,
        "expected ATTACH_DEPTH_LIMIT at 7; stdout={stdout7}"
    );
    assert!(
        stderr7.contains("--max-embedded-depth=7"),
        "0127 hint must name configured cap 7; stderr={stderr7}"
    );
    assert!(
        method5_nest_depth(&out7) < 8,
        "8th nest must be absent at depth 7; depth={}",
        method5_nest_depth(&out7)
    );

    let out8 = dir.path().join("unique8.pst");
    let report8 = dir.path().join("report8");
    let ok = run_unique_pst(&[
        "unique-pst",
        src.to_str().expect("utf8"),
        "--out",
        out8.to_str().expect("utf8"),
        "--report-dir",
        report8.to_str().expect("utf8"),
        "--json",
        "--qc-level",
        "off",
        "--max-embedded-depth",
        "8",
    ]);
    let stdout8 = String::from_utf8_lossy(&ok.stdout);
    assert!(
        ok.status.success(),
        "exit={:?} stderr={} stdout={stdout8}",
        ok.status.code(),
        String::from_utf8_lossy(&ok.stderr)
    );
    let v8: serde_json::Value = serde_json::from_str(&stdout8).expect("json");
    assert_eq!(v8["export"]["max_embedded_depth"].as_u64(), Some(8));
    assert_eq!(depth_limit_count(&v8), 0, "clean at 8; stdout={stdout8}");
    assert_eq!(
        method5_nest_depth(&out8),
        8,
        "8th nest must be present at depth 8; stdout={stdout8}"
    );
}

#[test]
fn clap_rejects_zero_nine_and_non_integer() {
    for bad in ["0", "9", "abc"] {
        let result = run_unique_pst(&[
            "unique-pst",
            "dummy.pst",
            "--out",
            "out.pst",
            "--max-embedded-depth",
            bad,
        ]);
        assert!(
            !result.status.success(),
            "expected usage error for {bad}; exit={:?}",
            result.status.code()
        );
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        );
        assert!(
            combined.contains("1 to 8"),
            "expected clap range text '1 to 8' for {bad}: {combined}"
        );
    }
    let help = run_unique_pst(&["unique-pst", "--help"]);
    let help_txt = format!(
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(
        help_txt.contains("identity-safe") && help_txt.contains("often need 8"),
        "0127 clap help; {help_txt}"
    );
}

#[test]
fn library_clamp_zero_to_one_and_nine_to_eight() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("tiny.pst");
    write_unicode_pst(
        &src,
        vec![base_msg("<t@ex.com>", "Tiny")],
        &[],
        &WritePstOpts::default(),
    )
    .expect("tiny");

    for (requested, effective) in [(0u32, 1u64), (9u32, 8u64)] {
        let out = dir.path().join(format!("unique_{requested}.pst"));
        let report = dir.path().join(format!("report_{requested}"));
        let args = tiny_cli_args(src.clone(), out, report.clone(), requested);
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
        let body = fs::read_to_string(&outcome.summary_path).expect("summary");
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(
            v["export"]["max_embedded_depth"].as_u64(),
            Some(effective),
            "requested {requested} → effective {effective}; body={body}"
        );
    }
}

/// Cancel summary must echo the clamped depth, not a hardcoded 3.
#[test]
fn cancel_summary_echoes_effective_depth() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("tiny.pst");
    write_unicode_pst(
        &src,
        vec![base_msg("<c@ex.com>", "Cancel")],
        &[],
        &WritePstOpts::default(),
    )
    .expect("tiny");

    for (requested, effective) in [(4u32, 4u64), (9u32, 8u64)] {
        let out = dir.path().join(format!("unique_cancel_{requested}.pst"));
        let report = dir.path().join(format!("report_cancel_{requested}"));
        let args = tiny_cli_args(src.clone(), out, report, requested);
        let cancel = Arc::new(AtomicBool::new(true));
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: Some(cancel),
                stderr_progress: false,
                on_progress: None,
                on_log: None,
            },
        )
        .expect("cancelled run");
        assert!(outcome.cancelled, "requested {requested}");
        let body = fs::read_to_string(&outcome.summary_path).expect("summary");
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        assert_eq!(
            v["export"]["max_embedded_depth"].as_u64(),
            Some(effective),
            "cancel must echo clamped depth, not hardcoded 3; requested {requested}; body={body}"
        );
    }
}
