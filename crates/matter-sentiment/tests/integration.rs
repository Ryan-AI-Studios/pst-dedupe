//! Integration tests for sentiment job (track 0049).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_core::{item_status, FilterSpec, ItemInput, ItemUpdate, Matter, SCOPE_ENTIRE_MATTER};
use matter_sentiment::{
    run_sentiment, SentimentOutcome, SentimentParams, JOB_KIND_SENTIMENT, METHOD_VADER_LEXICON_V1,
};

fn temp_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    let matter = Matter::create(&root, name).expect("create");
    (tmp, matter)
}

fn put_text(matter: &Matter, text: &str) -> String {
    matter.put_bytes(text.as_bytes()).expect("put")
}

fn insert_with_text(matter: &Matter, text: &str) -> String {
    let digest = put_text(matter, text);
    let item = matter
        .insert_item(ItemInput {
            path: Some("msg.txt".into()),
            status: item_status::EXTRACTED.into(),
            text_sha256: Some(digest),
            ..Default::default()
        })
        .expect("insert");
    item.id
}

fn run_default(matter: &Matter) -> SentimentOutcome {
    let job = matter.create_job(JOB_KIND_SENTIMENT).expect("job");
    run_sentiment(matter, &job.id, &SentimentParams::default(), None, |_| {}).expect("run")
}

#[test]
fn clear_positive_scores_positive() {
    let (_tmp, matter) = temp_matter("sent-pos");
    let id = insert_with_text(
        &matter,
        "This is wonderful amazing excellent fantastic great news!!!",
    );
    match run_default(&matter) {
        SentimentOutcome::Succeeded(r) => {
            assert!(r.scanned_count >= 1);
            assert_eq!(r.method, METHOD_VADER_LEXICON_V1);
        }
        other => panic!("unexpected {other:?}"),
    }
    let item = matter.get_item(&id).expect("item");
    assert_eq!(item.sentiment_polarity.as_deref(), Some("positive"));
    assert!(item.sentiment_compound.unwrap_or(0.0) >= 0.05);
    assert_eq!(
        item.sentiment_method.as_deref(),
        Some(METHOD_VADER_LEXICON_V1)
    );
}

#[test]
fn clear_negative_scores_negative() {
    let (_tmp, matter) = temp_matter("sent-neg");
    let id = insert_with_text(
        &matter,
        "This is terrible awful horrible disgusting hate and worst!!!",
    );
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));
    let item = matter.get_item(&id).expect("item");
    assert_eq!(item.sentiment_polarity.as_deref(), Some("negative"));
    assert!(item.sentiment_compound.unwrap_or(0.0) <= -0.05);
}

#[test]
fn neutral_body_scores_neutral() {
    let (_tmp, matter) = temp_matter("sent-neu");
    let id = insert_with_text(
        &matter,
        "The meeting is scheduled for Tuesday at 3pm in conference room B.",
    );
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));
    let item = matter.get_item(&id).expect("item");
    assert_eq!(item.sentiment_polarity.as_deref(), Some("neutral"));
    assert!(item.sentiment_compound.is_some());
}

#[test]
fn hostile_lead_plus_long_footer_still_negative() {
    let (_tmp, matter) = temp_matter("sent-footer");
    let mut body = String::from(
        "You are a complete idiot and I hate everything about this disgusting deal!!!\n\n",
    );
    // Long confidentiality footer that would dilute whole-doc VADER toward 0.
    for _ in 0..40 {
        body.push_str(
            "This email and any attachments may contain confidential information intended solely for the intended recipient. \
If you are not the intended recipient please delete this email. Privileged and confidential. Unauthorized disclosure is prohibited. \
Strictly confidential. Do not distribute. Confidentiality notice applies.\n",
        );
    }
    let id = insert_with_text(&matter, &body);
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));
    let item = matter.get_item(&id).expect("item");
    assert_eq!(
        item.sentiment_polarity.as_deref(),
        Some("negative"),
        "unit-extreme + footer strip must keep hostile lead; compound={:?} min={:?} max={:?}",
        item.sentiment_compound,
        item.sentiment_compound_min,
        item.sentiment_compound_max
    );
}

