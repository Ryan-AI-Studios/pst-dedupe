//! Integration tests for entity_scan (track 0046).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_core::{entity_flags, item_status, FilterSpec, ItemInput, Matter, SCOPE_ENTIRE_MATTER};
use matter_entity::{
    luhn_valid, normalize_email, run_entity_scan, safe_byte_slice, scan_text, EntityScanOutcome,
    EntityScanParams, JOB_KIND_ENTITY_SCAN,
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

fn insert_with_text(matter: &Matter, text: &str, subject: Option<&str>) -> String {
    let digest = put_text(matter, text);
    let item = matter
        .insert_item(ItemInput {
            path: Some("msg.txt".into()),
            status: item_status::EXTRACTED.into(),
            subject: subject.map(|s| s.into()),
            text_sha256: Some(digest),
            ..Default::default()
        })
        .expect("insert");
    item.id
}

#[test]
fn luhn_and_email_normalize_unit() {
    assert!(luhn_valid("4111111111111111"));
    assert!(!luhn_valid("4111111111111112"));
    let a = normalize_email("bob@example.com,").unwrap();
    let b = normalize_email("bob@example.com").unwrap();
    assert_eq!(a, b);
}

#[test]
fn scan_finds_synthetic_hits_masked() {
    let text = include_str!("../../../fixtures/entity/sample_pii.txt");
    let packs = matter_entity::default_pack_ids();
    let hits = scan_text(text, "text", &packs);
    assert!(hits.iter().any(|h| h.entity_type == "email"));
    assert!(hits.iter().any(|h| h.entity_type == "credit_card"));
    assert!(hits.iter().any(|h| h.entity_type == "ssn_us"));
    assert!(hits.iter().any(|h| h.entity_type == "phone_us"));
    assert!(hits.iter().any(|h| h.entity_type == "currency_usd"));
    // No cleartext PAN/SSN in masks.
    for h in &hits {
        assert!(!h.masked_value.contains("4111111111111111"));
        assert!(!h.masked_value.contains("219-09-9999"));
        if h.entity_type == "email" {
            assert!(
                h.masked_value.contains('@') && h.masked_value.contains('.'),
                "domain visible: {}",
                h.masked_value
            );
        }
    }
}

#[test]
fn job_scan_stores_hits_and_filter() {
    let (_tmp, matter) = temp_matter("ent-scan");
    let text = "Contact alice@competitor.com SSN 219-09-9999 card 4111111111111111";
    let id = insert_with_text(&matter, text, Some("Invoice"));
    let job = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("job");

    let outcome =
        run_entity_scan(&matter, &job.id, &EntityScanParams::default(), None, |_| {}).expect("run");
    match outcome {
        EntityScanOutcome::Succeeded(r) => {
            assert!(r.scanned_count >= 1);
            assert!(r.hit_count >= 2);
        }
        other => panic!("unexpected {other:?}"),
    }

    let hits = matter.list_entity_hits(&id).expect("hits");
    assert!(!hits.is_empty());
    for h in &hits {
        assert!(!h.match_hash.is_empty());
        assert!(!h.masked_value.is_empty());
    }

    let item = matter.get_item(&id).expect("item");
    assert!(item.entity_hit_count > 0);
    assert!(
        item.entity_flags & entity_flags::EMAIL != 0 || item.entity_flags & entity_flags::SSN != 0
    );
    assert!(item.entity_scanned_text_sha256.is_some());

    // Filter presets.
    let mut pii = FilterSpec::preset_has_pii();
    pii.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter.list_items_filtered_thin(&pii, 50, 0).expect("pii");
    assert!(rows.iter().any(|r| r.id == id));

    let mut email = FilterSpec::preset_has_email();
    email.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter
        .list_items_filtered_thin(&email, 50, 0)
        .expect("email");
    assert!(rows.iter().any(|r| r.id == id));
}

#[test]
fn digest_change_rescans_without_reset() {
    let (_tmp, matter) = temp_matter("ent-digest");
    let id = insert_with_text(&matter, "email only bob@example.com", None);
    let job1 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("job1");
    run_entity_scan(
        &matter,
        &job1.id,
        &EntityScanParams {
            reset: false,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run1");

    let hits1 = matter.list_entity_hits(&id).expect("h1");
    assert!(hits1.iter().any(|h| h.entity_type == "email"));
    assert!(!hits1.iter().any(|h| h.entity_type == "credit_card"));

    // Second run same digest → skip.
    let job2 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("job2");
    let o2 = run_entity_scan(
        &matter,
        &job2.id,
        &EntityScanParams {
            reset: false,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run2");
    match o2 {
        EntityScanOutcome::Succeeded(r) => {
            assert_eq!(r.scanned_count, 0, "should skip unchanged");
            assert!(r.skipped_count >= 1);
        }
        other => panic!("{other:?}"),
    }

    // Mutate text_sha256 (new CAS body with card).
    let new_digest = put_text(&matter, "now has card 4111111111111111 and bob@example.com");
    matter
        .connection()
        .execute(
            "UPDATE items SET text_sha256 = ?1 WHERE id = ?2",
            [&new_digest, &id],
        )
        .expect("update digest");

    let job3 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("job3");
    let o3 = run_entity_scan(
        &matter,
        &job3.id,
        &EntityScanParams {
            reset: false,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run3");
    match o3 {
        EntityScanOutcome::Succeeded(r) => {
            assert!(r.scanned_count >= 1, "must rescan after digest change");
        }
        other => panic!("{other:?}"),
    }

    let hits3 = matter.list_entity_hits(&id).expect("h3");
    assert!(
        hits3.iter().any(|h| h.entity_type == "credit_card"),
        "replaced hits should include card"
    );
    // Prior-only-email set replaced.
    assert!(hits3.iter().any(|h| h.entity_type == "email"));
}

#[test]
fn cancel_resume() {
    let (_tmp, matter) = temp_matter("ent-cancel");
    for i in 0..5 {
        let _ = insert_with_text(
            &matter,
            &format!("item {i} bob{i}@example.com"),
            Some(&format!("subj {i}")),
        );
    }
    let job = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("job");
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let completed_seen = Arc::new(AtomicBool::new(false));
    let completed_seen2 = completed_seen.clone();
    let outcome = run_entity_scan(
        &matter,
        &job.id,
        &EntityScanParams {
            batch_size: 1,
            ..Default::default()
        },
        Some(&|| cancel_flag2.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                completed_seen2.store(true, Ordering::SeqCst);
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");

    // With batch_size=1 and cancel after first progress, pause must stick.
    match outcome {
        EntityScanOutcome::Paused(s) => {
            assert!(s.completed_count >= 1);
            assert!(
                s.completed_count < 5,
                "must not finish all five before pause"
            );
            assert!(completed_seen.load(Ordering::SeqCst));
        }
        other => panic!("expected Paused, got {other:?}"),
    }

    // Resume: same job_id reloads checkpoint frozen params and finishes remaining items.
    cancel_flag.store(false, Ordering::SeqCst);
    let o2 = run_entity_scan(
        &matter,
        &job.id,
        &EntityScanParams {
            batch_size: 1,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("resume");
    match o2 {
        EntityScanOutcome::Succeeded(r) => {
            assert!(
                r.completed_count >= 5,
                "resume must complete remaining candidates"
            );
        }
        other => panic!("expected Succeeded after resume, got {other:?}"),
    }
}

#[test]
fn audit_start_complete_includes_pack_versions() {
    let (_tmp, matter) = temp_matter("ent-audit");
    let _id = insert_with_text(&matter, "bob@example.com card 4111111111111111", None);
    let job = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("job");
    let outcome =
        run_entity_scan(&matter, &job.id, &EntityScanParams::default(), None, |_| {}).expect("run");
    assert!(matches!(outcome, EntityScanOutcome::Succeeded(_)));

    let conn = matter.connection();
    let mut stmt = conn
        .prepare(
            "SELECT action, params_json FROM audit_events \
             WHERE action IN ('entity_scan.start', 'entity_scan.complete') \
             ORDER BY seq ASC",
        )
        .expect("prepare");
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert!(
        rows.iter().any(|(a, _)| a == "entity_scan.start"),
        "missing entity_scan.start: {rows:?}"
    );
    let complete = rows
        .iter()
        .find(|(a, _)| a == "entity_scan.complete")
        .expect("entity_scan.complete");
    let v: serde_json::Value = serde_json::from_str(&complete.1).expect("json");
    let packs = v
        .get("packs")
        .and_then(|p| p.as_array())
        .expect("packs array on complete");
    assert!(
        packs.len() >= 5,
        "expected pack audit entries for five packs, got {packs:?}"
    );
    for p in packs {
        assert!(p.get("pack_id").and_then(|x| x.as_str()).is_some());
        assert!(p.get("pack_version").and_then(|x| x.as_u64()).is_some());
    }
}

#[test]
fn offsets_safe_slice() {
    assert!(safe_byte_slice("abc", 0, 99).is_none());
    assert_eq!(safe_byte_slice("abc", 0, 2), Some("ab"));
}

#[test]
fn reset_wipes_and_rescan() {
    let (_tmp, matter) = temp_matter("ent-reset");
    let id = insert_with_text(&matter, "alice@example.com", None);
    let job1 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("j1");
    run_entity_scan(
        &matter,
        &job1.id,
        &EntityScanParams::default(),
        None,
        |_| {},
    )
    .expect("r1");
    assert!(!matter.list_entity_hits(&id).expect("h").is_empty());

    let job2 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("j2");
    run_entity_scan(
        &matter,
        &job2.id,
        &EntityScanParams {
            reset: true,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("r2");
    let hits = matter.list_entity_hits(&id).expect("h2");
    assert!(!hits.is_empty());
}

#[test]
fn truncated_scan_does_not_permanent_skip_on_larger_cap() {
    // PII only appears after the small cap → first scan misses it; larger max must rescan.
    let pad = "x".repeat(200);
    let text = format!("{pad} secret bob@example.com");
    let (_tmp, matter) = temp_matter("ent-trunc");
    let id = insert_with_text(&matter, &text, None);

    let job1 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("j1");
    let o1 = run_entity_scan(
        &matter,
        &job1.id,
        &EntityScanParams {
            reset: false,
            max_text_bytes: 50,
            packs: vec!["email".into()],
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run1");
    match o1 {
        EntityScanOutcome::Succeeded(r) => {
            assert!(r.scanned_count >= 1);
            assert!(r.truncated_count >= 1, "body must truncate under small cap");
        }
        other => panic!("{other:?}"),
    }
    let hits1 = matter.list_entity_hits(&id).expect("h1");
    assert!(
        !hits1.iter().any(|h| h.entity_type == "email"),
        "email is past cap — must not be found yet: {hits1:?}"
    );
    let item1 = matter.get_item(&id).expect("item1");
    let marker1 = item1
        .entity_scanned_text_sha256
        .expect("marker after trunc");
    assert!(
        marker1.contains("trunc=50"),
        "stored fingerprint must record truncation: {marker1}"
    );
    assert!(!marker1.contains("trunc=full"));

    // Same params would still not skip (trunc ≠ full), but larger cap must find the hit.
    let job2 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("j2");
    let o2 = run_entity_scan(
        &matter,
        &job2.id,
        &EntityScanParams {
            reset: false,
            max_text_bytes: 10_000,
            packs: vec!["email".into()],
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run2");
    match o2 {
        EntityScanOutcome::Succeeded(r) => {
            assert!(
                r.scanned_count >= 1,
                "must rescan after prior truncation: {r:?}"
            );
            assert_eq!(r.skipped_count, 0);
        }
        other => panic!("{other:?}"),
    }
    let hits2 = matter.list_entity_hits(&id).expect("h2");
    assert!(
        hits2.iter().any(|h| h.entity_type == "email"),
        "larger cap must surface email past old trunc: {hits2:?}"
    );
    let item2 = matter.get_item(&id).expect("item2");
    let marker2 = item2.entity_scanned_text_sha256.expect("marker full");
    assert!(
        marker2.contains("trunc=full"),
        "full success fingerprint: {marker2}"
    );
}

#[test]
fn missing_cas_does_not_permanent_skip() {
    let (_tmp, matter) = temp_matter("ent-cas-miss");
    // Fake digest not present in CAS.
    let fake = "a".repeat(64);
    let item = matter
        .insert_item(ItemInput {
            path: Some("ghost.txt".into()),
            status: item_status::EXTRACTED.into(),
            subject: Some("no body subject".into()),
            text_sha256: Some(fake.clone()),
            ..Default::default()
        })
        .expect("insert");
    let id = item.id;

    let job1 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("j1");
    let o1 = run_entity_scan(
        &matter,
        &job1.id,
        &EntityScanParams {
            reset: false,
            packs: vec!["email".into()],
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run1");
    match o1 {
        EntityScanOutcome::Succeeded(r) => {
            assert!(r.error_count >= 1, "CAS miss must count as error: {r:?}");
            assert!(r.scanned_count >= 1);
            assert_eq!(r.skipped_count, 0);
        }
        other => panic!("{other:?}"),
    }
    let item1 = matter.get_item(&id).expect("item1");
    let marker1 = item1
        .entity_scanned_text_sha256
        .expect("err fingerprint stored");
    assert!(
        marker1.contains(&format!("body=err:{fake}")),
        "CAS failure fingerprint: {marker1}"
    );

    // Second run with reset:false must NOT skip (err ≠ full body digest).
    let job2 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("j2");
    let o2 = run_entity_scan(
        &matter,
        &job2.id,
        &EntityScanParams {
            reset: false,
            packs: vec!["email".into()],
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run2");
    match o2 {
        EntityScanOutcome::Succeeded(r) => {
            assert_eq!(r.skipped_count, 0, "must not permanent-skip CAS failures");
            assert!(r.scanned_count >= 1, "must re-attempt: {r:?}");
            assert!(r.error_count >= 1);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn pack_set_change_forces_rescan() {
    let (_tmp, matter) = temp_matter("ent-packs");
    let text = "alice@example.com SSN 219-09-9999";
    let id = insert_with_text(&matter, text, None);

    let job1 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("j1");
    run_entity_scan(
        &matter,
        &job1.id,
        &EntityScanParams {
            reset: false,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run1");
    let hits1 = matter.list_entity_hits(&id).expect("h1");
    assert!(hits1.iter().any(|h| h.entity_type == "email"));
    assert!(hits1.iter().any(|h| h.entity_type == "ssn_us"));

    // Narrow packs only — fingerprint packs= differs → must rescan, not skip.
    let job2 = matter.create_job(JOB_KIND_ENTITY_SCAN).expect("j2");
    let o2 = run_entity_scan(
        &matter,
        &job2.id,
        &EntityScanParams {
            reset: false,
            packs: vec!["ssn_us".into()],
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run2");
    match o2 {
        EntityScanOutcome::Succeeded(r) => {
            assert!(r.scanned_count >= 1, "pack change must rescan: {r:?}");
            assert_eq!(r.skipped_count, 0);
        }
        other => panic!("{other:?}"),
    }
    let hits2 = matter.list_entity_hits(&id).expect("h2");
    assert!(
        hits2.iter().any(|h| h.entity_type == "ssn_us"),
        "ssn retained under ssn_us-only pack"
    );
    assert!(
        !hits2.iter().any(|h| h.entity_type == "email"),
        "email hits replaced away when pack set is ssn_us only: {hits2:?}"
    );
}
