//! Integration tests for matter-dedupe (synthetic items only).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use camino::Utf8PathBuf;
use matter_core::{
    item_dedup_role, item_dedup_tier, item_role, item_status, ItemInput, Matter,
    FAMILY_KIND_EMAIL_ATTACHMENTS,
};
use matter_dedupe::{
    logical_hash_key, message_id_key, run_dedupe, CompactKey, DedupeOutcome, DedupeParams,
    FamilyPolicy,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
    (dir, path)
}

fn make_matter(base: &Utf8PathBuf, name: &str) -> Matter {
    let root = base.join(name);
    Matter::create(&root, name).expect("create")
}

fn email_parent(
    matter: &Matter,
    path: &str,
    mid: Option<&str>,
    logical: Option<&str>,
) -> matter_core::Item {
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            file_category: Some("email".into()),
            path: Some(path.into()),
            message_id: mid.map(|s| s.into()),
            logical_hash: logical.map(|s| s.into()),
            logical_hash_version: if logical.is_some() { Some(1) } else { None },
            ..Default::default()
        })
        .expect("parent")
}

fn attach(
    matter: &Matter,
    parent_id: &str,
    family_id: &str,
    path: &str,
    native: Option<&str>,
    size: i64,
) -> matter_core::Item {
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            file_category: Some("attachment".into()),
            parent_item_id: Some(parent_id.into()),
            family_id: Some(family_id.into()),
            path: Some(path.into()),
            native_sha256: native.map(|s| s.into()),
            size_bytes: Some(size),
            ..Default::default()
        })
        .expect("attach")
}

fn run_default(matter: &Matter, job_id: &str) -> DedupeOutcome {
    let params = DedupeParams {
        batch_size: 10,
        ..DedupeParams::default()
    };
    run_dedupe(matter, job_id, &params, None, |_| {}).expect("run")
}

#[test]
fn same_mid_one_unique_one_duplicate() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "mid");
    let job = matter.create_job("dedupe").expect("job");

    let a = email_parent(
        &matter,
        "src1/a",
        Some("<MID1@ex.com>"),
        Some(&"a".repeat(64)),
    );
    let b = email_parent(
        &matter,
        "src2/b",
        Some("mid1@ex.com"),
        Some(&"b".repeat(64)),
    );

    let out = run_default(&matter, &job.id);
    assert!(matches!(out, DedupeOutcome::Succeeded(_)));

    let a2 = matter.get_item(&a.id).unwrap();
    let b2 = matter.get_item(&b.id).unwrap();
    assert_eq!(a2.dedup_role.as_deref(), Some(item_dedup_role::UNIQUE));
    assert_eq!(a2.dedup_tier.as_deref(), Some(item_dedup_tier::MESSAGE_ID));
    assert_eq!(b2.dedup_role.as_deref(), Some(item_dedup_role::DUPLICATE));
    assert_eq!(b2.duplicate_of_item_id.as_deref(), Some(a.id.as_str()));
    assert_eq!(b2.dedup_tier.as_deref(), Some(item_dedup_tier::MESSAGE_ID));
}

#[test]
fn empty_mid_same_logical_duplicate() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "logical");
    let job = matter.create_job("dedupe").expect("job");
    let lh = "c".repeat(64);

    let a = email_parent(&matter, "p1", None, Some(&lh));
    let b = email_parent(&matter, "p2", None, Some(&lh));

    let _ = run_default(&matter, &job.id);
    let a2 = matter.get_item(&a.id).unwrap();
    let b2 = matter.get_item(&b.id).unwrap();
    assert_eq!(a2.dedup_role.as_deref(), Some(item_dedup_role::UNIQUE));
    assert_eq!(
        a2.dedup_tier.as_deref(),
        Some(item_dedup_tier::LOGICAL_HASH)
    );
    assert_eq!(b2.dedup_role.as_deref(), Some(item_dedup_role::DUPLICATE));
    assert_eq!(
        b2.dedup_tier.as_deref(),
        Some(item_dedup_tier::LOGICAL_HASH)
    );
    assert_eq!(b2.duplicate_of_item_id.as_deref(), Some(a.id.as_str()));
}

