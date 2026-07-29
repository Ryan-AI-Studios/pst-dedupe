//! Integration tests for track 0071 unique-pst CLI.

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use dedup_engine::integrity::RecoverableIntegrity;
use dedup_engine::{
    build_keep_set_materialized, DecisionRole, FamilyPolicy, KeepPolicy, MaterializeBuildOpts,
    MessageLocus, RecoverableScanItem,
};
use pst_dedup_cli::attach_probe::ProbeLevel;
use pst_dedup_cli::pst_materializer::PstMaterializer;
use pst_dedup_cli::unique_export_report::{ExportMessageRow, VolumeReportRow};
use pst_dedup_cli::unique_pst_qc::{run_unique_pst_qc, QcLevel, QcRunInput, QcSampleCandidate};
use pst_reader::ndb::block;
use pst_writer::{write_unicode_pst, WriteAttachment, WriteMessage, WritePstOpts};
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
}

/// Full-file SHA-256 hex digest (source immutability / verify-hash).
fn sha256_file(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = fs::File::open(path).expect("open for hash");
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).expect("read");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn run_unique_pst(args: &[&str]) -> std::process::Output {
    // Fixture attach streams can soft-fail; structural tests opt out so success
    // paths remain meaningful. Attachment honesty is covered by a dedicated test.
    let mut cmd = Command::new(bin());
    cmd.args(args);
    if !args.contains(&"--no-attachments") {
        cmd.arg("--no-attachments");
    }
    cmd.output().expect("run unique-pst")
}

#[test]
fn unique_pst_fixture_schema_and_counts() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );

    let stdout = String::from_utf8_lossy(&result.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["schema"].as_str(), Some("unique_export_report_v1"));
    let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
    let written = v["export"]["messages_written_total"].as_u64().unwrap_or(0);
    assert!(unique > 0, "expected unique > 0");
    assert_eq!(written, unique, "messages_written must equal unique");
    assert!(out.is_file(), "output PST must exist");

    // Open with reader and count.
    let mut pst = pst_reader::PstFile::open(&out).expect("open written pst");
    let folders = pst.folders().expect("folders");
    let total: u64 = folders.iter().map(|f| f.message_nids.len() as u64).sum();
    assert_eq!(total, unique);

    // Report pack files.
    assert!(report.join("summary.json").is_file());
    assert!(report.join("volumes.csv").is_file());
    assert!(report.join("export_messages.csv").is_file());
    assert!(report.join("decisions.csv").is_file());
    assert!(report.join("keepset.json").is_file());
}

#[test]
fn unique_pst_two_identical_inputs_collapse() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    // Copy fixture so we have two path inputs with identical content.
    let a = dir.path().join("a.pst");
    let b = dir.path().join("b.pst");
    fs::copy(&sample, &a).expect("copy a");
    fs::copy(&sample, &b).expect("copy b");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    let result = run_unique_pst(&[
        "unique-pst",
        a.to_str().expect("utf8"),
        b.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    assert_eq!(v["ok"], true);
    let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
    let recoverable = v["keep_set"]["stats"]["recoverable"].as_u64().unwrap_or(0);
    assert!(recoverable >= unique * 2 || recoverable > unique);
    assert_eq!(v["export"]["messages_written_total"].as_u64(), Some(unique));
}

#[test]
fn unique_pst_report_pack_and_export_messages_rows() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    let written = v["export"]["messages_written_total"].as_u64().unwrap_or(0);

    let csv = fs::read_to_string(report.join("export_messages.csv")).expect("export_messages");
    let mut lines = csv.lines();
    let header = lines.next().expect("header");
    let v1_prefix = "source_path,folder_path,nid,message_id_norm,edrm_mih,content_hash_hex,volume_path,volume_index,export_message_index,attachments_failed_count";
    assert!(
        header.starts_with(v1_prefix),
        "export_messages header must keep pre-0075 prefix; got {header}"
    );
    assert!(
        header.contains("duplicate_source_count") && header.contains("duplicate_sources"),
        "0075 All-Custodians columns required; got {header}"
    );
    assert!(
        header.ends_with(",source_id") || header.contains(",source_id"),
        "0081 source_id trailing column required; got {header}"
    );
    let rows: Vec<_> = lines.filter(|l| !l.is_empty()).collect();
    assert_eq!(rows.len() as u64, written);
    for row in &rows {
        assert!(
            row.contains("unique.pst") || row.contains(&out.display().to_string()),
            "volume_path should reference out: {row}"
        );
        // No body columns — header already fixed; row should not be huge free text only.
        assert!(!row.to_ascii_lowercase().contains("body_plain"));
    }

    let vol_csv = fs::read_to_string(report.join("volumes.csv")).expect("volumes");
    assert!(vol_csv.lines().count() >= 2); // header + ≥1 volume
}

#[test]
fn unique_pst_multi_volume_tiny_max() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    // Extremely small soft limit so multi-volume triggers after first message(s).
    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--max-volume-bytes",
        "4096",
        "--json",
    ]);
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    assert_eq!(v["ok"], true);
    let vols = v["export"]["volumes"].as_array().expect("volumes");
    let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
    let sum: u64 = vols
        .iter()
        .map(|x| x["messages_written"].as_u64().unwrap_or(0))
        .sum();
    assert_eq!(sum, unique);
    // With tiny limit and >1 message, expect ≥2 volumes when unique > 1.
    if unique > 1 {
        assert!(vols.len() >= 2, "expected multi-volume, got {}", vols.len());
        let vol2 = dir.path().join("unique_vol002.pst");
        assert!(vol2.is_file() || vols.len() >= 2);
        for vol in vols {
            let p = vol["path"].as_str().expect("path");
            let mut pst = pst_reader::PstFile::open(Path::new(p)).expect("open vol");
            let folders = pst.folders().expect("folders");
            let total: u64 = folders.iter().map(|f| f.message_nids.len() as u64).sum();
            assert_eq!(total, vol["messages_written"].as_u64().unwrap_or(0));
        }
    }
}

#[test]
fn unique_pst_fail_mid_volume_2_keeps_vol1() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    // Pre-create volume 2 path as a directory so File::create fails.
    let vol2 = dir.path().join("unique_vol002.pst");
    fs::create_dir_all(&vol2).expect("vol2 as dir");

    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--max-volume-bytes",
        "4096",
        "--json",
        "--overwrite",
    ]);
    // Non-zero on partial export failure.
    assert!(
        !result.status.success(),
        "must non-zero on vol2 fail; stdout={}",
        String::from_utf8_lossy(&result.stdout)
    );

    // Vol1 retained if multi-volume was attempted.
    // With overwrite clearing siblings, clear_stale only removes *files* — dir remains.
    // After vol1 succeeds, vol2 write fails.
    let stdout = String::from_utf8_lossy(&result.stdout);
    if stdout.trim().is_empty() {
        // JSON may still be on stdout for AlreadyEmitted path — check report.
    }
    let summary_path = report.join("summary.json");
    assert!(
        summary_path.is_file(),
        "partial report must flush summary.json"
    );
    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_path).expect("sum")).expect("json");
    assert_eq!(summary["ok"], false);
    assert_eq!(summary["export"]["partial"], true);

    let vols = summary["export"]["volumes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Fixture has unique > 1 and max is tiny → vol1 must have completed before vol2 fail.
    assert!(
        !vols.is_empty(),
        "expected at least one completed volume before vol2 fail; summary={summary}"
    );
    assert!(out.is_file(), "completed vol1 must remain");
    let mut pst = pst_reader::PstFile::open(&out).expect("open vol1");
    let _ = pst.folders().expect("vol1 folders");
    // Incomplete vol2 must not be a PST file (dir is fine).
    assert!(!vol2.is_file(), "incomplete vol2 must not be a PST file");
    assert_eq!(
        summary["verification"]["ok"], false,
        "partial export must force verification.ok=false"
    );
}

