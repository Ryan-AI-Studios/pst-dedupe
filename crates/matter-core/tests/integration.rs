//! Integration tests for matter-core (tempdir matters).

use std::fs;

use camino::Utf8PathBuf;
use matter_core::{
    verify_audit_chain, AuditEventInput, Error, ItemErrorInput, ItemInput, JobState, Matter,
    DB_FILE, EXPORTS_DIR, INDEX_DIR, LOGS_DIR, SCHEMA_VERSION,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

#[test]
fn create_matter_opens_db_and_layout_exists() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-a");

    let matter = Matter::create(&root, "Test Matter").expect("create");
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    assert_eq!(matter.info().expect("info").name, "Test Matter");
    assert_eq!(
        matter.info().expect("info").schema_version,
        SCHEMA_VERSION,
        "denormalized matters.schema_version matches SCHEMA_VERSION on create"
    );

    assert!(root.join(DB_FILE).as_std_path().is_file());
    assert!(root.join("blobs").join("sha256").as_std_path().is_dir());
    assert!(root.join(INDEX_DIR).as_std_path().is_dir());
    assert!(root.join(EXPORTS_DIR).as_std_path().is_dir());
    assert!(root.join(LOGS_DIR).as_std_path().is_dir());

    let reopened = Matter::open(&root).expect("open");
    assert_eq!(reopened.id(), matter.id());
    assert_eq!(reopened.schema_version().expect("ver"), SCHEMA_VERSION);
    reopened
        .verify_audit_chain()
        .expect("audit ok after create");
}

/// Drift in denormalized `matters.schema_version` is repaired on open/migrate.
#[test]
fn migrate_resyncs_matters_schema_version() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-schema-sync");

    {
        let matter = Matter::create(&root, "Schema Sync").expect("create");
        assert_eq!(matter.info().expect("info").schema_version, SCHEMA_VERSION);
        // Force denormalized column out of sync while schema_meta stays current.
        matter
            .connection()
            .execute("UPDATE matters SET schema_version = 0", [])
            .expect("drift matters.schema_version");
        assert_eq!(
            matter.info().expect("info").schema_version,
            0,
            "precondition: denormalized column drifted"
        );
        assert_eq!(
            matter.schema_version().expect("meta"),
            SCHEMA_VERSION,
            "precondition: schema_meta still current"
        );
    }

    // open() re-runs migrate(), which must re-align matters.schema_version.
    let reopened = Matter::open(&root).expect("open after drift");
    assert_eq!(reopened.schema_version().expect("meta"), SCHEMA_VERSION);
    assert_eq!(
        reopened.info().expect("info").schema_version,
        SCHEMA_VERSION,
        "migrate must re-sync denormalized matters.schema_version"
    );
}

#[test]
fn cas_put_get_round_trip_and_reject_clobber() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-cas");
    let matter = Matter::create(&root, "CAS").expect("create");

    let data = b"raw physical evidence bytes";
    let digest = matter.put_bytes(data).expect("put");
    assert_eq!(digest.len(), 64);
    assert!(matter.blob_exists(&digest).expect("exists"));

    let got = matter.get_bytes(&digest).expect("get");
    assert_eq!(got.as_slice(), data);

    // Same bytes again is fine (idempotent).
    let digest2 = matter.put_bytes(data).expect("put same");
    assert_eq!(digest2, digest);

    // Different bytes that would collide is impossible for real SHA-256, so
    // simulate collision by writing a different file at the object path and
    // then putting the original digest's *other* content via direct CAS path
    // rewrite: put different content under a hand-crafted collision is hard.
    // Instead: place wrong content at path, then put_bytes of the real digest's
    // expected content path — we force collision by writing garbage to the
    // object path of a *new* payload after computing its digest... easier path:
    // write garbage where digest-of-A lives, then put A again.
    let path = matter.cas().object_path(&digest).expect("obj path");
    fs::write(path.as_std_path(), b"TAMPERED DIFFERENT BYTES").expect("tamper");

    let err = matter.put_bytes(data).expect_err("must reject clobber");
    match err {
        Error::CasCollision { digest: d } => assert_eq!(d, digest),
        other => panic!("expected CasCollision, got {other:?}"),
    }
}

