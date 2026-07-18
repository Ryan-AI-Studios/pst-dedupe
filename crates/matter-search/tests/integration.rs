//! Integration tests for matter-search (tempdir matters + Tantivy).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_core::{
    item_role, item_status, FilterCondition, FilterSpec, ItemInput, Matter, SCOPE_ENTIRE_MATTER,
};
use matter_search::{
    compose_keyword_filter, delete_then_add, remove_index_dir, run_fts_index, search_keyword,
    FtsIndexParams, FtsOutcome, KeywordQuery, MatterIndex, FTS_STAGE,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

fn insert_text_item(matter: &Matter, path: &str, subject: &str, body: &[u8]) -> String {
    let digest = matter.put_bytes(body).expect("cas");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some(path.into()),
            subject: Some(subject.into()),
            text_sha256: Some(digest),
            ..Default::default()
        })
        .expect("item");
    item.id
}

fn run_index(matter: &Matter, reset: bool) -> FtsOutcome {
    let job = matter.create_job("fts_index").expect("job");
    let params = FtsIndexParams {
        reset,
        batch_size: 50,
        ..Default::default()
    };
    run_fts_index(matter, &job.id, &params, None, |_| {}).expect("run")
}

#[test]
fn index_two_items_query_matches_one() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-two");
    let matter = Matter::create(&root, "Two").expect("create");
    let id_alpha = insert_text_item(&matter, "a.txt", "A", b"uniquealpha word here");
    let _id_beta = insert_text_item(&matter, "b.txt", "B", b"uniquebeta other body");

    let outcome = run_index(&matter, true);
    assert!(matches!(outcome, FtsOutcome::Succeeded(_)));

    let hits = search_keyword(
        &root,
        &KeywordQuery {
            query: "uniquealpha".into(),
            limit: 20,
            offset: 0,
        },
    )
    .expect("search");
    assert_eq!(hits.item_ids, vec![id_alpha]);
}

#[test]
fn boolean_and_or() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-bool");
    let matter = Matter::create(&root, "Bool").expect("create");
    let id_both = insert_text_item(&matter, "both.txt", "Both", b"red blue together");
    let id_red = insert_text_item(&matter, "red.txt", "Red", b"red only here");
    let _id_blue = insert_text_item(&matter, "blue.txt", "Blue", b"blue only here");

    run_index(&matter, true);

    let and_hits = search_keyword(
        &root,
        &KeywordQuery {
            query: "red AND blue".into(),
            limit: 20,
            offset: 0,
        },
    )
    .expect("and");
    assert_eq!(and_hits.item_ids, vec![id_both.clone()]);

    let or_hits = search_keyword(
        &root,
        &KeywordQuery {
            query: "red OR blue".into(),
            limit: 20,
            offset: 0,
        },
    )
    .expect("or");
    let set: HashSet<_> = or_hits.item_ids.into_iter().collect();
    assert!(set.contains(&id_both));
    assert!(set.contains(&id_red));
    assert_eq!(set.len(), 3);
}

#[test]
fn phrase_query() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-phrase");
    let matter = Matter::create(&root, "Phrase").expect("create");
    let id_phrase = insert_text_item(&matter, "p.txt", "P", b"the quick brown fox jumps");
    let _id_other = insert_text_item(&matter, "o.txt", "O", b"the brown quick fox elsewhere");

    run_index(&matter, true);

    let hits = search_keyword(
        &root,
        &KeywordQuery {
            query: r#""quick brown""#.into(),
            limit: 20,
            offset: 0,
        },
    )
    .expect("phrase");
    assert_eq!(hits.item_ids, vec![id_phrase]);
}

