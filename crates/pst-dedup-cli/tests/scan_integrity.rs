//! Integration tests for track 0065 scan integrity.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_sample() -> PathBuf {
    // aspose_outlook.pst has messages; sample.pst is structure-only.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
}

/// Full-file SHA-256 hex digest (proves whole-file immutability, not just head/tail).
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

#[test]
fn happy_path_sample_json_schema() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/sample.pst missing");
        return;
    }
    let out = Command::new(bin())
        .args(["scan", sample.to_str().expect("utf8"), "--json"])
        .output()
        .expect("run scan");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let summary = &v["summary"];
    assert_eq!(summary["schema"].as_str(), Some("scan_integrity_v1"));
    assert_eq!(summary["mode"].as_str(), Some("best-effort"));
    assert!(summary["preflight"].is_object());
    assert!(summary["preflight"]["recommendation"].is_string());
    assert!(summary["files"].is_array());
    let status = summary["files"][0]["status"].as_str();
    assert!(
        status == Some("opened") || status == Some("partial"),
        "status={status:?}"
    );
}

#[test]
fn missing_path_usage_exit() {
    let out = Command::new(bin())
        .args(["scan", "C:\\nonexistent\\no-such-file.pst", "--json"])
        .output()
        .expect("run");
    assert!(!out.status.success());
    // Usage exit is 2
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn multi_file_bad_open_without_allow_nonzero() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/sample.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let bad = dir.path().join("junk.pst");
    fs::write(&bad, b"not a real pst file!!!!").expect("write bad");

    let out = Command::new(bin())
        .args([
            "scan",
            sample.to_str().expect("utf8"),
            bad.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run");
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["ok"], false);
    assert!(v["summary"]["failed_files"].as_u64().unwrap_or(0) >= 1);
    assert!(v["summary"]["recoverable_messages"].as_u64().unwrap_or(0) > 0);
}

#[test]
fn multi_file_bad_open_with_allow_zero() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/sample.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let bad = dir.path().join("junk.pst");
    fs::write(&bad, b"not a real pst file!!!!").expect("write bad");

    let out = Command::new(bin())
        .args([
            "scan",
            sample.to_str().expect("utf8"),
            bad.to_str().expect("utf8"),
            "--json",
            "--allow-failed-files",
        ])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["ok"], true);
    assert!(v["summary"]["failed_files"].as_u64().unwrap_or(0) >= 1);
    assert!(v["summary"]["recoverable_messages"].as_u64().unwrap_or(0) > 0);
    // Preflight should note failed file rate
    let rec = v["summary"]["preflight"]["recommendation"].as_str();
    assert!(
        rec == Some("re_export_recommended")
            || rec == Some("not_export_ready")
            || rec == Some("ok"),
        "rec={rec:?}"
    );
}

#[test]
fn source_immutability_after_scan() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/sample.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let copy = dir.path().join("copy.pst");
    fs::copy(&sample, &copy).expect("copy");
    let before = sha256_file(&copy);

    let out = Command::new(bin())
        .args(["scan", copy.to_str().expect("utf8"), "--json"])
        .output()
        .expect("run");
    assert!(out.status.success());

    let after = sha256_file(&copy);
    assert_eq!(before, after, "source PST bytes must not change");
}

#[test]
fn streaming_integrity_csv_sidecar_and_strict_artifacts() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/sample.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let csv = dir.path().join("report.csv");
    let integrity = dir.path().join("report.integrity.csv");

    // Good fixture in strict with --csv: streams integrity sidecar (header at minimum).
    let out = Command::new(bin())
        .args([
            "scan",
            sample.to_str().expect("utf8"),
            "--csv",
            csv.to_str().expect("utf8"),
            "--json",
            "--mode",
            "strict",
        ])
        .output()
        .expect("run");

    // Good fixture in strict should succeed if no skips.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(csv.exists(), "dedup csv must exist after scan");
    // Sidecar auto-created when --csv set
    assert!(
        integrity.exists(),
        "integrity sidecar must exist; stdout={stdout}"
    );
    let integrity_text = fs::read_to_string(&integrity).expect("read integrity");
    assert!(
        integrity_text.contains("SourcePath"),
        "header required: {integrity_text}"
    );
    assert!(integrity_text.contains("IsOrphaned"));
    assert!(integrity_text.contains("Class"));
}

