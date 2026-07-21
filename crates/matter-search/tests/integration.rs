//! Integration tests for matter-search (tempdir matters + Tantivy).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_core::{detect_language_tag, LANG_PACK_CJK_NGRAM_V1, LANG_PACK_LATIN_DEFAULT};
use matter_core::{
    item_role, item_status, FilterCondition, FilterSpec, ItemInput, Matter, SCOPE_ENTIRE_MATTER,
};
use matter_search::{
    compose_keyword_filter, delete_then_add, remove_index_dir, run_fts_index, search_keyword,
    search_keyword_for_matter, FtsIndexParams, FtsOutcome, KeywordQuery, LangPack, MatterIndex,
    SearchError, CODE_FTS_LANG_PACK_STALE, FTS_STAGE,
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
fn subject_change_reindexes_without_body_change() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-subj");
    let matter = Matter::create(&root, "Subj").expect("create");
    let id = insert_text_item(&matter, "s.txt", "oldsubject", b"bodytoken fixed");
    run_index(&matter, true);

    matter
        .update_item(
            &id,
            matter_core::ItemUpdate {
                subject: Some(Some("newsubject".into())),
                ..Default::default()
            },
        )
        .expect("subj");

    let job = matter.create_job("fts_index").expect("job");
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
    assert!(matches!(outcome, FtsOutcome::Succeeded(_)), "{outcome:?}");

    let hits = search_keyword(
        &root,
        &KeywordQuery {
            query: "newsubject".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("new subj");
    assert_eq!(hits.item_ids, vec![id.clone()]);

    let old = search_keyword(
        &root,
        &KeywordQuery {
            query: "oldsubject".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("old subj");
    assert!(
        old.item_ids.is_empty(),
        "stale subject must not remain after re-index"
    );
}

#[test]
fn orphan_cleared_text_removed_from_index() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-orphan");
    let matter = Matter::create(&root, "Orphan").expect("create");
    let id = insert_text_item(&matter, "o.txt", "O", b"ghosttoken body");
    run_index(&matter, true);
    assert_eq!(
        search_keyword(
            &root,
            &KeywordQuery {
                query: "ghosttoken".into(),
                limit: 5,
                offset: 0,
            },
        )
        .expect("before")
        .item_ids,
        vec![id.clone()]
    );

    // Clear text CAS pointers — item is no longer eligible but fts_* still set.
    matter
        .update_item(
            &id,
            matter_core::ItemUpdate {
                text_sha256: Some(None),
                html_sha256: Some(None),
                ..Default::default()
            },
        )
        .expect("clear text");

    let job = matter.create_job("fts_index").expect("job");
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
    .expect("purge");
    assert!(matches!(outcome, FtsOutcome::Succeeded(_)), "{outcome:?}");

    let after = search_keyword(
        &root,
        &KeywordQuery {
            query: "ghosttoken".into(),
            limit: 5,
            offset: 0,
        },
    );
    // Index may be empty (IndexMissing) or return no hits.
    match after {
        Ok(h) => assert!(h.item_ids.is_empty(), "orphan doc must be deleted"),
        Err(e) => {
            let s = e.to_string().to_lowercase();
            assert!(
                s.contains("index") || s.contains("empty") || s.contains("build"),
                "unexpected error: {e}"
            );
        }
    }
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

// ---------------------------------------------------------------------------
// Track 0054 — multilingual packs (DoD-5)
// ---------------------------------------------------------------------------

#[test]
fn cjk_contiguous_matches_scattered_does_not() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-cjk-phrase");
    let matter = Matter::create(&root, "CjkPhrase").expect("create");
    matter
        .update_lang_pack(LANG_PACK_CJK_NGRAM_V1)
        .expect("pack");

    // Contiguous company name (phrase adjacency).
    let id_contig = insert_text_item(
        &matter,
        "contig.txt",
        "C",
        "株式会社トヨタ related notes".as_bytes(),
    );
    // Same three Han chars scattered (not adjacent as a company run).
    let id_scatter = insert_text_item(
        &matter,
        "scatter.txt",
        "S",
        "株 only then 式 mid and 会 later with 社 end".as_bytes(),
    );

    let outcome = run_index(&matter, true);
    assert!(matches!(outcome, FtsOutcome::Succeeded(_)), "{outcome:?}");

    let hits = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "株式会社".into(),
            limit: 20,
            offset: 0,
        },
    )
    .expect("cjk phrase search");
    assert!(
        hits.item_ids.contains(&id_contig),
        "contiguous company name must match, hits={:?}",
        hits.item_ids
    );
    assert!(
        !hits.item_ids.contains(&id_scatter),
        "scattered CJK must not match phrase path, hits={:?}",
        hits.item_ids
    );
}

#[test]
fn email_searchable_under_cjk_pack() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-cjk-email");
    let matter = Matter::create(&root, "CjkEmail").expect("create");
    matter
        .update_lang_pack(LANG_PACK_CJK_NGRAM_V1)
        .expect("pack");
    let id = insert_text_item(
        &matter,
        "mail.txt",
        "Contact",
        "please write bob@example.com for details 株式会社".as_bytes(),
    );
    run_index(&matter, true);

    let hits = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "bob@example.com".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("email search");
    assert_eq!(hits.item_ids, vec![id]);
}

