//! Integration tests for matter-core (tempdir matters).

use std::fs;

use camino::Utf8PathBuf;
use matter_core::{
    compute_email_logical_hash, item_role, item_status, verify_audit_chain, AuditEventInput,
    EmailLogicalInput, Error, FilterSpec, ItemErrorInput, ItemInput, ItemUpdate, JobState,
    LogicalAttachment, Matter, DB_FILE, EXPORTS_DIR, FAMILY_KIND_EMAIL_ATTACHMENTS, INDEX_DIR,
    LOGICAL_HASH_VERSION, LOGS_DIR, PUT_READER_BUF_SIZE, SCHEMA_VERSION, WORKSPACE_DIR,
    WORKSPACE_TEMP_DIR,
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
    assert!(root
        .join(WORKSPACE_DIR)
        .join(WORKSPACE_TEMP_DIR)
        .as_std_path()
        .is_dir());

    let reopened = Matter::open(&root).expect("open");
    assert_eq!(reopened.id(), matter.id());
    assert_eq!(reopened.schema_version().expect("ver"), SCHEMA_VERSION);
    reopened
        .verify_audit_chain()
        .expect("audit ok after create");
}

#[test]
fn workspace_temp_orphan_cleaned_on_open() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-temp-clean");
    {
        let matter = Matter::create(&root, "Temp Clean").expect("create");
        let orphan = matter.workspace_temp_dir().join("orphan-evidence.pst");
        fs::write(orphan.as_std_path(), b"leftover crash residue").expect("write orphan");
        assert!(orphan.as_std_path().is_file());
    }
    let reopened = Matter::open(&root).expect("open cleans temp");
    let orphan = reopened.workspace_temp_dir().join("orphan-evidence.pst");
    assert!(
        !orphan.as_std_path().exists(),
        "orphan under workspace/temp must be removed on Matter::open"
    );
    assert!(
        reopened.workspace_temp_dir().as_std_path().is_dir(),
        "workspace/temp directory itself remains"
    );
}

#[test]
fn open_for_read_preserves_workspace_temp() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-temp-read");
    {
        let matter = Matter::create(&root, "Temp Read").expect("create");
        let live = matter.workspace_temp_dir().join("live-materialized.pst");
        fs::write(live.as_std_path(), b"in-use by extract").expect("write");
    }
    let reader = Matter::open_for_read(&root).expect("open_for_read");
    let live = reader.workspace_temp_dir().join("live-materialized.pst");
    assert!(
        live.as_std_path().is_file(),
        "open_for_read must not delete workspace/temp contents"
    );
    // Full open still cleans.
    let _ = Matter::open(&root).expect("open");
    assert!(!live.as_std_path().exists());
}

#[test]
fn put_reader_multi_chunk_matches_put_bytes_via_matter() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-stream-cas");
    let matter = Matter::create(&root, "Stream CAS").expect("create");

    let mut data = Vec::with_capacity(PUT_READER_BUF_SIZE * 2 + 9);
    for i in 0..(PUT_READER_BUF_SIZE * 2 + 9) {
        data.push((i % 199) as u8);
    }
    let expected = matter.put_bytes(&data).expect("put_bytes");
    let mut cursor = std::io::Cursor::new(data.as_slice());
    // Same content already in CAS — put_reader must return identical digest.
    let streamed = matter.put_reader(&mut cursor).expect("put_reader");
    assert_eq!(streamed, expected);

    // Fresh stream into empty CAS via a second matter path under same store:
    // re-read and confirm bytes match.
    let got = matter.get_bytes(&streamed).expect("get");
    assert_eq!(got.as_slice(), data.as_slice());
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
fn schema_v12_on_create() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-v12");
    let matter = Matter::create(&root, "V12").expect("create");
    assert_eq!(SCHEMA_VERSION, 32);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    assert_eq!(matter.info().expect("info").schema_version, SCHEMA_VERSION);
    // Default coding catalog seeded on create.
    let defs = matter.list_code_definitions().expect("defs");
    assert_eq!(defs.len(), 6);
    // saved_searches table present (empty); keyword column exists via v10.
    let saved = matter.list_saved_searches().expect("saved");
    assert!(saved.is_empty());
    // FTS bookkeeping columns present and null for new items.
    let digest = matter.put_bytes(b"probe").expect("cas");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            text_sha256: Some(digest),
            path: Some("p.txt".into()),
            ..Default::default()
        })
        .expect("item");
    let cands = matter.list_fts_candidates(0, 10).expect("fts cands");
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].id, item.id);
    assert!(cands[0].fts_text_sha256.is_none());
}