#[test]
fn threshold_rerun_relabels_without_text_change() {
    let (_tmp, matter) = temp_matter("sent-relabel");
    // Mild positive that sits around ~0.10 under default thresholds → positive.
    let id = insert_with_text(&matter, "I am happy with this outcome.");
    let job1 = matter.create_job(JOB_KIND_SENTIMENT).expect("job1");
    run_sentiment(
        &matter,
        &job1.id,
        &SentimentParams {
            pos_threshold: 0.05,
            neg_threshold: -0.05,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run1");
    let item1 = matter.get_item(&id).expect("item1");
    let compound = item1.sentiment_compound.expect("compound");
    assert!(
        compound >= 0.05,
        "fixture must score mildly positive at 0.05; got {compound}"
    );
    assert_eq!(item1.sentiment_polarity.as_deref(), Some("positive"));
    let scanned = item1.sentiment_scanned_text_sha256.clone();

    // Raise pos_threshold above compound → neutral via relabel, no text change.
    let new_pos = (compound + 0.05).max(0.20);
    let job2 = matter.create_job(JOB_KIND_SENTIMENT).expect("job2");
    let o2 = run_sentiment(
        &matter,
        &job2.id,
        &SentimentParams {
            pos_threshold: new_pos,
            neg_threshold: -0.05,
            reset: false,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run2");
    match o2 {
        SentimentOutcome::Succeeded(r) => {
            assert!(r.relabeled_count >= 1, "expected relabel path");
            assert_eq!(r.scanned_count, 0, "must not full-rescore");
        }
        other => panic!("unexpected {other:?}"),
    }
    let item2 = matter.get_item(&id).expect("item2");
    assert_eq!(item2.sentiment_polarity.as_deref(), Some("neutral"));
    assert_eq!(item2.sentiment_compound, Some(compound));
    assert_eq!(item2.sentiment_scanned_text_sha256, scanned);
    assert_eq!(item2.sentiment_pos_threshold, Some(new_pos));
}

#[test]
fn empty_text_leaves_null_polarity() {
    let (_tmp, matter) = temp_matter("sent-empty");
    // Whitespace-only body after put still has digest; strip yields empty → unscored.
    let id = insert_with_text(&matter, "   \n\t  \n  ");
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));
    let item = matter.get_item(&id).expect("item");
    assert!(item.sentiment_polarity.is_none(), "unscored must stay NULL");
    assert!(item.sentiment_compound.is_none());
    assert!(item.sentiment_method.is_none());
    // Fingerprint "attempted empty" so re-runs can skip without CAS re-read.
    assert_eq!(
        item.sentiment_scanned_text_sha256.as_deref(),
        item.text_sha256.as_deref()
    );
    assert!(item.sentiment_scanned_at.is_some());

    // Filter: unscored preset finds the item.
    let mut spec = FilterSpec::preset_unscored();
    spec.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter
        .list_items_filtered_thin(&spec, 50, 0)
        .expect("filter");
    assert!(rows.iter().any(|r| r.id == id));
}

#[test]
fn rescore_empty_clears_prior_scores() {
    // F-01: hostile score → replace body with whitespace/disclaimer-only → re-run
    // must clear polarity/compound to NULL (not leave stale negative).
    let (_tmp, matter) = temp_matter("sent-clear-stale");
    let hostile = "This is terrible awful horrible disgusting hate and worst!!!";
    let id = insert_with_text(&matter, hostile);
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));
    let scored = matter.get_item(&id).expect("scored");
    assert_eq!(scored.sentiment_polarity.as_deref(), Some("negative"));
    assert!(scored.sentiment_compound.is_some());

    // Replace body with whitespace-only CAS (empty after strip).
    let empty_digest = put_text(&matter, "   \n\t  \n  ");
    matter
        .update_item(
            &id,
            ItemUpdate {
                text_sha256: Some(Some(empty_digest.clone())),
                ..Default::default()
            },
        )
        .expect("update text");

    let job2 = matter.create_job(JOB_KIND_SENTIMENT).expect("job2");
    let o2 = run_sentiment(&matter, &job2.id, &SentimentParams::default(), None, |_| {})
        .expect("rescore");
    match o2 {
        SentimentOutcome::Succeeded(r) => {
            assert!(r.unscored_count >= 1, "expected unscored path");
            assert_eq!(r.scanned_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }

    let cleared = matter.get_item(&id).expect("cleared");
    assert!(
        cleared.sentiment_polarity.is_none(),
        "stale polarity must be NULL after empty rescore"
    );
    assert!(cleared.sentiment_compound.is_none());
    assert!(cleared.sentiment_method.is_none());
    assert!(cleared.sentiment_pos.is_none());
    assert_eq!(
        cleared.sentiment_scanned_text_sha256.as_deref(),
        Some(empty_digest.as_str()),
        "empty-attempt fingerprint"
    );

    // has_sentiment false / unscored filter hits; negative filter excludes.
    let mut unscored = FilterSpec::preset_unscored();
    unscored.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter
        .list_items_filtered_thin(&unscored, 50, 0)
        .expect("unscored");
    assert!(rows.iter().any(|r| r.id == id));

    let mut neg = FilterSpec::preset_negative_tone();
    neg.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter.list_items_filtered_thin(&neg, 50, 0).expect("neg");
    assert!(!rows.iter().any(|r| r.id == id));

    // Re-run with same empty body skips via unscored fingerprint (no CAS rescore).
    let job3 = matter.create_job(JOB_KIND_SENTIMENT).expect("job3");
    let o3 = run_sentiment(&matter, &job3.id, &SentimentParams::default(), None, |_| {})
        .expect("skip empty");
    match o3 {
        SentimentOutcome::Succeeded(r) => {
            assert!(r.skipped_count >= 1, "unscored fingerprint skip");
            assert_eq!(r.scanned_count, 0);
            assert_eq!(r.unscored_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn text_change_rescores_polarity() {
    // F-04: score positive → change body to negative → re-run → polarity negative.
    let (_tmp, matter) = temp_matter("sent-text-change");
    let pos = include_str!("../../../fixtures/sentiment/pos.txt");
    let neg = include_str!("../../../fixtures/sentiment/neg.txt");
    let id = insert_with_text(&matter, pos);
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));
    let item1 = matter.get_item(&id).expect("item1");
    assert_eq!(item1.sentiment_polarity.as_deref(), Some("positive"));

    let neg_digest = put_text(&matter, neg);
    matter
        .update_item(
            &id,
            ItemUpdate {
                text_sha256: Some(Some(neg_digest)),
                ..Default::default()
            },
        )
        .expect("update text");

    let job2 = matter.create_job(JOB_KIND_SENTIMENT).expect("job2");
    let o2 = run_sentiment(&matter, &job2.id, &SentimentParams::default(), None, |_| {})
        .expect("rescore");
    match o2 {
        SentimentOutcome::Succeeded(r) => {
            assert!(r.scanned_count >= 1, "must full-rescore on text change");
            assert_eq!(r.skipped_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }
    let item2 = matter.get_item(&id).expect("item2");
    assert_eq!(item2.sentiment_polarity.as_deref(), Some("negative"));
    assert!(item2.sentiment_compound.unwrap_or(0.0) <= -0.05);
}

#[test]
fn cas_load_failure_clears_stale_sentiment() {
    // Codex r2 P2: score → point text_sha256 at missing CAS blob → re-run
    // must clear polarity (not leave stale scores) while counting an error.
    let (_tmp, matter) = temp_matter("sent-cas-fail");
    let id = insert_with_text(
        &matter,
        "This is terrible awful horrible disgusting hate and worst!!!",
    );
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));
    assert_eq!(
        matter
            .get_item(&id)
            .expect("scored")
            .sentiment_polarity
            .as_deref(),
        Some("negative")
    );

    let missing = "a".repeat(64); // valid hex shape, not in CAS
    matter
        .update_item(
            &id,
            ItemUpdate {
                text_sha256: Some(Some(missing)),
                ..Default::default()
            },
        )
        .expect("point at missing blob");

    let job2 = matter.create_job(JOB_KIND_SENTIMENT).expect("job2");
    let o2 = run_sentiment(&matter, &job2.id, &SentimentParams::default(), None, |_| {})
        .expect("cas fail run");
    match o2 {
        SentimentOutcome::Succeeded(r) => {
            assert!(r.error_count >= 1, "expected CAS error; got {r:?}");
        }
        other => panic!("unexpected {other:?}"),
    }

    let cleared = matter.get_item(&id).expect("cleared");
    assert!(cleared.sentiment_polarity.is_none());
    assert!(cleared.sentiment_compound.is_none());
    assert!(cleared.sentiment_method.is_none());
    // Must not fingerprint failed digest as successful empty-attempt.
    assert!(cleared.sentiment_scanned_text_sha256.is_none());
}

#[test]
fn clear_text_sha256_clears_stale_sentiment_on_rerun() {
    // Codex P2: score → null out text_sha256 (as empty office/pdf extract may do)
    // → re-run must clear polarity so filters no longer treat the item as scored.
    let (_tmp, matter) = temp_matter("sent-null-text");
    let id = insert_with_text(
        &matter,
        "This is terrible awful horrible disgusting hate and worst!!!",
    );
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));
    let scored = matter.get_item(&id).expect("scored");
    assert_eq!(scored.sentiment_polarity.as_deref(), Some("negative"));

    matter
        .update_item(
            &id,
            ItemUpdate {
                text_sha256: Some(None),
                ..Default::default()
            },
        )
        .expect("clear text");
    let mid = matter.get_item(&id).expect("mid");
    assert!(mid.text_sha256.is_none());
    assert_eq!(mid.sentiment_polarity.as_deref(), Some("negative")); // stale until job

    let job2 = matter.create_job(JOB_KIND_SENTIMENT).expect("job2");
    let o2 = run_sentiment(&matter, &job2.id, &SentimentParams::default(), None, |_| {})
        .expect("cleanup run");
    match o2 {
        SentimentOutcome::Succeeded(r) => {
            assert!(
                r.unscored_count >= 1,
                "must visit previously-scored item with NULL text; got {r:?}"
            );
        }
        other => panic!("unexpected {other:?}"),
    }

    let cleared = matter.get_item(&id).expect("cleared");
    assert!(cleared.sentiment_polarity.is_none());
    assert!(cleared.sentiment_compound.is_none());
    assert!(cleared.sentiment_method.is_none());
    assert!(cleared.sentiment_scanned_text_sha256.is_none());

    let mut unscored = FilterSpec::preset_unscored();
    unscored.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter
        .list_items_filtered_thin(&unscored, 50, 0)
        .expect("unscored");
    assert!(rows.iter().any(|r| r.id == id));

    let mut neg = FilterSpec::preset_negative_tone();
    neg.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter.list_items_filtered_thin(&neg, 50, 0).expect("neg");
    assert!(!rows.iter().any(|r| r.id == id));
}

#[test]
fn determinism_same_text_twice() {
    let (_tmp, matter) = temp_matter("sent-det");
    let text = "Absolutely fantastic wonderful success and joy!!!";
    let id = insert_with_text(&matter, text);
    let job1 = matter.create_job(JOB_KIND_SENTIMENT).expect("j1");
    run_sentiment(
        &matter,
        &job1.id,
        &SentimentParams {
            reset: true,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("r1");
    let a = matter.get_item(&id).expect("a");

    let job2 = matter.create_job(JOB_KIND_SENTIMENT).expect("j2");
    run_sentiment(
        &matter,
        &job2.id,
        &SentimentParams {
            reset: true,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("r2");
    let b = matter.get_item(&id).expect("b");
    assert_eq!(a.sentiment_compound, b.sentiment_compound);
    assert_eq!(a.sentiment_polarity, b.sentiment_polarity);
    assert_eq!(a.sentiment_pos, b.sentiment_pos);
    assert_eq!(a.sentiment_neu, b.sentiment_neu);
    assert_eq!(a.sentiment_neg, b.sentiment_neg);
}

#[test]
fn cancel_mid_run_pauses_and_resume_completes() {
    let (_tmp, matter) = temp_matter("sent-cancel");
    for i in 0..8 {
        insert_with_text(
            &matter,
            &format!("This is terrible awful horrible message number {i}!!!"),
        );
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let progress_calls = Arc::new(AtomicU64::new(0));
    let progress_flag = progress_calls.clone();
    // Cancel after first item progress callback.
    let job = matter.create_job(JOB_KIND_SENTIMENT).expect("job");
    let outcome = run_sentiment(
        &matter,
        &job.id,
        &SentimentParams {
            batch_size: 2,
            ..Default::default()
        },
        Some(&|| cancel_flag.load(Ordering::SeqCst)),
        |_| {
            let n = progress_flag.fetch_add(1, Ordering::SeqCst);
            if n >= 1 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");
    match outcome {
        SentimentOutcome::Paused(s) => {
            assert!(s.completed_count < 8);
            assert!(s.completed_count >= 1);
        }
        other => panic!("expected Paused, got {other:?}"),
    }

    // Resume: same job id with checkpoint.
    cancel.store(false, Ordering::SeqCst);
    let outcome2 = run_sentiment(
        &matter,
        &job.id,
        &SentimentParams {
            batch_size: 2,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("resume");
    match outcome2 {
        SentimentOutcome::Succeeded(r) => {
            assert!(r.completed_count >= 8);
        }
        other => panic!("expected Succeeded on resume, got {other:?}"),
    }
}

#[test]
fn negative_filter_excludes_null_unscored() {
    let (_tmp, matter) = temp_matter("sent-filter");
    let neg_id = insert_with_text(
        &matter,
        "This is terrible awful horrible disgusting hate!!!",
    );
    // Item with no text_sha256 is not a candidate — insert without text.
    let no_text = matter
        .insert_item(ItemInput {
            path: Some("empty.msg".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect("no text")
        .id;
    assert!(matches!(
        run_default(&matter),
        SentimentOutcome::Succeeded(_)
    ));

    let mut neg = FilterSpec::preset_negative_tone();
    neg.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter.list_items_filtered_thin(&neg, 50, 0).expect("neg");
    assert!(rows.iter().any(|r| r.id == neg_id));
    assert!(!rows.iter().any(|r| r.id == no_text));

    let mut unscored = FilterSpec::preset_unscored();
    unscored.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter
        .list_items_filtered_thin(&unscored, 50, 0)
        .expect("unscored");
    assert!(rows.iter().any(|r| r.id == no_text));
    assert!(!rows.iter().any(|r| r.id == neg_id));
}

#[test]
fn skip_when_fingerprint_matches() {
    let (_tmp, matter) = temp_matter("sent-skip");
    insert_with_text(&matter, "Wonderful excellent fantastic news!!!");
    let job1 = matter.create_job(JOB_KIND_SENTIMENT).expect("j1");
    run_sentiment(&matter, &job1.id, &SentimentParams::default(), None, |_| {}).expect("r1");
    let job2 = matter.create_job(JOB_KIND_SENTIMENT).expect("j2");
    let o2 =
        run_sentiment(&matter, &job2.id, &SentimentParams::default(), None, |_| {}).expect("r2");
    match o2 {
        SentimentOutcome::Succeeded(r) => {
            assert!(r.skipped_count >= 1);
            assert_eq!(r.scanned_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }
}