#[test]
fn different_logical_no_mid_both_unique() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "diff");
    let job = matter.create_job("dedupe").expect("job");

    let a = email_parent(&matter, "p1", None, Some(&"d".repeat(64)));
    let b = email_parent(&matter, "p2", None, Some(&"e".repeat(64)));

    let _ = run_default(&matter, &job.id);
    assert_eq!(
        matter.get_item(&a.id).unwrap().dedup_role.as_deref(),
        Some(item_dedup_role::UNIQUE)
    );
    assert_eq!(
        matter.get_item(&b.id).unwrap().dedup_role.as_deref(),
        Some(item_dedup_role::UNIQUE)
    );
}

#[test]
fn same_mid_different_logical_policy_a_conflict() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "conflict");
    let job = matter.create_job("dedupe").expect("job");

    let _a = email_parent(&matter, "p1", Some("same@ex.com"), Some(&"1".repeat(64)));
    let _b = email_parent(&matter, "p2", Some("same@ex.com"), Some(&"2".repeat(64)));

    match run_default(&matter, &job.id) {
        DedupeOutcome::Succeeded(s) => {
            assert!(s.mid_logical_conflicts >= 1);
            assert_eq!(s.unique, 1);
            assert_eq!(s.duplicate, 1);
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
}

#[test]
fn family_parent_dup_marks_children() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "family");
    let job = matter.create_job("dedupe").expect("job");

    let fam1 = matter
        .insert_family(FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam1");
    let fam2 = matter
        .insert_family(FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam2");

    let p1 = email_parent(&matter, "p1", Some("fam@ex.com"), None);
    // link parent to family
    matter
        .set_item_family_role(&p1.id, Some(&fam1.id), item_role::PARENT, None)
        .expect("role");
    let p2 = email_parent(&matter, "p2", Some("fam@ex.com"), None);
    matter
        .set_item_family_role(&p2.id, Some(&fam2.id), item_role::PARENT, None)
        .expect("role");

    let digest = "aa".repeat(32);
    let c1 = attach(&matter, &p1.id, &fam1.id, "p1/file.pdf", Some(&digest), 100);
    let c2 = attach(&matter, &p2.id, &fam2.id, "p2/file.pdf", Some(&digest), 100);

    let _ = run_default(&matter, &job.id);

    let p2r = matter.get_item(&p2.id).unwrap();
    assert_eq!(p2r.dedup_role.as_deref(), Some(item_dedup_role::DUPLICATE));

    let c2r = matter.get_item(&c2.id).unwrap();
    assert_eq!(c2r.dedup_role.as_deref(), Some(item_dedup_role::DUPLICATE));
    assert_eq!(c2r.dedup_tier.as_deref(), Some(item_dedup_tier::FAMILY));
    assert_eq!(c2r.duplicate_of_item_id.as_deref(), Some(c1.id.as_str()));
    // Graph intact
    assert_eq!(c2r.parent_item_id.as_deref(), Some(p2.id.as_str()));
    assert_eq!(c2r.family_id.as_deref(), Some(fam2.id.as_str()));
    assert!(
        matter.get_item(&c1.id).unwrap().dedup_role.is_none()
            || matter.get_item(&c1.id).unwrap().dedup_role.as_deref()
                != Some(item_dedup_role::DUPLICATE)
            || matter.get_item(&c1.id).unwrap().dedup_tier.as_deref()
                != Some(item_dedup_tier::FAMILY)
    );
}

#[test]
fn family_attach_name_size_when_digest_differs() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "attach-namesize");
    let job = matter.create_job("dedupe").expect("job");

    let fam1 = matter.insert_family("").unwrap();
    let fam2 = matter.insert_family("").unwrap();
    let p1 = email_parent(&matter, "p1", Some("ns@ex.com"), None);
    matter
        .set_item_family_role(&p1.id, Some(&fam1.id), item_role::PARENT, None)
        .unwrap();
    let p2 = email_parent(&matter, "p2", Some("ns@ex.com"), None);
    matter
        .set_item_family_role(&p2.id, Some(&fam2.id), item_role::PARENT, None)
        .unwrap();

    let c1 = attach(
        &matter,
        &p1.id,
        &fam1.id,
        "p1/Report.DOCX",
        Some(&"11".repeat(32)),
        2048,
    );
    let c2 = attach(
        &matter,
        &p2.id,
        &fam2.id,
        "p2/report.docx",
        Some(&"22".repeat(32)),
        2048,
    );

    let _ = run_default(&matter, &job.id);
    let c2r = matter.get_item(&c2.id).unwrap();
    assert_eq!(c2r.duplicate_of_item_id.as_deref(), Some(c1.id.as_str()));
    assert_ne!(c2r.duplicate_of_item_id.as_deref(), Some(p1.id.as_str()));
    assert_eq!(c2r.dedup_tier.as_deref(), Some(item_dedup_tier::FAMILY));
}

