//! Track 0145: `scan_candidates_v1` emit + `keep-set --from-scan-json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn run_ok(args: &[&str]) -> std::process::Output {
    let out = Command::new(bin()).args(args).output().expect("run");
    assert!(
        out.status.success(),
        "cmd {args:?} failed status={:?} stderr={} stdout={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    out
}

fn run_code(args: &[&str], expect: i32) -> std::process::Output {
    let out = Command::new(bin()).args(args).output().expect("run");
    assert_eq!(
        out.status.code(),
        Some(expect),
        "cmd {args:?} code={:?} stderr={} stdout={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    out
}

fn winner_keys(keep_set_json: &Path) -> Vec<(String, u64)> {
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(keep_set_json).expect("read ks")).expect("json");
    let mut keys: Vec<(String, u64)> = v["winners"]
        .as_array()
        .expect("winners")
        .iter()
        .map(|w| {
            (
                w["locus"]["source_path"].as_str().unwrap_or("").to_string(),
                w["locus"]["nid"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    keys.sort();
    keys
}

#[test]
fn emit_schema_and_scan_json_has_no_candidates_key() {
    let sample = fixture("aspose_outlook.pst");
    if !sample.exists() {
        eprintln!("skip: aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let cand = dir.path().join("cands.json");
    let out = run_ok(&[
        "scan",
        sample.to_str().expect("utf8"),
        "--json",
        "--emit-candidates",
        cand.to_str().expect("utf8"),
    ]);
    let stdout: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("scan json");
    assert!(stdout.get("candidates").is_none());
    assert!(stdout["summary"].is_object());

    let sidecar: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cand).expect("sidecar")).expect("cand json");
    assert_eq!(sidecar["schema"].as_str(), Some("scan_candidates_v1"));
    let n = sidecar["candidates"].as_array().expect("arr").len() as u64;
    assert_eq!(
        n,
        sidecar["summary"]["recoverable_messages"]
            .as_u64()
            .unwrap_or(0)
    );
    assert!(n > 0);
    assert!(String::from_utf8_lossy(&out.stderr).contains("candidates:"));
}

#[test]
fn emit_sorts_two_inputs_default_scan_does_not() {
    let a = fixture("aspose_outlook.pst");
    let b = fixture("sample.pst");
    if !a.exists() || !b.exists() {
        eprintln!("skip: fixtures missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let cand = dir.path().join("cands.json");
    // Argv order: sample then aspose (often reverse of sorted basename).
    run_ok(&[
        "scan",
        b.to_str().expect("utf8"),
        a.to_str().expect("utf8"),
        "--emit-candidates",
        cand.to_str().expect("utf8"),
        "--json",
    ]);
    let sidecar: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cand).expect("read")).expect("json");
    let order: Vec<String> = sidecar["input_path_sort_order"]
        .as_array()
        .expect("order")
        .iter()
        .map(|v| v.as_str().unwrap_or("").to_string())
        .collect();
    let mut expected = order.clone();
    expected.sort_by_key(|s| s.to_lowercase());
    assert_eq!(order, expected, "emit must sort like keep-set");

    let scan_out = run_ok(&[
        "scan",
        b.to_str().expect("utf8"),
        a.to_str().expect("utf8"),
        "--json",
    ]);
    let scan: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&scan_out.stdout)).expect("scan json");
    let first = scan["summary"]["files"][0]["path"].as_str().unwrap_or("");
    assert!(
        first.to_lowercase().contains("sample") || first.ends_with("sample.pst"),
        "default scan stays argv order, first={first}"
    );
}

#[test]
fn keep_set_reuse_matches_live_winners() {
    let sample = fixture("aspose_outlook.pst");
    if !sample.exists() {
        eprintln!("skip: aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let cand = dir.path().join("cands.json");
    let live_ks = dir.path().join("live.json");
    let reuse_ks = dir.path().join("reuse.json");
    run_ok(&[
        "scan",
        sample.to_str().expect("utf8"),
        "--emit-candidates",
        cand.to_str().expect("utf8"),
        "--json",
    ]);
    run_ok(&[
        "keep-set",
        sample.to_str().expect("utf8"),
        "--keep-set-json",
        live_ks.to_str().expect("utf8"),
        "--json",
    ]);
    run_ok(&[
        "keep-set",
        "--from-scan-json",
        cand.to_str().expect("utf8"),
        "--keep-set-json",
        reuse_ks.to_str().expect("utf8"),
        "--json",
    ]);
    assert_eq!(winner_keys(&live_ks), winner_keys(&reuse_ks));
    let live: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&live_ks).expect("live")).expect("json");
    let reuse: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&reuse_ks).expect("reuse")).expect("json");
    assert_eq!(live["stats"], reuse["stats"]);
}