#[test]
fn dedupe_batch_and_checkpoint_same_transaction() {
    use matter_core::{item_dedup_role, item_dedup_tier, item_role, item_status, DedupRoleUpdate};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-dedup-txn");
    let matter = Matter::create(&root, "Dedup Txn").expect("create");
    let job = matter.create_job("dedupe").expect("job");

    let a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            file_category: Some("email".into()),
            path: Some("a".into()),
            message_id: Some("mid-a@example.com".into()),
            ..Default::default()
        })
        .expect("a");

    let updates = vec![DedupRoleUpdate {
        item_id: a.id.clone(),
        dedup_role: Some(item_dedup_role::UNIQUE.into()),
        duplicate_of_item_id: None,
        dedup_tier: Some(item_dedup_tier::MESSAGE_ID.into()),
        dedup_group_id: Some(a.id.clone()),
        deduped_at: Some("2020-01-01T00:00:00Z".into()),
        dedup_job_id: Some(job.id.clone()),
        extra_json: None,
    }];
    matter
        .apply_dedup_batch_with_checkpoint(&job.id, "dedupe", &updates, r#"{"cursor_index":1}"#, 1)
        .expect("batch");

    let item = matter.get_item(&a.id).expect("get");
    assert_eq!(item.dedup_role.as_deref(), Some(item_dedup_role::UNIQUE));
    assert_eq!(
        item.dedup_tier.as_deref(),
        Some(item_dedup_tier::MESSAGE_ID)
    );
    assert_eq!(item.dedup_job_id.as_deref(), Some(job.id.as_str()));

    let cp = matter
        .get_checkpoint(&job.id, "dedupe")
        .expect("cp")
        .expect("present");
    assert_eq!(cp.completed_count, 1);
    assert!(cp.cursor_json.contains("cursor_index"));

    let counts = matter.count_by_dedup_role().expect("counts");
    assert_eq!(counts.unique, 1);

    let parents = matter.list_email_parents_for_dedupe().expect("parents");
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0].id, a.id);
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

    // insert_item with parent_item_id must bump parent attachment_count immediately.
    let parent_after_att1 = matter.get_item(&parent.id).expect("parent after att1");
    assert_eq!(
        parent_after_att1.attachment_count,
        Some(1),
        "insert path recomputes attachment_count"
    );

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
fn attachment_count_reparent_and_clear() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-reparent");
    let matter = Matter::create(&root, "Reparent").expect("create");
    let family = matter.insert_family("").expect("family");

    let parent_a = matter
        .insert_item(ItemInput {
            path: Some("a".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            attachment_count: Some(0),
            ..Default::default()
        })
        .expect("parent_a");
    let parent_b = matter
        .insert_item(ItemInput {
            path: Some("b".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            attachment_count: Some(0),
            ..Default::default()
        })
        .expect("parent_b");

    let child = matter
        .insert_item(ItemInput {
            path: Some("child".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent_a.id.clone()),
            ..Default::default()
        })
        .expect("child");

    assert_eq!(
        matter.get_item(&parent_a.id).expect("a").attachment_count,
        Some(1)
    );

    // Reparent A → B via set_item_family_role.
    matter
        .set_item_family_role(
            &child.id,
            Some(&family.id),
            item_role::ATTACHMENT,
            Some(&parent_b.id),
        )
        .expect("reparent");
    assert_eq!(
        matter.get_item(&parent_a.id).expect("a").attachment_count,
        Some(0),
        "old parent decremented"
    );
    assert_eq!(
        matter.get_item(&parent_b.id).expect("b").attachment_count,
        Some(1),
        "new parent incremented"
    );

    // Clear parent via update_item Some(None).
    matter
        .update_item(
            &child.id,
            ItemUpdate {
                parent_item_id: Some(None),
                role: Some(Some(item_role::STANDALONE.into())),
                ..Default::default()
            },
        )
        .expect("clear parent");
    assert_eq!(
        matter.get_item(&parent_b.id).expect("b").attachment_count,
        Some(0),
        "clear parent zeros old count"
    );
    let cleared = matter.get_item(&child.id).expect("child");
    assert!(cleared.parent_item_id.is_none());
}

#[test]
fn item_update_some_none_clears_subject() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-clear");
    let matter = Matter::create(&root, "Clear").expect("create");
    let item = matter
        .insert_item(ItemInput {
            subject: Some("keep me".into()),
            status: item_status::NORMALIZED.into(),
            ..Default::default()
        })
        .expect("insert");
    assert_eq!(item.subject.as_deref(), Some("keep me"));

    let cleared = matter
        .update_item(
            &item.id,
            ItemUpdate {
                subject: Some(None),
                ..Default::default()
            },
        )
        .expect("clear subject");
    assert!(cleared.subject.is_none());
}