#[test]
fn family_attach_unmatched_null_not_parent() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "attach-unmatched");
    let job = matter.create_job("dedupe").expect("job");

    let fam1 = matter.insert_family("").unwrap();
    let fam2 = matter.insert_family("").unwrap();
    let p1 = email_parent(&matter, "p1", Some("um@ex.com"), None);
    matter
        .set_item_family_role(&p1.id, Some(&fam1.id), item_role::PARENT, None)
        .unwrap();
    let p2 = email_parent(&matter, "p2", Some("um@ex.com"), None);
    matter
        .set_item_family_role(&p2.id, Some(&fam2.id), item_role::PARENT, None)
        .unwrap();

    let _c1 = attach(
        &matter,
        &p1.id,
        &fam1.id,
        "p1/a.pdf",
        Some(&"aa".repeat(32)),
        10,
    );
    let c2 = attach(
        &matter,
        &p2.id,
        &fam2.id,
        "p2/totally-different.bin",
        Some(&"bb".repeat(32)),
        999,
    );

    let _ = run_default(&matter, &job.id);
    let c2r = matter.get_item(&c2.id).unwrap();
    assert_eq!(c2r.dedup_role.as_deref(), Some(item_dedup_role::DUPLICATE));
    assert_eq!(c2r.dedup_tier.as_deref(), Some(item_dedup_tier::FAMILY));
    assert!(c2r.duplicate_of_item_id.is_none());
    assert_ne!(c2r.duplicate_of_item_id.as_deref(), Some(p1.id.as_str()));
    let extra = c2r.extra_json.unwrap_or_default();
    assert!(
        extra.contains("family_attach_unmatched"),
        "extra_json={extra}"
    );
}

#[test]
fn cancel_mid_batch_paused_then_resume() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "cancel");
    let job = matter.create_job("dedupe").expect("job");

    for i in 0..20 {
        email_parent(
            &matter,
            &format!("p{i:02}"),
            Some(&format!("m{i}@ex.com")),
            Some(&format!("{i:064}")),
        );
    }

    let params = DedupeParams {
        batch_size: 3,
        ..DedupeParams::default()
    };

    // Cancel after progress advances a bit — trip after first few cancel polls.
    let checks = AtomicU64::new(0);
    let cancel_flag = AtomicBool::new(false);
    let out = run_dedupe(
        &matter,
        &job.id,
        &params,
        Some(&|| {
            let n = checks.fetch_add(1, Ordering::SeqCst);
            // Allow first few parent resolutions then cancel.
            if n > 5 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
            cancel_flag.load(Ordering::SeqCst)
        }),
        |_| {},
    )
    .expect("run");

    match out {
        DedupeOutcome::Paused(s) => {
            assert!(s.completed_count < 20);
        }
        other => panic!("expected Paused, got {other:?}"),
    }

    // Resume: reset=false so we don't wipe committed work; but our engine
    // only resets when fresh. Resume with same params reset=true still skips
    // reset because checkpoint cursor > 0.
    let out2 = run_dedupe(&matter, &job.id, &params, None, |_| {}).expect("resume");
    match out2 {
        DedupeOutcome::Succeeded(s) => {
            assert_eq!(s.completed_count, 20);
            assert_eq!(s.unique, 20);
        }
        other => panic!("expected Succeeded on resume, got {other:?}"),
    }
}