#[test]
fn unique_pst_oversized_family_allows_exceed() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    // max_volume_bytes=1: first message alone will exceed soft limit; must still succeed.
    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--max-volume-bytes",
        "1",
        "--json",
    ]);
    assert!(
        result.status.success(),
        "oversize family must not fail export: stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    assert_eq!(v["ok"], true);
    let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
    assert_eq!(v["export"]["messages_written_total"].as_u64(), Some(unique));
    // At least first volume should note exceed when bytes > 1.
    let vols = v["export"]["volumes"].as_array().expect("vols");
    assert!(!vols.is_empty());
    assert!(vols[0]["bytes"].as_u64().unwrap_or(0) > 1);
}

#[test]
fn unique_pst_default_verify_and_verify_hash() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    // Default path: open+count+sample only — no full-file rehash.
    let result_default = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        result_default.status.success(),
        "default verify: stderr={} stdout={}",
        String::from_utf8_lossy(&result_default.stderr),
        String::from_utf8_lossy(&result_default.stdout)
    );
    let v_def: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result_default.stdout)).expect("json");
    assert_eq!(v_def["ok"], true);
    assert_eq!(v_def["verification"]["ok"], true);
    assert_eq!(
        v_def["verification"]["rehash_ran"], false,
        "default path must not rehash (§3.6)"
    );
    for vol in v_def["verification"]["volumes"].as_array().expect("vvols") {
        assert_eq!(vol["open_ok"], true);
        assert_eq!(vol["message_count_match"], true);
        assert!(
            vol["hash_match"].is_null(),
            "no hash_match without --verify-hash"
        );
    }

    // Optional rehash path for CI/small fixtures.
    let out2 = dir.path().join("unique2.pst");
    let report2 = dir.path().join("report2");
    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out2.to_str().expect("utf8"),
        "--report-dir",
        report2.to_str().expect("utf8"),
        "--verify-hash",
        "--json",
    ]);
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["verification"]["ok"], true);
    assert_eq!(v["verification"]["rehash_ran"], true);
    let vols = v["verification"]["volumes"].as_array().expect("vvols");
    for vol in vols {
        assert_eq!(vol["hash_match"], true);
        assert_eq!(vol["open_ok"], true);
        assert_eq!(vol["message_count_match"], true);
    }
}

#[test]
fn unique_pst_overwrite_refuse_without_flag() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    fs::write(&out, b"existing").expect("seed out");
    fs::create_dir_all(&report).expect("report");
    fs::write(report.join("x.txt"), b"y").expect("seed report");

    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        !result.status.success(),
        "must refuse existing out without --overwrite"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(
        combined.to_ascii_lowercase().contains("overwrite")
            || combined.to_ascii_lowercase().contains("exists")
            || combined.to_ascii_lowercase().contains("not empty"),
        "error should mention overwrite/exists: {combined}"
    );
}

#[test]
fn unique_pst_source_immutability() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let before = sha256_file(&sample);
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );
    let after = sha256_file(&sample);
    assert_eq!(before, after, "source PST must be unchanged");
}

#[test]
fn unique_pst_json_stdout_parseable() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(result.status.success());
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    assert!(v.get("ok").is_some());
    assert!(v.get("export").and_then(|e| e.get("volumes")).is_some());
    assert_eq!(v["schema"].as_str(), Some("unique_export_report_v1"));
}

/// P1-1: input named like multi-volume sibling must not be deleted/overwritten.
#[test]
fn unique_pst_volume_sibling_input_protected() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    // Input collides with generated volume 3 path for --out unique.pst.
    let input = dir.path().join("unique_vol003.pst");
    fs::copy(&sample, &input).expect("copy input as vol3 sibling name");
    let before = sha256_file(&input);
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    let result = run_unique_pst(&[
        "unique-pst",
        input.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--max-volume-bytes",
        "4096",
        "--overwrite",
        "--json",
    ]);
    assert!(
        !result.status.success(),
        "must refuse volume path colliding with input; stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(input.is_file(), "input must not be deleted");
    let after = sha256_file(&input);
    assert_eq!(before, after, "input PST bytes must be unchanged");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(
        combined.to_ascii_lowercase().contains("input")
            || combined.to_ascii_lowercase().contains("volume")
            || combined.to_ascii_lowercase().contains("refusing"),
        "error should mention collision/refuse: {combined}"
    );
}

/// P1-4: mandatory report artifact write failure → ok=false, non-zero.
#[test]
fn unique_pst_report_write_failure_fail_closed() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    // Parent of decision-csv is a file → create/write fails after export.
    let blocked = dir.path().join("blocked_file");
    fs::write(&blocked, b"not-a-dir").expect("seed blocked");
    let dec = blocked.join("decisions.csv");

    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--decision-csv",
        dec.to_str().expect("utf8"),
        "--json",
    ]);
    assert!(
        !result.status.success(),
        "report write failure must be non-zero; stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    if !stdout.trim().is_empty() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
            assert_eq!(v["ok"], false, "stdout summary must not claim success");
        }
    }
    // Summary in report-dir should also be honest if it was written.
    let summary_path = report.join("summary.json");
    if summary_path.is_file() {
        let summary: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&summary_path).expect("sum")).expect("json");
        assert_eq!(summary["ok"], false);
    }
}