#[test]
fn cross_matter_family_rejected() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-xmatter");
    let matter = Matter::create(&root, "Home").expect("create");

    // Inject a foreign matter + family into the same DB (multi-matter edge case).
    {
        let db_path = root.join(DB_FILE);
        let conn = rusqlite::Connection::open(db_path.as_std_path()).expect("open db");
        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_foreign', 'Foreign', '2020-01-01T00:00:00Z', 2, '/tmp/foreign')",
            [],
        )
        .expect("foreign matter");
        conn.execute(
            "INSERT INTO item_families (id, matter_id, kind, created_at) \
             VALUES ('fam_foreign', 'mat_foreign', 'email_attachments', '2020-01-01T00:00:00Z')",
            [],
        )
        .expect("foreign family");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, parent_item_id, logical_hash_version) \
             VALUES ('itm_foreign_parent', 'mat_foreign', NULL, 'fam_foreign', 'p', NULL, \
             NULL, NULL, 'extracted', NULL, NULL, NULL, '2020-01-01T00:00:00Z', \
             'parent', NULL, 0)",
            [],
        )
        .expect("foreign parent item");
    }

    let err = matter
        .insert_item(ItemInput {
            family_id: Some("fam_foreign".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect_err("cross-matter family on insert");
    match err {
        Error::CrossMatterFamily(_) => {}
        other => panic!("expected CrossMatterFamily, got {other:?}"),
    }

    let local = matter
        .insert_item(ItemInput {
            path: Some("local".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect("local item");

    let err = matter
        .set_item_family_role(&local.id, Some("fam_foreign"), item_role::ATTACHMENT, None)
        .expect_err("cross-matter family on set_role");
    match err {
        Error::CrossMatterFamily(_) => {}
        other => panic!("expected CrossMatterFamily, got {other:?}"),
    }

    let err = matter
        .set_item_family_role(
            &local.id,
            None,
            item_role::ATTACHMENT,
            Some("itm_foreign_parent"),
        )
        .expect_err("cross-matter parent on set_role");
    match err {
        Error::CrossMatterFamily(_) => {}
        other => panic!("expected CrossMatterFamily, got {other:?}"),
    }

    let err = matter
        .insert_item(ItemInput {
            parent_item_id: Some("itm_foreign_parent".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect_err("cross-matter parent on insert");
    match err {
        Error::CrossMatterFamily(_) => {}
        other => panic!("expected CrossMatterFamily, got {other:?}"),
    }
}

#[test]
fn family_cohesion_parent_child_same_family_required() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-cohesion");
    let matter = Matter::create(&root, "Cohesion").expect("create");

    let family_a = matter.insert_family("").expect("family_a");
    let family_b = matter.insert_family("").expect("family_b");

    let parent_a = matter
        .insert_item(ItemInput {
            path: Some("parent_a".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family_a.id.clone()),
            attachment_count: Some(0),
            ..Default::default()
        })
        .expect("parent in family A");

    // 1) insert_item: child family B + parent in family A → FamilyCohesion.
    let err = matter
        .insert_item(ItemInput {
            path: Some("child_insert_mismatch".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family_b.id.clone()),
            parent_item_id: Some(parent_a.id.clone()),
            ..Default::default()
        })
        .expect_err("insert cross-family parent/child");
    match err {
        Error::FamilyCohesion(msg) => {
            assert!(
                msg.contains("parent item must share family_id with child"),
                "unexpected message: {msg}"
            );
        }
        other => panic!("expected FamilyCohesion, got {other:?}"),
    }

    // 2) update_item: same mismatch after insert without parent.
    let orphan = matter
        .insert_item(ItemInput {
            path: Some("orphan".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family_b.id.clone()),
            ..Default::default()
        })
        .expect("orphan in B");
    let err = matter
        .update_item(
            &orphan.id,
            ItemUpdate {
                parent_item_id: Some(Some(parent_a.id.clone())),
                ..Default::default()
            },
        )
        .expect_err("update cross-family parent/child");
    match err {
        Error::FamilyCohesion(_) => {}
        other => panic!("expected FamilyCohesion, got {other:?}"),
    }

    // 3) set_item_family_role: same mismatch.
    let err = matter
        .set_item_family_role(
            &orphan.id,
            Some(&family_b.id),
            item_role::ATTACHMENT,
            Some(&parent_a.id),
        )
        .expect_err("set_role cross-family parent/child");
    match err {
        Error::FamilyCohesion(_) => {}
        other => panic!("expected FamilyCohesion, got {other:?}"),
    }

    // Parent link with no family on either side → reject.
    let bare_parent = matter
        .insert_item(ItemInput {
            path: Some("bare_parent".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            ..Default::default()
        })
        .expect("bare parent");
    let bare_child = matter
        .insert_item(ItemInput {
            path: Some("bare_child".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect("bare child");
    let err = matter
        .set_item_family_role(
            &bare_child.id,
            None,
            item_role::ATTACHMENT,
            Some(&bare_parent.id),
        )
        .expect_err("parent link without family");
    match err {
        Error::FamilyCohesion(_) => {}
        other => panic!("expected FamilyCohesion, got {other:?}"),
    }

    // 4) Happy path: parent + child same family (insert, set_role inherit, members).
    let child = matter
        .insert_item(ItemInput {
            path: Some("child_ok".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family_a.id.clone()),
            parent_item_id: Some(parent_a.id.clone()),
            ..Default::default()
        })
        .expect("same-family child insert");
    assert_eq!(child.family_id.as_deref(), Some(family_a.id.as_str()));
    assert_eq!(child.parent_item_id.as_deref(), Some(parent_a.id.as_str()));

    let sibling = matter
        .insert_item(ItemInput {
            path: Some("sibling".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect("sibling bare");
    // Omitting family_id inherits from parent.
    let sibling = matter
        .set_item_family_role(&sibling.id, None, item_role::ATTACHMENT, Some(&parent_a.id))
        .expect("inherit family from parent");
    assert_eq!(sibling.family_id.as_deref(), Some(family_a.id.as_str()));
    assert_eq!(
        sibling.parent_item_id.as_deref(),
        Some(parent_a.id.as_str())
    );

    let members = matter.list_family_members(&family_a.id).expect("members");
    let ids: Vec<_> = members.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&parent_a.id.as_str()));
    assert!(ids.contains(&child.id.as_str()));
    assert!(ids.contains(&sibling.id.as_str()));
    assert_eq!(
        matter
            .get_item(&parent_a.id)
            .expect("parent")
            .attachment_count,
        Some(2)
    );
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

#[test]
fn list_sources_and_item_counts() {
    use matter_core::{item_status, ItemInput, Matter};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-list");
    let matter = Matter::create(&root, "List").expect("create");
    assert!(matter.list_sources().expect("empty").is_empty());
    assert_eq!(matter.count_items().expect("count"), 0);

    let s = matter
        .insert_source(r"C:\exports\a", "folder", "importing", None)
        .expect("src");
    let sources = matter.list_sources().expect("list");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, s.id);

    matter
        .insert_item(ItemInput {
            source_id: Some(s.id.clone()),
            path: Some("mail.pst".into()),
            status: item_status::DISCOVERED.into(),
            file_category: Some("pst".into()),
            ..Default::default()
        })
        .expect("pst");
    matter
        .insert_item(ItemInput {
            source_id: Some(s.id),
            path: Some("note.txt".into()),
            status: item_status::DISCOVERED.into(),
            file_category: Some("other".into()),
            ..Default::default()
        })
        .expect("txt");

    assert_eq!(matter.count_items().expect("count"), 2);
    let psts = matter.list_items_by_file_category("pst").expect("psts");
    assert_eq!(psts.len(), 1);
    assert_eq!(psts[0].path.as_deref(), Some("mail.pst"));

    // open_for_read sees the same rows
    drop(matter);
    let reader = Matter::open_for_read(&root).expect("read");
    assert_eq!(reader.list_sources().expect("s").len(), 1);
    assert_eq!(
        reader.list_items_by_file_category("pst").expect("p").len(),
        1
    );
}

/// `clear_dedupe_fields(include_attachments=true)` must only clear attaches under
/// eligible email parents — not unrelated parented items (e.g. under standalone).
#[test]
fn clear_dedupe_fields_skips_unrelated_attachments() {
    use matter_core::{
        item_dedup_role, item_dedup_tier, item_role, item_status, DedupRoleUpdate, Matter,
    };

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-clear-att");
    let matter = Matter::create(&root, "ClearAtt").expect("create");
    let job = matter.create_job("dedupe").expect("job");

    let fam_email = matter.insert_family("").expect("fam_email");
    let fam_other = matter.insert_family("").expect("fam_other");

    let email_parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            file_category: Some("email".into()),
            family_id: Some(fam_email.id.clone()),
            path: Some("mail/p1".into()),
            message_id: Some("p1@ex.com".into()),
            ..Default::default()
        })
        .expect("email parent");
    let email_att = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            file_category: Some("attachment".into()),
            family_id: Some(fam_email.id.clone()),
            parent_item_id: Some(email_parent.id.clone()),
            path: Some("mail/p1/a.pdf".into()),
            size_bytes: Some(10),
            ..Default::default()
        })
        .expect("email att");

    // Non-email container: standalone + child — not an eligible email parent tree.
    let other_parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            file_category: Some("other".into()),
            family_id: Some(fam_other.id.clone()),
            path: Some("bag".into()),
            ..Default::default()
        })
        .expect("other parent");
    let other_att = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            file_category: Some("attachment".into()),
            family_id: Some(fam_other.id.clone()),
            parent_item_id: Some(other_parent.id.clone()),
            path: Some("bag/x.bin".into()),
            size_bytes: Some(5),
            ..Default::default()
        })
        .expect("other att");

    let now = "2020-01-01T00:00:00Z";
    let updates = vec![
        DedupRoleUpdate {
            item_id: email_parent.id.clone(),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            duplicate_of_item_id: None,
            dedup_tier: Some(item_dedup_tier::MESSAGE_ID.into()),
            dedup_group_id: Some(email_parent.id.clone()),
            deduped_at: Some(now.into()),
            dedup_job_id: Some(job.id.clone()),
            extra_json: None,
        },
        DedupRoleUpdate {
            item_id: email_att.id.clone(),
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            duplicate_of_item_id: None,
            dedup_tier: Some(item_dedup_tier::FAMILY.into()),
            dedup_group_id: Some(email_parent.id.clone()),
            deduped_at: Some(now.into()),
            dedup_job_id: Some(job.id.clone()),
            extra_json: None,
        },
        DedupRoleUpdate {
            item_id: other_att.id.clone(),
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            duplicate_of_item_id: None,
            dedup_tier: Some(item_dedup_tier::FAMILY.into()),
            dedup_group_id: Some(other_parent.id.clone()),
            deduped_at: Some(now.into()),
            dedup_job_id: Some(job.id.clone()),
            extra_json: None,
        },
    ];
    matter
        .apply_dedup_batch_with_checkpoint(&job.id, "dedupe", &updates, r#"{"n":1}"#, 1)
        .expect("seed roles");

    matter
        .clear_dedupe_fields(true)
        .expect("clear with attachments");

    let email_parent2 = matter.get_item(&email_parent.id).unwrap();
    let email_att2 = matter.get_item(&email_att.id).unwrap();
    let other_att2 = matter.get_item(&other_att.id).unwrap();

    assert!(
        email_parent2.dedup_role.is_none(),
        "eligible parent fields cleared"
    );
    assert!(
        email_att2.dedup_role.is_none(),
        "eligible parent attach fields cleared"
    );
    assert_eq!(
        other_att2.dedup_role.as_deref(),
        Some(item_dedup_role::DUPLICATE),
        "unrelated attach must retain dedupe fields"
    );
    assert_eq!(
        other_att2.dedup_tier.as_deref(),
        Some(item_dedup_tier::FAMILY)
    );
    assert_eq!(other_att2.dedup_job_id.as_deref(), Some(job.id.as_str()));
}

#[test]
fn thread_batch_and_checkpoint_same_transaction() {
    use matter_core::{item_role, item_status, item_thread_method, ThreadFieldUpdate};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-thread-txn");
    let matter = Matter::create(&root, "Thread Txn").expect("create");
    let job = matter.create_job("thread").expect("job");

    let a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            file_category: Some("email".into()),
            path: Some("a".into()),
            message_id: Some("a@ex.com".into()),
            in_reply_to: Some("b@ex.com".into()),
            references_json: Some(r#"["b@ex.com"]"#.into()),
            ..Default::default()
        })
        .expect("a");

    let updates = vec![ThreadFieldUpdate {
        item_id: a.id.clone(),
        thread_id: Some("tid-1".into()),
        thread_root_item_id: Some(a.id.clone()),
        thread_method: Some(item_thread_method::HEADERS.into()),
        threaded_at: Some("2020-01-01T00:00:00Z".into()),
        thread_job_id: Some(job.id.clone()),
    }];
    matter
        .apply_thread_batch_with_checkpoint(&job.id, "thread", &updates, r#"{"cursor_index":1}"#, 1)
        .expect("batch");

    let item = matter.get_item(&a.id).expect("get");
    assert_eq!(item.thread_id.as_deref(), Some("tid-1"));
    assert_eq!(
        item.thread_method.as_deref(),
        Some(item_thread_method::HEADERS)
    );
    assert_eq!(item.in_reply_to.as_deref(), Some("b@ex.com"));

    let cp = matter
        .get_checkpoint(&job.id, "thread")
        .expect("cp")
        .expect("present");
    assert_eq!(cp.completed_count, 1);

    let parents = matter.list_email_parents_for_thread().expect("parents");
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0].id, a.id);
    assert_eq!(parents[0].in_reply_to.as_deref(), Some("b@ex.com"));

    matter.clear_thread_fields(false).expect("clear");
    let cleared = matter.get_item(&a.id).expect("get");
    assert!(cleared.thread_id.is_none());
    assert_eq!(
        cleared.in_reply_to.as_deref(),
        Some("b@ex.com"),
        "header storage must not be cleared"
    );
}

#[test]
fn near_dup_batch_and_checkpoint_same_transaction() {
    use matter_core::{item_near_dup_role, item_role, item_status, NearDupFieldUpdate};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-neardup-txn");
    let matter = Matter::create(&root, "NearDup Txn").expect("create");
    let job = matter.create_job("neardup").expect("job");

    let digest = matter
        .put_bytes(b"hello near-dup body text enough chars")
        .expect("cas");
    let a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("doc-a.txt".into()),
            text_sha256: Some(digest),
            ..Default::default()
        })
        .expect("a");

    let updates = vec![NearDupFieldUpdate {
        item_id: a.id.clone(),
        near_dup_group_id: Some("gid-1".into()),
        near_dup_role: Some(item_near_dup_role::PIVOT.into()),
        near_dup_similarity: Some(1.0),
        near_dup_pivot_item_id: Some(a.id.clone()),
        near_dup_method: Some("minhash_shingle_v1".into()),
        near_duped_at: Some("2020-01-01T00:00:00Z".into()),
        near_dup_job_id: Some(job.id.clone()),
    }];
    matter
        .apply_near_dup_batch_with_checkpoint(
            &job.id,
            "neardup",
            &updates,
            r#"{"phase":"write","cursor_index":1}"#,
            1,
        )
        .expect("batch");

    let item = matter.get_item(&a.id).expect("get");
    assert_eq!(item.near_dup_group_id.as_deref(), Some("gid-1"));
    assert_eq!(
        item.near_dup_role.as_deref(),
        Some(item_near_dup_role::PIVOT)
    );
    assert_eq!(item.near_dup_similarity, Some(1.0));
    assert_eq!(item.near_dup_method.as_deref(), Some("minhash_shingle_v1"));

    let cp = matter
        .get_checkpoint(&job.id, "neardup")
        .expect("cp")
        .expect("present");
    assert_eq!(cp.completed_count, 1);

    let cands = matter.list_neardup_candidates(true).expect("cands");
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].id, a.id);

    matter.clear_near_dup_fields().expect("clear");
    let cleared = matter.get_item(&a.id).expect("get");
    assert!(cleared.near_dup_role.is_none());
    assert!(cleared.near_dup_group_id.is_none());
}

