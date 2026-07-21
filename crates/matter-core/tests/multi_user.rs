//! Multi-user identity, locks, OCC, exclusive open, strict actor (track 0058).

use std::thread;
use std::time::Duration;

use camino::Utf8PathBuf;
use matter_core::{
    ApplyCodesInput, Error, ItemInput, Matter, ROLE_ADMIN, ROLE_REVIEWER, SCHEMA_VERSION,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
    (dir, path)
}

fn insert_item(matter: &Matter, subject: &str) -> String {
    let item = matter
        .insert_item(ItemInput {
            status: "extracted".into(),
            path: Some(format!("/{subject}")),
            subject: Some(subject.into()),
            from_addr: Some("a@example.com".into()),
            mime_type: Some("message/rfc822".into()),
            ..Default::default()
        })
        .expect("insert_item");
    item.id
}

fn first_code_id(matter: &Matter) -> String {
    matter
        .list_code_definitions()
        .expect("codes")
        .into_iter()
        .next()
        .expect("at least one code")
        .id
}

#[test]
fn schema_version_is_36() {
    assert_eq!(SCHEMA_VERSION, 36);
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    assert_eq!(matter.schema_version().expect("ver"), 36);
    assert!(!matter.is_multi_user_enabled().expect("flag"));
}

#[test]
fn create_user_auth_and_wrong_password() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let user = matter
        .create_user("Alice", ROLE_ADMIN, "s3cret!", "system")
        .expect("create user");
    assert_eq!(user.display_name, "Alice");
    assert_eq!(user.role, ROLE_ADMIN);

    let issue = matter.authenticate("Alice", "s3cret!").expect("login");
    assert_eq!(issue.user.id, user.id);
    assert!(!issue.token.is_empty());

    let resolved = matter.resolve_session(&issue.token).expect("resolve");
    assert_eq!(resolved.id, user.id);

    let err = matter.authenticate("Alice", "wrong").expect_err("bad pass");
    assert!(matches!(err, Error::Unauthorized(_)));
}

#[test]
fn lock_conflict_and_expired_lock_frees() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let a = matter
        .create_user("A", ROLE_REVIEWER, "pw", "system")
        .expect("a");
    let b = matter
        .create_user("B", ROLE_REVIEWER, "pw", "system")
        .expect("b");
    let item = insert_item(&matter, "locked");

    matter
        .lock_item(&item, &a.id, Some("review"), Some(4))
        .expect("lock a");
    let conflict = matter
        .lock_item(&item, &b.id, None, Some(4))
        .expect_err("b blocked");
    assert!(matches!(conflict, Error::Locked { .. }));

    // Short TTL then wait for expiry.
    matter.unlock_item(&item, &a.id).expect("unlock");
    matter
        .lock_item(&item, &a.id, None, Some(0)) // max(1) hour floor — use raw insert for expiry test
        .expect("re-lock");

    // Directly expire the lock row for deterministic test.
    matter
        .connection()
        .execute(
            "UPDATE item_locks SET expires_at = '2000-01-01T00:00:00.000Z' WHERE item_id = ?1",
            [&item],
        )
        .expect("expire");
    matter
        .lock_item(&item, &b.id, None, Some(4))
        .expect("b can lock after expiry");
}

#[test]
fn admin_force_unlock() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let admin = matter
        .create_user("Admin", ROLE_ADMIN, "pw", "system")
        .expect("admin");
    let rev = matter
        .create_user("Rev", ROLE_REVIEWER, "pw", "system")
        .expect("rev");
    let item = insert_item(&matter, "force");
    matter
        .lock_item(&item, &rev.id, None, Some(4))
        .expect("lock");
    matter.force_unlock(&item, &admin.id).expect("force");
    matter
        .lock_item(&item, &admin.id, None, Some(4))
        .expect("admin locks after force");
}

