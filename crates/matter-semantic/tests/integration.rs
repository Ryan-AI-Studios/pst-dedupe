//! Integration tests for semantic index + query (track 0050 DoD-6).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_core::{
    item_status, FilterCondition, FilterSpec, ItemInput, ItemUpdate, Matter, SCOPE_ENTIRE_MATTER,
};
use matter_semantic::{
    run_semantic_index, run_semantic_index_with_embedder, sanitize_model_id, search_semantic,
    Embedder, MockEmbedder, SemanticIndexParams, SemanticOutcome, SemanticQuery,
    JOB_KIND_SEMANTIC_INDEX, MOCK_MODEL_ID,
};

fn temp_matter(name: &str) -> (tempfile::TempDir, Matter, Utf8PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    let matter = Matter::create(&root, name).expect("create");
    (tmp, matter, root)
}

fn put_text(matter: &Matter, text: &str) -> String {
    matter.put_bytes(text.as_bytes()).expect("put")
}

fn insert_with_text(matter: &Matter, path: &str, text: &str) -> String {
    let digest = put_text(matter, text);
    let item = matter
        .insert_item(ItemInput {
            path: Some(path.into()),
            status: item_status::EXTRACTED.into(),
            text_sha256: Some(digest),
            ..Default::default()
        })
        .expect("insert");
    item.id
}

fn insert_with_text_custodian(matter: &Matter, path: &str, text: &str, custodian: &str) -> String {
    let digest = put_text(matter, text);
    let item = matter
        .insert_item(ItemInput {
            path: Some(path.into()),
            status: item_status::EXTRACTED.into(),
            text_sha256: Some(digest),
            custodian: Some(custodian.into()),
            ..Default::default()
        })
        .expect("insert");
    item.id
}

fn run_index(matter: &Matter) -> SemanticOutcome {
    let job = matter.create_job(JOB_KIND_SEMANTIC_INDEX).expect("job");
    let params = SemanticIndexParams::default();
    run_semantic_index(matter, &job.id, &params, None, |_| {}).expect("run")
}

fn entire_matter_filter() -> FilterSpec {
    FilterSpec {
        scope: SCOPE_ENTIRE_MATTER.into(),
        ..FilterSpec::default()
    }
}