#[test]
fn fts_batch_checkpoint_and_candidates() {
    use matter_core::{item_role, item_status, FtsFieldUpdate};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-fts-txn");
    let matter = Matter::create(&root, "Fts Txn").expect("create");
    let job = matter.create_job("fts_index").expect("job");

    let digest = matter.put_bytes(b"keyword body alpha").expect("cas");
    let a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("doc-a.txt".into()),
            text_sha256: Some(digest.clone()),
            subject: Some("Alpha".into()),
            ..Default::default()
        })
        .expect("a");

    matter
        .apply_fts_batch_with_checkpoint(
            &job.id,
            "fts_index",
            &[FtsFieldUpdate {
                item_id: a.id.clone(),
                fts_text_sha256: Some(digest.clone()),
                fts_indexed_at: Some("2020-01-01T00:00:00Z".into()),
                fts_error: None,
            }],
            r#"{"phase":"index","cursor_index":1}"#,
            1,
        )
        .expect("batch");

    let cands = matter.list_fts_candidates(0, 100).expect("cands");
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].id, a.id);
    assert_eq!(cands[0].fts_text_sha256.as_deref(), Some(digest.as_str()));

    let attach_map = matter
        .list_attachment_names_for_parents(std::slice::from_ref(&a.id))
        .expect("atts");
    assert!(!attach_map.contains_key(&a.id) || attach_map.get(&a.id).is_some_and(|v| v.is_empty()));

    matter.clear_fts_fields().expect("clear");
    let cands2 = matter.list_fts_candidates(0, 100).expect("cands2");
    assert!(cands2[0].fts_text_sha256.is_none());

    // filtered-in-ids: empty hits → empty
    let empty = matter
        .list_items_filtered_thin_in_ids(&FilterSpec::default(), &[], 10, 0)
        .expect("empty");
    assert!(empty.is_empty());
    assert_eq!(
        matter
            .count_items_filtered_in_ids(&FilterSpec::default(), &[])
            .expect("c0"),
        0
    );
}