#[test]
fn incremental_reindex_after_text_change() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-incr");
    let matter = Matter::create(&root, "Incr").expect("create");
    let digest1 = matter.put_bytes(b"originalword body").expect("cas1");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("x.txt".into()),
            subject: Some("X".into()),
            text_sha256: Some(digest1),
            ..Default::default()
        })
        .expect("item");

    run_index(&matter, true);
    let hits1 = search_keyword(
        &root,
        &KeywordQuery {
            query: "originalword".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("h1");
    assert_eq!(hits1.item_ids, vec![item.id.clone()]);

    let digest2 = matter.put_bytes(b"updatedword body").expect("cas2");
    matter
        .update_item(
            &item.id,
            matter_core::ItemUpdate {
                text_sha256: Some(Some(digest2)),
                ..Default::default()
            },
        )
        .expect("update");

    // Incremental (reset:false)
    let job = matter.create_job("fts_index").expect("job2");
    let outcome = run_fts_index(
        &matter,
        &job.id,
        &FtsIndexParams {
            reset: false,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("incr");
    assert!(matches!(outcome, FtsOutcome::Succeeded(_)));

    let old = search_keyword(
        &root,
        &KeywordQuery {
            query: "originalword".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("old");
    assert!(old.item_ids.is_empty());

    let new = search_keyword(
        &root,
        &KeywordQuery {
            query: "updatedword".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("new");
    assert_eq!(new.item_ids, vec![item.id.clone()]);
    // Still one hit per id
    assert_eq!(new.item_ids.len(), 1);
}

#[test]
fn delete_before_add_no_duplicate_ids() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-dedup");
    let matter = Matter::create(&root, "Dedup").expect("create");
    let id = insert_text_item(&matter, "d.txt", "D", b"dupword content");

    let index = MatterIndex::open_or_create(&root).expect("open");
    let fts = index.fts_schema().clone();
    let mut writer = index.writer(20_000_000).expect("w");
    // Index twice without SQLite mark — delete-before-add should keep one doc.
    delete_then_add(&mut writer, &fts, &id, "D", "dupword content", "d.txt", "").unwrap();
    delete_then_add(&mut writer, &fts, &id, "D", "dupword content", "d.txt", "").unwrap();
    writer.commit().unwrap();
    drop(writer);
    index.shutdown();

    let hits = search_keyword(
        &root,
        &KeywordQuery {
            query: "dupword".into(),
            limit: 20,
            offset: 0,
        },
    )
    .expect("search");
    assert_eq!(hits.item_ids, vec![id]);
}

#[test]
fn reset_rebuild_after_handle_drop() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-reset");
    let matter = Matter::create(&root, "Reset").expect("create");
    insert_text_item(&matter, "a.txt", "A", b"rebuildword alpha");

    // Open a reader, then **explicitly drop the reader** before shutdown/reset.
    // MatterIndex::shutdown alone does not drop a separately held IndexReader
    // (Windows mmap lock requirement).
    let handle = MatterIndex::open_or_create(&root).expect("open");
    let reader = handle.reader().expect("reader");
    drop(reader);
    handle.shutdown();

    let outcome = run_index(&matter, true);
    assert!(matches!(outcome, FtsOutcome::Succeeded(_)), "{outcome:?}");

    let hits = search_keyword(
        &root,
        &KeywordQuery {
            query: "rebuildword".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("search");
    assert_eq!(hits.item_ids.len(), 1);
}

#[test]
fn invalid_query_returns_error_no_panic() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-badq");
    let matter = Matter::create(&root, "BadQ").expect("create");
    insert_text_item(&matter, "a.txt", "A", b"hello world");
    run_index(&matter, true);

    let err = search_keyword(
        &root,
        &KeywordQuery {
            query: "\"unterminated".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect_err("invalid");
    let msg = err.to_string();
    assert!(
        msg.to_ascii_lowercase().contains("query") || msg.contains("invalid"),
        "got: {msg}"
    );
}

#[test]
fn compose_filter_keyword_intersection() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-compose");
    let matter = Matter::create(&root, "Compose").expect("create");

    let digest_a = matter.put_bytes(b"secretword alice body").expect("cas");
    let alice = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("alice.txt".into()),
            subject: Some("Alice".into()),
            custodian: Some("alice@example.com".into()),
            text_sha256: Some(digest_a),
            ..Default::default()
        })
        .expect("alice");
    let digest_b = matter.put_bytes(b"secretword bob body").expect("cas");
    let _bob = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            path: Some("bob.txt".into()),
            subject: Some("Bob".into()),
            custodian: Some("bob@example.com".into()),
            text_sha256: Some(digest_b),
            ..Default::default()
        })
        .expect("bob");

    // Promote both to review so default scope works, or use entire_matter.
    run_index(&matter, true);

    let filter = FilterSpec {
        scope: SCOPE_ENTIRE_MATTER.into(),
        conditions: vec![FilterCondition {
            field: "custodian".into(),
            op: "contains".into(),
            value: Some(serde_json::Value::String("alice".into())),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };

    let (count, rows) = compose_keyword_filter(&matter, &root, Some("secretword"), &filter, 100, 0)
        .expect("compose");
    assert_eq!(count, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, alice.id);
}

#[test]
fn family_expand_after_intersect() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-family");
    let matter = Matter::create(&root, "Family").expect("create");

    let fam = matter
        .insert_family(matter_core::FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam");
    let digest = matter
        .put_bytes(b"parentkeyword only in parent")
        .expect("cas");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            path: Some("parent.eml".into()),
            subject: Some("Parent".into()),
            text_sha256: Some(digest),
            ..Default::default()
        })
        .expect("parent");
    let child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            parent_item_id: Some(parent.id.clone()),
            family_id: Some(fam.id.clone()),
            path: Some("attach.pdf".into()),
            subject: Some("Attach".into()),
            // no text — not in FTS hits
            ..Default::default()
        })
        .expect("child");

    run_index(&matter, true);

    let filter_no_fam = FilterSpec {
        scope: SCOPE_ENTIRE_MATTER.into(),
        include_family: false,
        ..FilterSpec::default()
    };
    let (c0, rows0) = compose_keyword_filter(
        &matter,
        &root,
        Some("parentkeyword"),
        &filter_no_fam,
        100,
        0,
    )
    .expect("no fam");
    assert_eq!(c0, 1);
    assert_eq!(rows0[0].id, parent.id);

    let filter_fam = FilterSpec {
        scope: SCOPE_ENTIRE_MATTER.into(),
        include_family: true,
        ..FilterSpec::default()
    };
    let (c1, rows1) =
        compose_keyword_filter(&matter, &root, Some("parentkeyword"), &filter_fam, 100, 0)
            .expect("fam");
    let ids: HashSet<_> = rows1.iter().map(|r| r.id.clone()).collect();
    assert!(c1 >= 2, "expected parent + child, count={c1}");
    assert!(ids.contains(&parent.id));
    assert!(ids.contains(&child.id));
}

#[test]
fn job_cancel_between_batches() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-cancel");
    let matter = Matter::create(&root, "Cancel").expect("create");
    for i in 0..5 {
        insert_text_item(
            &matter,
            &format!("{i}.txt"),
            &format!("S{i}"),
            format!("bodyword{i} content").as_bytes(),
        );
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let job = matter.create_job("fts_index").expect("job");
    let params = FtsIndexParams {
        reset: true,
        batch_size: 1,
        ..Default::default()
    };
    // Cancel after first progress callback.
    let outcome = run_fts_index(
        &matter,
        &job.id,
        &params,
        Some(&|| cancel_flag2.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");

    match outcome {
        FtsOutcome::Paused(s) => {
            assert!(s.completed_count >= 1);
            let cp = matter
                .get_checkpoint(&job.id, FTS_STAGE)
                .expect("cp")
                .expect("present");
            assert!(cp.completed_count >= 1);
        }
        FtsOutcome::Succeeded(s) => {
            // Race: might finish before cancel is seen if very fast; still ok if checkpointed.
            assert!(s.completed_count >= 1);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn no_fts5_tables_as_primary() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-no-fts5");
    let matter = Matter::create(&root, "NoFts5").expect("create");
    insert_text_item(&matter, "a.txt", "A", b"check fts5 absent");
    run_index(&matter, true);

    let n: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table','virtual table') \
             AND (sql LIKE '%USING fts5%' OR sql LIKE '%USING FTS5%' OR name LIKE '%fts5%')",
            [],
            |row| row.get(0),
        )
        .expect("query");
    assert_eq!(n, 0, "must not create FTS5 tables as primary");

    // Index dir exists on disk.
    assert!(MatterIndex::index_dir(&root).as_std_path().exists());
    // remove after drop
    remove_index_dir(&root).expect("rm");
}

#[test]
fn missing_index_honest_error() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-missing");
    let _matter = Matter::create(&root, "Missing").expect("create");
    // index/ may exist empty from matter layout — search should still be honest.
    let err = search_keyword(
        &root,
        &KeywordQuery {
            query: "anything".into(),
            limit: 10,
            offset: 0,
        },
    );
    assert!(err.is_err(), "expected error for empty/missing index");
}
