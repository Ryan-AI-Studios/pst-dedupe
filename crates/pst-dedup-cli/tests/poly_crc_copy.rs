//! Integration tests for track 0143 poly-class CRC operator copy.

use std::path::PathBuf;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
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

fn poly_note_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|l| l.contains("note:") && l.contains("poly-class CRC"))
        .collect()
}

fn expected_note(sources: u64) -> String {
    pst_dedup_cli::scan::poly_crc_note(sources).expect("note")
}

#[test]
fn scan_json_aspose_poly_crc_note() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
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
    assert!(v.get("poly_class_crc").is_none(), "no root boolean");
    let summary = &v["summary"];
    assert!(summary.get("poly_class_crc").is_none());
    assert_eq!(summary["files"][0]["poly_class_crc"], true);
    let sources = summary["poly_class_crc_sources"]
        .as_u64()
        .expect("poly_class_crc_sources");
    assert!(sources >= 1, "aspose must be poly-class; {summary}");
    assert_eq!(
        summary["poly_crc_note"].as_str(),
        Some(expected_note(sources).as_str())
    );
    assert_eq!(summary["preflight"]["recommendation"].as_str(), Some("ok"));
    let reasons = summary["preflight"]["reasons"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        reasons.iter().all(|r| {
            r.as_str()
                .map(|s| !s.to_ascii_lowercase().contains("poly"))
                .unwrap_or(true)
        }),
        "preflight.reasons must not contain poly; {reasons:?}"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    let notes = poly_note_lines(&err);
    assert_eq!(notes.len(), 1, "exactly one poly note; stderr={err}");
    assert!(notes[0].contains(&expected_note(sources)));
}

#[test]
fn scan_json_crc_log_limit_zero_still_prints_poly_note() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let path = sample.to_str().expect("utf8");
    let out = run(&["scan", path, "--json", "--crc-log-limit", "0"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _v = parse_stdout_json(&out);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        poly_note_lines(&err).len(),
        1,
        "crc-log-limit 0 must not silence poly note; stderr={err}"
    );
}

#[test]
fn dups_json_aspose_carries_poly_crc_note() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let path = sample.to_str().expect("utf8");
    let out = run(&["dups", path, "--json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert!(v["summary"]["poly_crc_note"].is_string());
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(poly_note_lines(&err).len(), 1, "stderr={err}");
}

#[test]
fn scan_human_aspose_prints_poly_sources_and_note() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let path = sample.to_str().expect("utf8");
    let out = run(&["scan", path]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("poly_sources="),
        "human crc line; stdout={stdout}"
    );
    assert!(
        stdout.contains("poly_crc:"),
        "human poly_crc line; stdout={stdout}"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(poly_note_lines(&err).len(), 1, "stderr={err}");
}

#[test]
fn keep_set_json_aspose_poly_note_on_stderr() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let path = sample.to_str().expect("utf8");
    let out = run(&["keep-set", path, "--json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(poly_note_lines(&err).len(), 1, "stderr={err}");
}