#[test]
fn cull_batch_and_checkpoint_same_transaction() {
    use matter_core::{item_cull_status, item_role, item_status, CullFieldUpdate};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-cull-txn");
    let matter = Matter::create(&root, "Cull Txn").expect("create");
    let job = matter.create_job("cull").expect("job");

    let a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("doc-a.txt".into()),
            size_bytes: Some(10),
            ..Default::default()
        })
        .expect("a");

    let updates = vec![CullFieldUpdate {
        item_id: a.id.clone(),
        cull_status: Some(item_cull_status::INCLUDED.into()),
        cull_reasons_json: Some("[]".into()),
        cull_preset_id: None,
        cull_preset_name: Some("unique_only".into()),
        culled_at: Some("2020-01-01T00:00:00Z".into()),
        cull_job_id: Some(job.id.clone()),
    }];
    matter
        .apply_cull_batch_with_checkpoint(
            &job.id,
            "cull",
            &updates,
            r#"{"cursor_index":1,"phase":"items"}"#,
            1,
        )
        .expect("batch");

    let item = matter.get_item(&a.id).expect("get");
    assert_eq!(
        item.cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
    assert_eq!(item.cull_preset_name.as_deref(), Some("unique_only"));
    assert_eq!(item.cull_job_id.as_deref(), Some(job.id.as_str()));

    let cp = matter
        .get_checkpoint(&job.id, "cull")
        .expect("cp")
        .expect("present");
    assert_eq!(cp.completed_count, 1);

    let cands = matter.list_cull_candidates(true).expect("cands");
    assert_eq!(cands.len(), 1);
    assert_eq!(cands[0].id, a.id);

    matter.clear_cull_fields(true).expect("clear");
    let cleared = matter.get_item(&a.id).expect("get");
    assert!(cleared.cull_status.is_none());
    assert!(cleared.cull_reasons_json.is_none());
}