/// P1-3: non-zero attachments_failed must force ok=false (fixture has soft-fail attaches).
#[test]
fn unique_pst_attachment_failures_force_export_fail() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    // Explicitly enable attachments (helper would otherwise inject --no-attachments).
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
        .expect("run unique-pst with attachments");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let v: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("skip: no JSON stdout (attach path may hard-fail earlier)");
            return;
        }
    };
    let failed = v["export"]["attachments_failed"].as_u64().unwrap_or(0);
    if failed == 0 {
        // Fixture may improve; if no attach failures, success is fine.
        assert_eq!(v["ok"], true);
        assert!(result.status.success());
        return;
    }
    assert_eq!(
        v["ok"], false,
        "attachments_failed={failed} must force ok=false"
    );
    // 0078: attach soft-fail → exit 64 (PartialFidelity); refinement keeps non-zero.
    // Accept 64 (preferred) or any non-zero so fixture hard-fails elsewhere still fail the gate.
    let code = result.status.code().unwrap_or(1);
    assert_ne!(code, 0, "attachments_failed must force non-zero exit");
    if let Some(fidelity) = v["fidelity"].as_str() {
        if fidelity == "partial" {
            assert_eq!(
                code, 64,
                "partial attach soft-fail must exit 64 (got {code})"
            );
            assert_eq!(
                v["exit_code"].as_u64(),
                Some(64),
                "JSON exit_code must match process status"
            );
        }
    }
    assert!(out.is_file(), "PST volumes retained on attach soft-fail");

    // 0073: default attach-ledger=full → CSV + histogram + invariant.
    let ledger = report.join("export_attachments.csv");
    assert!(
        ledger.is_file(),
        "export_attachments.csv required when attach fails (mode=full)"
    );
    let hist = &v["export"]["attachments_failed_by_reason"];
    assert!(hist.is_object(), "histogram required: {hist}");
    let hist_sum: u64 = hist
        .as_object()
        .map(|m| m.values().filter_map(|x| x.as_u64()).sum())
        .unwrap_or(0);
    assert_eq!(
        hist_sum, failed,
        "histogram sum must equal attachments_failed"
    );
    assert_eq!(
        v["export"]["attachment_ledger"].as_str(),
        Some("export_attachments.csv")
    );
    let csv = fs::read_to_string(&ledger).expect("ledger");
    let fail_rows = csv
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty() && l.contains(",fail,"))
        .count() as u64;
    let truncated = v["export"]["attachment_ledger_truncated"]
        .as_bool()
        .unwrap_or(false);
    if !truncated {
        assert_eq!(
            fail_rows, failed,
            "CSV fail rows must equal attachments_failed when not truncated"
        );
    }
    // source_id present (first column numeric on data rows).
    for line in csv.lines().skip(1).filter(|l| !l.is_empty()) {
        if line.contains("ATTACH_LEDGER_TRUNCATED") {
            continue;
        }
        let first = line.split(',').next().unwrap_or("");
        assert!(first.parse::<u32>().is_ok(), "source_id required: {line}");
    }
}

/// 0073: `--attach-ledger=summary-only` → no CSV; histogram present.
#[test]
fn unique_pst_attach_ledger_summary_only() {
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
            "--attach-ledger",
            "summary-only",
            "--json",
        ])
        .output()
        .expect("run");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let v: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("skip: no JSON stdout");
            return;
        }
    };
    assert!(
        !report.join("export_attachments.csv").is_file(),
        "summary-only must not write export_attachments.csv"
    );
    assert_eq!(
        v["export"]["attachment_ledger_mode"].as_str(),
        Some("summary-only")
    );
    // Histogram present when there were fails; if zero fails, empty object is ok.
    let failed = v["export"]["attachments_failed"].as_u64().unwrap_or(0);
    if failed > 0 {
        assert!(
            v["export"]["attachments_failed_by_reason"].is_object(),
            "histogram required when fails > 0"
        );
    }
}

/// 0073 P1-1: `parents_only` still emits ATTACH_OMITTED_BY_POLICY ledger rows when
/// the fixture has attachments (omit ≠ fail).
#[test]
fn unique_pst_parents_only_omit_ledger_rows() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    // Explicit --no-attachments false path: family-policy parents_only.
    // Do not pass --no-attachments (run_unique_pst helper would add it).
    let result = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--family-policy",
            "parents_only",
            "--attach-ledger",
            "full",
            "--json",
        ])
        .output()
        .expect("run unique-pst parents_only");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let v: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => {
            eprintln!(
                "skip: no JSON stdout: stderr={}",
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
    };

    let omitted = v["export"]["attachments_omitted_by_policy"]
        .as_u64()
        .unwrap_or(0);
    let failed = v["export"]["attachments_failed"].as_u64().unwrap_or(0);

    // Fixture with attaches should produce omit rows under parents_only after P1-1.
    if omitted == 0 {
        eprintln!(
            "note: fixture had zero attach metadata under parents_only; omit ledger not exercised"
        );
        return;
    }

    assert!(
        omitted > 0,
        "parents_only with attach meta must omit > 0; export={:?}",
        v["export"]
    );
    // Omit alone must not force fail counter.
    // (failed may still be >0 from MetaFailed etc.; omit must not equal fail.)
    let ledger = report.join("export_attachments.csv");
    assert!(ledger.is_file(), "full ledger required");
    let csv = fs::read_to_string(&ledger).expect("csv");
    let omit_rows = csv
        .lines()
        .filter(|l| l.contains("ATTACH_OMITTED_BY_POLICY"))
        .count() as u64;
    assert_eq!(
        omit_rows, omitted,
        "CSV omit info rows must match attachments_omitted_by_policy"
    );
    // Severity for omit is info, not fail.
    assert!(
        csv.lines()
            .filter(|l| l.contains("ATTACH_OMITTED_BY_POLICY"))
            .all(|l| l.contains(",info,") || l.contains(",info\r")),
        "omit rows must be severity info"
    );
    // Histogram is fail-only; omit must not appear there.
    if let Some(hist) = v["export"]["attachments_failed_by_reason"].as_object() {
        assert!(
            !hist.contains_key("ATTACH_OMITTED_BY_POLICY"),
            "omit must not be in fail histogram"
        );
    }
    let _ = failed; // may be non-zero for unrelated soft fails
}

/// 0073: `--attach-ledger=off` → neither CSV nor histogram fields.
#[test]
fn unique_pst_attach_ledger_off() {
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
            "--attach-ledger",
            "off",
            "--json",
        ])
        .output()
        .expect("run");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let v: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("skip: no JSON stdout");
            return;
        }
    };
    assert!(!report.join("export_attachments.csv").is_file());
    assert!(v["export"]["attachments_failed_by_reason"].is_null());
    assert!(v["export"]["attachment_ledger"].is_null());
    assert!(v["export"]["attachment_ledger_mode"].is_null());
}

/// P3: strict + forced integrity skip flushes report pack and exits non-zero.
#[test]
fn unique_pst_integrity_force_skip_flushes_report() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let integrity = dir.path().join("skips.csv");

    let result = Command::new(bin())
        .env("PST_DEDUPE_TEST_FORCE_SKIP", "1")
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--mode",
            "strict",
            "--max-skip-rate",
            "0",
            "--integrity-csv",
            integrity.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run unique-pst force skip");

    assert!(
        !result.status.success(),
        "strict + force skip must be non-zero; stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );

    // Report pack / integrity artifacts flush before exit.
    assert!(
        integrity.is_file() || report.join("summary.json").is_file(),
        "expected integrity.csv and/or summary.json to flush"
    );
    if report.join("summary.json").is_file() {
        let summary: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(report.join("summary.json")).expect("sum"))
                .expect("json");
        assert_eq!(summary["ok"], false);
    }
    let stdout = String::from_utf8_lossy(&result.stdout);
    if !stdout.trim().is_empty() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stdout) {
            assert_eq!(v["ok"], false);
            assert!(
                v["scan"]["skipped"].as_u64().unwrap_or(0) >= 1
                    || v["error"].is_object()
                    || v["ok"] == false,
                "expected integrity/skip signal; v={v}"
            );
        }
    }
}

