//! Integration tests for headless matter CLI (track 0045).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use tempfile::tempdir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn run_ok(args: &[&str]) {
    let out = Command::new(bin()).args(args).output().expect("spawn");
    assert!(
        out.status.success(),
        "command failed: {:?} stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn run_status(args: &[&str]) -> std::process::Output {
    Command::new(bin()).args(args).output().expect("spawn")
}

#[test]
fn matter_create_and_info_json() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m1");
    let matter_s = matter.to_str().unwrap();

    let out = run_status(&[
        "matter", "create", "--path", matter_s, "--name", "cli-test", "--json",
    ]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("parse create json");
    assert_eq!(v["ok"], true);
    assert!(v["id"].as_str().is_some());
    assert_eq!(v["name"], "cli-test");

    let out = run_status(&["matter", "info", "--path", matter_s, "--json"]);
    assert!(out.status.success());
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["name"], "cli-test");
    assert!(v["schema_version"].as_u64().unwrap() >= 24);
}

#[test]
fn matter_create_encrypt_open_wrong_password() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("menc");
    let matter_s = matter.to_str().unwrap();

    let out = Command::new(bin())
        .args([
            "matter",
            "create",
            "--path",
            matter_s,
            "--name",
            "enc-cli",
            "--encrypt",
            "--json",
        ])
        .env("PST_DEDUPE_MATTER_PASSPHRASE", "right-pass-xyz")
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value =
        serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).expect("json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["encryption_enabled"], true);

    // Wrong passphrase → fail closed
    let out = Command::new(bin())
        .args(["matter", "info", "--path", matter_s, "--json"])
        .env("PST_DEDUPE_MATTER_PASSPHRASE", "wrong-pass")
        .output()
        .expect("spawn");
    assert!(
        !out.status.success(),
        "wrong passphrase must fail: stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );

    // Correct passphrase → info ok
    let out = Command::new(bin())
        .args(["matter", "info", "--path", matter_s, "--json"])
        .env("PST_DEDUPE_MATTER_PASSPHRASE", "right-pass-xyz")
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value =
        serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).expect("json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["encryption_enabled"], true);
}

#[test]
fn job_list_empty_json() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m2");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "jobs", "--json",
    ]);
    let out = run_status(&["job", "list", "--path", matter_s, "--json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["count"], 0);
}

#[test]
fn relative_path_in_params_exit_2() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m3");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "rel", "--json",
    ]);
    let out = run_status(&[
        "job",
        "run",
        "--path",
        matter_s,
        "--kind",
        "ingest",
        "--params-json",
        r#"{"path":"relative/pkg.zip"}"#,
        "--json",
    ]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("usage error json");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "usage");
}