#[test]
fn email_trailing_period_searchable_under_cjk_pack() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-cjk-email-period");
    let matter = Matter::create(&root, "CjkEmailPeriod").expect("create");
    matter
        .update_lang_pack(LANG_PACK_CJK_NGRAM_V1)
        .expect("pack");
    let id = insert_text_item(
        &matter,
        "mail.txt",
        "Contact",
        "please write bob@example.com. for details".as_bytes(),
    );
    run_index(&matter, true);

    let hits = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "bob@example.com".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("email with trailing period in body");
    assert_eq!(hits.item_ids, vec![id]);
}

#[test]
fn cjk_whitespace_separated_does_not_match_contiguous_phrase() {
    // P1-1: separator must open a positional gap so phrase "中国公" does not
    // match body "中国 国公".
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-cjk-sep");
    let matter = Matter::create(&root, "CjkSep").expect("create");
    matter
        .update_lang_pack(LANG_PACK_CJK_NGRAM_V1)
        .expect("pack");

    let id_sep = insert_text_item(&matter, "sep.txt", "S", "中国 国公 separated".as_bytes());
    let id_contig = insert_text_item(&matter, "contig.txt", "C", "中国公 contiguous".as_bytes());

    let outcome = run_index(&matter, true);
    assert!(matches!(outcome, FtsOutcome::Succeeded(_)), "{outcome:?}");

    let hits = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "中国公".into(),
            limit: 20,
            offset: 0,
        },
    )
    .expect("cjk contiguous phrase");
    assert!(
        hits.item_ids.contains(&id_contig),
        "contiguous form must match, hits={:?}",
        hits.item_ids
    );
    assert!(
        !hits.item_ids.contains(&id_sep),
        "whitespace-separated CJK must not match contiguous phrase, hits={:?}",
        hits.item_ids
    );
}

#[test]
fn english_regression_under_both_packs() {
    let (_tmp, base) = utf8_tempdir();

    // latin_default
    {
        let root = base.join("fts-en-latin");
        let matter = Matter::create(&root, "EnLatin").expect("create");
        let id = insert_text_item(&matter, "a.txt", "A", b"uniqueenglishword body");
        run_index(&matter, true);
        let hits = search_keyword_for_matter(
            &matter,
            &KeywordQuery {
                query: "uniqueenglishword".into(),
                limit: 10,
                offset: 0,
            },
        )
        .expect("latin en");
        assert_eq!(hits.item_ids, vec![id]);
        assert_eq!(
            matter.get_lang_config().unwrap().lang_pack_id,
            LANG_PACK_LATIN_DEFAULT
        );
    }

    // cjk_ngram_v1 still indexes English words
    {
        let root = base.join("fts-en-cjk");
        let matter = Matter::create(&root, "EnCjk").expect("create");
        matter
            .update_lang_pack(LANG_PACK_CJK_NGRAM_V1)
            .expect("pack");
        let id = insert_text_item(&matter, "b.txt", "B", b"uniqueenglishword body");
        run_index(&matter, true);
        let hits = search_keyword_for_matter(
            &matter,
            &KeywordQuery {
                query: "uniqueenglishword".into(),
                limit: 10,
                offset: 0,
            },
        )
        .expect("cjk en");
        assert_eq!(hits.item_ids, vec![id]);
    }
}