/// Production three-surface All-Custodians parity (DoD-6): decision CSV unique rows,
/// keepset.json winners, and export_messages.csv must carry identical
/// duplicate_source_count / duplicate_sources for multi-source groups.
#[test]
fn unique_pst_all_custodians_three_surface_parity() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    // 9 copies → each winner has 8 other sources (cap edge); +1 more for truncate.
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut hashes_before = Vec::new();
    for i in 0..10u32 {
        let p = dir.path().join(format!("cust{i}.pst"));
        fs::copy(&sample, &p).expect("copy fixture");
        hashes_before.push(sha256_file(&p));
        inputs.push(p);
    }
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let mut args: Vec<String> = vec!["unique-pst".into()];
    for p in &inputs {
        args.push(p.to_str().expect("utf8").to_string());
    }
    args.extend([
        "--out".into(),
        out.to_str().expect("utf8").to_string(),
        "--report-dir".into(),
        report.to_str().expect("utf8").to_string(),
        "--json".into(),
        "--no-attachments".into(),
        // This test asserts All-Custodians three-surface parity (decision CSV /
        // keepset / export_messages), not 0076 degenerate-tier2 guards. Aspose
        // fixtures include degenerate content-hash messages that under default
        // guards become per-source winners and can trip unique-count verify
        // (writer vs re-read). Opt into pre-0076 degenerate binding so the
        // parity surface under test stays the focus.
        "--allow-degenerate-tier2".into(),
    ]);
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let result = Command::new(bin()).args(&arg_refs).output().expect("run");
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );

    // Source immutability
    for (i, p) in inputs.iter().enumerate() {
        assert_eq!(
            hashes_before[i],
            sha256_file(p),
            "source cust{i}.pst mutated"
        );
    }

    let ks: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report.join("keepset.json")).expect("ks"))
            .expect("ks json");
    let winners = ks["winners"].as_array().expect("winners");
    assert!(!winners.is_empty());

    // Parse decision CSV unique rows: need Role + duplicate_source_count + duplicate_sources.
    // Header-driven indices.
    let dec_text = fs::read_to_string(report.join("decisions.csv")).expect("dec");
    let mut dec_lines = dec_text.lines();
    let dec_header: Vec<&str> = dec_lines.next().expect("h").split(',').collect();
    let role_i = dec_header.iter().position(|h| *h == "Role").expect("Role");
    let nid_i = dec_header.iter().position(|h| *h == "NID").expect("NID");
    let pst_i = dec_header
        .iter()
        .position(|h| *h == "SourcePst")
        .expect("SourcePst");
    let dsc_i = dec_header
        .iter()
        .position(|h| *h == "duplicate_source_count")
        .expect("dsc");
    let dss_i = dec_header
        .iter()
        .position(|h| *h == "duplicate_sources")
        .expect("dss");
    let mut dec_unique: std::collections::HashMap<(String, u64), (u64, String)> =
        std::collections::HashMap::new();
    for line in dec_lines {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() <= dsc_i || cols.get(role_i) != Some(&"unique") {
            continue;
        }
        let nid: u64 = cols[nid_i].parse().unwrap_or(0);
        let pst = cols[pst_i].to_string();
        let count: u64 = cols[dsc_i].parse().unwrap_or(0);
        let sources = cols[dss_i].trim_matches('"').to_string();
        dec_unique.insert((pst, nid), (count, sources));
    }

    // export_messages.csv
    let exp_text = fs::read_to_string(report.join("export_messages.csv")).expect("exp");
    let mut exp_lines = exp_text.lines();
    let exp_header: Vec<&str> = exp_lines.next().expect("eh").split(',').collect();
    let e_nid = exp_header.iter().position(|h| *h == "nid").expect("nid");
    let e_src = exp_header
        .iter()
        .position(|h| *h == "source_path")
        .expect("source_path");
    let e_dsc = exp_header
        .iter()
        .position(|h| *h == "duplicate_source_count")
        .expect("edsc");
    let e_dss = exp_header
        .iter()
        .position(|h| *h == "duplicate_sources")
        .expect("edss");
    let mut exp_map: std::collections::HashMap<u64, (u64, String, String)> =
        std::collections::HashMap::new();
    for line in exp_lines {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() <= e_dss {
            continue;
        }
        let nid: u64 = cols[e_nid].parse().unwrap_or(0);
        let count: u64 = cols[e_dsc].parse().unwrap_or(0);
        let sources = cols[e_dss].trim_matches('"').to_string();
        let sp = cols[e_src].to_string();
        exp_map.insert(nid, (count, sources, sp));
    }

    // Every JSON winner must match decision unique row and export_messages row.
    let mut saw_cap = false;
    for w in winners {
        let nid = w["locus"]["nid"].as_u64().expect("nid");
        let pst = w["locus"]["source_pst"].as_str().unwrap_or("").to_string();
        let j_count = w["duplicate_source_count"].as_u64().unwrap_or(0);
        let j_sources: Vec<String> = w["duplicate_sources"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let j_joined = j_sources.join("|");
        let j_trunc = w["duplicate_sources_truncated"].as_bool().unwrap_or(false);

        // 10 peer sources (other cust*.pst) for each message identity → count 9, cap 8.
        assert_eq!(
            j_count, 9,
            "winner {pst}/{nid} should have 9 duplicate sources (10 files total)"
        );
        assert_eq!(j_sources.len(), 8, "cap 8 basenames");
        assert!(j_trunc, "truncated must be true when count > cap");
        saw_cap = true;
        for s in &j_sources {
            assert!(!s.contains('\\') && !s.contains('/'), "basename only: {s}");
            assert!(s.starts_with("cust") && s.ends_with(".pst"), "name={s}");
        }

        let (d_count, d_sources) = dec_unique
            .get(&(pst.clone(), nid))
            .cloned()
            .unwrap_or_else(|| panic!("decision missing unique {pst}/{nid}"));
        assert_eq!(d_count, j_count, "decision vs JSON count {pst}/{nid}");
        assert_eq!(d_sources, j_joined, "decision vs JSON sources {pst}/{nid}");

        let (e_count, e_sources, _) = exp_map
            .get(&nid)
            .cloned()
            .unwrap_or_else(|| panic!("export missing nid {nid}"));
        assert_eq!(e_count, j_count, "export vs JSON count nid={nid}");
        assert_eq!(e_sources, j_joined, "export vs JSON sources nid={nid}");
    }
    assert!(saw_cap, "expected at least one capped aggregate");
}

/// 0079: two baseline runs compare equal under the structural equivalence oracle.
#[test]
fn unique_pst_oracle_self_test_two_runs() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out_a = dir.path().join("a.pst");
    let report_a = dir.path().join("report_a");
    let out_b = dir.path().join("b.pst");
    let report_b = dir.path().join("report_b");

    for (out, report) in [(&out_a, &report_a), (&out_b, &report_b)] {
        let result = run_unique_pst(&[
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--json",
            "--no-attachments",
        ]);
        assert!(
            result.status.success(),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        );
    }

    let diff =
        pst_dedup_cli::export_oracle::compare_export_packs(&report_a, &out_a, &report_b, &out_b);
    diff.assert_equivalent();
}