#[test]
fn job_checkpoint_write_read_resume() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-jobs");
    let matter = Matter::create(&root, "Jobs").expect("create");

    let job = matter.create_job("ingest").expect("create job");
    assert_eq!(job.state, JobState::Pending);

    matter
        .set_job_state(&job.id, JobState::Running, None)
        .expect("running");

    let cursor = r#"{"zip_entry":42,"file":"mail.pst"}"#;
    matter
        .put_checkpoint(&job.id, "expand", cursor, 42)
        .expect("checkpoint");

    // Simulate crash: drop and reopen.
    drop(matter);
    let matter = Matter::open(&root).expect("reopen");

    let cp = matter
        .get_checkpoint(&job.id, "expand")
        .expect("get cp")
        .expect("present");
    assert_eq!(cp.cursor_json, cursor);
    assert_eq!(cp.completed_count, 42);
    assert_eq!(cp.stage, "expand");

    // Resume: advance checkpoint and finish.
    matter
        .put_checkpoint(&job.id, "expand", r#"{"zip_entry":100}"#, 100)
        .expect("advance");
    let job = matter
        .set_job_state(&job.id, JobState::Succeeded, None)
        .expect("done");
    assert_eq!(job.state, JobState::Succeeded);

    let cp2 = matter
        .get_checkpoint(&job.id, "expand")
        .expect("get")
        .expect("present");
    assert_eq!(cp2.completed_count, 100);
}

#[test]
fn item_error_does_not_delete_parent_item() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-err");
    let matter = Matter::create(&root, "Errors").expect("create");

    let source = matter
        .insert_source(r"C:\exports\pkg.zip", "purview_package", "importing", None)
        .expect("source");
    let job = matter.create_job("process").expect("job");
    let item = matter
        .insert_item(ItemInput {
            id: None,
            source_id: Some(source.id.clone()),
            family_id: None,
            path: Some("mail/msg-1.eml".into()),
            native_sha256: None,
            logical_hash: None,
            message_id: None,
            status: "partial".into(),
            size_bytes: Some(128),
            created_at: None,
            modified_at: None,
        })
        .expect("item");

    let err = matter
        .record_item_error(ItemErrorInput {
            item_id: Some(item.id.clone()),
            source_id: Some(source.id.clone()),
            job_id: Some(job.id.clone()),
            stage: "extract".into(),
            code: "PARSE_FAIL".into(),
            message: "truncated body".into(),
            detail: Some(r#"{"offset":99}"#.into()),
        })
        .expect("record error");

    assert_eq!(err.code, "PARSE_FAIL");

    // Parent item still present.
    let still = matter.get_item(&item.id).expect("item still exists");
    assert_eq!(still.status, "partial");
    assert_eq!(still.id, item.id);

    let by_item = matter.item_errors_for_item(&item.id).expect("by item");
    assert_eq!(by_item.len(), 1);
    let by_source = matter
        .item_errors_for_source(&source.id)
        .expect("by source");
    assert_eq!(by_source.len(), 1);
    let by_job = matter.item_errors_for_job(&job.id).expect("by job");
    assert_eq!(by_job.len(), 1);
}

#[test]
fn audit_append_verify_and_detect_broken_chain() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-audit");
    let matter = Matter::create(&root, "Audit").expect("create");

    // create already wrote matter.create
    let e2 = matter
        .append_audit(AuditEventInput {
            actor: "tester".into(),
            action: "source.add".into(),
            entity: "source:s1".into(),
            params_json: r#"{"path":"a.pst"}"#.into(),
            tool_version: "0.1.0".into(),
        })
        .expect("append");
    assert_eq!(e2.seq, 2);
    assert_ne!(e2.prev_hash, e2.entry_hash);

    matter
        .append_audit(AuditEventInput {
            actor: "tester".into(),
            action: "job.start".into(),
            entity: "job:j1".into(),
            params_json: "{}".into(),
            tool_version: "0.1.0".into(),
        })
        .expect("append 3");

    matter.verify_audit_chain().expect("valid chain");
    verify_audit_chain(matter.connection()).expect("free fn");

    // Tamper with prev_hash via raw SQL (no public mutators for audit history).
    matter
        .connection()
        .execute(
            "UPDATE audit_events SET prev_hash = ?1 WHERE seq = ?2",
            rusqlite::params![
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                2i64
            ],
        )
        .expect("corrupt");

    let broken = matter.verify_audit_chain().expect_err("must fail");
    match broken {
        Error::AuditChainBroken { seq, .. } => assert_eq!(seq, 2),
        other => panic!("expected AuditChainBroken, got {other:?}"),
    }
}