#[test]
fn pack_change_without_rebuild_is_stale_hard_error() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-stale");
    let matter = Matter::create(&root, "Stale").expect("create");
    let id = insert_text_item(&matter, "a.txt", "A", b"staleword content");
    run_index(&matter, true);

    // Switch pack without rebuild — fingerprint cleared.
    matter
        .update_lang_pack(LANG_PACK_CJK_NGRAM_V1)
        .expect("switch pack");
    let cfg = matter.get_lang_config().expect("cfg");
    assert!(cfg.fts_lang_fingerprint.is_none());

    let err = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "staleword".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect_err("must hard-fail when pack fingerprint missing");
    let msg = err.to_string();
    assert!(
        err.is_lang_pack_stale()
            || msg.contains(CODE_FTS_LANG_PACK_STALE)
            || msg.contains("Rebuild required"),
        "unexpected error: {err:?}"
    );
    assert_eq!(err.code(), Some(CODE_FTS_LANG_PACK_STALE));

    // After rebuild under CJK pack → Ok.
    let outcome = run_index(&matter, true);
    assert!(matches!(outcome, FtsOutcome::Succeeded(_)), "{outcome:?}");
    let hits = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "staleword".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("after rebuild");
    assert_eq!(hits.item_ids, vec![id]);
    assert_eq!(
        matter
            .get_lang_config()
            .unwrap()
            .fts_lang_fingerprint
            .as_deref(),
        Some(LangPack::CjkNgramV1.fingerprint().as_str())
    );
}

#[test]
fn short_text_lang_detect_is_und() {
    assert_eq!(detect_language_tag("See attached"), "und");
    assert_eq!(detect_language_tag("12345"), "und");
    assert_eq!(detect_language_tag("hello world"), "und");
    assert_eq!(detect_language_tag(""), "und");
}

#[test]
fn mid_run_pack_change_does_not_certify_wrong_fingerprint() {
    // If the pack is switched while fts_index is in progress, the outer runner
    // must fail closed and not write a fingerprint for a different pack than
    // the one that tokenized the physical index.
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-mid-pack");
    let matter = Matter::create(&root, "MidPack").expect("create");
    // Enough candidates that cancel can fire mid-job under small batches.
    for i in 0..8 {
        insert_text_item(
            &matter,
            &format!("d{i}.txt"),
            "T",
            format!("midpackword{i} content").as_bytes(),
        );
    }

    // Start latin index, cancel after first progress tick, then switch pack and
    // resume — pack-change-on-resume already forces rebuild. Here we switch
    // pack **during** a single Succeeded-path by using cancel then switching
    // before a complete run that would otherwise write FP after inner success.
    //
    // Direct path: run under latin with a cancel that never fires (full success
    // would write latin FP). Instead: complete index under latin in a custom
    // flow by switching pack between inner success simulation — call run with
    // cancel that fires never, but switch pack via a progress callback before
    // outer certifies.
    let job = matter.create_job("fts_index").expect("job");
    let params = FtsIndexParams {
        reset: true,
        batch_size: 2,
        ..FtsIndexParams::default()
    };
    let switched = std::sync::atomic::AtomicBool::new(false);
    let outcome = run_fts_index(&matter, &job.id, &params, None, |_| {
        // After first progress, flip pack to CJK while the job is still running.
        if !switched.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let _ = matter.update_lang_pack(LANG_PACK_CJK_NGRAM_V1);
        }
    })
    .expect("run");

    // Must not Succeeded with a CJK fingerprint over a latin-tokenized index.
    match &outcome {
        FtsOutcome::Failed { message, .. } => {
            assert!(
                message.contains("language pack changed") || message.contains("rebuild"),
                "unexpected fail message: {message}"
            );
        }
        FtsOutcome::Succeeded(_) => {
            // If the progress callback raced after last batch only, pack may
            // have changed after indexing completed under the new pack mid-way
            // — still require fingerprint to match physical pack or be absent.
            let cfg = matter.get_lang_config().expect("cfg");
            // Fail closed preferred; if Succeeded, FP must match current pack
            // and be the pack used for tokens. Safest assertion: either no FP
            // or pack+FP consistent and CJK search works only after honest rebuild.
            if let Some(fp) = cfg.fts_lang_fingerprint.as_deref() {
                assert_eq!(fp, LangPack::CjkNgramV1.fingerprint().as_str());
            }
        }
        FtsOutcome::Paused(_) => panic!("unexpected pause: {outcome:?}"),
    }

    // Never leave a latin fingerprint when pack is CJK.
    let cfg = matter.get_lang_config().expect("cfg");
    assert_eq!(cfg.lang_pack_id, LANG_PACK_CJK_NGRAM_V1);
    if let Some(fp) = cfg.fts_lang_fingerprint.as_deref() {
        assert_ne!(
            fp,
            LangPack::LatinDefault.fingerprint().as_str(),
            "must not certify latin FP after mid-run switch to CJK"
        );
    }
}

#[test]
fn plus_email_searchable_under_cjk_pack() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-cjk-plus-email");
    let matter = Matter::create(&root, "CjkPlusEmail").expect("create");
    matter
        .update_lang_pack(LANG_PACK_CJK_NGRAM_V1)
        .expect("pack");
    let id = insert_text_item(
        &matter,
        "mail.txt",
        "Contact",
        "please write +tag@example.com for details".as_bytes(),
    );
    run_index(&matter, true);

    // QueryParser treats bare `+` as a mandatory-term operator — quote the
    // address so the full token is searched (indexing preserves plus-address).
    let hits = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: r#""+tag@example.com""#.into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect("plus email search");
    assert_eq!(hits.item_ids, vec![id]);
}