#[test]
fn clear_cull_fields_respects_attachment_eligibility() {
    use matter_core::{item_cull_status, item_role, item_status, CullFieldUpdate};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-cull-clear-eligible");
    let matter = Matter::create(&root, "Cull Clear Eligible").expect("create");
    let job = matter.create_job("cull").expect("job");

    let standalone = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("doc.txt".into()),
            size_bytes: Some(10),
            ..Default::default()
        })
        .expect("standalone");
    // Attachment-role row without parent link (eligibility is role-based only).
    let attach = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            path: Some("a.bin".into()),
            size_bytes: Some(1),
            ..Default::default()
        })
        .expect("attach");

    assert_eq!(
        matter.get_item(&attach.id).unwrap().role.as_deref(),
        Some(item_role::ATTACHMENT)
    );

    let stamp = |id: &str| CullFieldUpdate {
        item_id: id.into(),
        cull_status: Some(item_cull_status::INCLUDED.into()),
        cull_reasons_json: Some("[]".into()),
        cull_preset_id: None,
        cull_preset_name: Some("unique_only".into()),
        culled_at: Some("2020-01-01T00:00:00Z".into()),
        cull_job_id: Some(job.id.clone()),
    };
    matter
        .apply_cull_batch_with_checkpoint(
            &job.id,
            "cull",
            &[stamp(&standalone.id), stamp(&attach.id)],
            r#"{"cursor_index":2}"#,
            2,
        )
        .expect("stamp");

    // process_attachments=false → clear only non-attachments.
    let n = matter.clear_cull_fields(false).expect("clear");
    assert_eq!(n, 1, "should clear standalone only");
    assert!(matter
        .get_item(&standalone.id)
        .unwrap()
        .cull_status
        .is_none());
    assert_eq!(
        matter.get_item(&attach.id).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED),
        "attachment cull fields must survive when process_attachments=false"
    );

    // process_attachments=true → eligible set includes attachments (SQLite
    // UPDATE rowcount may include already-null standalone rows).
    let n2 = matter.clear_cull_fields(true).expect("clear all eligible");
    assert!(n2 >= 1, "should touch attachment row, got {n2}");
    assert!(matter.get_item(&attach.id).unwrap().cull_status.is_none());
    assert!(matter
        .get_item(&standalone.id)
        .unwrap()
        .cull_status
        .is_none());
}

#[test]
fn cull_preset_crud() {
    use matter_core::CullPresetInput;

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-cull-preset");
    let matter = Matter::create(&root, "Cull Preset").expect("create");

    let created = matter
        .upsert_cull_preset(CullPresetInput {
            id: None,
            name: "my_window".into(),
            description: Some("test".into()),
            rules_json: r#"{"version":1,"exclude_exact_duplicates":true}"#.into(),
            created_by: Some("tester".into()),
        })
        .expect("insert");
    assert_eq!(created.name, "my_window");
    assert!(!created.id.is_empty());

    let listed = matter.list_cull_presets().expect("list");
    assert_eq!(listed.len(), 1);

    let got = matter.get_cull_preset(&created.id).expect("get");
    assert_eq!(got.rules_json, created.rules_json);

    let updated = matter
        .upsert_cull_preset(CullPresetInput {
            id: Some(created.id.clone()),
            name: "my_window".into(),
            description: Some("updated".into()),
            rules_json: r#"{"version":1,"exclude_exact_duplicates":false}"#.into(),
            created_by: None,
        })
        .expect("update");
    assert_eq!(updated.description.as_deref(), Some("updated"));
    assert!(updated.rules_json.contains("false"));

    matter.delete_cull_preset(&created.id).expect("delete");
    assert!(matter.list_cull_presets().expect("list").is_empty());
    // Item cull fields are independent — delete does not require items.
}