#[test]
fn unknown_kind_exit_2() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m4");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "k", "--json",
    ]);
    let out = run_status(&[
        "job",
        "run",
        "--path",
        matter_s,
        "--kind",
        "not_a_real_kind",
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn profile_list_includes_builtins() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m5");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "p", "--json",
    ]);
    let out = run_status(&["profile", "list", "--path", matter_s, "--json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    let profiles = v["profiles"].as_array().unwrap();
    assert!(
        profiles
            .iter()
            .any(|p| p["id"].as_str() == Some("builtin:standard")
                || p["name"].as_str() == Some("standard")),
        "expected builtin standard in {profiles:?}"
    );
}

#[test]
fn workflow_list_includes_builtins() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m6");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "w", "--json",
    ]);
    let out = run_status(&["workflow", "list", "--path", matter_s, "--json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    let wfs = v["workflows"].as_array().unwrap();
    assert!(
        wfs.iter().any(|w| {
            w["id"]
                .as_str()
                .map(|s| s.contains("reduce_only_chain") || s.contains("builtin:"))
                .unwrap_or(false)
                || w["name"].as_str() == Some("reduce_only_chain")
        }),
        "expected builtin workflow in {wfs:?}"
    );
}

#[test]
fn profile_import_and_run_classify_only() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m7");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "imp", "--json",
    ]);

    let profile_file = tmp.path().join("prof.json");
    fs::write(
        &profile_file,
        r#"{
          "name": "cli_classify_only",
          "description": "test",
          "body": {
            "version": 1,
            "stages": {
              "classify": { "enabled": true, "params": {} }
            }
          }
        }"#,
    )
    .unwrap();

    let out = run_status(&[
        "profile",
        "import",
        "--path",
        matter_s,
        "--file",
        profile_file.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    let profile_id = v["id"].as_str().unwrap().to_string();

    let out = run_status(&[
        "profile",
        "run",
        "--path",
        matter_s,
        "--profile",
        &profile_id,
        "--json",
    ]);
    assert!(
        out.status.success(),
        "profile run failed code={:?} stdout={} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("job envelope");
    assert_eq!(v["ok"], true);
    assert_eq!(v["state"], "succeeded");
    assert_eq!(v["kind"], "profile_run");

    // Children should have parent_job_id set.
    let parent = v["job_id"].as_str().unwrap();
    let out = run_status(&[
        "job", "list", "--path", matter_s, "--parent", parent, "--json",
    ]);
    assert!(out.status.success());
    let list: Value =
        serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    let jobs = list["jobs"].as_array().unwrap();
    assert!(!jobs.is_empty(), "expected child jobs under profile_run");
    assert!(jobs.iter().all(|j| j["parent_job_id"] == parent));
}

#[test]
fn workflow_import_run_and_report_export() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m8");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "wf", "--json",
    ]);

    let wf_file = tmp.path().join("wf.json");
    fs::write(
        &wf_file,
        r#"{
          "name": "cli_wf_classify",
          "body": {
            "version": 1,
            "nodes": [
              {
                "id": "n1",
                "type": "job",
                "kind": "classify",
                "enabled": true,
                "soft_fail": false,
                "params": {}
              }
            ]
          }
        }"#,
    )
    .unwrap();

    let out = run_status(&[
        "workflow",
        "import",
        "--path",
        matter_s,
        "--file",
        wf_file.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    let wf_id = v["id"].as_str().unwrap().to_string();

    let out = run_status(&[
        "workflow",
        "run",
        "--path",
        matter_s,
        "--workflow",
        &wf_id,
        "--json",
    ]);
    assert!(
        out.status.success(),
        "workflow run failed stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["state"], "succeeded");

    let report_out = tmp.path().join("report_pack");
    let out = run_status(&[
        "report",
        "export",
        "--path",
        matter_s,
        "--out",
        report_out.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        out.status.success(),
        "report export failed stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert!(report_out.exists());
}

#[test]
fn job_cancel_pending() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m9");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "c", "--json",
    ]);

    // Durable pending job via matter-core, then cancel via CLI.
    let root = camino::Utf8PathBuf::from_path_buf(matter.clone()).expect("utf8");
    let m = matter_core::Matter::open(&root).expect("open");
    let pending = m.create_job("classify").expect("create pending");
    drop(m);

    let out = run_status(&[
        "job",
        "cancel",
        "--path",
        matter_s,
        "--job-id",
        &pending.id,
        "--json",
    ]);
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["state"], "cancelled");

    let out = run_status(&[
        "job",
        "status",
        "--path",
        matter_s,
        "--job-id",
        &pending.id,
        "--json",
    ]);
    assert!(out.status.success());
    let st: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(st["state"], "cancelled");
}

#[test]
fn ingest_sample_package() {
    let pkg = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("purview")
        .join("sample_package");
    if !pkg.exists() {
        eprintln!("skip ingest: sample_package missing");
        return;
    }
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m-ing");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "ing", "--json",
    ]);
    let out = run_status(&[
        "ingest",
        "--path",
        matter_s,
        "--source",
        pkg.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["state"], "succeeded");
    assert_eq!(v["kind"], "ingest");
    assert!(v.get("completed_count").and_then(|c| c.as_u64()).is_some());
}

#[test]
fn qc_run_empty_matter() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m-qc");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "qc", "--json",
    ]);
    let out = run_status(&["qc", "run", "--path", matter_s, "--json"]);
    let code = out.status.code();
    // Empty matter QC finishes terminal (0 success or 4 job failed — not usage/IO).
    assert!(
        matches!(code, Some(0) | Some(4)),
        "unexpected exit {:?} stdout={} stderr={}",
        code,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("qc json");
    assert!(v.get("job_id").is_some());
    assert_eq!(v["kind"], "qc");
    let state = v["state"].as_str().unwrap_or("");
    assert!(
        matches!(state, "succeeded" | "failed"),
        "unexpected state {state}"
    );
}