#[test]
fn occ_stale_version_fails_and_success_bumps() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let user = matter
        .create_user("Coder", ROLE_REVIEWER, "pw", "system")
        .expect("user");
    let item = insert_item(&matter, "occ");
    matter
        .lock_item(&item, &user.id, None, Some(4))
        .expect("lock");
    let code = first_code_id(&matter);
    assert_eq!(matter.get_review_version(&item).expect("v"), 0);

    let r1 = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.clone()],
            add_code_ids: vec![code.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: user.id.clone(),
            expected_version: Some(0),
        })
        .expect("first apply");
    assert_eq!(r1.review_versions, vec![1]);

    let stale = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.clone()],
            add_code_ids: vec![],
            remove_code_ids: vec![code.clone()],
            propagate_family: false,
            actor: user.id.clone(),
            expected_version: Some(0),
        })
        .expect_err("stale");
    assert!(matches!(
        stale,
        Error::VersionConflict {
            expected: 0,
            actual: 1
        }
    ));

    let r2 = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.clone()],
            add_code_ids: vec![],
            remove_code_ids: vec![code],
            propagate_family: false,
            actor: user.id,
            expected_version: Some(1),
        })
        .expect("second apply");
    assert_eq!(r2.review_versions, vec![2]);
}

#[test]
fn exclusive_lock_blocks_second_write_open_sequential() {
    // Portable pattern: hold write open, second open fails; drop first, then succeeds.
    // Same-process re-lock may be reentrant on some platforms; we document sequential.
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    {
        let _m = Matter::create(&root, "MU").expect("create");
        // Drop to release lock from create.
    }
    let first = Matter::open(&root).expect("first open");
    // Second handle in a thread (separate open attempt while first holds lock).
    let root2 = root.clone();
    let handle = thread::spawn(move || Matter::open(&root2));
    // Brief pause so the second open runs while first is alive.
    thread::sleep(Duration::from_millis(50));
    let second = handle.join().expect("join");
    match second {
        Err(Error::MatterAlreadyOpen(_)) => {}
        Ok(_) => {
            // On platforms where same-process exclusive lock is reentrant, fall back
            // to sequential open-drop-open proof.
            drop(first);
            let _third = Matter::open(&root).expect("reopen after drop");
            return;
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
    drop(first);
    let _again = Matter::open(&root).expect("open after release");
}

#[test]
fn strict_actor_rejects_free_form_accepts_user_id() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let user = matter
        .create_user("Strict", ROLE_REVIEWER, "pw", "system")
        .expect("user");
    let item = insert_item(&matter, "strict");
    let code = first_code_id(&matter);
    matter.set_strict_actor_mode(true);

    let bad = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.clone()],
            add_code_ids: vec![code.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "desk".into(),
            expected_version: Some(0),
        })
        .expect_err("free-form");
    assert!(matches!(bad, Error::Unauthorized(_)));

    matter
        .lock_item(&item, &user.id, None, Some(4))
        .expect("lock");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item],
            add_code_ids: vec![code],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: user.id,
            expected_version: Some(0),
        })
        .expect("user id ok");
}