#[test]
fn promote_batch_and_checkpoint_same_transaction() {
    use matter_core::{item_role, item_status, PromoteFieldUpdate, DEFAULT_REVIEW_SET_NAME};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-promote-txn");
    let matter = Matter::create(&root, "Promote Txn").expect("create");
    let job = matter.create_job("promote").expect("job");

    let set = matter
        .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
        .expect("set");
    assert!(set.is_default);
    assert_eq!(set.name, DEFAULT_REVIEW_SET_NAME);

    // Idempotent ensure.
    let set2 = matter
        .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
        .expect("set2");
    assert_eq!(set.id, set2.id);

    let a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("doc-a.txt".into()),
            size_bytes: Some(10),
            ..Default::default()
        })
        .expect("a");

    let updates = vec![PromoteFieldUpdate {
        item_id: a.id.clone(),
        in_review: Some(1),
        review_set_id: Some(set.id.clone()),
        review_order: Some(1),
        promoted_at: Some("2020-01-01T00:00:00Z".into()),
        promote_job_id: Some(job.id.clone()),
        promote_policy: Some("unique_only".into()),
    }];
    matter
        .apply_promote_batch_with_checkpoint(
            &job.id,
            "promote",
            &updates,
            r#"{"cursor_index":1}"#,
            1,
        )
        .expect("batch");

    let item = matter.get_item(&a.id).expect("get");
    assert_eq!(item.in_review, Some(1));
    assert_eq!(item.review_set_id.as_deref(), Some(set.id.as_str()));
    assert_eq!(item.review_order, Some(1));
    assert_eq!(item.promote_policy.as_deref(), Some("unique_only"));

    let cp = matter
        .get_checkpoint(&job.id, "promote")
        .expect("cp")
        .expect("present");
    assert_eq!(cp.completed_count, 1);

    matter
        .update_review_set_snapshot(&set.id, "unique_only", Some("{}"), 1)
        .expect("snap");
    let set3 = matter.get_review_set(&set.id).expect("get set");
    assert_eq!(set3.item_count, 1);
    assert_eq!(set3.policy.as_deref(), Some("unique_only"));

    matter
        .clear_review_membership_for_set(&set.id)
        .expect("clear");
    let cleared = matter.get_item(&a.id).expect("get");
    assert_eq!(cleared.in_review, Some(0));
    assert!(cleared.review_set_id.is_none());
    assert!(cleared.review_order.is_none());
}

#[test]
fn review_sets_partial_unique_rejects_double_default() {
    use matter_core::{DB_FILE, DEFAULT_REVIEW_SET_NAME};
    use rusqlite::Connection;

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-review-unique");
    let matter = Matter::create(&root, "Review Unique").expect("create");
    let matter_id = matter.info().unwrap().id;

    let a = matter
        .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
        .expect("default");
    assert!(a.is_default);
    drop(matter);

    // Raw insert of a second default must fail at the DB layer.
    let db = root.join(DB_FILE);
    let conn = Connection::open(db.as_std_path()).expect("open db");
    let err = conn
        .execute(
            "INSERT INTO review_sets (id, matter_id, name, is_default, policy, policy_json, \
             item_count, created_at, updated_at, created_by) \
             VALUES ('rset_evil', ?1, 'Evil', 1, NULL, NULL, 0, \
             '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
            rusqlite::params![matter_id],
        )
        .expect_err("second default");
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("unique"),
        "expected unique violation, got {msg}"
    );
}

#[test]
fn list_review_thin_respects_in_review_and_order() {
    use matter_core::{item_role, item_status, PromoteFieldUpdate, DEFAULT_REVIEW_SET_NAME};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-review-list");
    let matter = Matter::create(&root, "Review List").expect("create");
    let set = matter
        .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
        .expect("set");

    let not_review = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Skip me".into()),
            path: Some("skip.txt".into()),
            ..Default::default()
        })
        .expect("skip");

    let first = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Second by order".into()),
            path: Some("b.txt".into()),
            text_sha256: None,
            ..Default::default()
        })
        .expect("first insert");
    let second = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("First by order".into()),
            path: Some("a.txt".into()),
            text_sha256: None,
            ..Default::default()
        })
        .expect("second insert");

    // Promote out of insert order: second gets order 1, first gets order 2.
    let job = matter.create_job("promote").expect("job");
    matter
        .apply_promote_batch_with_checkpoint(
            &job.id,
            "promote",
            &[
                PromoteFieldUpdate {
                    item_id: second.id.clone(),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(1),
                    promoted_at: Some("2020-01-01T00:00:00Z".into()),
                    promote_job_id: Some(job.id.clone()),
                    promote_policy: Some("unique_only".into()),
                },
                PromoteFieldUpdate {
                    item_id: first.id.clone(),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(2),
                    promoted_at: Some("2020-01-01T00:00:00Z".into()),
                    promote_job_id: Some(job.id.clone()),
                    promote_policy: Some("unique_only".into()),
                },
            ],
            r#"{"cursor_index":2}"#,
            2,
        )
        .expect("promote");

    assert_eq!(matter.count_in_review(None).expect("count"), 2);
    assert_eq!(matter.count_in_review(Some(&set.id)).expect("count set"), 2);

    let rows = matter.list_review_thin(None, 100, 0).expect("list");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, second.id);
    assert_eq!(rows[0].review_order, Some(1));
    assert_eq!(rows[0].subject.as_deref(), Some("First by order"));
    assert_eq!(rows[1].id, first.id);
    assert_eq!(rows[1].review_order, Some(2));
    // Missing text_sha256 still lists fine.
    assert!(rows[0].text_sha256.is_none());
    assert!(rows[1].text_sha256.is_none());
    // Non-review item absent.
    assert!(!rows.iter().any(|r| r.id == not_review.id));

    // Default set id helper.
    assert_eq!(
        matter.get_default_review_set_id().expect("id").as_deref(),
        Some(set.id.as_str())
    );

    // Paging.
    let page = matter.list_review_thin(None, 1, 1).expect("page");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, first.id);
}

