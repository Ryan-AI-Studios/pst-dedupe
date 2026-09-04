//! Track 0106: unique-eml `--max-embedded-depth` (spawn CLI; do **not** inject
//! `--no-attachments`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
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

fn run_unique_eml(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("run unique-eml")
}

fn read_summary(out: &Path) -> serde_json::Value {
    let path = out.join("summary.json");
    let body = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("summary.json missing at {}: {e}", path.display());
    });
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("summary json: {e}; body={body}"))
}

fn pack_eml_text(out: &Path) -> String {
    let vol = out.join("VOL001");
    let mut combined = String::new();
    let entries = fs::read_dir(&vol).unwrap_or_else(|e| panic!("VOL001 {}: {e}", vol.display()));
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) == Some("eml") {
            combined.push_str(&fs::read_to_string(&p).expect("read eml"));
            combined.push('\n');
        }
    }
    combined
}

fn depth_limit_in_csv(out: &Path) -> bool {
    let csv = out.join("export_attachments.csv");
    if !csv.exists() {
        return false;
    }
    let body = fs::read_to_string(&csv).expect("csv");
    body.contains("ATTACH_DEPTH_LIMIT")
}

fn depth_limit_in_summary(v: &serde_json::Value) -> u64 {
    v["attachments_failed_by_reason"]["ATTACH_DEPTH_LIMIT"]
        .as_u64()
        .or_else(|| {
            v["attachments_failed_by_reason"]
                .as_object()
                .and_then(|m| m.get("ATTACH_DEPTH_LIMIT"))
                .and_then(|x| x.as_u64())
        })
        .unwrap_or(0)
}

#[test]
fn default_depth_3_fails_fourth_nest() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src4.pst");
    let out = dir.path().join("pack");
    write_source(&src, 4);

    let result = run_unique_eml(&[
        "unique-eml",
        src.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--json",
        "--allow-partial-fidelity",
    ]);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let v = read_summary(&out);
    assert_eq!(
        v["max_embedded_depth"].as_u64(),
        Some(3),
        "default depth; stdout={stdout} stderr={stderr}"
    );
    assert!(
        depth_limit_in_summary(&v) >= 1 || depth_limit_in_csv(&out),
        "expected ATTACH_DEPTH_LIMIT at default 3; summary={v} stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("--max-embedded-depth=3"),
        "0127 unique-eml hint must name configured cap 3; stderr={stderr}"
    );
    assert_eq!(
        stderr.matches("--max-embedded-depth=3").count(),
        1,
        "0127 unique-eml hint must appear once (not twice); stderr={stderr}"
    );
    let eml = pack_eml_text(&out);
    assert!(
        eml.contains("Subject: Depth 4"),
        "3rd nest (Depth 4) must be present at default 3:\n{eml}"
    );
    assert!(
        !eml.contains("Subject: Leaf"),
        "4th nest (Leaf) must be absent at default 3:\n{eml}"
    );
}

#[test]
fn depth_4_recovers_fourth_nest() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src4.pst");
    let out = dir.path().join("pack");
    write_source(&src, 4);

    let result = run_unique_eml(&[
        "unique-eml",
        src.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--json",
        "--allow-partial-fidelity",
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
    let v = read_summary(&out);
    assert_eq!(v["max_embedded_depth"].as_u64(), Some(4));
    assert_eq!(
        depth_limit_in_summary(&v),
        0,
        "no ATTACH_DEPTH_LIMIT at 4; summary={v}"
    );
    let eml = pack_eml_text(&out);
    assert!(
        eml.contains("Subject: Leaf"),
        "4th nest (Leaf) must be present at depth 4:\n{eml}"
    );
}

#[test]
fn ceiling_8_fails_at_7_succeeds_at_8() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src8.pst");
    write_source(&src, 8);

    let out7 = dir.path().join("pack7");
    let fail = run_unique_eml(&[
        "unique-eml",
        src.to_str().expect("utf8"),
        "--out",
        out7.to_str().expect("utf8"),
        "--json",
        "--allow-partial-fidelity",
        "--max-embedded-depth",
        "7",
    ]);
    let stdout7 = String::from_utf8_lossy(&fail.stdout);
    let stderr7 = String::from_utf8_lossy(&fail.stderr);
    let v7 = read_summary(&out7);
    assert_eq!(
        v7["max_embedded_depth"].as_u64(),
        Some(7),
        "stdout={stdout7} stderr={stderr7}"
    );
    assert!(
        depth_limit_in_summary(&v7) >= 1 || depth_limit_in_csv(&out7),
        "expected ATTACH_DEPTH_LIMIT at 7; summary={v7}"
    );
    assert!(
        stderr7.contains("--max-embedded-depth=7"),
        "0127 unique-eml hint must name configured cap 7; stderr={stderr7}"
    );
    assert_eq!(
        stderr7.matches("--max-embedded-depth=7").count(),
        1,
        "0127 unique-eml hint must appear once (not twice); stderr={stderr7}"
    );
    let eml7 = pack_eml_text(&out7);
    assert!(
        !eml7.contains("Subject: Leaf"),
        "8th nest must be absent at depth 7:\n{eml7}"
    );

    let out8 = dir.path().join("pack8");
    let ok = run_unique_eml(&[
        "unique-eml",
        src.to_str().expect("utf8"),
        "--out",
        out8.to_str().expect("utf8"),
        "--json",
        "--allow-partial-fidelity",
        "--max-embedded-depth",
        "8",
    ]);
    let stdout8 = String::from_utf8_lossy(&ok.stdout);
    let stderr8 = String::from_utf8_lossy(&ok.stderr);
    assert!(
        ok.status.success(),
        "exit={:?} stderr={stderr8} stdout={stdout8}",
        ok.status.code()
    );
    let v8 = read_summary(&out8);
    assert_eq!(v8["max_embedded_depth"].as_u64(), Some(8));
    assert_eq!(depth_limit_in_summary(&v8), 0, "clean at 8; summary={v8}");
    let eml8 = pack_eml_text(&out8);
    assert!(
        eml8.contains("Subject: Leaf"),
        "8th nest must be present at depth 8:\n{eml8}"
    );
}

#[test]
fn late_manifest_err_still_emits_depth_hint() {
    let dir = TempDir::new().expect("tmp");
    let src = dir.path().join("src4.pst");
    let out = dir.path().join("pack");
    let man_block = dir.path().join("manifest_as_dir");
    write_source(&src, 4);
    fs::create_dir_all(&man_block).expect("manifest path as directory");

    let result = run_unique_eml(&[
        "unique-eml",
        src.to_str().expect("utf8"),
        "--out",
        out.to_str().expect("utf8"),
        "--manifest-json",
        man_block.to_str().expect("utf8"),
        "--json",
        "--allow-partial-fidelity",
    ]);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !result.status.success(),
        "manifest dir must fail the pack; exit={:?} stderr={stderr} stdout={stdout}",
        result.status.code()
    );
    assert!(
        stderr.contains("--max-embedded-depth=3"),
        "0127 unique-eml hint must still print on late Err; stderr={stderr} stdout={stdout}"
    );
}

#[test]
fn clap_rejects_zero_nine_and_non_integer() {
    for bad in ["0", "9", "abc"] {
        let result = run_unique_eml(&[
            "unique-eml",
            "dummy.pst",
            "--out",
            "out_pack",
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
    let help = run_unique_eml(&["unique-eml", "--help"]);
    let help_txt = format!(
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(
        help_txt.contains("identity-safe") && help_txt.contains("often need 8"),
        "0127 unique-eml clap help; {help_txt}"
    );
}