#[test]
fn transactional_batch_roles_and_checkpoint() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "txn");
    let job = matter.create_job("dedupe").expect("job");

    email_parent(&matter, "p1", Some("t@ex.com"), None);
    email_parent(&matter, "p2", Some("t@ex.com"), None);

    let _ = run_default(&matter, &job.id);
    let cp = matter
        .get_checkpoint(&job.id, "dedupe")
        .unwrap()
        .expect("checkpoint");
    assert!(cp.completed_count >= 2);
    let counts = matter.count_by_dedup_role().unwrap();
    assert_eq!(counts.unique, 1);
    assert!(counts.duplicate >= 1);
}

#[test]
fn compact_key_types_fixed_size() {
    assert_eq!(std::mem::size_of::<CompactKey>(), 32);
    let k = logical_hash_key(&"ab".repeat(32)).unwrap();
    assert_eq!(k.len(), 32);
    let m = message_id_key("<X@Y.com>").unwrap();
    assert_eq!(m.len(), 32);
}

#[test]
fn empty_keys_unique_none() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "empty");
    let job = matter.create_job("dedupe").expect("job");

    let a = email_parent(&matter, "p1", None, None);
    let b = email_parent(&matter, "p2", Some(""), Some(""));

    let _ = run_default(&matter, &job.id);
    let a2 = matter.get_item(&a.id).unwrap();
    let b2 = matter.get_item(&b.id).unwrap();
    assert_eq!(a2.dedup_role.as_deref(), Some(item_dedup_role::UNIQUE));
    assert_eq!(a2.dedup_tier.as_deref(), Some(item_dedup_tier::NONE));
    assert_eq!(b2.dedup_role.as_deref(), Some(item_dedup_role::UNIQUE));
    assert_eq!(b2.dedup_tier.as_deref(), Some(item_dedup_tier::NONE));
}

#[test]
fn parents_only_skips_attachments() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "parents-only");
    let job = matter.create_job("dedupe").expect("job");
    let fam1 = matter.insert_family("").unwrap();
    let fam2 = matter.insert_family("").unwrap();
    let p1 = email_parent(&matter, "p1", Some("po@ex.com"), None);
    matter
        .set_item_family_role(&p1.id, Some(&fam1.id), item_role::PARENT, None)
        .unwrap();
    let p2 = email_parent(&matter, "p2", Some("po@ex.com"), None);
    matter
        .set_item_family_role(&p2.id, Some(&fam2.id), item_role::PARENT, None)
        .unwrap();
    let c2 = attach(
        &matter,
        &p2.id,
        &fam2.id,
        "p2/x.pdf",
        Some(&"cc".repeat(32)),
        1,
    );

    let params = DedupeParams {
        family_policy: FamilyPolicy::ParentsOnly,
        batch_size: 10,
        ..DedupeParams::default()
    };
    let _ = run_dedupe(&matter, &job.id, &params, None, |_| {}).unwrap();
    assert!(matter.get_item(&c2.id).unwrap().dedup_role.is_none());
}

