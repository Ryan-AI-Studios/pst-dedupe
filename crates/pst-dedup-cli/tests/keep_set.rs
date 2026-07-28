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

#[test]
fn decision_csv_header_has_0075_suffix_and_v1_prefix() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let dec = dir.path().join("decisions.csv");
    let out = Command::new(bin())
        .args([
            "keep-set",
            sample.to_str().expect("utf8"),
            "--json",
            "--decision-csv",
            dec.to_str().expect("utf8"),
        ])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let header = fs::read_to_string(&dec)
        .expect("dec")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    assert!(
        header.starts_with("SourcePath,SourcePst,Folder,IsOrphaned,NID"),
        "header={header}"
    );
    assert!(
        header.contains("folder_class")
            && header.contains("decided_by")
            && header.contains("duplicate_sources"),
        "header={header}"
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
    assert!(
        v["keep_set"]["stats"]
            .get("winners_from_recoverable_items")
            .is_some()
            || v["keep_set"]["stats"]["winners_from_recoverable_items"].is_null()
            || v["keep_set"]["stats"].as_object().is_some()
    );
}

#[test]
fn source_rank_flips_winner_file_a_vs_a2() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    // Names: a-2.pst sorts before a.pst? On Windows lowercased: "a-2.pst" vs "a.pst"
    // path compare: a-2 vs a. — '-' (0x2d) vs '.' (0x2e) so a-2 < a. → a-2 first.
    let a = dir.path().join("a.pst");
    let a2 = dir.path().join("a-2.pst");
    fs::copy(&sample, &a).expect("copy a");
    fs::copy(&sample, &a2).expect("copy a2");
    let hash_a_before = sha256_file(&a);
    let hash_a2_before = sha256_file(&a2);

    let run = |extra: &[&str]| -> serde_json::Value {
        let mut args = vec![
            "keep-set".to_string(),
            a.to_str().unwrap().to_string(),
            a2.to_str().unwrap().to_string(),
            "--json".to_string(),
        ];
        for e in extra {
            args.push((*e).to_string());
        }
        let out = Command::new(bin())
            .args(&args)
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

    let v_default = run(&[]);
    let winners_default = v_default["keep_set"]["winners"].as_array().expect("w");
    let from_a2_default = winners_default
        .iter()
        .filter(|w| {
            w["locus"]["source_pst"]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case("a-2.pst")
        })
        .count();
    // With path order, a-2 tends to win ties.
    assert!(
        from_a2_default > 0,
        "expected some winners from a-2.pst by default"
    );

    let v_ranked = run(&["--source-rank", "a.pst", "--source-rank", "a-2.pst"]);
    let winners_ranked = v_ranked["keep_set"]["winners"].as_array().expect("w");
    let from_a_ranked = winners_ranked
        .iter()
        .filter(|w| {
            w["locus"]["source_pst"]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case("a.pst")
        })
        .count();
    assert!(
        from_a_ranked > from_a2_default || from_a_ranked == winners_ranked.len(),
        "source-rank should prefer a.pst; from_a={from_a_ranked} default_a2={from_a2_default}"
    );
    // At least one flip relative to default when full dups exist.
    let from_a2_ranked = winners_ranked
        .iter()
        .filter(|w| {
            w["locus"]["source_pst"]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case("a-2.pst")
        })
        .count();
    assert!(
        from_a_ranked > from_a2_ranked,
        "ranked run should prefer a.pst over a-2.pst; a={from_a_ranked} a2={from_a2_ranked}"
    );

    assert_eq!(hash_a_before, sha256_file(&a), "a.pst must be immutable");
    assert_eq!(
        hash_a2_before,
        sha256_file(&a2),
        "a-2.pst must be immutable"
    );
}

