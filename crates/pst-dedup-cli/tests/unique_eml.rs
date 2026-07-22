//! Integration tests for track 0067 unique-eml CLI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn fixture_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst")
}

/// Full-file SHA-256 hex digest (source immutability).
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
fn unique_eml_fixture_schema_and_counts() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("pack");
    let dec = dir.path().join("decisions.csv");
    let ks = dir.path().join("keepset.json");

    let result = Command::new(bin())
        .args([
            "unique-eml",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--json",
            "--decision-csv",
            dec.to_str().expect("utf8"),
            "--keep-set-json",
            ks.to_str().expect("utf8"),
        ])
        .output()
        .expect("run unique-eml");
    assert!(
        result.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );

    let stdout = String::from_utf8_lossy(&result.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["eml_pack_schema"].as_str(), Some("eml_pack_v1"));
    assert_eq!(v["schema"].as_str(), Some("keep_set_v1"));

    let eml_written = v["eml_written"].as_u64().unwrap_or(0);
    let unique = v["unique"].as_u64().unwrap_or(0);
    assert!(unique > 0, "expected unique > 0");
    assert_eq!(
        eml_written, unique,
        "eml_written must equal unique on success"
    );
    assert!(v["volumes"].as_u64().unwrap_or(0) >= 1);

    // Pack layout: VOL001 under out, manifest at root.
    assert!(out.join("VOL001").is_dir(), "VOL001 must exist");
    let manifest_path = out.join("manifest.json");
    assert!(manifest_path.exists(), "manifest.json must exist");
    let man: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("man")).expect("man json");
    assert_eq!(man["schema"].as_str(), Some("eml_pack_v1"));
    assert_eq!(man["date_tz"].as_str(), Some("UTC"));
    assert_eq!(man["stats"]["eml_written"].as_u64(), Some(eml_written));
    let messages = man["messages"].as_array().expect("messages");
    assert_eq!(messages.len() as u64, eml_written);
    for m in messages {
        let rel = m["eml_relpath"].as_str().expect("relpath");
        assert!(
            rel.starts_with("VOL"),
            "relpath must be under volume: {rel}"
        );
        assert!(out.join(rel).is_file(), "eml missing: {rel}");
    }

    // At least one EML should have UTC Date when submit_time present.
    let eml_files: Vec<_> = fs::read_dir(out.join("VOL001"))
        .expect("vol")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("eml"))
        .collect();
    assert!(!eml_files.is_empty());
    let sample_eml = fs::read_to_string(&eml_files[0]).expect("read eml");
    if sample_eml.contains("Date:") {
        assert!(
            sample_eml.contains(" +0000"),
            "Date must be UTC +0000: {}",
            sample_eml
                .lines()
                .find(|l| l.starts_with("Date:"))
                .unwrap_or("")
        );
    }
}

#[test]
fn unique_eml_nonempty_out_without_overwrite_errors() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("pack");
    fs::create_dir_all(&out).expect("mkdir");
    fs::write(out.join("existing.txt"), b"hi").expect("seed");

    let result = Command::new(bin())
        .args([
            "unique-eml",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        !result.status.success(),
        "must refuse non-empty out without --overwrite"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(
        combined.to_ascii_lowercase().contains("overwrite")
            || combined.to_ascii_lowercase().contains("not empty"),
        "error should mention overwrite/not empty: {combined}"
    );
}

#[test]
fn unique_eml_source_immutability() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let before = sha256_file(&sample);
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("pack");

    let result = Command::new(bin())
        .args([
            "unique-eml",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );
    let after = sha256_file(&sample);
    assert_eq!(before, after, "source PST must be unchanged");
}

#[test]
fn unique_eml_parents_only_still_writes_eml() {
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
            "--family-policy",
            "parents_only",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&result.stdout)).expect("json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["eml_written"], v["unique"]);
    // parents_only: no attach parts expected in stats (fixture may still have 0 attaches).
    assert_eq!(v["attach_parts_written"].as_u64().unwrap_or(0), 0);
}

#[test]
fn unique_eml_manifest_order_matches_keep_set_winners() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out = dir.path().join("pack");
    let ks = dir.path().join("keepset.json");

    let result = Command::new(bin())
        .args([
            "unique-eml",
            sample.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--keep-set-json",
            ks.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        result.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&result.stderr)
    );

    let keep: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&ks).expect("ks")).expect("ks json");
    let man: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("manifest.json")).expect("man"))
            .expect("man json");

    let winners = keep["winners"].as_array().expect("winners");
    let messages = man["messages"].as_array().expect("messages");
    assert_eq!(winners.len(), messages.len());
    for (i, (w, m)) in winners.iter().zip(messages.iter()).enumerate() {
        let w_nid = w["locus"]["nid"].as_u64().expect("w nid");
        let m_nid = m["nid"].as_u64().expect("m nid");
        assert_eq!(
            w_nid, m_nid,
            "manifest message[{i}] nid must match keep_set.winners[{i}]"
        );
        let w_path = w["locus"]["source_path"].as_str().unwrap_or("");
        let m_path = m["source_path"].as_str().unwrap_or("");
        assert_eq!(w_path, m_path, "source_path order mismatch at {i}");
        // Counter in filename is 1-based index into keep_set.winners.
        let rel = m["eml_relpath"].as_str().expect("rel");
        let file = rel.rsplit('/').next().unwrap_or(rel);
        let counter = format!("{:06}", i + 1);
        assert!(
            file.starts_with(&format!("{counter}_")),
            "eml counter must be keep_set order: expected {counter}_… got {file}"
        );
    }
}