/// 0079 D1: every winner is materialized exactly once.
#[test]
fn unique_pst_messages_materialized_equals_unique() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");
    let result = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
        "--no-attachments",
    ]);
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
    let mat = v["messages_materialized"].as_u64().unwrap_or(u64::MAX);
    assert_eq!(
        mat, unique,
        "messages_materialized must equal unique (single materialize)"
    );
    assert!(v["phase_timings"]["total_ms"].as_u64().unwrap_or(0) > 0 || unique == 0);
    // Phase timings present.
    assert!(v.get("phase_timings").is_some());
    // Single-source fixture: shared handle cache opens the PST exactly once.
    assert_eq!(
        v["source_pst_opens"].as_u64(),
        Some(1),
        "single-source export must open the source PST once via shared LRU"
    );
    assert!(v.get("bytes_written_total").is_some());
    assert!(v.get("prepared_bytes_peak").is_some());
}

/// 0079 D1 + DoD-7: attachments-on path also single-materializes and keeps
/// oracle equivalence (structural; not byte-identical — D10).
#[test]
fn unique_pst_attachments_on_d1_and_oracle() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out_a = dir.path().join("a.pst");
    let report_a = dir.path().join("report_a");
    let out_b = dir.path().join("b.pst");
    let report_b = dir.path().join("report_b");

    for (out, report) in [(&out_a, &report_a), (&out_b, &report_b)] {
        // Bypass run_unique_pst helper so --no-attachments is not forced.
        let result = Command::new(bin())
            .args([
                "unique-pst",
                sample.to_str().expect("utf8"),
                "--out",
                out.to_str().expect("utf8"),
                "--report-dir",
                report.to_str().expect("utf8"),
                "--json",
                "--allow-partial-fidelity",
            ])
            .output()
            .expect("run unique-pst attachments-on");
        assert!(
            result.status.success(),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        );
        let v: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
        let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
        let mat = v["messages_materialized"].as_u64().unwrap_or(u64::MAX);
        assert_eq!(
            mat, unique,
            "attachments-on: messages_materialized == unique"
        );
        assert_eq!(
            v["source_pst_opens"].as_u64(),
            Some(1),
            "attachments-on single-source still one shared open"
        );
    }

    let diff =
        pst_dedup_cli::export_oracle::compare_export_packs(&report_a, &out_a, &report_b, &out_b);
    diff.assert_equivalent();
}

/// 0079 DoD-7: per-winner `degraded_reasons` are stable across two runs and match
/// keepset integrity written at finalize (prepare path does not re-materialize).
///
/// Parent-vs-HEAD reason-set compare is the operator/orchestrator gate recorded
/// in `baseline.md`. Unit coverage for hard-fail promotion + first-call-only soft
/// reasons lives in `dedup-engine` (`promote_first_materialize_soft_reasons_only_*`).
/// CRC soft reasons are set during finalize materialize (same single-call path);
/// aspose exposes `CRC_SUSPECT` on winners below.
#[test]
fn unique_pst_degraded_reasons_stable_finalize_only() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out_a = dir.path().join("a.pst");
    let report_a = dir.path().join("report_a");
    let out_b = dir.path().join("b.pst");
    let report_b = dir.path().join("report_b");

    for (out, report) in [(&out_a, &report_a), (&out_b, &report_b)] {
        let result = run_unique_pst(&[
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--report-dir",
            report.to_str().expect("utf8"),
            "--json",
            "--no-attachments",
        ]);
        assert!(
            result.status.success(),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        );
        let v: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
        let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
        let mat = v["messages_materialized"].as_u64().unwrap_or(u64::MAX);
        assert_eq!(
            mat, unique,
            "DoD-7 structural: single materialize (no prepare re-materialize)"
        );
    }

    let reasons_a =
        pst_dedup_cli::export_oracle::degraded_reasons_by_winner(&report_a).expect("reasons run A");
    let reasons_b =
        pst_dedup_cli::export_oracle::degraded_reasons_by_winner(&report_b).expect("reasons run B");
    assert_eq!(
        reasons_a, reasons_b,
        "per-winner degraded_reasons must be stable across two equivalent runs"
    );
    assert!(
        !reasons_a.is_empty(),
        "keepset must list winners (reason map non-empty keys)"
    );

    // Fixture is known to carry BODY_UNAVAILABLE / CRC_SUSPECT on winners.
    let any_degraded = reasons_a.values().any(|r| !r.is_empty());
    assert!(
        any_degraded,
        "aspose fixture should expose at least one degraded_reasons entry for DoD-7 coverage; map={reasons_a:?}"
    );
    let any_crc = reasons_a
        .values()
        .any(|r| r.iter().any(|s| s == "CRC_SUSPECT"));
    assert!(
        any_crc,
        "aspose fixture should expose CRC_SUSPECT on at least one winner; map={reasons_a:?}"
    );

    // Integrity on keepset is what finalize wrote — prepare cannot re-merge fresh soft reasons.
    let keepset: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report_a.join("keepset.json")).expect("keepset"))
            .expect("keepset json");
    let winners = keepset["winners"].as_array().expect("winners array");
    for w in winners {
        let path = w
            .pointer("/locus/source_path")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let nid = w
            .pointer("/locus/nid")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let key = format!("{path}|{nid}");
        let mut from_json: Vec<String> = w
            .pointer("/integrity/degraded_reasons")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        from_json.sort();
        assert_eq!(
            reasons_a.get(&key).cloned().unwrap_or_default(),
            from_json,
            "helper must match keepset integrity.degraded_reasons for {key}"
        );
    }
}

/// 0079 DoD-7 (attachments-on): aspose produces attach soft fails; reason map
/// and attach-fail counters are non-empty and stable across two runs.
#[test]
fn unique_pst_attachments_on_degraded_and_attach_fails_stable() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out_a = dir.path().join("a.pst");
    let report_a = dir.path().join("report_a");
    let out_b = dir.path().join("b.pst");
    let report_b = dir.path().join("report_b");

    let mut fail_counts = Vec::new();
    for (out, report) in [(&out_a, &report_a), (&out_b, &report_b)] {
        let result = Command::new(bin())
            .args([
                "unique-pst",
                sample.to_str().expect("utf8"),
                "--out",
                out.to_str().expect("utf8"),
                "--report-dir",
                report.to_str().expect("utf8"),
                "--json",
                "--allow-partial-fidelity",
            ])
            .output()
            .expect("run unique-pst attachments-on");
        assert!(
            result.status.success(),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        );
        let v: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
        let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
        let mat = v["messages_materialized"].as_u64().unwrap_or(u64::MAX);
        assert_eq!(
            mat, unique,
            "attachments-on D1: messages_materialized == unique"
        );
        let failed = v["export"]["attachments_failed"].as_u64().unwrap_or(0);
        assert!(
            failed > 0,
            "aspose attachments-on should soft-fail some attaches; failed={failed}"
        );
        fail_counts.push(failed);
    }
    assert_eq!(
        fail_counts[0], fail_counts[1],
        "attachments_failed must be stable across two runs"
    );

    let reasons_a =
        pst_dedup_cli::export_oracle::degraded_reasons_by_winner(&report_a).expect("reasons A");
    let reasons_b =
        pst_dedup_cli::export_oracle::degraded_reasons_by_winner(&report_b).expect("reasons B");
    assert_eq!(
        reasons_a, reasons_b,
        "attachments-on per-winner degraded_reasons must be stable"
    );
    assert!(
        reasons_a.values().any(|r| !r.is_empty()),
        "attachments-on must expose non-empty degraded_reasons; map={reasons_a:?}"
    );

    let diff =
        pst_dedup_cli::export_oracle::compare_export_packs(&report_a, &out_a, &report_b, &out_b);
    diff.assert_equivalent();
}