/// Family pass: seed a mid-family checkpoint with some attaches already marked,
/// then resume and assert summary.duplicate matches DB (no double-count on reprocess).
#[test]
fn family_cancel_mid_parent_resume_counts_match_db() {
    use matter_core::DedupRoleUpdate;
    use serde_json::json;

    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "family-cancel");
    let job = matter.create_job("dedupe").expect("job");

    let fam1 = matter.insert_family("").unwrap();
    let fam2 = matter.insert_family("").unwrap();
    let p1 = email_parent(&matter, "p1", Some("fc@ex.com"), None);
    matter
        .set_item_family_role(&p1.id, Some(&fam1.id), item_role::PARENT, None)
        .unwrap();
    let p2 = email_parent(&matter, "p2", Some("fc@ex.com"), None);
    matter
        .set_item_family_role(&p2.id, Some(&fam2.id), item_role::PARENT, None)
        .unwrap();

    // Many attaches so family batch commits mid-parent with batch_size=2.
    let mut p2_children = Vec::new();
    for i in 0..6 {
        attach(
            &matter,
            &p1.id,
            &fam1.id,
            &format!("p1/a{i}.bin"),
            Some(&format!("{:0>64}", i)),
            10 + i,
        );
        p2_children.push(attach(
            &matter,
            &p2.id,
            &fam2.id,
            &format!("p2/a{i}.bin"),
            Some(&format!("{:0>64}", i + 100)),
            10 + i,
        ));
    }

    // Finish parent pass fully first (no cancel).
    let params = DedupeParams {
        batch_size: 2,
        ..DedupeParams::default()
    };
    // Force parents-only first to assign parent roles without family.
    let parent_only = DedupeParams {
        family_policy: FamilyPolicy::ParentsOnly,
        batch_size: 2,
        ..DedupeParams::default()
    };
    let out_parents = run_dedupe(&matter, &job.id, &parent_only, None, |_| {}).expect("parents");
    assert!(matches!(out_parents, DedupeOutcome::Succeeded(_)));

    // Simulate crash mid-family: mark first 2 attaches as family dups and write
    // a family-phase checkpoint with family_cursor=0 (reprocess current parent).
    let now = "2026-01-01T00:00:00Z";
    let mut staged = Vec::new();
    for c in p2_children.iter().take(2) {
        staged.push(DedupRoleUpdate {
            item_id: c.id.clone(),
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            duplicate_of_item_id: None,
            dedup_tier: Some(item_dedup_tier::FAMILY.into()),
            dedup_group_id: Some(p1.id.clone()),
            deduped_at: Some(now.into()),
            dedup_job_id: Some(job.id.clone()),
            extra_json: None,
        });
    }
    // Parent pass left unique=1 duplicate=1 for parents; family already counted 2.
    let cursor = json!({
        "cursor_index": 2,
        "completed_count": 2,
        "unique": 1,
        "duplicate": 3, // 1 parent dup + 2 family attaches already counted
        "skipped": 0,
        "mid_logical_conflicts": 0,
        "phase": "family",
        "family_cursor": 0,
        "params": params
    });
    matter
        .apply_dedup_batch_with_checkpoint(&job.id, "dedupe", &staged, &cursor.to_string(), 2)
        .expect("seed checkpoint");

    // Resume with suppress-children policy — reprocesses all 6 attaches of p2;
    // first 2 must not inflate duplicate count.
    let out2 = run_dedupe(&matter, &job.id, &params, None, |_| {}).expect("resume family");
    match out2 {
        DedupeOutcome::Succeeded(s) => {
            let counts = matter.count_by_dedup_role().unwrap();
            assert_eq!(
                s.duplicate, counts.duplicate,
                "summary.duplicate must match DB after family resume (no double-count); summary={s:?} db={counts:?}"
            );
            assert_eq!(s.unique, counts.unique);
            // 1 parent dup + 6 family attaches
            assert_eq!(s.duplicate, 7);
            assert_eq!(s.unique, 1);
        }
        other => panic!("expected Succeeded on resume, got {other:?}"),
    }

    let children = matter.list_attachments(&p2.id).unwrap();
    assert_eq!(children.len(), 6);
    for c in children {
        assert_eq!(c.dedup_role.as_deref(), Some(item_dedup_role::DUPLICATE));
        assert_eq!(c.dedup_tier.as_deref(), Some(item_dedup_tier::FAMILY));
    }
}