#[test]
fn unique_eml_rejects_malicious_volume_prefix() {
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
            "--volume-prefix",
            r"..\escape",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        !result.status.success(),
        "must reject path-traversal volume prefix"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(
        combined.to_ascii_lowercase().contains("volume")
            || combined.to_ascii_lowercase().contains("prefix"),
        "error should mention volume/prefix: {combined}"
    );
}

/// P1: `--out` parent of input PST with `--overwrite` must refuse (would delete source).
#[test]
fn unique_eml_overwrite_refuses_out_containing_input_pst() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    // Copy fixture into temp; --out is the parent dir (contains the PST).
    let pst_copy = dir.path().join("mail.pst");
    fs::copy(&sample, &pst_copy).expect("copy fixture");
    let before = sha256_file(&pst_copy);
    let out = dir.path().to_path_buf();

    let result = Command::new(bin())
        .args([
            "unique-eml",
            pst_copy.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--overwrite",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        !result.status.success(),
        "must refuse --out that contains input PST; stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    )
    .to_ascii_lowercase();
    assert!(
        combined.contains("out")
            || combined.contains("input")
            || combined.contains("delete")
            || combined.contains("refus"),
        "error should explain unsafe out/input: {combined}"
    );
    assert!(pst_copy.is_file(), "source PST must still exist");
    let after = sha256_file(&pst_copy);
    assert_eq!(before, after, "source PST bytes must be unchanged");
}

/// P1: `--integrity-csv` equal to input PST must refuse (would truncate source).
#[test]
fn unique_eml_integrity_csv_refuses_equal_input_pst() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let pst_copy = dir.path().join("mail.pst");
    fs::copy(&sample, &pst_copy).expect("copy fixture");
    let before = sha256_file(&pst_copy);
    let out = dir.path().join("pack");

    let result = Command::new(bin())
        .args([
            "unique-eml",
            pst_copy.to_str().expect("utf8"),
            "--out",
            out.to_str().expect("utf8"),
            "--integrity-csv",
            pst_copy.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        !result.status.success(),
        "must refuse --integrity-csv equal to input PST; stdout={} stderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    )
    .to_ascii_lowercase();
    assert!(
        combined.contains("integrity") || combined.contains("input") || combined.contains("refus"),
        "error should explain integrity/input collision: {combined}"
    );
    assert!(pst_copy.is_file(), "source PST must still exist");
    let after = sha256_file(&pst_copy);
    assert_eq!(before, after, "source PST bytes must be unchanged");
}

/// Dual identical inputs: keep-set collapses dups; eml_written == unique ≈ single-file unique.
#[test]
fn unique_eml_dual_identical_inputs_collapses_to_single_unique_count() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let a = dir.path().join("a.pst");
    let b = dir.path().join("b.pst");
    fs::copy(&sample, &a).expect("copy a");
    fs::copy(&sample, &b).expect("copy b");
    let out_single = dir.path().join("pack_single");
    let out_dual = dir.path().join("pack_dual");

    let single = Command::new(bin())
        .args([
            "unique-eml",
            a.to_str().expect("utf8"),
            "--out",
            out_single.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run single");
    assert!(
        single.status.success(),
        "single stderr={}",
        String::from_utf8_lossy(&single.stderr)
    );
    let single_v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&single.stdout)).expect("json");
    let single_unique = single_v["unique"].as_u64().expect("unique");
    assert!(single_unique > 0);

    let dual = Command::new(bin())
        .args([
            "unique-eml",
            a.to_str().expect("utf8"),
            b.to_str().expect("utf8"),
            "--out",
            out_dual.to_str().expect("utf8"),
            "--json",
        ])
        .output()
        .expect("run dual");
    assert!(
        dual.status.success(),
        "dual stderr={} stdout={}",
        String::from_utf8_lossy(&dual.stderr),
        String::from_utf8_lossy(&dual.stdout)
    );
    let dual_v: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&dual.stdout)).expect("json");
    let eml_written = dual_v["eml_written"].as_u64().expect("eml_written");
    let unique = dual_v["unique"].as_u64().expect("unique");
    assert_eq!(eml_written, unique, "eml_written must equal unique");
    // Identical copies: unique must collapse to single-file unique (not ~2×).
    assert_eq!(
        unique, single_unique,
        "dual identical inputs must collapse to single-file unique count"
    );
    assert!(
        unique < single_unique.saturating_mul(2) || single_unique == 0,
        "unique must not be double-counted"
    );
}

#[test]
fn unique_eml_deterministic_rerun() {
    let sample = fixture_sample();
    if !sample.exists() {
        eprintln!("skip: fixtures/aspose_outlook.pst missing");
        return;
    }
    let dir = TempDir::new().expect("tmp");
    let out1 = dir.path().join("pack1");
    let out2 = dir.path().join("pack2");

    for out in [&out1, &out2] {
        let result = Command::new(bin())
            .args([
                "unique-eml",
                sample.to_str().expect("utf8"),
                "--out",
                out.to_str().expect("utf8"),
                "--json",
            ])
            .output()
            .expect("run");
        assert!(
            result.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    let man1: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out1.join("manifest.json")).expect("m1"))
            .expect("j1");
    let man2: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out2.join("manifest.json")).expect("m2"))
            .expect("j2");
    assert_eq!(man1["stats"]["eml_written"], man2["stats"]["eml_written"]);
    assert_eq!(man1["stats"]["unique"], man2["stats"]["unique"]);
    let msgs1 = man1["messages"].as_array().expect("m1");
    let msgs2 = man2["messages"].as_array().expect("m2");
    assert_eq!(msgs1.len(), msgs2.len());
    for (a, b) in msgs1.iter().zip(msgs2.iter()) {
        assert_eq!(a["eml_relpath"], b["eml_relpath"]);
        assert_eq!(a["nid"], b["nid"]);
        assert_eq!(a["content_hash_hex"], b["content_hash_hex"]);
    }
}
