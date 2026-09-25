//! Integration tests for track 0144 integrity CSV honesty.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_aspose() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
}

fn fixture_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample.pst")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .env_remove("RUST_LOG")
        .env("NO_COLOR", "1")
        .args(args)
        .output()
        .expect("pst-dedup")
}

fn parse_stdout_json(out: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "json parse: {e}; stdout={stdout} stderr={}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn assert_header_only(path: &std::path::Path) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines
        .next()
        .unwrap_or_else(|| panic!("missing header: {text}"));
    assert!(
        header.contains("SourcePath") && header.contains("Class"),
        "9-column header required: {header}"
    );
    assert_eq!(
        header.split(',').count(),
        9,
        "header must have 9 columns: {header}"
    );
    let extra: Vec<_> = lines.collect();
    assert!(
        extra.is_empty(),
        "header-only: no data rows; extra={extra:?} text={text}"
    );
}

fn assert_poly_header_only_summary(summary: &Value) {
    assert_eq!(summary["integrity_csv_rows"].as_u64(), Some(0));
    assert_eq!(
        summary["integrity_csv_omitted_reason"].as_str(),
        Some(pst_dedup_cli::scan::INTEGRITY_CSV_OMITTED_CRC_TAINT)
    );
    assert!(
        summary["crc_suspect_messages"].as_u64().unwrap_or(0) > 0,
        "crc_suspect_messages: {summary}"
    );
    assert!(
        summary["poly_class_crc_sources"].as_u64().unwrap_or(0) >= 1,
        "poly_class_crc_sources: {summary}"
    );
    assert!(
        summary.get("poly_crc_note").is_some(),
        "0143 note still present"
    );
}

#[test]
fn scan_json_aspose_integrity_csv_header_only() {
    let sample = fixture_aspose();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let integrity = dir.path().join("skips.csv");
    let path = sample.to_str().expect("utf8");
    let ic = integrity.to_str().expect("utf8");
    let out = run(&["scan", path, "--json", "--integrity-csv", ic]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert_poly_header_only_summary(&v["summary"]);
    assert_header_only(&integrity);
}

#[test]
fn dups_json_aspose_integrity_csv_header_only() {
    let sample = fixture_aspose();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let integrity = dir.path().join("skips.csv");
    let path = sample.to_str().expect("utf8");
    let ic = integrity.to_str().expect("utf8");
    let out = run(&["dups", path, "--json", "--integrity-csv", ic]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert_poly_header_only_summary(&v["summary"]);
    assert_header_only(&integrity);
}

#[test]
fn scan_json_aspose_csv_sidecar_honesty() {
    let sample = fixture_aspose();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let csv = dir.path().join("report.csv");
    let sidecar = dir.path().join("report.integrity.csv");
    let path = sample.to_str().expect("utf8");
    let csv_s = csv.to_str().expect("utf8");
    let out = run(&["scan", path, "--json", "--csv", csv_s]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert_poly_header_only_summary(&v["summary"]);
    assert!(sidecar.exists(), "sidecar missing");
    assert_header_only(&sidecar);
}

#[test]
fn scan_json_sample_pst_is_poly_header_only() {
    // Execute-time re-check: sample.pst is dual-rate poly (crc_suspect=1), not a clean store.
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/sample.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let integrity = dir.path().join("skips.csv");
    let path = sample.to_str().expect("utf8");
    let ic = integrity.to_str().expect("utf8");
    let out = run(&["scan", path, "--json", "--integrity-csv", ic]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert_poly_header_only_summary(&v["summary"]);
    assert_header_only(&integrity);
}

#[test]
fn scan_json_no_sink_omits_both_keys() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/sample.pst missing");
        return;
    }
    let path = sample.to_str().expect("utf8");
    let out = run(&["scan", path, "--json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    let summary = &v["summary"];
    assert!(summary.get("integrity_csv_rows").is_none(), "{summary}");
    assert!(
        summary.get("integrity_csv_omitted_reason").is_none(),
        "{summary}"
    );
}

#[test]
fn scan_human_aspose_integrity_csv_taint_sentence() {
    let sample = fixture_aspose();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let integrity = dir.path().join("skips.csv");
    let path = sample.to_str().expect("utf8");
    let ic = integrity.to_str().expect("utf8");
    let out = run(&["scan", path, "--integrity-csv", ic]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("CRC_SUSPECT is taint, not a skip"),
        "human line missing; stdout={stdout}"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !err.lines()
            .any(|l| l.contains("note:") && l.contains("taint")),
        "no extra 0144 stderr note; stderr={err}"
    );
}
