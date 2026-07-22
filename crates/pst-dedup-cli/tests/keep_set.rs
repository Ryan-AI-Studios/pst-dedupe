//! Integration tests for track 0066 keep-set CLI.

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
fn keep_set_json_schema_and_decision_csv_header() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let dec = dir.path().join("decisions.csv");
    let ks = dir.path().join("keepset.json");

    let out = Command::new(bin())
        .args([
            "keep-set",
            sample.to_str().expect("utf8"),
            "--json",
            "--decision-csv",
            dec.to_str().expect("utf8"),
            "--keep-set-json",
            ks.to_str().expect("utf8"),
        ])
        .output()
        .expect("run keep-set");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["schema"].as_str(), Some("keep_set_v1"));
    assert_eq!(v["ok"], true);
    assert!(v["keep_set"]["winners"].is_array());
    assert!(v["keep_set"]["stats"].is_object());
    assert!(v["keep_set"]["stats"]["recoverable"].as_u64().unwrap_or(0) > 0);

    assert!(dec.exists(), "decision CSV must exist");
    let dec_text = fs::read_to_string(&dec).expect("read decisions");
    let header = dec_text.lines().next().unwrap_or("");
    assert!(
        header.contains("SourcePath")
            && header.contains("Role")
            && header.contains("ContentHash")
            && header.contains("PromotedFromFailure"),
        "decision header required: {header}"
    );
    let data_rows: Vec<&str> = dec_text
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .collect();
    assert!(
        !data_rows.is_empty(),
        "expected ≥1 decision row; content={dec_text}"
    );

    assert!(ks.exists(), "keep-set JSON must exist");
    let ks_v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&ks).expect("read ks")).expect("ks json");
    assert_eq!(ks_v["schema"].as_str(), Some("keep_set_v1"));
}

#[test]
fn keep_set_input_flag_works() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let out = Command::new(bin())
        .args([
            "keep-set",
            "--input",
            sample.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run keep-set --input");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert_eq!(v["schema"].as_str(), Some("keep_set_v1"));
    assert_eq!(v["ok"], true);
}

#[test]
fn path_order_determinism_two_copies() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    // Two copies of the same content under different names → full-file dups.
    // Names chosen so sort order is deterministic: a_copy before z_copy.
    let a_copy = dir.path().join("a_copy.pst");
    let z_copy = dir.path().join("z_copy.pst");
    fs::copy(&sample, &a_copy).expect("copy a");
    fs::copy(&sample, &z_copy).expect("copy z");

    let run = |first: &Path, second: &Path| -> serde_json::Value {
        let out = Command::new(bin())
            .args([
                "keep-set",
                first.to_str().expect("utf8"),
                second.to_str().expect("utf8"),
                "--json",
                "--policy",
                "first_seen",
            ])
            .output()
            .expect("run keep-set");
        assert!(
            out.status.success(),
            "stderr={} stdout={}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json")
    };

    let v1 = run(&z_copy, &a_copy); // arg order swapped vs sorted order
    let v2 = run(&a_copy, &z_copy);

    let winners1 = v1["keep_set"]["winners"].as_array().expect("w1");
    let winners2 = v2["keep_set"]["winners"].as_array().expect("w2");
    assert_eq!(
        winners1.len(),
        winners2.len(),
        "winner count must match across arg order"
    );
    // Same content copies: unique set size should be ≤ messages in one PST.
    assert!(!winners1.is_empty());

    // Winner loci (pst name + nid) must be identical after path sort.
    let keys = |winners: &Vec<serde_json::Value>| -> Vec<(String, u64)> {
        let mut k: Vec<(String, u64)> = winners
            .iter()
            .map(|w| {
                let pst = w["locus"]["source_pst"].as_str().unwrap_or("").to_string();
                let nid = w["locus"]["nid"].as_u64().unwrap_or(0);
                (pst, nid)
            })
            .collect();
        k.sort();
        k
    };
    assert_eq!(
        keys(winners1),
        keys(winners2),
        "path-sorted keep-set winners must not depend on CLI arg order"
    );

    // first_seen after path sort prefers a_copy over z_copy for ties.
    // At least one winner should come from a_copy when full dups exist.
    let from_a = winners1
        .iter()
        .filter(|w| {
            w["locus"]["source_pst"]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case("a_copy.pst")
        })
        .count();
    assert!(
        from_a > 0,
        "expected winners from a_copy (lexicographically first); winners={winners1:?}"
    );
}