#[test]
fn list_review_thin_family_parent_before_child() {
    use matter_core::{item_role, item_status, PromoteFieldUpdate, DEFAULT_REVIEW_SET_NAME};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-review-family");
    let matter = Matter::create(&root, "Review Family").expect("create");
    let set = matter
        .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
        .expect("set");
    let family = matter.insert_family("").expect("family");

    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            subject: Some("Parent mail".into()),
            path: Some("mail.eml".into()),
            attachment_count: Some(1),
            ..Default::default()
        })
        .expect("parent");
    let child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("Attach.pdf".into()),
            path: Some("mail.eml/Attach.pdf".into()),
            ..Default::default()
        })
        .expect("child");

    // Promote with parent before child in review_order (promote contract).
    let job = matter.create_job("promote").expect("job");
    matter
        .apply_promote_batch_with_checkpoint(
            &job.id,
            "promote",
            &[
                PromoteFieldUpdate {
                    item_id: parent.id.clone(),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(1),
                    promoted_at: Some("2020-01-01T00:00:00Z".into()),
                    promote_job_id: Some(job.id.clone()),
                    promote_policy: Some("unique_plus_family".into()),
                },
                PromoteFieldUpdate {
                    item_id: child.id.clone(),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(2),
                    promoted_at: Some("2020-01-01T00:00:00Z".into()),
                    promote_job_id: Some(job.id.clone()),
                    promote_policy: Some("unique_plus_family".into()),
                },
            ],
            r#"{"cursor_index":2}"#,
            2,
        )
        .expect("promote");

    let rows = matter.list_review_thin(None, 100, 0).expect("list");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, parent.id);
    assert!(rows[0].parent_item_id.is_none());
    assert_eq!(rows[0].family_id.as_deref(), Some(family.id.as_str()));
    assert_eq!(rows[1].id, child.id);
    assert_eq!(rows[1].parent_item_id.as_deref(), Some(parent.id.as_str()));
    assert_eq!(rows[1].family_id.as_deref(), Some(family.id.as_str()));
}

/// Keyset `list_classify_candidates`: force vs candidate predicate + resume cursor.
#[test]
fn list_classify_candidates_keyset_force_and_resume() {
    use matter_core::{item_status, ApplyClassificationInput, CategoryApplyResult, ItemInput};

    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-classify-list");
    let matter = Matter::create(&root, "ClassifyList").expect("create");

    // Decisive taxonomy_v1 pdf — non-force must omit.
    let good = matter
        .insert_item(ItemInput {
            path: Some("good.pdf".into()),
            status: item_status::EXTRACTED.into(),
            file_category: Some("pdf".into()),
            ..Default::default()
        })
        .expect("good");
    let applied = matter
        .apply_classification(ApplyClassificationInput {
            item_id: good.id.clone(),
            force: true,
            category: "pdf".into(),
            method: "extension".into(),
            taxonomy: "taxonomy_v1".into(),
            mime_type: None,
            status: Some("ok".into()),
            error: None,
        })
        .expect("apply good");
    assert!(matches!(applied, CategoryApplyResult::Applied { .. }));

    // Legacy attachment — non-force must include.
    let legacy = matter
        .insert_item(ItemInput {
            path: Some("legacy.bin".into()),
            status: item_status::EXTRACTED.into(),
            file_category: Some("attachment".into()),
            ..Default::default()
        })
        .expect("legacy");

    // NULL category — non-force must include.
    let null_cat = matter
        .insert_item(ItemInput {
            path: Some("unknown.dat".into()),
            status: item_status::EXTRACTED.into(),
            file_category: None,
            ..Default::default()
        })
        .expect("null");

    // Wrong taxonomy on decisive category — non-force must include.
    let wrong_tax = matter
        .insert_item(ItemInput {
            path: Some("old.docx".into()),
            status: item_status::EXTRACTED.into(),
            file_category: Some("document".into()),
            ..Default::default()
        })
        .expect("wrong tax item");
    // Direct SQL: set legacy taxonomy without going through apply (which would set v1).
    matter
        .connection()
        .execute(
            "UPDATE items SET category_taxonomy = 'taxonomy_v0', category_method = 'extension' \
             WHERE id = ?1",
            rusqlite::params![wrong_tax.id],
        )
        .expect("set wrong tax");

    let non_force = matter
        .list_classify_candidates(None, 100, false, false)
        .expect("non-force list");
    let nf_ids: Vec<&str> = non_force.iter().map(|c| c.id.as_str()).collect();
    assert!(
        !nf_ids.contains(&good.id.as_str()),
        "taxonomy_v1 decisive not listed when force=false"
    );
    assert!(nf_ids.contains(&legacy.id.as_str()));
    assert!(nf_ids.contains(&null_cat.id.as_str()));
    assert!(nf_ids.contains(&wrong_tax.id.as_str()));
    // Stable id order.
    let mut sorted = nf_ids.clone();
    sorted.sort();
    assert_eq!(nf_ids, sorted);

    let forced = matter
        .list_classify_candidates(None, 100, true, false)
        .expect("force list");
    let f_ids: Vec<&str> = forced.iter().map(|c| c.id.as_str()).collect();
    assert!(
        f_ids.contains(&good.id.as_str()),
        "force must list already taxonomy_v1 rows"
    );
    assert_eq!(forced.len(), 4);

    // Resume: after first non-force id, do not re-list earlier ids.
    let first = non_force[0].id.clone();
    let after = matter
        .list_classify_candidates(Some(&first), 100, false, false)
        .expect("after keyset");
    assert!(after.iter().all(|c| c.id > first));
    assert!(!after.iter().any(|c| c.id == first));
    assert_eq!(after.len(), non_force.len() - 1);
}