#[test]
fn fingerprint_mismatch_exits_2() {
    let sample = fixture("aspose_outlook.pst");
    if !sample.exists() {
        eprintln!("skip: aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let cand = dir.path().join("cands.json");
    let ks = dir.path().join("ks.json");
    run_ok(&[
        "scan",
        sample.to_str().expect("utf8"),
        "--emit-candidates",
        cand.to_str().expect("utf8"),
        "--json",
    ]);
    let out = run_code(
        &[
            "keep-set",
            sample.to_str().expect("utf8"),
            "--from-scan-json",
            cand.to_str().expect("utf8"),
            "--no-tier2",
            "--keep-set-json",
            ks.to_str().expect("utf8"),
        ],
        2,
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("fingerprint"), "{err}");
    assert!(!ks.exists(), "must not write keep-set JSON on mismatch");
}

#[test]
fn stale_mtime_exits_2() {
    let sample = fixture("aspose_outlook.pst");
    if !sample.exists() {
        eprintln!("skip: aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let local = dir.path().join("mail.pst");
    fs::copy(&sample, &local).expect("copy");
    let cand = dir.path().join("cands.json");
    run_ok(&[
        "scan",
        local.to_str().expect("utf8"),
        "--emit-candidates",
        cand.to_str().expect("utf8"),
        "--json",
    ]);
    let f = fs::OpenOptions::new()
        .write(true)
        .open(&local)
        .expect("open");
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        .expect("mtime");
    drop(f);
    run_code(
        &[
            "keep-set",
            local.to_str().expect("utf8"),
            "--from-scan-json",
            cand.to_str().expect("utf8"),
        ],
        2,
    );
}

#[test]
fn wrong_schema_scan_json_and_keep_set_envelope_exit_2() {
    let sample = fixture("aspose_outlook.pst");
    if !sample.exists() {
        eprintln!("skip: aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let scan_json = dir.path().join("scan.json");
    let scan_out = run_ok(&["scan", sample.to_str().expect("utf8"), "--json"]);
    fs::write(&scan_json, &scan_out.stdout).expect("write scan json");
    run_code(
        &[
            "keep-set",
            "--from-scan-json",
            scan_json.to_str().expect("utf8"),
        ],
        2,
    );

    let ks_out = run_ok(&["keep-set", sample.to_str().expect("utf8"), "--json"]);
    let env_path = dir.path().join("envelope.json");
    fs::write(&env_path, &ks_out.stdout).expect("write env");
    run_code(
        &[
            "keep-set",
            "--from-scan-json",
            env_path.to_str().expect("utf8"),
        ],
        2,
    );
}

#[test]
fn extra_envelope_key_exits_2() {
    let sample = fixture("aspose_outlook.pst");
    if !sample.exists() {
        eprintln!("skip: aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let cand = dir.path().join("cands.json");
    run_ok(&[
        "scan",
        sample.to_str().expect("utf8"),
        "--emit-candidates",
        cand.to_str().expect("utf8"),
        "--json",
    ]);
    let mut v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cand).expect("read")).expect("json");
    v["extra_field"] = serde_json::json!(true);
    fs::write(&cand, serde_json::to_string_pretty(&v).expect("ser")).expect("write");
    run_code(
        &["keep-set", "--from-scan-json", cand.to_str().expect("utf8")],
        2,
    );
}

#[test]
fn positional_mismatch_and_path_guard_exit_2() {
    let sample = fixture("aspose_outlook.pst");
    let other = fixture("sample.pst");
    if !sample.exists() || !other.exists() {
        eprintln!("skip: fixtures missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let cand = dir.path().join("cands.json");
    run_ok(&[
        "scan",
        sample.to_str().expect("utf8"),
        "--emit-candidates",
        cand.to_str().expect("utf8"),
        "--json",
    ]);
    run_code(
        &[
            "keep-set",
            other.to_str().expect("utf8"),
            "--from-scan-json",
            cand.to_str().expect("utf8"),
        ],
        2,
    );
    run_code(
        &[
            "keep-set",
            sample.to_str().expect("utf8"),
            "--from-scan-json",
            sample.to_str().expect("utf8"),
        ],
        2,
    );
}

#[test]
fn dups_help_has_no_from_scan_json() {
    let out = Command::new(bin())
        .args(["dups", "--help"])
        .output()
        .expect("help");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("from-scan-json"), "{text}");
}

#[test]
fn keep_set_help_names_from_scan_json() {
    let out = Command::new(bin())
        .args(["keep-set", "--help"])
        .output()
        .expect("help");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("from-scan-json"), "{text}");
    assert!(text.contains("scan_candidates_v1"), "{text}");
}