/// Unmatched family attach sets flag; a later reset run that can match must clear it.
#[test]
fn family_match_clears_stale_unmatched_flag() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "stale-unmatched");
    let job = matter.create_job("dedupe").expect("job");

    let fam1 = matter.insert_family("").unwrap();
    let fam2 = matter.insert_family("").unwrap();
    let p1 = email_parent(&matter, "p1", Some("st@ex.com"), None);
    matter
        .set_item_family_role(&p1.id, Some(&fam1.id), item_role::PARENT, None)
        .unwrap();
    let p2 = email_parent(&matter, "p2", Some("st@ex.com"), None);
    matter
        .set_item_family_role(&p2.id, Some(&fam2.id), item_role::PARENT, None)
        .unwrap();

    // First: no twin on canonical → unmatched.
    let c2 = attach(
        &matter,
        &p2.id,
        &fam2.id,
        "p2/report.pdf",
        Some(&"bb".repeat(32)),
        42,
    );

    let _ = run_default(&matter, &job.id);
    let c2r = matter.get_item(&c2.id).unwrap();
    assert!(
        c2r.extra_json
            .as_deref()
            .unwrap_or("")
            .contains("family_attach_unmatched"),
        "first run should mark unmatched"
    );

    // Add matching attach on unique parent and re-run with reset.
    let _c1 = attach(
        &matter,
        &p1.id,
        &fam1.id,
        "p1/report.pdf",
        Some(&"aa".repeat(32)), // different digest
        42,                     // same size + name → name+size match
    );

    let job2 = matter.create_job("dedupe").expect("job2");
    let _ = run_default(&matter, &job2.id);
    let c2r2 = matter.get_item(&c2.id).unwrap();
    let twin_id = matter
        .list_attachments(&p1.id)
        .unwrap()
        .into_iter()
        .find(|a| a.path.as_deref() == Some("p1/report.pdf"))
        .expect("twin attach")
        .id;
    assert_eq!(c2r2.duplicate_of_item_id.as_deref(), Some(twin_id.as_str()));
    let extra = c2r2.extra_json.unwrap_or_default();
    assert!(
        !extra.contains("family_attach_unmatched"),
        "matched re-run must strip stale unmatched flag; extra={extra}"
    );
}

/// Non-empty but invalid checkpoint cursor is a hard error (not a fresh start).
#[test]
fn corrupt_checkpoint_is_error_not_fresh_start() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "corrupt-cp");
    let job = matter.create_job("dedupe").expect("job");
    email_parent(&matter, "p1", Some("c@ex.com"), None);

    matter
        .put_checkpoint(&job.id, "dedupe", "NOT-VALID-JSON{{{", 0)
        .expect("put garbage checkpoint");

    let params = DedupeParams {
        batch_size: 10,
        ..DedupeParams::default()
    };
    let err = run_dedupe(&matter, &job.id, &params, None, |_| {}).expect_err("corrupt");
    let msg = err.to_string();
    assert!(
        msg.contains("corrupt checkpoint"),
        "expected corrupt checkpoint error, got: {msg}"
    );
}

/// Resume freezes params from checkpoint even when call-site params differ.
/// `use_message_id: false` in checkpoint must not collapse on MID alone when
/// logical hashes differ.
#[test]
fn resume_prefers_checkpoint_params_over_call() {
    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "resume-params");
    let job = matter.create_job("dedupe").expect("job");

    let a = email_parent(&matter, "p1", Some("same@ex.com"), Some(&"1".repeat(64)));
    let b = email_parent(&matter, "p2", Some("same@ex.com"), Some(&"2".repeat(64)));

    // Seed a fresh-looking checkpoint that freezes non-default params.
    let frozen = DedupeParams {
        use_message_id: false,
        use_logical_hash: true,
        family_policy: FamilyPolicy::ParentsOnly,
        reset: false,
        batch_size: 10,
    };
    let cursor = serde_json::json!({
        "cursor_index": 0,
        "completed_count": 0,
        "unique": 0,
        "duplicate": 0,
        "skipped": 0,
        "mid_logical_conflicts": 0,
        "phase": "parents",
        "family_cursor": 0,
        "params": frozen
    });
    matter
        .put_checkpoint(&job.id, "dedupe", &cursor.to_string(), 0)
        .expect("seed checkpoint");

    // Call with defaults (use_message_id: true) — checkpoint must win.
    let call = DedupeParams {
        use_message_id: true,
        batch_size: 10,
        ..DedupeParams::default()
    };
    let out = run_dedupe(&matter, &job.id, &call, None, |_| {}).expect("run");
    assert!(
        matches!(out, DedupeOutcome::Succeeded(_)),
        "expected Succeeded, got {out:?}"
    );

    let a2 = matter.get_item(&a.id).unwrap();
    let b2 = matter.get_item(&b.id).unwrap();
    // Different logical hashes + MID disabled → both unique (no MID collapse).
    assert_eq!(a2.dedup_role.as_deref(), Some(item_dedup_role::UNIQUE));
    assert_eq!(b2.dedup_role.as_deref(), Some(item_dedup_role::UNIQUE));
    assert_eq!(
        a2.dedup_tier.as_deref(),
        Some(item_dedup_tier::LOGICAL_HASH)
    );
    assert_eq!(
        b2.dedup_tier.as_deref(),
        Some(item_dedup_tier::LOGICAL_HASH)
    );

    // Checkpoint should still record frozen params.
    let cp = matter
        .get_checkpoint(&job.id, "dedupe")
        .unwrap()
        .expect("cp");
    let v: serde_json::Value = serde_json::from_str(&cp.cursor_json).unwrap();
    assert_eq!(v["params"]["use_message_id"], false);
    assert_eq!(v["params"]["family_policy"], "parents_only");
}