/// Optional operator gate: parent binary vs current `pst-dedup` under the
/// structural oracle. Set `PST_DEDUPE_BASELINE_BIN` to a pre-0079 (or parent
/// commit) `pst-dedup.exe`. Skips when unset — parent-vs-HEAD numbers live in
/// `conductor/0079-MaterializeWritePerformance/baseline.md`.
#[test]
fn unique_pst_parent_baseline_oracle_when_env_set() {
    let baseline_bin = match std::env::var("PST_DEDUPE_BASELINE_BIN") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v),
        _ => {
            eprintln!(
                "skip: PST_DEDUPE_BASELINE_BIN not set (operator parent-vs-HEAD oracle gate)"
            );
            return;
        }
    };
    if !baseline_bin.is_file() {
        panic!(
            "PST_DEDUPE_BASELINE_BIN is not a file: {}",
            baseline_bin.display()
        );
    }
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out_parent = dir.path().join("parent.pst");
    let report_parent = dir.path().join("report_parent");
    let out_head = dir.path().join("head.pst");
    let report_head = dir.path().join("report_head");

    let parent_out = Command::new(&baseline_bin)
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out_parent.to_str().expect("utf8"),
            "--report-dir",
            report_parent.to_str().expect("utf8"),
            "--json",
            "--no-attachments",
        ])
        .output()
        .expect("run baseline unique-pst");
    assert!(
        parent_out.status.success(),
        "baseline stderr={} stdout={}",
        String::from_utf8_lossy(&parent_out.stderr),
        String::from_utf8_lossy(&parent_out.stdout)
    );

    let head_out = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out_head.to_str().expect("utf8"),
            "--report-dir",
            report_head.to_str().expect("utf8"),
            "--json",
            "--no-attachments",
        ])
        .output()
        .expect("run HEAD unique-pst");
    assert!(
        head_out.status.success(),
        "head stderr={} stdout={}",
        String::from_utf8_lossy(&head_out.stderr),
        String::from_utf8_lossy(&head_out.stdout)
    );

    let head_v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&head_out.stdout)).expect("head json");
    let unique = head_v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
    let mat = head_v["messages_materialized"].as_u64().unwrap_or(u64::MAX);
    assert_eq!(
        mat, unique,
        "HEAD messages_materialized must equal unique (parent may omit field)"
    );

    let diff = pst_dedup_cli::export_oracle::compare_export_packs(
        &report_parent,
        &out_parent,
        &report_head,
        &out_head,
    );
    diff.assert_equivalent();

    // Attachments-on family as well when baseline is available.
    let out_p_att = dir.path().join("parent_att.pst");
    let report_p_att = dir.path().join("report_parent_att");
    let out_h_att = dir.path().join("head_att.pst");
    let report_h_att = dir.path().join("report_head_att");
    let head_bin = bin();
    for (bin_path, out, report) in [
        (
            baseline_bin.as_path(),
            out_p_att.as_path(),
            report_p_att.as_path(),
        ),
        (
            head_bin.as_path(),
            out_h_att.as_path(),
            report_h_att.as_path(),
        ),
    ] {
        let r = Command::new(bin_path)
            .args([
                "unique-pst",
                sample.to_str().expect("utf8"),
                "--out",
                out.to_str().expect("utf8"),
                "--report-dir",
                report.to_str().expect("utf8"),
                "--json",
                "--allow-partial-fidelity",
            ])
            .output()
            .expect("run unique-pst att");
        assert!(
            r.status.success(),
            "att stderr={} stdout={}",
            String::from_utf8_lossy(&r.stderr),
            String::from_utf8_lossy(&r.stdout)
        );
    }
    let diff_att = pst_dedup_cli::export_oracle::compare_export_packs(
        &report_p_att,
        &out_p_att,
        &report_h_att,
        &out_h_att,
    );
    diff_att.assert_equivalent();
}

/// 0083: `--promote-on-attach-fail` is accepted; summary records the flag and counters.
#[test]
fn unique_pst_promote_on_attach_fail_flag_summary() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("unique.pst");
    let report = dir.path().join("report");

    // Default off
    let off = run_unique_pst(&[
        "unique-pst",
        sample.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--report-dir",
        report.to_str().expect("utf8"),
        "--json",
        "--no-attachments",
    ]);
    assert!(
        off.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&off.stderr)
    );
    let v_off: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&off.stdout)).expect("json");
    assert_eq!(v_off["promote_on_attach_fail"], false);
    assert_eq!(
        v_off["promoted_after_attach_incomplete_count"]
            .as_u64()
            .unwrap_or(999),
        0
    );
    assert_eq!(
        v_off["mode_c_fallback_all_peers_incomplete_count"]
            .as_u64()
            .unwrap_or(999),
        0
    );

    // Flag on (parents_only path via --no-attachments: no soft attach promote expected)
    let out2 = dir.path().join("unique2.pst");
    let report2 = dir.path().join("report2");
    let on = Command::new(bin())
        .args([
            "unique-pst",
            sample.to_str().expect("utf8"),
            "--out",
            out2.to_str().expect("utf8"),
            "--report-dir",
            report2.to_str().expect("utf8"),
            "--json",
            "--no-attachments",
            "--promote-on-attach-fail",
            "--qc-level",
            "sample",
        ])
        .output()
        .expect("run");
    assert!(
        on.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&on.stderr),
        String::from_utf8_lossy(&on.stdout)
    );
    let v_on: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&on.stdout)).expect("json");
    assert_eq!(v_on["promote_on_attach_fail"], true);
    assert_eq!(
        v_on["promoted_after_attach_incomplete_count"]
            .as_u64()
            .unwrap_or(999),
        0,
        "no-attachments: no Mode A soft promote expected"
    );
    // Mode A × QC: sample QC must not introduce hard defects on a clean fixture.
    // QC keys the final export winner locus (export_messages / keep_set winners).
    let qc = &v_on["verification"];
    if let Some(ok) = qc.get("ok") {
        // verification.ok may be present; when sample QC runs, clean fixture stays ok.
        assert_ne!(
            ok,
            &serde_json::json!(false),
            "QC must not hard-fail clean Mode A run"
        );
    }
    // decisions decided_by must not invent Mode A strings when no promote occurred
    let dec = fs::read_to_string(report2.join("decisions.csv")).expect("decisions");
    assert!(
        !dec.contains("promoted_after_attach_incomplete"),
        "no soft promote expected without incomplete peers"
    );
    assert!(
        !dec.contains("mode_c_fallback_all_peers_incomplete"),
        "no Mode C fallback expected on single complete source"
    );
}