fn custodian_filter(name: &str) -> FilterSpec {
    FilterSpec {
        scope: SCOPE_ENTIRE_MATTER.into(),
        conditions: vec![FilterCondition {
            field: "custodian".into(),
            op: "eq".into(),
            value: Some(serde_json::Value::String(name.into())),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    }
}

#[test]
fn mock_index_and_query_e2e() {
    let (_tmp, matter, root) = temp_matter("sem-e2e");
    let id = insert_with_text(
        &matter,
        "a.txt",
        "confidential fraud investigation bribery scheme documents",
    );
    insert_with_text(&matter, "b.txt", "weekend picnic recipes and garden tips");

    match run_index(&matter) {
        SemanticOutcome::Succeeded(r) => {
            assert!(r.embedded_count >= 2);
            assert_eq!(r.model_id, MOCK_MODEL_ID);
        }
        other => panic!("unexpected {other:?}"),
    }

    let meta = matter.get_semantic_meta().expect("meta");
    assert!(meta.semantic_enabled);
    assert_eq!(meta.semantic_model_id.as_deref(), Some(MOCK_MODEL_ID));

    let emb = MockEmbedder::default();
    let res = search_semantic(
        &matter,
        &root,
        &SemanticQuery {
            text: "fraud bribery investigation".into(),
            top_n_items: 10,
            min_score: None,
        },
        &entire_matter_filter(),
        &emb,
    )
    .expect("search");
    assert!(!res.hits.is_empty());
    assert_eq!(res.hits[0].item_id, id);
}

#[test]
fn prefilter_excludes_out_of_scope_relevant_doc() {
    // Alice has the highly relevant fraud docs (many); Bob has a weaker in-filter doc.
    // With top_n_items=1, a forbidden global-top-k-then-post-filter would take Alice
    // globally then ∩ Bob → empty. Pre-filter must still return Bob.
    let (_tmp, matter, root) = temp_matter("sem-prefilter");
    for i in 0..12 {
        insert_with_text_custodian(
            &matter,
            &format!("alice{i}.txt"),
            "fraud investigation bribery kickback scheme money laundering confidential",
            "Alice",
        );
    }
    let bob = insert_with_text_custodian(&matter, "bob.txt", "fraud notes draft", "Bob");

    assert!(matches!(run_index(&matter), SemanticOutcome::Succeeded(_)));

    let emb = MockEmbedder::default();
    let res = search_semantic(
        &matter,
        &root,
        &SemanticQuery {
            text: "fraud investigation bribery".into(),
            top_n_items: 1,
            min_score: None,
        },
        &custodian_filter("Bob"),
        &emb,
    )
    .expect("search");

    assert!(
        res.hits.iter().all(|h| h.item_id == bob),
        "only Bob's items allowed; got {:?}",
        res.hits
    );
    assert!(
        !res.hits.is_empty(),
        "Bob's weaker in-filter doc must still surface under top_n=1 pre-filter"
    );
    assert_eq!(res.hits[0].item_id, bob);
}

#[test]
fn group_before_limit_long_doc_does_not_monopolize() {
    let (_tmp, matter, root) = temp_matter("sem-group");
    // Long multi-chunk doc packed with the query theme.
    let long = "relevant fraud bribery investigation topic. ".repeat(80);
    let _long_id = insert_with_text(&matter, "long.txt", &long);
    // Several short docs with same theme.
    let short_ids: Vec<String> = (0..5)
        .map(|i| {
            insert_with_text(
                &matter,
                &format!("short{i}.txt"),
                "fraud bribery investigation summary",
            )
        })
        .collect();

    assert!(matches!(run_index(&matter), SemanticOutcome::Succeeded(_)));

    let emb = MockEmbedder::default();
    let res = search_semantic(
        &matter,
        &root,
        &SemanticQuery {
            text: "fraud bribery investigation".into(),
            top_n_items: 5,
            min_score: None,
        },
        &entire_matter_filter(),
        &emb,
    )
    .expect("search");

    // Group-before-limit returns items; short docs must appear (not only long's chunks).
    let hit_ids: Vec<&str> = res.hits.iter().map(|h| h.item_id.as_str()).collect();
    assert_eq!(res.hits.len(), 5.min(1 + short_ids.len()));
    let short_hits = short_ids
        .iter()
        .filter(|id| hit_ids.contains(&id.as_str()))
        .count();
    assert!(
        short_hits >= 2,
        "expected multiple short items in top_n after grouping; hits={hit_ids:?}"
    );
}

#[test]
fn two_theme_long_doc_mid_theme_finds_item() {
    let (_tmp, matter, root) = temp_matter("sem-twotheme");
    let mut text = String::new();
    // Distinct regions: opening / mid-theme / closing. Mid must land in non-zero ordinal.
    text.push_str(&"alpha opening boilerplate words. ".repeat(40));
    let mid_start_hint = text.len();
    text.push_str(&"beta special midtopic keyword unicorn xylophone. ".repeat(40));
    let mid_end_hint = text.len();
    text.push_str(&"gamma closing footer words. ".repeat(40));
    let id = insert_with_text(&matter, "long.txt", &text);
    insert_with_text(&matter, "other.txt", "completely different picnic recipes");

    // Small chunks so mid theme is its own chunk(s).
    let job = matter.create_job(JOB_KIND_SEMANTIC_INDEX).expect("job");
    let params = SemanticIndexParams {
        chunk_chars: 200,
        chunk_overlap: 40,
        max_chunks_per_item: 48,
        ..SemanticIndexParams::default()
    };
    match run_semantic_index(&matter, &job.id, &params, None, |_| {}).expect("run") {
        SemanticOutcome::Succeeded(r) => {
            assert!(
                r.total_chunks >= 3,
                "long doc must produce multiple chunks; total_chunks={}",
                r.total_chunks
            );
        }
        other => panic!("unexpected {other:?}"),
    }

    let emb = MockEmbedder::default();
    let res = search_semantic(
        &matter,
        &root,
        &SemanticQuery {
            text: "midtopic unicorn xylophone".into(),
            top_n_items: 5,
            min_score: None,
        },
        &entire_matter_filter(),
        &emb,
    )
    .expect("search");
    assert!(!res.hits.is_empty());
    assert_eq!(res.hits[0].item_id, id);
    let hit = &res.hits[0];
    let ord = hit.best_ordinal.expect("winning chunk ordinal");
    assert!(
        ord > 0,
        "mid-theme query must win on a non-first chunk (ordinal={ord}); whole-doc-only would often pick 0"
    );
    if let (Some(s), Some(e)) = (hit.best_start_offset, hit.best_end_offset) {
        // Winning chunk should overlap the mid-theme region (hints, not exact).
        assert!(
            e > mid_start_hint && s < mid_end_hint,
            "winning offsets [{s},{e}) should overlap mid theme [{mid_start_hint},{mid_end_hint})"
        );
    }
}

#[test]
fn model_namespace_isolation() {
    let (_tmp, matter, root) = temp_matter("sem-ns");
    insert_with_text(&matter, "a.txt", "fraud investigation documents");

    // Index under model A (default mock:hash_v1).
    assert!(matches!(run_index(&matter), SemanticOutcome::Succeeded(_)));
    let dir_a = matter_semantic::namespace_dir(&root, MOCK_MODEL_ID).expect("ns a");
    assert!(dir_a.as_std_path().exists(), "model A namespace must exist");

    // Index under model B into a separate namespace (different mock model_id).
    let model_b = "mock:other_v1";
    let emb_b = MockEmbedder::with_model_id(model_b);
    let job_b = matter.create_job(JOB_KIND_SEMANTIC_INDEX).expect("job b");
    let params_b = SemanticIndexParams {
        model_id: model_b.into(),
        reset: false,
        ..SemanticIndexParams::default()
    };
    match run_semantic_index_with_embedder(&matter, &job_b.id, &params_b, &emb_b, None, |_| {})
        .expect("index b")
    {
        SemanticOutcome::Succeeded(r) => {
            assert!(r.embedded_count >= 1);
            assert_eq!(r.model_id, model_b);
        }
        other => panic!("index b unexpected {other:?}"),
    }
    let dir_b = matter_semantic::namespace_dir(&root, model_b).expect("ns b");
    assert!(dir_b.as_std_path().exists(), "model B namespace must exist");
    assert_ne!(dir_a, dir_b, "namespaces must be distinct paths");
    // A's files still present; B must not consume them as its own.
    assert!(
        dir_a.join("items").as_std_path().exists(),
        "A items dir remains"
    );
    assert!(
        dir_b.join("items").as_std_path().exists(),
        "B items dir exists separately"
    );

    // Query with embedder A while meta is B → fail closed (never read A as B).
    let emb_a = MockEmbedder::default();
    let err = search_semantic(
        &matter,
        &root,
        &SemanticQuery {
            text: "fraud".into(),
            top_n_items: 5,
            min_score: None,
        },
        &entire_matter_filter(),
        &emb_a,
    )
    .expect_err("must fail closed on model mismatch");
    assert!(
        err.to_string().contains("mismatch") || err.to_string().contains("model"),
        "err={err}"
    );

    // Query with matching B embedder succeeds from B namespace only.
    let res_b = search_semantic(
        &matter,
        &root,
        &SemanticQuery {
            text: "fraud".into(),
            top_n_items: 5,
            min_score: None,
        },
        &entire_matter_filter(),
        &emb_b,
    )
    .expect("search b");
    assert!(!res_b.hits.is_empty());
}

#[test]
fn text_change_reembeds() {
    let (_tmp, matter, _root) = temp_matter("sem-reembed");
    let id = insert_with_text(&matter, "a.txt", "original alpha content about apples");
    assert!(matches!(run_index(&matter), SemanticOutcome::Succeeded(_)));
    let item1 = matter.get_item(&id).expect("item");
    let emb1 = item1.semantic_embedded_text_sha256.clone();
    assert!(emb1.is_some());

    let new_digest = put_text(&matter, "updated beta content about bananas only");
    matter
        .update_item(
            &id,
            ItemUpdate {
                text_sha256: Some(Some(new_digest.clone())),
                ..Default::default()
            },
        )
        .expect("update");

    match run_index(&matter) {
        SemanticOutcome::Succeeded(r) => {
            assert!(r.embedded_count >= 1, "must re-embed changed text");
        }
        other => panic!("unexpected {other:?}"),
    }
    let item2 = matter.get_item(&id).expect("item");
    assert_eq!(
        item2.semantic_embedded_text_sha256.as_deref(),
        Some(new_digest.as_str())
    );
    assert_ne!(item2.semantic_embedded_text_sha256, emb1);
}

#[test]
fn sanitize_rejects_traversal() {
    assert!(sanitize_model_id("../evil").is_err());
    assert!(sanitize_model_id(r"C:\windows\system32").is_err());
    assert!(sanitize_model_id("/etc/passwd").is_err());
    assert_eq!(
        sanitize_model_id("mock:hash_v1").expect("ok"),
        "mock_hash_v1"
    );
}

#[test]
fn cancel_resume_smoke() {
    let (_tmp, matter, _root) = temp_matter("sem-cancel");
    for i in 0..6 {
        insert_with_text(
            &matter,
            &format!("d{i}.txt"),
            &format!("document number {i} with unique token tok{i}"),
        );
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let calls = Arc::new(AtomicU64::new(0));
    let calls2 = calls.clone();
    let job = matter.create_job(JOB_KIND_SEMANTIC_INDEX).expect("job");
    let params = SemanticIndexParams {
        batch_size: 1,
        ..SemanticIndexParams::default()
    };
    let emb = MockEmbedder::default();

    let outcome = run_semantic_index_with_embedder(
        &matter,
        &job.id,
        &params,
        &emb,
        Some(&|| {
            let n = calls2.fetch_add(1, Ordering::SeqCst) + 1;
            // Cancel after a couple of cancel polls.
            if n >= 4 {
                cancel_flag2.store(true, Ordering::SeqCst);
            }
            cancel_flag2.load(Ordering::SeqCst)
        }),
        |_| {},
    )
    .expect("run");

    match outcome {
        SemanticOutcome::Paused(s) => {
            assert!(s.completed_count > 0);
            assert!(s.completed_count < 6);
        }
        // Depending on cancel timing we might finish; still ok if succeeded.
        SemanticOutcome::Succeeded(_) => {}
        other => panic!("unexpected {other:?}"),
    }

    // Resume with same job id / checkpoint.
    cancel_flag.store(false, Ordering::SeqCst);
    match run_semantic_index_with_embedder(&matter, &job.id, &params, &emb, None, |_| {})
        .expect("resume")
    {
        SemanticOutcome::Succeeded(r) => {
            assert!(r.completed_count >= 6 || r.embedded_count + r.skipped_count >= 1);
        }
        SemanticOutcome::Paused(_) => {}
        other => panic!("unexpected resume {other:?}"),
    }
}

#[test]
fn skip_when_fingerprint_matches() {
    let (_tmp, matter, _root) = temp_matter("sem-skip");
    insert_with_text(&matter, "a.txt", "stable content for skip test");
    assert!(matches!(run_index(&matter), SemanticOutcome::Succeeded(_)));
    match run_index(&matter) {
        SemanticOutcome::Succeeded(r) => {
            assert!(r.skipped_count >= 1);
            assert_eq!(r.embedded_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// F-01: changing chunk params with `reset: false` must re-embed (not skip-all).
/// Prior fingerprint is captured before meta overwrite; digests alone are not enough.
#[test]
fn chunk_param_change_reembeds_without_reset() {
    let (_tmp, matter, _root) = temp_matter("sem-chunk-fp");
    insert_with_text(
        &matter,
        "a.txt",
        "stable content that must re-embed when chunk_chars changes",
    );
    insert_with_text(
        &matter,
        "b.txt",
        "second stable document for chunk param fingerprint change",
    );

    let first_fp = match run_index(&matter) {
        SemanticOutcome::Succeeded(r) => {
            assert!(r.embedded_count >= 2, "first index embeds all items");
            assert_eq!(r.skipped_count, 0);
            r.fingerprint
        }
        other => panic!("first index unexpected {other:?}"),
    };

    let job = matter.create_job(JOB_KIND_SEMANTIC_INDEX).expect("job");
    let params = SemanticIndexParams {
        // Same model_id; different chunk size → different fingerprint.
        chunk_chars: 400,
        chunk_overlap: 60,
        reset: false,
        ..SemanticIndexParams::default()
    };
    match run_semantic_index(&matter, &job.id, &params, None, |_| {}).expect("reindex") {
        SemanticOutcome::Succeeded(r) => {
            assert!(
                r.embedded_count >= 1,
                "chunk param change must re-embed; embedded={} skipped={}",
                r.embedded_count,
                r.skipped_count
            );
            // Must not skip-all after fingerprint change.
            assert!(
                r.skipped_count < r.completed_count,
                "must not skip every item after fingerprint change; {:?}",
                r
            );
            assert_eq!(
                r.embedded_count, r.completed_count,
                "all items should re-embed when fingerprint differs"
            );
            assert_ne!(
                r.fingerprint, first_fp,
                "report fingerprint must reflect new chunk params"
            );

            let emb = MockEmbedder::default();
            let expected_fp = params.fingerprint(emb.dimensions(), emb.engine_tag());
            assert_eq!(r.fingerprint, expected_fp);
        }
        other => panic!("reindex unexpected {other:?}"),
    }

    let meta = matter.get_semantic_meta().expect("meta");
    let emb = MockEmbedder::default();
    let expected_fp = params.fingerprint(emb.dimensions(), emb.engine_tag());
    assert_eq!(
        meta.semantic_fingerprint.as_deref(),
        Some(expected_fp.as_str())
    );
    assert_ne!(
        meta.semantic_fingerprint.as_deref(),
        Some(first_fp.as_str())
    );
}

/// Codex P1: cancel mid same-model fingerprint rebuild; query must not mix old/new
/// chunk geometry — only items with active fingerprint are scored.
#[test]
fn mid_rebuild_query_excludes_stale_fingerprint_vectors() {
    let (_tmp, matter, root) = temp_matter("sem-mid-query");
    for i in 0..6 {
        insert_with_text(
            &matter,
            &format!("doc{i}.txt"),
            &format!("stable body for mid rebuild query isolation item {i}"),
        );
    }
    match run_index(&matter) {
        SemanticOutcome::Succeeded(r) => assert_eq!(r.embedded_count, 6),
        other => panic!("initial {other:?}"),
    }

    let job = matter.create_job(JOB_KIND_SEMANTIC_INDEX).expect("job");
    let params = SemanticIndexParams {
        chunk_chars: 400,
        chunk_overlap: 60,
        reset: false,
        batch_size: 1,
        ..SemanticIndexParams::default()
    };
    let emb = MockEmbedder::default();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let calls = Arc::new(AtomicU64::new(0));
    let calls2 = calls.clone();

    let paused = run_semantic_index_with_embedder(
        &matter,
        &job.id,
        &params,
        &emb,
        Some(&|| {
            let n = calls2.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= 4 {
                cancel_flag2.store(true, Ordering::SeqCst);
            }
            cancel_flag2.load(Ordering::SeqCst)
        }),
        |_| {},
    )
    .expect("pause");

    let partial = match paused {
        SemanticOutcome::Paused(s) => {
            assert!(s.embedded_count > 0 && s.embedded_count < 6, "{s:?}");
            s.embedded_count
        }
        SemanticOutcome::Succeeded(_) => {
            // Race finished fully — still ok if all new fingerprint.
            return;
        }
        other => panic!("{other:?}"),
    };

    // Active meta fingerprint is the NEW one; only re-embedded items score.
    let res = search_semantic(
        &matter,
        &root,
        &SemanticQuery {
            text: "stable body mid rebuild".into(),
            top_n_items: 50,
            min_score: None,
        },
        &entire_matter_filter(),
        &emb,
    )
    .expect("query mid-rebuild");
    assert!(
        res.hits.len() as u64 <= partial,
        "must not include stale fingerprint vectors; hits={} partial_embedded={partial}",
        res.hits.len()
    );
    assert!(
        !res.hits.is_empty(),
        "partial re-embed should still return newly embedded items"
    );
}

/// F-09: cancel mid fingerprint-change job; resume must re-embed remaining items
/// (frozen fingerprint_matches in checkpoint), not skip after early meta write.
#[test]
fn fingerprint_change_cancel_resume_reembeds_remaining() {
    let (_tmp, matter, _root) = temp_matter("sem-fp-resume");
    for i in 0..8 {
        insert_with_text(
            &matter,
            &format!("doc{i}.txt"),
            &format!("stable body content for resume fingerprint test item {i}"),
        );
    }

    // Initial index with defaults.
    match run_index(&matter) {
        SemanticOutcome::Succeeded(r) => {
            assert_eq!(r.embedded_count, 8);
            assert_eq!(r.skipped_count, 0);
        }
        other => panic!("initial unexpected {other:?}"),
    }

    // New job: different chunk params, cancel after a few items.
    let job = matter.create_job(JOB_KIND_SEMANTIC_INDEX).expect("job");
    let params = SemanticIndexParams {
        chunk_chars: 400,
        chunk_overlap: 60,
        reset: false,
        batch_size: 1,
        ..SemanticIndexParams::default()
    };
    let emb = MockEmbedder::default();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let calls = Arc::new(AtomicU64::new(0));
    let calls2 = calls.clone();

    let paused = run_semantic_index_with_embedder(
        &matter,
        &job.id,
        &params,
        &emb,
        Some(&|| {
            let n = calls2.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= 5 {
                cancel_flag2.store(true, Ordering::SeqCst);
            }
            cancel_flag2.load(Ordering::SeqCst)
        }),
        |_| {},
    )
    .expect("paused run");

    let partial_embedded = match paused {
        SemanticOutcome::Paused(s) => {
            assert!(
                s.completed_count > 0 && s.completed_count < 8,
                "expected partial progress; {s:?}"
            );
            // Meta already has new fingerprint after early write — that is OK
            // only if checkpoint freezes fingerprint_matches=false.
            s.embedded_count
        }
        SemanticOutcome::Succeeded(r) => {
            // Cancel may race to full success; still require full re-embed.
            assert_eq!(
                r.embedded_count, 8,
                "if finished in one shot, all re-embedded"
            );
            return;
        }
        other => panic!("unexpected pause outcome {other:?}"),
    };

    // Resume: remaining items must re-embed, not skip due to rewritten meta.
    cancel_flag.store(false, Ordering::SeqCst);
    match run_semantic_index_with_embedder(&matter, &job.id, &params, &emb, None, |_| {})
        .expect("resume")
    {
        SemanticOutcome::Succeeded(r) => {
            assert_eq!(
                r.completed_count, 8,
                "resume should finish all items; {r:?}"
            );
            assert_eq!(
                r.embedded_count, 8,
                "all items must re-embed on fingerprint change across resume; partial before cancel={partial_embedded}; {r:?}"
            );
            assert_eq!(
                r.skipped_count, 0,
                "must not skip after fingerprint change; {r:?}"
            );
        }
        other => panic!("resume unexpected {other:?}"),
    }
}