/// After mid-parent family batch commit, cancel is polled and can pause.
#[test]
fn family_cancel_after_mid_parent_batch_pauses() {
    use matter_core::DedupRoleUpdate;
    use serde_json::json;

    let (_tmp, base) = utf8_tempdir();
    let matter = make_matter(&base, "family-cancel-poll");
    let job = matter.create_job("dedupe").expect("job");

    let fam1 = matter.insert_family("").unwrap();
    let fam2 = matter.insert_family("").unwrap();
    let p1 = email_parent(&matter, "p1", Some("fcp@ex.com"), None);
    matter
        .set_item_family_role(&p1.id, Some(&fam1.id), item_role::PARENT, None)
        .unwrap();
    let p2 = email_parent(&matter, "p2", Some("fcp@ex.com"), None);
    matter
        .set_item_family_role(&p2.id, Some(&fam2.id), item_role::PARENT, None)
        .unwrap();

    for i in 0..8 {
        attach(
            &matter,
            &p1.id,
            &fam1.id,
            &format!("p1/a{i}.bin"),
            Some(&format!("{:0>64}", i)),
            10 + i,
        );
        attach(
            &matter,
            &p2.id,
            &fam2.id,
            &format!("p2/a{i}.bin"),
            Some(&format!("{:0>64}", i + 100)),
            10 + i,
        );
    }

    // Finish parent pass only (frozen params stay suppress so family can resume).
    let params = DedupeParams {
        batch_size: 2,
        family_policy: FamilyPolicy::ParentsOnly,
        ..DedupeParams::default()
    };
    let _ = run_dedupe(&matter, &job.id, &params, None, |_| {}).expect("parents");

    // Seed family-phase checkpoint with suppress policy (frozen params) so resume
    // enters family pass; parents already assigned.
    let family_params = DedupeParams {
        batch_size: 2,
        reset: false,
        family_policy: FamilyPolicy::SuppressChildrenWithParent,
        ..DedupeParams::default()
    };
    let cursor = json!({
        "cursor_index": 2,
        "completed_count": 2,
        "unique": 1,
        "duplicate": 1,
        "skipped": 0,
        "mid_logical_conflicts": 0,
        "phase": "family",
        "family_cursor": 0,
        "params": family_params
    });
    matter
        .apply_dedup_batch_with_checkpoint(
            &job.id,
            "dedupe",
            &[] as &[DedupRoleUpdate],
            &cursor.to_string(),
            2,
        )
        .expect("seed family checkpoint");

    // Cancel after first post-commit progress tick (mid-parent batch with batch_size=2).
    let cancel_flag = AtomicBool::new(false);
    let out = run_dedupe(
        &matter,
        &job.id,
        &family_params,
        Some(&|| cancel_flag.load(Ordering::SeqCst)),
        |_| {
            // First progress after a family batch commit → request cancel so the
            // post-commit cancel poll path returns Paused.
            cancel_flag.store(true, Ordering::SeqCst);
        },
    )
    .expect("family run");

    match out {
        DedupeOutcome::Paused(s) => {
            assert!(s.completed_count >= 2, "should have finished parents");
        }
        other => panic!("expected Paused after mid-family cancel, got {other:?}"),
    }
}