/// Strict mode + forced message skips must exit non-zero and stream integrity CSV rows.
#[test]
fn strict_force_skip_nonzero_and_integrity_csv() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let report = dir.path().join("report.csv");
    let skips = dir.path().join("skips.csv");

    let out = Command::new(bin())
        .env("PST_DEDUPE_TEST_FORCE_SKIP", "1")
        .args([
            "scan",
            sample.to_str().expect("utf8"),
            "--mode",
            "strict",
            "--csv",
            report.to_str().expect("utf8"),
            "--json",
            "--integrity-csv",
            skips.to_str().expect("utf8"),
        ])
        .output()
        .expect("run");

    assert!(
        !out.status.success(),
        "strict + real skips must be non-zero; stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );

    assert!(skips.exists(), "integrity CSV must exist");
    let integrity_text = fs::read_to_string(&skips).expect("read integrity");
    let mut lines = integrity_text.lines();
    let header = lines.next().unwrap_or("");
    assert!(
        header.contains("Class") && header.contains("Reason"),
        "header required: {header}"
    );
    let data_rows: Vec<&str> = lines.filter(|l| !l.trim().is_empty()).collect();
    assert!(
        !data_rows.is_empty(),
        "expected ≥1 integrity data row; content={integrity_text}"
    );
    assert!(
        data_rows.iter().any(|r| r.contains("MESSAGE_READ_FAILED")
            || r.contains("skip")
            || r.contains("test force skip")),
        "expected skip / MESSAGE_READ_FAILED row; content={integrity_text}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["summary"]["schema"].as_str(), Some("scan_integrity_v1"));
    assert!(
        v["summary"]["skipped"].as_u64().unwrap_or(0) >= 1,
        "summary.skipped must be >= 1; summary={}",
        v["summary"]
    );
}

#[test]
fn integrity_csv_explicit_path() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/sample.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let ic = dir.path().join("skips.csv");
    let out = Command::new(bin())
        .args([
            "scan",
            sample.to_str().expect("utf8"),
            "--integrity-csv",
            ic.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run");
    assert!(out.status.success());
    assert!(ic.exists());
    let text = fs::read_to_string(&ic).expect("read");
    assert!(text.lines().next().unwrap_or("").contains("Reason"));
}

#[test]
fn not_pst_extension_usage() {
    let dir = TempDir::new().expect("tmp");
    let f = dir.path().join("file.txt");
    fs::write(&f, b"x").expect("write");
    let out = Command::new(bin())
        .args(["scan", f.to_str().expect("utf8")])
        .output()
        .expect("run");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn non_finite_max_skip_rate_rejected() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixture missing");
        return;
    }
    let out = Command::new(bin())
        .args([
            "scan",
            sample.to_str().expect("utf8"),
            "--max-skip-rate",
            "NaN",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(!out.status.success(), "NaN threshold must fail usage");
}

#[test]
fn resolved_paths_are_absolute_in_json() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixture missing");
        return;
    }
    let out = Command::new(bin())
        .args(["scan", sample.to_str().expect("utf8"), "--json"])
        .output()
        .expect("run");
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let path = v["summary"]["files"][0]["path"]
        .as_str()
        .expect("path string");
    assert!(
        Path::new(path).is_absolute(),
        "source path must be absolute, got {path}"
    );
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for x in chars.by_ref() {
                if x.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn folder_progress_line_count(stderr: &str) -> usize {
    // Spec: folder-only events have `folder=` and omit `msg_i=`. Strip ANSI so
    // styled field names still match (0140 Codex P2).
    strip_ansi(stderr)
        .split(['\n', '\r'])
        .filter(|l| l.contains("folder=") && !l.contains("msg_i="))
        .count()
}

fn probe_progress_line_count(stderr: &str) -> usize {
    stderr
        .lines()
        .filter(|l| l.contains("attempted=") && l.contains("bytes=") && l.contains("source="))
        .count()
}

fn scan_cmd(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .env_remove("RUST_LOG")
        .env("NO_COLOR", "1")
        .args(args)
        .output()
        .expect("run scan")
}

#[test]
fn scan_folder_progress_cadence_aspose() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let path = sample.to_str().expect("utf8");

    let default = scan_cmd(&["scan", path, "--json"]);
    assert!(
        default.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&default.stderr)
    );
    let default_json: serde_json::Value =
        serde_json::from_slice(&default.stdout).expect("default json");
    assert!(default_json.is_object());
    let folders = default_json["summary"]["files"][0]["folders"]
        .as_u64()
        .expect("folders") as usize;
    assert!(
        folders >= 2,
        "fixture must have multiple folders, got {folders}"
    );
    let default_err = String::from_utf8_lossy(&default.stderr);
    assert_eq!(
        folder_progress_line_count(&default_err),
        0,
        "default verbosity must hide folder progress; stderr={default_err}"
    );

    let v = scan_cmd(&["-v", "scan", path, "--json"]);
    assert!(
        v.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&v.stderr)
    );
    let v_json: serde_json::Value = serde_json::from_slice(&v.stdout).expect("-v json");
    assert!(v_json.is_object());
    let v_err = String::from_utf8_lossy(&v.stderr);
    let v_n = folder_progress_line_count(&v_err);
    assert!(
        v_n < folders,
        "-v folder progress must be cadence, got {v_n} for {folders} folders; stderr={v_err}"
    );
    assert!(v_n >= 1, "-v must emit at least first/last; stderr={v_err}");

    let vv = scan_cmd(&["-vv", "scan", path, "--json"]);
    assert!(
        vv.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&vv.stderr)
    );
    let vv_err = String::from_utf8_lossy(&vv.stderr);
    let vv_n = folder_progress_line_count(&vv_err);
    assert_eq!(
        vv_n, folders,
        "-vv must be one folder line each, got {vv_n} for {folders}; stderr={vv_err}"
    );
}