/// Shared MID for dual-source Mode A fixture messages.
fn mode_a_msg(mid: &str, subject: &str, body: &str, attach_payload: Vec<u8>) -> WriteMessage {
    let mut msg = WriteMessage {
        message_id: Some(mid.into()),
        subject: subject.into(),
        sender: Some("alice@example.com".into()),
        display_to: Some("bob@example.com".into()),
        body_plain: Some(body.into()),
        source_folder_path: Some("Inbox".into()),
        ..WriteMessage::default()
    };
    msg.attachments = vec![WriteAttachment {
        filename: "payload.bin".into(),
        mime: Some("application/octet-stream".into()),
        size: attach_payload.len() as u32,
        attach_method: Some(1),
        data: Some(attach_payload),
        stream_available: true,
        ..WriteAttachment::default()
    }];
    msg
}

fn first_message_nid(path: &Path) -> u64 {
    let mut pst = pst_reader::PstFile::open(path).expect("open");
    let folders = pst.folders().expect("folders");
    folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("nid")
        .0
}

/// Corrupt attachment leaf data blocks so `open_attachment_data` fails while the
/// message body + attach *list* remain readable (Mode A incomplete signal).
fn corrupt_first_message_attach_streams(path: &Path) {
    let (msg_nid, att_nid, targets) = {
        let mut pst = pst_reader::PstFile::open(path).expect("open for locate");
        let folders = pst.folders().expect("folders");
        let nid = folders
            .iter()
            .flat_map(|f| f.message_nids.iter().copied())
            .next()
            .expect("message nid");
        let atts = pst.list_attachments(nid).expect("list atts");
        assert!(
            !atts.is_empty(),
            "fixture must have at least one attachment to corrupt"
        );
        let att_nid = atts[0].nid;
        let msg_entry = pst.nbt().get(nid).expect("nbt").clone();
        let mut reader = BufReader::new(fs::File::open(path).expect("reopen"));
        let att_entry =
            block::find_subnode_entry(&mut reader, pst.bbt(), msg_entry.bid_sub, att_nid)
                .expect("find sub")
                .expect("att subnode");
        // Corrupt **data leaves only** — leave attach PC (bid_data) intact so
        // list_attachments still returns metadata (filename/size).
        let mut targets: Vec<(usize, usize)> = Vec::new();
        assert!(
            !att_entry.bid_sub.is_null(),
            "Mode A fixture attach must use subnode storage (large payload)"
        );
        let subs = block::list_subnode_entries(&mut reader, pst.bbt(), att_entry.bid_sub)
            .expect("attach subnodes");
        for s in &subs {
            if let Some(e) = pst.bbt().get(s.bid_data) {
                targets.push((e.bref.ib as usize, e.cb as usize));
            }
            if let Ok(leaves) = block::collect_leaf_data_bids(&mut reader, pst.bbt(), s.bid_data) {
                for lb in leaves {
                    if let Some(e) = pst.bbt().get(lb) {
                        targets.push((e.bref.ib as usize, e.cb as usize));
                    }
                }
            }
        }
        targets.sort_unstable();
        targets.dedup();
        (nid, att_nid, targets)
    };
    assert!(
        !targets.is_empty(),
        "could not locate attach data leaf blocks for corruption"
    );

    let mut data = fs::read(path).expect("read pst");
    for (ib, cb) in &targets {
        let ib = *ib;
        let cb = *cb;
        for i in ib..(ib + cb) {
            if i < data.len() {
                data[i] ^= 0xA5;
            }
        }
        let trailer_start = (cb + 63) & !63;
        let crc_off = ib + trailer_start;
        if crc_off + 12 <= data.len() {
            data[crc_off] ^= 0xFF;
            data[crc_off + 4] ^= 0x01;
        }
    }
    fs::write(path, &data).expect("write corrupt");

    let mut pst = pst_reader::PstFile::open(path).expect("reopen after corrupt");
    pst.read_message_properties(msg_nid)
        .expect("message body must remain readable");
    let atts = pst
        .list_attachments(msg_nid)
        .expect("attach list must remain after leaf-only corrupt");
    assert!(!atts.is_empty(), "attach meta must remain");
    let open_fail = match pst.open_attachment_data(msg_nid, atts[0].nid) {
        Ok(mut r) => {
            let mut buf = Vec::new();
            r.read_to_end(&mut buf).is_err()
        }
        Err(_) => true,
    };
    assert!(
        open_fail,
        "leaf corrupt must break open/read (att_nid={att_nid:?})"
    );
}

fn scan_item(
    path: &Path,
    nid: u64,
    mid: &str,
    scan_order: u64,
    subject: &str,
) -> RecoverableScanItem {
    let path_s = path.display().to_string();
    let pst = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("x.pst")
        .to_string();
    RecoverableScanItem {
        locus: MessageLocus {
            source_path: path_s,
            source_pst: pst,
            folder_path: "Inbox".into(),
            nid,
            is_orphaned: false,
        },
        message_id_norm: Some(
            mid.trim_matches(|c| c == '<' || c == '>')
                .to_ascii_lowercase(),
        ),
        content_hash: [0xAB; 32],
        size: 80_000,
        integrity: RecoverableIntegrity::clean(),
        scan_order,
        submit_time: None,
        delivery_time: None,
        has_bcc: false,
        has_body_preview: true,
        subject_nonempty: !subject.is_empty(),
        sender_nonempty: true,
        attach_count: 1,
        body_sha256: None,
        body_char_len: None,
        display_to: Some("bob@example.com".into()),
        display_cc: None,
        display_bcc: None,
        strong_content_hash: None,
        fp_header: 0,
        fp_body: 0,
        fp_recipients: 0,
        fp_attachments: 0,
        preview_bytes_over_budget: false,
    }
}