#[test]
fn integrity_skip_not_in_decision() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let dec = dir.path().join("decisions.csv");
    let integrity = dir.path().join("skips.csv");

    let out = Command::new(bin())
        .env("PST_DEDUPE_TEST_FORCE_SKIP", "1")
        .args([
            "keep-set",
            sample.to_str().expect("utf8"),
            "--mode",
            "strict",
            "--json",
            "--decision-csv",
            dec.to_str().expect("utf8"),
            "--integrity-csv",
            integrity.to_str().expect("utf8"),
            // Force non-zero on any skip in strict.
            "--max-skip-rate",
            "0",
        ])
        .output()
        .expect("run keep-set force skip");

    // Strict + forced skips should be non-zero, but artifacts flush first.
    assert!(
        !out.status.success(),
        "strict + force skip must be non-zero; stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );

    assert!(integrity.exists(), "integrity CSV must exist");
    let integrity_text = fs::read_to_string(&integrity).expect("read integrity");
    assert!(
        integrity_text.contains("MESSAGE_READ_FAILED")
            || integrity_text.contains("skip")
            || integrity_text.contains("test force skip"),
        "expected skip rows; content={integrity_text}"
    );

    assert!(dec.exists(), "decision CSV must still flush");
    let dec_text = fs::read_to_string(&dec).expect("read decisions");
    // Skipped messages must not appear as decision rows (only recoverable).
    // Decision rows may still exist for non-forced messages, but force-skip
    // reason text must not be in the decision CSV.
    assert!(
        !dec_text.contains("test force skip"),
        "force-skip messages must not appear in decision CSV"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["ok"], false);
    assert!(
        v["scan"]["skipped"].as_u64().unwrap_or(0) >= 1,
        "scan.skipped must be >= 1; scan={}",
        v["scan"]
    );
}

#[test]
fn source_immutability_after_keep_set() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let copy = dir.path().join("copy.pst");
    fs::copy(&sample, &copy).expect("copy");
    let before = sha256_file(&copy);

    let out = Command::new(bin())
        .args([
            "keep-set",
            copy.to_str().expect("utf8"),
            "--json",
            "--materialize",
            "--decision-csv",
            dir.path().join("dec.csv").to_str().expect("utf8"),
            "--keep-set-json",
            dir.path().join("ks.json").to_str().expect("utf8"),
        ])
        .output()
        .expect("run keep-set");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );

    let after = sha256_file(&copy);
    assert_eq!(before, after, "source PST bytes must not change");
}

#[test]
fn strict_non_zero_flushes_artifacts() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let dec = dir.path().join("decisions.csv");
    let ks = dir.path().join("keepset.json");
    let integrity = dir.path().join("skips.csv");

    let out = Command::new(bin())
        .env("PST_DEDUPE_TEST_FORCE_SKIP", "1")
        .args([
            "keep-set",
            sample.to_str().expect("utf8"),
            "--mode",
            "strict",
            "--json",
            "--decision-csv",
            dec.to_str().expect("utf8"),
            "--keep-set-json",
            ks.to_str().expect("utf8"),
            "--integrity-csv",
            integrity.to_str().expect("utf8"),
            "--max-skip-rate",
            "0",
        ])
        .output()
        .expect("run");

    assert!(
        !out.status.success(),
        "strict integrity fail must be non-zero"
    );
    assert!(
        dec.exists(),
        "decision-csv must be written before non-zero exit"
    );
    assert!(
        ks.exists(),
        "keep-set-json must be written before non-zero exit"
    );
    let dec_header = fs::read_to_string(&dec)
        .expect("dec")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    assert!(
        dec_header.contains("SourcePath"),
        "decision CSV header required: {dec_header}"
    );
    let ks_v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&ks).expect("ks")).expect("ks json");
    assert_eq!(ks_v["schema"].as_str(), Some("keep_set_v1"));
}

#[test]
fn empty_paths_usage_exit() {
    let out = Command::new(bin())
        .args(["keep-set", "--json"])
        .output()
        .expect("run");
    assert!(!out.status.success());
    // Usage exit is 2 (clap missing required OR our empty-merge check).
    // Clap may also fail if it requires something else; accept non-zero.
    let code = out.status.code();
    assert!(
        code == Some(2) || code == Some(1),
        "expected usage-ish exit, got {code:?}; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn materialize_preserves_body_unavailable_sole_winners() {
    // Fixture has messages whose full extract hits Invalid HID (BODY_UNAVAILABLE at scan)
    // but properties recover. Materialize must keep them as unique+degraded, not drop.
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let dec = dir.path().join("dec.csv");
    let ks = dir.path().join("ks.json");

    let out = Command::new(bin())
        .args([
            "keep-set",
            sample.to_str().expect("utf8"),
            "--json",
            "--materialize",
            "--decision-csv",
            dec.to_str().expect("utf8"),
            "--keep-set-json",
            ks.to_str().expect("utf8"),
        ])
        .output()
        .expect("run keep-set --materialize");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    let recoverable = v["keep_set"]["stats"]["recoverable"].as_u64().unwrap_or(0);
    let unique = v["keep_set"]["stats"]["unique"].as_u64().unwrap_or(0);
    let failed = v["keep_set"]["stats"]["materialize_failed"]
        .as_u64()
        .unwrap_or(0);
    let dropped = v["keep_set"]["stats"]["groups_dropped_materialize"]
        .as_u64()
        .unwrap_or(0);
    assert!(recoverable >= 1, "expected recoverable messages");
    assert_eq!(
        unique, recoverable,
        "sole BODY_UNAVAILABLE winners must remain unique (not ghost-dropped); v={}",
        v["keep_set"]["stats"]
    );
    assert_eq!(failed, 0, "no materialize_failed expected on this fixture");
    assert_eq!(dropped, 0, "no groups_dropped_materialize expected");
    assert!(
        v["keep_set"]["stats"]["degraded_winners"]
            .as_u64()
            .unwrap_or(0)
            >= 1,
        "fixture has known degraded body winners"
    );

    let dec_text = fs::read_to_string(&dec).expect("dec");
    assert!(
        !dec_text.contains("materialize_failed"),
        "decision CSV must not mark materialize_failed for recoverable body-unavail"
    );
}