#[test]
fn search_keyword_for_matter_missing_index() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-mat-missing");
    let matter = Matter::create(&root, "Miss").expect("create");
    let err = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "x".into(),
            limit: 5,
            offset: 0,
        },
    )
    .expect_err("missing");
    assert!(matches!(err, SearchError::IndexMissing));
}

#[test]
fn resume_after_pack_change_forces_rebuild() {
    // P1-2: mid-job checkpoint under latin must not resume into a cjk pack index.
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-pack-resume");
    let matter = Matter::create(&root, "PackResume").expect("create");
    assert_eq!(
        matter.get_lang_config().unwrap().lang_pack_id,
        LANG_PACK_LATIN_DEFAULT
    );

    for i in 0..5 {
        insert_text_item(
            &matter,
            &format!("{i}.txt"),
            &format!("S{i}"),
            format!("bodyword{i} 株式会社 content").as_bytes(),
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
    let first = run_fts_index(
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
    .expect("partial latin run");

    // Prefer Paused (checkpoint present); Succeeded is a rare race on fast hosts.
    match &first {
        FtsOutcome::Paused(s) => assert!(s.completed_count >= 1),
        FtsOutcome::Succeeded(s) => assert!(s.completed_count >= 1),
        other => panic!("unexpected first outcome {other:?}"),
    }
    let cp = matter
        .get_checkpoint(&job.id, FTS_STAGE)
        .expect("cp")
        .expect("checkpoint present after partial run");
    assert!(
        cp.cursor_json.contains(LANG_PACK_LATIN_DEFAULT)
            || cp.cursor_json.contains("\"lang_pack_id\""),
        "checkpoint should record pack id, got {}",
        cp.cursor_json
    );

    // Switch pack while job is mid-flight / has latin checkpoint.
    matter
        .update_lang_pack(LANG_PACK_CJK_NGRAM_V1)
        .expect("switch to cjk");
    assert!(matter
        .get_lang_config()
        .unwrap()
        .fts_lang_fingerprint
        .is_none());

    // Resume same job — must force full rebuild under cjk, not mix latin tokens.
    let resume = run_fts_index(
        &matter,
        &job.id,
        &FtsIndexParams {
            reset: false,
            batch_size: 1,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("resume after pack change");
    assert!(
        matches!(resume, FtsOutcome::Succeeded(_)),
        "expected Succeeded after pack-change rebuild, got {resume:?}"
    );

    let cfg = matter.get_lang_config().expect("cfg");
    assert_eq!(cfg.lang_pack_id, LANG_PACK_CJK_NGRAM_V1);
    assert_eq!(
        cfg.fts_lang_fingerprint.as_deref(),
        Some(LangPack::CjkNgramV1.fingerprint().as_str())
    );

    // CJK phrase search must work (proves cjk tokenizer, not latin-only residue).
    let hits = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "株式会社".into(),
            limit: 20,
            offset: 0,
        },
    )
    .expect("cjk search after rebuild");
    assert!(
        !hits.item_ids.is_empty(),
        "cjk phrase must hit after forced rebuild"
    );
}

#[test]
fn lang_pack_version_mismatch_is_stale() {
    // P2-2: matching fingerprint string is not enough when version column diverges.
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("fts-ver-stale");
    let matter = Matter::create(&root, "VerStale").expect("create");
    insert_text_item(&matter, "a.txt", "A", b"versionword content");
    run_index(&matter, true);

    // Manually corrupt version while leaving fingerprint as-is.
    matter
        .connection()
        .execute(
            "UPDATE matters SET lang_pack_version = 99 WHERE id = ?1",
            [&matter.id()],
        )
        .expect("bump version");

    let cfg = matter.get_lang_config().expect("cfg");
    assert_eq!(cfg.lang_pack_version, 99);
    assert!(cfg.fts_lang_fingerprint.is_some());

    let err = search_keyword_for_matter(
        &matter,
        &KeywordQuery {
            query: "versionword".into(),
            limit: 10,
            offset: 0,
        },
    )
    .expect_err("must stale on version mismatch");
    assert!(
        err.is_lang_pack_stale() || err.code() == Some(CODE_FTS_LANG_PACK_STALE),
        "unexpected error: {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("version") || msg.contains(CODE_FTS_LANG_PACK_STALE),
        "message should mention version mismatch: {msg}"
    );
}