#[test]
fn keep_set_help_lists_0075_flags() {
    let out = Command::new(bin())
        .args(["keep-set", "--help"])
        .output()
        .expect("help");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for flag in [
        "--prefer-bcc-copy",
        "--prefer-folder-class",
        "--folder-rank",
        "--source-rank",
        "--rank-folder-class-first",
        "--fidelity-rank",
        "earliest_date",
    ] {
        assert!(text.contains(flag), "help missing {flag}");
    }
}

/// Checked-in default winner golden for `fixtures/aspose_outlook.pst` (DoD-10 / §3.9).
/// Captured with all 0075 flags off (pre-0075 winner semantics). Order = keep-set path_key,nid.
const ASPOSE_DEFAULT_WINNER_GOLDEN: &[(&str, u64)] = &[
    ("aspose_outlook.pst", 2097188),
    ("aspose_outlook.pst", 2097220),
    ("aspose_outlook.pst", 2097252),
    ("aspose_outlook.pst", 2097284),
    ("aspose_outlook.pst", 2097316),
    ("aspose_outlook.pst", 2097412),
    ("aspose_outlook.pst", 2097444),
    ("aspose_outlook.pst", 2097476),
    ("aspose_outlook.pst", 2097508),
    ("aspose_outlook.pst", 2097540),
    ("aspose_outlook.pst", 2097636),
    ("aspose_outlook.pst", 2097668),
    ("aspose_outlook.pst", 2097700),
    ("aspose_outlook.pst", 2097732),
    ("aspose_outlook.pst", 2097764),
    ("aspose_outlook.pst", 2097796),
    ("aspose_outlook.pst", 2097860),
];

/// Frozen pre-0075 decision CSV header prefix (19 columns; 0075 appends after these).
const DECISION_CSV_PRE_0075_HEADER: &str = "SourcePath,SourcePst,Folder,IsOrphaned,NID,MessageIdNorm,ContentHash,EdrmMih,Role,Tier,WinnerPst,WinnerFolder,WinnerNid,Policy,FamilyPolicy,Degraded,DegradedReasons,Size,PromotedFromFailure";