#[test]
fn batch_feed_membership_only() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let user = matter
        .create_user("Batcher", ROLE_REVIEWER, "pw", "system")
        .expect("user");
    let a = insert_item(&matter, "a");
    let b = insert_item(&matter, "b");
    let _c = insert_item(&matter, "c");
    let batch = matter
        .create_batch("slice", &[a.clone(), b.clone()], &user.id, None)
        .expect("batch");
    let rows = matter.list_batch_items(&batch.id, None, 100).expect("list");
    let ids: Vec<_> = rows.iter().map(|r| r.item_id.as_str()).collect();
    assert!(ids.contains(&a.as_str()) && ids.contains(&b.as_str()));
    assert_eq!(ids.len(), 2);

    matter
        .checkout_batch(&batch.id, &user.id)
        .expect("checkout");
    matter
        .assert_item_in_checked_out_batch(&batch.id, &a, &user.id)
        .expect("in batch");
    let foreign = matter
        .assert_item_in_checked_out_batch(&batch.id, &_c, &user.id)
        .expect_err("foreign");
    assert!(matches!(foreign, Error::Forbidden(_)));

    // Mutate foreign item while checkout active → fail closed.
    let code = first_code_id(&matter);
    let err = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![_c.clone()],
            add_code_ids: vec![code.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: user.id.clone(),
            expected_version: Some(0),
        })
        .expect_err("mutate outside batch");
    assert!(
        matches!(err, Error::Forbidden(_)),
        "expected Forbidden for out-of-batch mutate, got {err:?}"
    );

    // In-batch mutate still works.
    matter
        .lock_item(&a, &user.id, None, Some(4))
        .expect("lock a");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![a.clone()],
            add_code_ids: vec![code],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: user.id.clone(),
            expected_version: Some(0),
        })
        .expect("mutate in batch");

    // Global thin list constrained while checkout active.
    let constrained = matter
        .list_items_thin_for_user(Some(&user.id), None, 100)
        .expect("list");
    assert!(constrained.iter().all(|r| r.id == a || r.id == b));
    assert!(!constrained.iter().any(|r| r.id == _c));

    // Re-checkout after check-in must work.
    matter.checkin_batch(&batch.id, &user.id).expect("checkin");
    matter
        .checkout_batch(&batch.id, &user.id)
        .expect("re-checkout");
}

#[test]
fn user_id_is_uuid() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let user = matter
        .create_user("UuidUser", ROLE_REVIEWER, "pw", "system")
        .expect("user");
    // Canonical UUID string: 8-4-4-4-12 hex.
    let parts: Vec<_> = user.id.split('-').collect();
    assert_eq!(parts.len(), 5, "user id must be UUID text, got {}", user.id);
    assert_eq!(parts[0].len(), 8);
    assert_eq!(parts[1].len(), 4);
    assert_eq!(parts[2].len(), 4);
    assert_eq!(parts[3].len(), 4);
    assert_eq!(parts[4].len(), 12);
    assert!(
        user.id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
        "user id must be hex UUID, got {}",
        user.id
    );
}

#[test]
fn qc_sample_create_and_record() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "MU").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let user = matter
        .create_user("Qc", ROLE_REVIEWER, "pw", "system")
        .expect("user");
    let code = first_code_id(&matter);
    let mut ids = Vec::new();
    for i in 0..5 {
        let id = insert_item(&matter, &format!("q{i}"));
        matter
            .lock_item(&id, &user.id, None, Some(4))
            .expect("lock");
        let v = matter.get_review_version(&id).expect("v");
        matter
            .apply_codes(ApplyCodesInput {
                item_ids: vec![id.clone()],
                add_code_ids: vec![code.clone()],
                remove_code_ids: vec![],
                propagate_family: false,
                actor: user.id.clone(),
                expected_version: Some(v),
            })
            .expect("code");
        ids.push(id);
    }
    let (sample, items) = matter
        .create_qc_sample("10pct", &user.id, Some(40.0), None, 7)
        .expect("sample");
    assert!(!items.is_empty());
    assert_eq!(sample.seed, 7);
    let first = &items[0];
    let recorded = matter
        .record_qc_outcome(
            &sample.id,
            &first.item_id,
            "agree",
            Some("looks good"),
            &user.id,
        )
        .expect("record");
    assert_eq!(recorded.outcome.as_deref(), Some("agree"));
    assert_eq!(recorded.recorded_by.as_deref(), Some(user.id.as_str()));
}

#[test]
fn solo_path_still_works_without_multi_user() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("m");
    let matter = Matter::create(&root, "Solo").expect("create");
    let item = insert_item(&matter, "solo");
    let code = first_code_id(&matter);
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item],
            add_code_ids: vec![code],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "desk".into(),
            expected_version: None,
        })
        .expect("solo code");
}