#[test]
fn scan_deep_attach_crc_log_limit_zero_silences_probe_lines() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let path = sample.to_str().expect("utf8");
    let out = scan_cmd(&[
        "scan",
        path,
        "--json",
        "--deep-attach-preflight",
        "--crc-log-limit",
        "0",
    ]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let probe = &v["summary"]["preflight"]["attach_probe"];
    assert_eq!(probe["enabled"], true, "probe must run; {probe}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        probe_progress_line_count(&err),
        0,
        "crc-log-limit 0 must silence per-attempt probe lines; stderr={err}"
    );
}

#[test]
fn scan_deep_attach_tiny_byte_budget_exposes_0147_coverage() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let path = sample.to_str().expect("utf8");
    let out = scan_cmd(&[
        "scan",
        path,
        "--json",
        "--deep-attach-preflight",
        "--deep-attach-level",
        "head",
        "--deep-attach-max-probe-bytes",
        "1024",
    ]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    let probe = &v["summary"]["preflight"]["attach_probe"];
    assert_eq!(probe["enabled"], true, "probe must run; {probe}");
    if probe["truncated"] == true {
        let reason = probe["budget_exhausted_reason"].as_str();
        assert!(
            matches!(
                reason,
                Some("probe_bytes")
                    | Some("max_attaches")
                    | Some("per_attach_timeout")
                    | Some("cancel")
            ),
            "truncated probe needs a 0147 reason; {probe}"
        );
        assert!(probe.get("candidate_attaches_total").is_some(), "{probe}");
    }
}

#[test]
fn scan_and_global_help_name_cadence() {
    let global = Command::new(bin())
        .env_remove("RUST_LOG")
        .args(["--help"])
        .output()
        .expect("help");
    let g = format!(
        "{}{}",
        String::from_utf8_lossy(&global.stdout),
        String::from_utf8_lossy(&global.stderr)
    );
    assert!(
        g.contains("periodic") && g.contains("scan/dups/keep-set"),
        "global -v help; {g}"
    );

    let scan = Command::new(bin())
        .env_remove("RUST_LOG")
        .args(["scan", "--help"])
        .output()
        .expect("scan help");
    let s = format!(
        "{}{}",
        String::from_utf8_lossy(&scan.stdout),
        String::from_utf8_lossy(&scan.stderr)
    );
    assert!(
        s.contains("per-attempt") && s.contains("crc-log-limit"),
        "scan --crc-log-limit help; {s}"
    );
    assert!(
        s.contains("256 MiB") && s.contains("mid-store"),
        "0147 scan --deep-attach-max-probe-bytes help; {s}"
    );

    let dups = Command::new(bin())
        .env_remove("RUST_LOG")
        .args(["dups", "--help"])
        .output()
        .expect("dups help");
    let d = format!(
        "{}{}",
        String::from_utf8_lossy(&dups.stdout),
        String::from_utf8_lossy(&dups.stderr)
    );
    assert!(d.contains("per-attempt"), "dups --crc-log-limit help; {d}");
}