#[test]
fn produce_and_gap_run_terminal() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m-pg");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "pg", "--json",
    ]);

    for (cmd, kind) in [("produce", "produce"), ("gap", "gap")] {
        let out = run_status(&[cmd, "run", "--path", matter_s, "--json"]);
        let code = out.status.code();
        assert!(
            matches!(code, Some(0) | Some(4)),
            "{cmd} exit {:?} stdout={} stderr={}",
            code,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let v: Value =
            serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
        assert_eq!(v["kind"], kind);
        assert!(v.get("job_id").is_some());
    }
}

#[test]
fn job_resume_paused() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m-resume");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "resume", "--json",
    ]);

    // Create a paused job (durable) then resume via CLI.
    let root = camino::Utf8PathBuf::from_path_buf(matter.clone()).expect("utf8");
    let m = matter_core::Matter::open(&root).expect("open");
    let job = m.create_job("classify").expect("create");
    m.set_job_state(&job.id, matter_core::JobState::Running, None)
        .expect("running");
    m.set_job_state(&job.id, matter_core::JobState::Paused, Some("test pause"))
        .expect("pause");
    // Minimal checkpoint so resume path is meaningful for some handlers.
    m.put_checkpoint(&job.id, "classify", "{}", 0)
        .expect("checkpoint");
    drop(m);

    let out = run_status(&[
        "job", "resume", "--path", matter_s, "--job-id", &job.id, "--json",
    ]);
    let code = out.status.code();
    // Resume should start and reach a terminal state (success or failed).
    assert!(
        matches!(code, Some(0) | Some(4)),
        "resume exit {:?} stdout={} stderr={}",
        code,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["job_id"], job.id);
}

#[test]
fn corrupt_matter_db_job_run_exit_5() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m-corrupt");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "corrupt", "--json",
    ]);
    // Overwrite matter.db with non-SQLite bytes so open/create_job fails as matter IO.
    let db = matter.join("matter.db");
    fs::write(&db, b"not-a-sqlite-database").unwrap();
    let out = run_status(&[
        "job", "run", "--path", matter_s, "--kind", "classify", "--json",
    ]);
    assert_eq!(
        out.status.code(),
        Some(5),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], false);
}

#[test]
fn dups_json_failed_pst_single_document() {
    let tmp = tempdir().unwrap();
    let fake = tmp.path().join("broken.pst");
    fs::write(&fake, b"not-a-pst").unwrap();
    let out = run_status(&["dups", fake.to_str().unwrap(), "--json"]);
    assert_ne!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("single JSON document on dups fail");
    assert_eq!(v["ok"], false);
    assert!(v.get("summary").is_some() || v.get("error").is_some());
}

#[test]
fn missing_matter_exit_5() {
    let tmp = tempdir().unwrap();
    let missing = tmp.path().join("nope");
    let out = run_status(&[
        "matter",
        "info",
        "--path",
        missing.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn job_failed_exit_4() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m-fail");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "fail", "--json",
    ]);
    // OCR defaults to disabled → fail-closed JobOutcome::Failed.
    let out = run_status(&["job", "run", "--path", matter_s, "--kind", "ocr", "--json"]);
    assert_eq!(
        out.status.code(),
        Some(4),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(std::str::from_utf8(&out.stdout).unwrap().trim()).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["state"], "failed");
}

#[test]
fn profile_import_reserved_name_exit_2() {
    let tmp = tempdir().unwrap();
    let matter = tmp.path().join("m-res");
    let matter_s = matter.to_str().unwrap();
    run_ok(&[
        "matter", "create", "--path", matter_s, "--name", "r", "--json",
    ]);
    let profile_file = tmp.path().join("bad.json");
    fs::write(
        &profile_file,
        r#"{
          "name": "standard",
          "body": { "version": 1, "stages": { "classify": { "enabled": true, "params": {} } } }
        }"#,
    )
    .unwrap();
    let out = run_status(&[
        "profile",
        "import",
        "--path",
        matter_s,
        "--file",
        profile_file.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn scan_json_stdout_parseable() {
    // Use workspace fixture PST — stdout must be pure JSON.
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("sample.pst");
    if !fixture.exists() {
        // Fallback to aspose fixture.
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("aspose_personalstorage.pst");
        assert!(fixture.exists(), "need a fixture PST for scan --json test");
        let out = run_status(&["scan", fixture.to_str().unwrap(), "--json"]);
        assert!(
            out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        let v: Value = serde_json::from_str(stdout.trim()).expect("scan --json pure JSON");
        assert!(v.get("summary").is_some());
        return;
    }
    let out = run_status(&["scan", fixture.to_str().unwrap(), "--json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("scan --json pure JSON");
    assert!(v.get("summary").is_some());
}
