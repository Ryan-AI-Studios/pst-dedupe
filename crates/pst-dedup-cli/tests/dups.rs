//! Integration tests for track 0142 `dups --json` listing totals.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
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

fn two_copies() -> Option<(TempDir, PathBuf, PathBuf)> {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return None;
    }
    let dir = TempDir::new().expect("tmp");
    let a = dir.path().join("a_copy.pst");
    let z = dir.path().join("z_copy.pst");
    fs::copy(&sample, &a).expect("copy a");
    fs::copy(&sample, &z).expect("copy z");
    Some((dir, a, z))
}

fn assert_listing_keys(v: &Value) {
    assert!(v["duplicates"].is_array(), "duplicates must stay an array");
    assert!(v["duplicates_total"].is_u64());
    assert!(v["duplicates_shown"].is_u64());
    assert!(v["duplicates_truncated"].is_boolean());
    assert!(v.get("duplicates_limit").is_some());
}

#[test]
fn dups_json_limit_one_two_copies() {
    let Some((_dir, a, z)) = two_copies() else {
        return;
    };
    let out = Command::new(bin())
        .args([
            "dups",
            a.to_str().expect("utf8"),
            z.to_str().expect("utf8"),
            "--json",
            "--limit",
            "1",
        ])
        .output()
        .expect("dups");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert_listing_keys(&v);
    let total = v["duplicates_total"].as_u64().expect("total");
    let shown = v["duplicates_shown"].as_u64().expect("shown");
    let summary_dups = v["summary"]["duplicates"]
        .as_u64()
        .expect("summary.duplicates");
    let t1 = v["summary"]["tier1_hits"].as_u64().unwrap_or(0);
    let t2 = v["summary"]["tier2_hits"].as_u64().unwrap_or(0);
    assert!(
        total > 1,
        "two aspose copies should yield >1 dups, got {total}"
    );
    assert_eq!(total, summary_dups);
    assert_eq!(total, t1 + t2);
    assert_eq!(shown, 1);
    assert_eq!(v["duplicates"].as_array().expect("arr").len(), 1);
    assert_eq!(v["duplicates_limit"].as_u64(), Some(1));
    assert_eq!(v["duplicates_truncated"], true);
}

#[test]
fn dups_json_limit_zero_unlimited() {
    let Some((_dir, a, z)) = two_copies() else {
        return;
    };
    let out = Command::new(bin())
        .args([
            "dups",
            a.to_str().expect("utf8"),
            z.to_str().expect("utf8"),
            "--json",
            "--limit",
            "0",
        ])
        .output()
        .expect("dups");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert_listing_keys(&v);
    let total = v["duplicates_total"].as_u64().expect("total");
    let shown = v["duplicates_shown"].as_u64().expect("shown");
    assert_eq!(shown, total);
    assert_eq!(v["duplicates"].as_array().expect("arr").len() as u64, total);
    assert!(
        v["duplicates_limit"].is_null(),
        "unlimited must be JSON null"
    );
    assert_eq!(v["duplicates_truncated"], false);
}

#[test]
fn dups_json_limit_ge_total() {
    let Some((_dir, a, z)) = two_copies() else {
        return;
    };
    let out = Command::new(bin())
        .args([
            "dups",
            a.to_str().expect("utf8"),
            z.to_str().expect("utf8"),
            "--json",
            "--limit",
            "10000",
        ])
        .output()
        .expect("dups");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    let total = v["duplicates_total"].as_u64().expect("total");
    assert_eq!(v["duplicates_shown"].as_u64(), Some(total));
    assert_eq!(v["duplicates_limit"].as_u64(), Some(10_000));
    assert_eq!(v["duplicates_truncated"], false);
}

#[test]
fn dups_json_single_aspose_zero_dups() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let out = Command::new(bin())
        .args(["dups", sample.to_str().expect("utf8"), "--json"])
        .output()
        .expect("dups");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert_listing_keys(&v);
    assert_eq!(v["duplicates_total"].as_u64(), Some(0));
    assert_eq!(v["duplicates_shown"].as_u64(), Some(0));
    assert_eq!(v["duplicates"].as_array().expect("arr").len(), 0);
    assert_eq!(v["duplicates_truncated"], false);
}

#[test]
fn scan_json_dups_parity_limit_one() {
    let Some((_dir, a, z)) = two_copies() else {
        return;
    };
    let out = Command::new(bin())
        .args([
            "scan",
            a.to_str().expect("utf8"),
            z.to_str().expect("utf8"),
            "--json",
            "--dups",
            "--limit",
            "1",
        ])
        .output()
        .expect("scan --dups");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert_listing_keys(&v);
    assert_eq!(v["duplicates"].as_array().expect("arr").len(), 1);
    assert_eq!(v["duplicates_limit"].as_u64(), Some(1));
    assert_eq!(v["duplicates_truncated"], true);
    assert!(v["duplicates_total"].as_u64().expect("total") > 1);
}

#[test]
fn scan_json_without_dups_omits_listing_fields() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let out = Command::new(bin())
        .args(["scan", sample.to_str().expect("utf8"), "--json"])
        .output()
        .expect("scan");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = parse_stdout_json(&out);
    assert!(v["duplicates"].is_null());
    assert!(v.get("duplicates_total").is_none());
    assert!(v.get("duplicates_shown").is_none());
    assert!(v.get("duplicates_truncated").is_none());
    assert!(v.get("duplicates_limit").is_none());
}

#[test]
fn dups_human_limit_one_shows_n_of_m() {
    let Some((_dir, a, z)) = two_copies() else {
        return;
    };
    let out = Command::new(bin())
        .args([
            "dups",
            a.to_str().expect("utf8"),
            z.to_str().expect("utf8"),
            "--limit",
            "1",
        ])
        .output()
        .expect("dups human");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 of") && stdout.contains("shown"),
        "human list must print N of M shown; stdout={stdout}"
    );
}

#[test]
fn dups_help_names_limit_sample() {
    let out = Command::new(bin())
        .env_remove("RUST_LOG")
        .args(["dups", "--help"])
        .output()
        .expect("help");
    let d = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        d.contains("per-attempt"),
        "0140 crc-log-limit help retained; {d}"
    );
    assert!(
        d.contains("duplicates_total") && d.contains("unlimited"),
        "0142 --limit help; {d}"
    );
}