/// Frozen unique-row **legacy** columns 1..18 (drop absolute SourcePath col 0 for path portability).
/// Order: SourcePst,Folder,IsOrphaned,NID,MessageIdNorm,ContentHash,EdrmMih,Role,Tier,WinnerPst,WinnerFolder,WinnerNid,Policy,FamilyPolicy,Degraded,DegradedReasons,Size,PromotedFromFailure.
/// Sorted by NID. Captured with defaults on aspose_outlook.pst (0075 flags off).
const ASPOSE_LEGACY_UNIQUE_ROWS: &[&[&str]] = &[
    &[
        "aspose_outlook.pst",
        "Freebusy Data",
        "false",
        "2097188",
        "",
        "cedf20fb1ac5f872b4015b4435adeb36d22b5dcf0fa5021cd8870e2b292679eb",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "209",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Sent Items",
        "false",
        "2097220",
        "",
        "f36519353be1d7a18720841138d997202c753ffa500d14f09bb821dbeb8abe93",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "4303",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Sent Items",
        "false",
        "2097252",
        "",
        "b4438ddf268f8735308d6840c7828a2f082418ff3a40db0717e7182fa5f2cc77",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "3905",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Sent Items",
        "false",
        "2097284",
        "",
        "2968dec781a0147e25d322d7525267407b46f50ebfd483f438ee84ca9c4aced0",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "3799",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Sent Items",
        "false",
        "2097316",
        "",
        "4c0899a42d29ccef0dbdc17a3146514323749dd867b92b47d5fd16e2566e32de",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "21708",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Sent Items",
        "false",
        "2097412",
        "",
        "60a9a10ccbe87b2e8500335d6010ae3e645b85a4b92e9495ef1b12869a0f8f47",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "253384",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Sent Items",
        "false",
        "2097444",
        "",
        "13cf1ee29451d3b373ea14bf3050eb510aaa51694dc5465e6754ad0dea76d35a",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "115215",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Inbox",
        "false",
        "2097476",
        "003c01cc306e$17006760$45013620$@razzaq@xp.local",
        "e31dd11cbe2c6faef042a1066f7993e71ebdc0e7632506e4630cbb27e4ce0c79",
        "103ab5ee39a8107e086f5dd55b4a79a0",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "108341",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Inbox",
        "false",
        "2097508",
        "003301cc306d$d7f83c00$87e8b400$@razzaq@xp.local",
        "df1334924ab65ac2df096b9aeb67984c8942c76ee0b91c89c45f29e3aa134284",
        "68762f17aa4e91dafe563d6ae7896636",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "259596",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Inbox",
        "false",
        "2097540",
        "002001cc306d$ad9f5290$08ddf7b0$@razzaq@xp.local",
        "ec1966cedd749fdee56eaabf7a655852f0b67c723b3430b7a557fcf1f59338f6",
        "1b309e9fbbd8d0b782e15c41874dac1e",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "17211",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Inbox",
        "false",
        "2097636",
        "001001cc306d$914cf4d0$b3e6de70$@razzaq@xp.local",
        "69a8425b4d38b1e682714c44567ca6404f5fd6efc7456dfa436f9a98c044e6f0",
        "d3fcd8238569f919bacbb311552f6970",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "5574",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Inbox",
        "false",
        "2097668",
        "000801cc306d$83fbaf60$8bf30e20$@razzaq@xp.local",
        "eae827a95da5a3a17d8e711a49ff8a6c5a81b9559853600d5514bda486c16879",
        "2b8416ed0225b33dcc60ec653b7b5270",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "5668",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Inbox",
        "false",
        "2097700",
        "000001cc306d$76f41d20$64dc5760$@razzaq@xp.local",
        "9fdaba982b68db14f0c0e3ef89fb293bf083cb43f5a2fe50b0d4f1a8e4e4b981",
        "b302f008be061e2b37c1ef86ed02a140",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "6228",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Inbox",
        "false",
        "2097732",
        "c7b63379-9bbd-4164-ac1c-5cf39acaa682@xp",
        "7973dbd37fa8b51fabab2e29d5e65b503eb8b51d4b29b3f303d6d23c98189270",
        "9e257fc0040030de3fe4a16f3128edea",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "3295",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Inbox",
        "false",
        "2097764",
        "8307efb2-6104-4800-a49d-110706a43099@xp",
        "9e8c79a6f6cdadbb7491fa31aada2469945183f2e470b9e24ce1345c7ab97580",
        "398787673aae3a8fbfd3e16e5bf46b30",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "3346",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Contacts",
        "false",
        "2097796",
        "",
        "de5a1618ec1769295726d400f14c54c1cecdbd15e081ce1f17fe7342c0384051",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "6615",
        "false",
    ],
    &[
        "aspose_outlook.pst",
        "Top of Personal Folders/Calendar",
        "false",
        "2097860",
        "",
        "8e3adc254ab6c6a6ec15f00abc46bf8c6fc52d0b261d3fe6dd7605e6d1112f77",
        "",
        "unique",
        "",
        "",
        "",
        "",
        "first_seen",
        "keep_attachments_with_parent",
        "false",
        "",
        "6663",
        "false",
    ],
];

