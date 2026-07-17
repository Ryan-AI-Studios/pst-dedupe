//! Integration tests for matter-core (tempdir matters).

use std::fs;

use camino::Utf8PathBuf;
use matter_core::{
    compute_email_logical_hash, item_role, item_status, verify_audit_chain, AuditEventInput,
    EmailLogicalInput, Error, ItemErrorInput, ItemInput, ItemUpdate, JobState, LogicalAttachment,
    Matter, DB_FILE, EXPORTS_DIR, FAMILY_KIND_EMAIL_ATTACHMENTS, INDEX_DIR, LOGICAL_HASH_VERSION,
    LOGS_DIR, SCHEMA_VERSION,
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
            source_id: Some(source.id.clone()),
            path: Some("mail/msg-1.eml".into()),
            status: item_status::PARTIAL.into(),
            size_bytes: Some(128),
            ..Default::default()
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
fn update_source_and_item_by_source_path() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-source-lookup");
    let matter = Matter::create(&root, "Lookup").expect("create");

    let source = matter
        .insert_source(r"C:\exports\pkg", "raw_dump", "importing", None)
        .expect("source");

    let updated = matter
        .update_source(&source.id, "ready", Some(r#"{"n":1}"#))
        .expect("update");
    assert_eq!(updated.status, "ready");
    assert_eq!(updated.cursor_json.as_deref(), Some(r#"{"n":1}"#));

    let item = matter
        .insert_item(ItemInput {
            source_id: Some(source.id.clone()),
            path: Some("files.zip!/a.txt".into()),
            native_sha256: Some("ab".to_string() + &"cd".repeat(31)),
            status: item_status::EXPANDED.into(),
            size_bytes: Some(3),
            ..Default::default()
        })
        .expect("item");

    let found = matter
        .item_by_source_path(&source.id, "files.zip!/a.txt")
        .expect("lookup")
        .expect("present");
    assert_eq!(found.id, item.id);
    assert!(found.native_sha256.is_some());

    let missing = matter
        .item_by_source_path(&source.id, "nope")
        .expect("lookup missing");
    assert!(missing.is_none());

    let listed = matter.list_items_for_source(&source.id).expect("list");
    assert_eq!(listed.len(), 1);
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

#[test]
fn schema_v2_on_create() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-v2");
    let matter = Matter::create(&root, "V2").expect("create");
    assert_eq!(SCHEMA_VERSION, 2);
    assert_eq!(matter.schema_version().expect("ver"), 2);
    assert_eq!(matter.info().expect("info").schema_version, 2);
}

#[test]
fn insert_update_item_normalized_fields() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-item-upd");
    let matter = Matter::create(&root, "Items").expect("create");

    let item = matter
        .insert_item(ItemInput {
            path: Some("msg.eml".into()),
            status: item_status::DISCOVERED.into(),
            subject: Some("Hello".into()),
            from_addr: Some("a@ex.com".into()),
            to_addrs_json: Some(r#"["b@ex.com"]"#.into()),
            bcc_addrs_json: Some(r#"["secret@ex.com"]"#.into()),
            file_category: Some("email".into()),
            ..Default::default()
        })
        .expect("insert");

    assert_eq!(item.role.as_deref(), Some(item_role::STANDALONE));
    assert_eq!(item.logical_hash_version, 0);
    assert_eq!(item.subject.as_deref(), Some("Hello"));
    assert_eq!(item.bcc_addrs_json.as_deref(), Some(r#"["secret@ex.com"]"#));

    let hash = compute_email_logical_hash(&EmailLogicalInput {
        message_id: Some("<x@y.com>".into()),
        subject: Some("Hello".into()),
        from: Some("a@ex.com".into()),
        to: vec!["b@ex.com".into()],
        cc: vec![],
        bcc: vec!["secret@ex.com".into()],
        sent: None,
        received: None,
        body: Some("body".into()),
        attachments: vec![],
    });

    let updated = matter
        .update_item(
            &item.id,
            ItemUpdate {
                status: Some(item_status::NORMALIZED.into()),
                logical_hash: Some(Some(hash.clone())),
                logical_hash_version: Some(LOGICAL_HASH_VERSION),
                message_id: Some(Some("x@y.com".into())),
                subject: Some(Some("Hello updated".into())),
                ..Default::default()
            },
        )
        .expect("update");

    assert_eq!(updated.status, item_status::NORMALIZED);
    assert_eq!(updated.logical_hash.as_deref(), Some(hash.as_str()));
    assert_eq!(updated.logical_hash_version, LOGICAL_HASH_VERSION);
    assert_eq!(updated.subject.as_deref(), Some("Hello updated"));
    // Unchanged fields preserved
    assert_eq!(updated.from_addr.as_deref(), Some("a@ex.com"));
    assert_eq!(
        updated.bcc_addrs_json.as_deref(),
        Some(r#"["secret@ex.com"]"#)
    );

    let by_hash = matter.items_by_logical_hash(&hash).expect("by hash");
    assert_eq!(by_hash.len(), 1);
    assert_eq!(by_hash[0].id, item.id);
}

#[test]
fn family_parent_two_attachments() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-family");
    let matter = Matter::create(&root, "Family").expect("create");

    let family = matter.insert_family("").expect("family default kind");
    assert_eq!(family.kind, FAMILY_KIND_EMAIL_ATTACHMENTS);
    assert_eq!(family.matter_id, matter.id());

    let parent = matter
        .insert_item(ItemInput {
            path: Some("mail/msg".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            subject: Some("With atts".into()),
            file_category: Some("email".into()),
            attachment_count: Some(0),
            native_sha256: Some(
                "parent_native_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            ),
            ..Default::default()
        })
        .expect("parent");

    let att1 = matter
        .insert_item(ItemInput {
            path: Some("mail/msg/a.pdf".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            file_category: Some("attachment".into()),
            native_sha256: Some(
                "att1_native_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            ),
            size_bytes: Some(100),
            ..Default::default()
        })
        .expect("att1");

    let att2 = matter
        .insert_item(ItemInput {
            path: Some("mail/msg/b.pdf".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect("att2");

    // Link att2 via set_item_family_role (bumps parent attachment_count).
    let att2 = matter
        .set_item_family_role(
            &att2.id,
            Some(&family.id),
            item_role::ATTACHMENT,
            Some(&parent.id),
        )
        .expect("link att2");

    assert_eq!(att2.family_id.as_deref(), Some(family.id.as_str()));
    assert_eq!(att2.role.as_deref(), Some(item_role::ATTACHMENT));
    assert_eq!(att2.parent_item_id.as_deref(), Some(parent.id.as_str()));

    // Also recompute after att1 (inserted with parent already set but count may be stale).
    matter
        .set_item_family_role(
            &att1.id,
            Some(&family.id),
            item_role::ATTACHMENT,
            Some(&parent.id),
        )
        .expect("relink att1");

    let members = matter.list_family_members(&family.id).expect("members");
    assert_eq!(members.len(), 3);

    let attachments = matter.list_attachments(&parent.id).expect("atts");
    assert_eq!(attachments.len(), 2);

    let parent_reload = matter.get_item(&parent.id).expect("parent");
    assert_eq!(parent_reload.attachment_count, Some(2));

    let p = matter
        .get_parent(&att1.id)
        .expect("get_parent")
        .expect("some");
    assert_eq!(p.id, parent.id);

    // Parent must exist when setting parent_item_id.
    let err = matter
        .set_item_family_role(
            &att1.id,
            Some(&family.id),
            item_role::ATTACHMENT,
            Some("nope"),
        )
        .expect_err("missing parent");
    match err {
        Error::ParentItemNotFound(id) => assert_eq!(id, "nope"),
        other => panic!("expected ParentItemNotFound, got {other:?}"),
    }

    // Audit: family.create present.
    matter.verify_audit_chain().expect("audit");
}

#[test]
fn native_vs_logical_hash_independence() {
    // Same logical fields → same logical_hash even when attachment natives differ?
    // Spec: same logical fields, different *message* native_sha256 still same logical_hash.
    // Message native is not in EmailLogicalInput at all.
    let input = EmailLogicalInput {
        message_id: Some("m@x.com".into()),
        subject: Some("Subj".into()),
        from: Some("a@x.com".into()),
        to: vec!["b@x.com".into()],
        cc: vec![],
        bcc: vec![],
        sent: Some("2021-01-01T00:00:00Z".into()),
        received: None,
        body: Some("body".into()),
        attachments: vec![LogicalAttachment {
            filename: "f.pdf".into(),
            size: 1,
            native_sha256: "same_att".into(),
        }],
    };
    let h1 = compute_email_logical_hash(&input);
    let h2 = compute_email_logical_hash(&input);
    assert_eq!(h1, h2);

    // Persist two items with different native_sha256 but same logical_hash.
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-nat-log");
    let matter = Matter::create(&root, "NatLog").expect("create");

    let a = matter
        .insert_item(ItemInput {
            native_sha256: Some(
                "native_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            ),
            logical_hash: Some(h1.clone()),
            logical_hash_version: Some(LOGICAL_HASH_VERSION),
            status: item_status::NORMALIZED.into(),
            ..Default::default()
        })
        .expect("a");
    let b = matter
        .insert_item(ItemInput {
            native_sha256: Some(
                "native_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            ),
            logical_hash: Some(h1.clone()),
            logical_hash_version: Some(LOGICAL_HASH_VERSION),
            status: item_status::NORMALIZED.into(),
            ..Default::default()
        })
        .expect("b");

    assert_ne!(a.native_sha256, b.native_sha256);
    assert_eq!(a.logical_hash, b.logical_hash);
    let group = matter.items_by_logical_hash(&h1).expect("group");
    assert_eq!(group.len(), 2);
}