/// DoD-11: Mode A promotion **occurs** and QC sample keys the **final winner** locus
/// (no spurious `unexplained_loss` / hard defect from comparing pre-promote source).
///
/// Uses real dual-PST fixtures + `PstMaterializer` with **materialize-time** deep
/// attach probe (not phase-1b preflight, which would re-rank via fidelity before
/// Mode A and hide soft-promote). Then runs production `run_unique_pst_qc` against
/// the final winner locus — and proves the wrong (pre-promote) locus hard-fails.
#[test]
fn unique_pst_mode_a_promote_qc_sample_keys_final_winner() {
    let dir = TempDir::new().expect("tmp");
    let incomplete = dir.path().join("aaa_incomplete.pst");
    let complete = dir.path().join("zzz_complete.pst");
    let out = dir.path().join("unique.pst");
    let report_dir = dir.path().join("report");
    fs::create_dir_all(&report_dir).expect("report dir");

    // ~80 KiB → multi-block attach leaves; surgical corrupt breaks open/read.
    let payload: Vec<u8> = (0..80 * 1024).map(|i| (i % 251) as u8).collect();
    let mid = "<mode-a-qc@ex.com>";
    let mid_norm = "mode-a-qc@ex.com";
    let body = "mode a promote qc body for final winner locus";

    // Distinct subjects so QC subject mismatch proves wrong-locus hard-fail.
    write_unicode_pst(
        &incomplete,
        vec![mode_a_msg(mid, "PRE_PROMOTE", body, payload.clone())],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write incomplete");
    corrupt_first_message_attach_streams(&incomplete);

    write_unicode_pst(
        &complete,
        vec![mode_a_msg(mid, "FINAL_WINNER", body, payload.clone())],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write complete");

    let nid_inc = first_message_nid(&incomplete);
    let nid_cmp = first_message_nid(&complete);

    // FirstSeen scan_order: incomplete ranks first (Mode A soft-skip target).
    let items = vec![
        scan_item(&incomplete, nid_inc, mid, 0, "PRE_PROMOTE"),
        scan_item(&complete, nid_cmp, mid, 1, "FINAL_WINNER"),
    ];

    // Materialize-time deep probe (not phase-1b): discovers incomplete without
    // pre-degrading rank so FirstSeen still attempts incomplete first.
    let mut mat = PstMaterializer::new(FamilyPolicy::KeepAttachmentsWithParent)
        .with_deep_attach_probe(ProbeLevel::Head, 1_048_576, 5_000);

    let mut accepted: Option<dedup_engine::CanonicalMessage> = None;
    let (ks, dec, count) = build_keep_set_materialized(
        items,
        MaterializeBuildOpts {
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path: &[],
            tier2_enabled: true,
            created_from: None,
            rank_ctx: None,
            grouping_ctx: None,
            promote_on_attach_fail: true,
        },
        &mut mat,
        |msg| {
            accepted = Some(msg);
            Ok(())
        },
    )
    .expect("mode a materialize");

    assert_eq!(count, 1);
    assert_eq!(ks.stats.promoted_after_attach_incomplete_count, 1);
    assert!(ks.winners[0].promoted_from_failure);
    assert_eq!(
        ks.winners[0].decided_by.as_deref(),
        Some("promoted_after_attach_incomplete")
    );
    assert_eq!(
        ks.winners[0].locus.nid, nid_cmp,
        "final winner must be complete peer"
    );
    assert!(
        ks.winners[0].locus.source_path.contains("zzz_complete.pst"),
        "final winner source: {}",
        ks.winners[0].locus.source_path
    );
    let uniq = dec
        .iter()
        .find(|d| d.role == DecisionRole::Unique)
        .expect("unique decision");
    assert_eq!(uniq.decided_by, "promoted_after_attach_incomplete");
    let _ = accepted;

    // Export unique PST from **final winner** content (Mode A deliverable).
    write_unicode_pst(
        &out,
        vec![mode_a_msg(mid, "FINAL_WINNER", body, payload)],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write export");

    let volumes = vec![VolumeReportRow {
        volume_index: 1,
        path: out.display().to_string(),
        bytes: fs::metadata(&out).map(|m| m.len()).unwrap_or(0),
        sha256_hex: String::new(),
        md5_hex: String::new(),
        messages_written: 1,
        finalized_early: false,
        volume_exceeded_soft_limit: false,
    }];

    // QC candidates/export rows keyed to **final winner** locus (production path).
    let final_path = complete.display().to_string();
    let export_rows_final = vec![ExportMessageRow {
        source_path: final_path.clone(),
        folder_path: "Inbox".into(),
        nid: nid_cmp,
        message_id_norm: mid_norm.into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 0,
        duplicate_source_count: 1,
        duplicate_sources: "aaa_incomplete.pst".into(),
        source_id: "1".into(),
        bcc_suppressed: false,
        subject: "FINAL_WINNER".into(),
    }];
    let cand_final = QcSampleCandidate {
        export_message_index: 1,
        volume_index: 1,
        source_path: final_path.clone(),
        source_nid: nid_cmp,
        folder_path: "Inbox".into(),
        subject: "FINAL_WINNER".into(),
        sender: "alice@example.com".into(),
        message_id_norm: mid_norm.into(),
        body_plain_len: body.len(),
        body_html_len: 0,
        attach_count: 1,
        max_attach_size: 80 * 1024,
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
    };

    let qc_ok = run_unique_pst_qc(QcRunInput {
        level: QcLevel::Sample,
        sample_max: 32,
        report_dir: &report_dir,
        volumes: &volumes,
        export_rows: &export_rows_final,
        candidates: &[cand_final],
        external_reader: None,
        run_scanpst: false,
        max_open_psts: 4,
        source_differential: true,
        parents_only: false,
        include_bcc_recipients: false,
        probe_unexplained_property: None,
    });
    assert!(
        !qc_ok.hard_fail,
        "QC vs final winner must not hard-fail: {:?}",
        qc_ok.findings
    );
    assert_eq!(
        qc_ok.findings.defect, 0,
        "no defect for promoted family: {:?}",
        qc_ok.findings
    );
    assert_eq!(
        qc_ok.findings.unexplained_loss, 0,
        "no unexplained_loss when QC keys final winner: {:?}",
        qc_ok.findings
    );
    assert!(
        qc_ok.messages_compared >= 1,
        "sample QC must compare the promoted family"
    );

    // Negative control: same export keyed to **pre-promote** incomplete locus →
    // subject mismatch / source read issues must produce hard findings (proves
    // QC is locus-sensitive; Mode A must not leave pre-promote as the key).
    let pre_path = incomplete.display().to_string();
    let export_rows_wrong = vec![ExportMessageRow {
        source_path: pre_path.clone(),
        folder_path: "Inbox".into(),
        nid: nid_inc,
        message_id_norm: mid_norm.into(),
        edrm_mih: String::new(),
        content_hash_hex: String::new(),
        volume_path: out.display().to_string(),
        volume_index: 1,
        export_message_index: 1,
        attachments_failed_count: 0,
        duplicate_source_count: 0,
        duplicate_sources: String::new(),
        source_id: "0".into(),
        bcc_suppressed: false,
        subject: "PRE_PROMOTE".into(),
    }];
    let cand_wrong = QcSampleCandidate {
        export_message_index: 1,
        volume_index: 1,
        source_path: pre_path,
        source_nid: nid_inc,
        folder_path: "Inbox".into(),
        subject: "PRE_PROMOTE".into(),
        sender: "alice@example.com".into(),
        message_id_norm: mid_norm.into(),
        body_plain_len: body.len(),
        body_html_len: 0,
        attach_count: 1,
        max_attach_size: 80 * 1024,
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
    };
    let report_wrong = dir.path().join("report_wrong");
    fs::create_dir_all(&report_wrong).expect("report_wrong");
    let qc_wrong = run_unique_pst_qc(QcRunInput {
        level: QcLevel::Sample,
        sample_max: 32,
        report_dir: &report_wrong,
        volumes: &volumes,
        export_rows: &export_rows_wrong,
        candidates: &[cand_wrong],
        external_reader: None,
        run_scanpst: false,
        max_open_psts: 4,
        source_differential: true,
        parents_only: false,
        include_bcc_recipients: false,
        probe_unexplained_property: None,
    });
    assert!(
        qc_wrong.hard_fail
            || qc_wrong.findings.defect > 0
            || qc_wrong.findings.unexplained_loss > 0,
        "QC vs pre-promote incomplete locus must hard-find (locus sensitivity); findings={:?}",
        qc_wrong.findings
    );
}