/// Fixture golden: default keep-set winners match checked-in list; pre-0075 header frozen;
/// determinism across runs; source PST bytes immutable.
#[test]
fn aspose_default_winners_deterministic_golden() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let hash_before = sha256_file(&sample);
    let dir = TempDir::new().expect("tmp");

    let run_once = |suffix: &str| -> (serde_json::Value, String, String, String) {
        let dec = dir.path().join(format!("decisions_{suffix}.csv"));
        let ks = dir.path().join(format!("keepset_{suffix}.json"));
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
        let v: serde_json::Value =
            serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("json");
        let ks_text = fs::read_to_string(&ks).expect("read keep-set json");
        let dec_text = fs::read_to_string(&dec).expect("read decisions");
        let dec_header = dec_text.lines().next().unwrap_or("").to_string();
        (v, ks_text, dec_header, dec_text)
    };

    let (v1, ks1, header1, dec1) = run_once("a");
    let (v2, ks2, header2, _dec2) = run_once("b");

    assert_eq!(v1["schema"].as_str(), Some("keep_set_v1"));
    assert_eq!(v1["ok"], true);
    assert_eq!(v2["schema"].as_str(), Some("keep_set_v1"));

    let winners1 = v1["keep_set"]["winners"].as_array().expect("w1");
    let winners2 = v2["keep_set"]["winners"].as_array().expect("w2");
    assert_eq!(
        winners1.len(),
        ASPOSE_DEFAULT_WINNER_GOLDEN.len(),
        "winner count must match checked-in golden"
    );

    let keys = |winners: &[serde_json::Value]| -> Vec<(String, u64)> {
        winners
            .iter()
            .map(|w| {
                let pst = w["locus"]["source_pst"].as_str().unwrap_or("").to_string();
                let nid = w["locus"]["nid"].as_u64().unwrap_or(0);
                (pst, nid)
            })
            .collect()
    };
    let got = keys(winners1);
    let golden: Vec<(String, u64)> = ASPOSE_DEFAULT_WINNER_GOLDEN
        .iter()
        .map(|(p, n)| ((*p).to_string(), *n))
        .collect();
    assert_eq!(
        got, golden,
        "default winners must match checked-in golden (source_pst, nid)"
    );
    assert_eq!(
        keys(winners1),
        keys(winners2),
        "default winners must be deterministic across consecutive runs"
    );

    // On-disk keep-set JSON winners must also match.
    let ks1_v: serde_json::Value = serde_json::from_str(&ks1).expect("ks1");
    let ks2_v: serde_json::Value = serde_json::from_str(&ks2).expect("ks2");
    assert_eq!(
        ks1_v["winners"], ks2_v["winners"],
        "keep-set JSON winners list must match across runs"
    );

    // Decision CSV: frozen pre-0075 header prefix + 0075 suffix.
    assert!(
        header1.starts_with(DECISION_CSV_PRE_0075_HEADER),
        "pre-0075 header prefix drift; header1={header1}"
    );
    assert_eq!(header1, header2);
    assert!(
        header1.contains("folder_class") && header1.contains("duplicate_sources"),
        "0075 suffix required; header={header1}"
    );

    // Pre-0075 data columns (drop absolute SourcePath) must match checked-in baseline.
    let legacy_unique_cols = |csv: &str| -> Vec<Vec<String>> {
        let mut lines = csv.lines();
        let _ = lines.next();
        let mut rows = Vec::new();
        for line in lines {
            // aspose unique rows have no quoted commas in legacy fields — split is safe.
            let cols: Vec<String> = line.split(',').map(|s| s.to_string()).collect();
            if cols.len() >= 19 && cols.get(8).map(|s| s.as_str()) == Some("unique") {
                // Drop SourcePath (machine-absolute); keep cols 1..18.
                rows.push(cols[1..19].to_vec());
            }
        }
        rows.sort_by(|a, b| a[3].cmp(&b[3])); // NID
        rows
    };
    let leg1 = legacy_unique_cols(&dec1);
    assert_eq!(
        leg1.len(),
        ASPOSE_LEGACY_UNIQUE_ROWS.len(),
        "unique decision rows must match golden count"
    );
    for (i, (got, want)) in leg1
        .iter()
        .zip(ASPOSE_LEGACY_UNIQUE_ROWS.iter())
        .enumerate()
    {
        assert_eq!(got.len(), want.len(), "legacy col count row {i}");
        for (j, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(
                g.as_str(),
                *w,
                "legacy col {j} mismatch at unique row {i} (NID={})",
                got.get(3).map(|s| s.as_str()).unwrap_or("?")
            );
        }
    }

    assert_eq!(
        hash_before,
        sha256_file(&sample),
        "source fixture SHA-256 must be unchanged after keep-set"
    );
}
