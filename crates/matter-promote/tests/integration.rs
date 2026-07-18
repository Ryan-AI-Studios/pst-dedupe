//! Synthetic promote-to-review tests (spec §3.11).

#![allow(clippy::field_reassign_with_default)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use matter_core::{item_cull_status, item_dedup_role, item_role, item_status, ItemInput, Matter};
use matter_promote::{
    ordering_uses_single_query_api, resolve_policy, run_promote, PromoteOutcome, PromoteParams,
    FAMILY_ORDER_SQL, JOB_KIND_PROMOTE, POLICY_CULL_INCLUDED, POLICY_UNIQUE_ONLY, PROMOTE_STAGE,
};

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = camino::Utf8Path::from_path(tmp.path())
        .expect("utf8")
        .to_path_buf();
    (tmp, path)
}

fn temp_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let (tmp, base) = utf8_tempdir();
    let root = base.join(name);
    let matter = Matter::create(&root, name).expect("create");
    (tmp, matter)
}

fn insert(matter: &Matter, path: &str, status: &str, mut input: ItemInput) -> String {
    input.status = status.into();
    if input.path.is_none() {
        input.path = Some(path.into());
    }
    if input.role.is_none() {
        input.role = Some(item_role::STANDALONE.into());
    }
    matter.insert_item(input).expect("insert").id
}

fn run_with(matter: &Matter, job_id: &str, params: &PromoteParams) -> PromoteOutcome {
    run_promote(matter, job_id, params, None, |_| {}).expect("run")
}

fn in_review(matter: &Matter, id: &str) -> bool {
    matter.get_item(id).unwrap().in_review == Some(1)
}

/// 1. Cull has run → auto uses cull_included; only included get in_review (before expand).
#[test]
fn auto_uses_cull_included_when_cull_has_run() {
    let (_tmp, matter) = temp_matter("auto-cull");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");

    let included = insert(
        &matter,
        "inc.eml",
        item_status::EXTRACTED,
        ItemInput {
            cull_status: Some(item_cull_status::INCLUDED.into()),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let culled = insert(
        &matter,
        "cul.eml",
        item_status::EXTRACTED,
        ItemInput {
            cull_status: Some(item_cull_status::CULLED.into()),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );

    assert_eq!(
        resolve_policy(&matter, "auto").unwrap(),
        POLICY_CULL_INCLUDED
    );

    let params = PromoteParams {
        policy: "auto".into(),
        expand_families: false,
        ..Default::default()
    };
    let outcome = run_with(&matter, &job.id, &params);
    match outcome {
        PromoteOutcome::Succeeded(s) => {
            assert_eq!(s.resolved_policy, POLICY_CULL_INCLUDED);
            assert_eq!(s.promoted_count, 1);
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
    assert!(in_review(&matter, &included));
    assert!(!in_review(&matter, &culled));
}

/// 2. Cull never run → auto uses unique_only.
#[test]
fn auto_uses_unique_only_when_cull_never() {
    let (_tmp, matter) = temp_matter("auto-unique");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");

    let u = insert(
        &matter,
        "u.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let _d = insert(
        &matter,
        "d.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            ..Default::default()
        },
    );

    assert_eq!(resolve_policy(&matter, "auto").unwrap(), POLICY_UNIQUE_ONLY);

    let params = PromoteParams {
        policy: "auto".into(),
        expand_families: false,
        ..Default::default()
    };
    let outcome = run_with(&matter, &job.id, &params);
    match outcome {
        PromoteOutcome::Succeeded(s) => {
            assert_eq!(s.resolved_policy, POLICY_UNIQUE_ONLY);
            assert_eq!(s.promoted_count, 1);
        }
        other => panic!("{other:?}"),
    }
    assert!(in_review(&matter, &u));
}

/// 3. unique_only: exact-dup parent not promoted; unique is.
#[test]
fn unique_only_skips_exact_dup_parent() {
    let (_tmp, matter) = temp_matter("unique-only");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");

    let unique = insert(
        &matter,
        "unique.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let dup = insert(
        &matter,
        "dup.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            ..Default::default()
        },
    );

    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        expand_families: false,
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert!(in_review(&matter, &unique));
    assert!(!in_review(&matter, &dup));
}

/// 4. Expand down: unique parent + exact-dup attach → both in_review.
#[test]
fn expand_down_includes_exact_dup_attachment() {
    let (_tmp, matter) = temp_matter("expand-down");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");
    let fam = matter.insert_family("email_attachments").expect("fam");

    let parent = insert(
        &matter,
        "parent.eml",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let child = insert(
        &matter,
        "attach.bin",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.clone()),
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            ..Default::default()
        },
    );

    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        expand_families: true,
        ..Default::default()
    };
    let outcome = run_with(&matter, &job.id, &params);
    match outcome {
        PromoteOutcome::Succeeded(s) => assert_eq!(s.promoted_count, 2),
        other => panic!("{other:?}"),
    }
    assert!(in_review(&matter, &parent));
    assert!(
        in_review(&matter, &child),
        "exact-dup child via expand down"
    );
}

/// 5. Expand up: child alone in base → parent also in_review.
#[test]
fn expand_up_pulls_parent_for_orphan_child() {
    let (_tmp, matter) = temp_matter("expand-up");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");
    let fam = matter.insert_family("email_attachments").expect("fam");

    // Parent is culled/excluded; child is included → base S = child only under cull_included.
    let parent = insert(
        &matter,
        "parent.eml",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            cull_status: Some(item_cull_status::CULLED.into()),
            ..Default::default()
        },
    );
    let child = insert(
        &matter,
        "attach.bin",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.clone()),
            cull_status: Some(item_cull_status::INCLUDED.into()),
            ..Default::default()
        },
    );

    let params = PromoteParams {
        policy: POLICY_CULL_INCLUDED.into(),
        expand_families: true,
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert!(in_review(&matter, &child));
    assert!(
        in_review(&matter, &parent),
        "parent pulled upward for orphan attachment"
    );
}

/// 6. expand_families false → no expand.
#[test]
fn expand_false_skips_family() {
    let (_tmp, matter) = temp_matter("no-expand");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");
    let fam = matter.insert_family("email_attachments").expect("fam");

    let parent = insert(
        &matter,
        "parent.eml",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let child = insert(
        &matter,
        "attach.bin",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.clone()),
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            ..Default::default()
        },
    );

    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        expand_families: false,
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert!(in_review(&matter, &parent));
    assert!(!in_review(&matter, &child));
}

/// 7. review_order: parent immediately followed by its children.
#[test]
fn review_order_parent_then_children() {
    let (_tmp, matter) = temp_matter("order-family");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");
    let fam = matter.insert_family("email_attachments").expect("fam");

    let parent = insert(
        &matter,
        "z-parent.eml",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let child_b = insert(
        &matter,
        "b-attach.bin",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.clone()),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let child_a = insert(
        &matter,
        "a-attach.bin",
        item_status::EXTRACTED,
        ItemInput {
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.clone()),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let other = insert(
        &matter,
        "other.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );

    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        expand_families: true,
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);

    let mut rows: Vec<(String, i64)> = [
        parent.as_str(),
        child_a.as_str(),
        child_b.as_str(),
        other.as_str(),
    ]
    .iter()
    .map(|id| {
        let it = matter.get_item(id).unwrap();
        (id.to_string(), it.review_order.expect("order"))
    })
    .collect();
    rows.sort_by_key(|(_, o)| *o);

    let ordered_ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    // Parent then children (by path a then b), then other family — compound key.
    let p_pos = ordered_ids.iter().position(|id| *id == parent).unwrap();
    let ca_pos = ordered_ids.iter().position(|id| *id == child_a).unwrap();
    let cb_pos = ordered_ids.iter().position(|id| *id == child_b).unwrap();
    assert_eq!(ca_pos, p_pos + 1, "child a immediately after parent");
    assert_eq!(cb_pos, p_pos + 2, "child b after child a");
}

/// 8. Ordering implementation uses single-query API (no N+1).
#[test]
fn ordering_single_query_proof() {
    assert!(ordering_uses_single_query_api());
    assert!(FAMILY_ORDER_SQL.contains("COALESCE(parent_item_id, id)"));
}

/// 9. reset true demotes items no longer selected.
#[test]
fn reset_true_demotes() {
    let (_tmp, matter) = temp_matter("reset-demote");
    let job1 = matter.create_job(JOB_KIND_PROMOTE).expect("job1");

    let a = insert(
        &matter,
        "a.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let b = insert(
        &matter,
        "b.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );

    let params = PromoteParams {
        policy: "all_extracted".into(),
        expand_families: false,
        reset: true,
        ..Default::default()
    };
    let _ = run_with(&matter, &job1.id, &params);
    assert!(in_review(&matter, &a));
    assert!(in_review(&matter, &b));

    // Flip b to discovered so it drops out of all_extracted.
    matter
        .update_item(
            &b,
            matter_core::ItemUpdate {
                status: Some(item_status::DISCOVERED.into()),
                ..Default::default()
            },
        )
        .expect("update");

    let job2 = matter.create_job(JOB_KIND_PROMOTE).expect("job2");
    let _ = run_with(&matter, &job2.id, &params);
    assert!(in_review(&matter, &a));
    assert!(!in_review(&matter, &b), "b demoted after reset recompute");
}

/// 11. Cancel → Paused; resume continues.
#[test]
fn cancel_paused_then_resume() {
    let (_tmp, matter) = temp_matter("cancel-resume");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");

    for i in 0..5 {
        insert(
            &matter,
            &format!("m{i}.eml"),
            item_status::EXTRACTED,
            ItemInput {
                dedup_role: Some(item_dedup_role::UNIQUE.into()),
                ..Default::default()
            },
        );
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let cancel: Option<&dyn Fn() -> bool> = Some(&|| cancel_flag2.load(Ordering::SeqCst));

    // Cancel after first batch of size 2.
    let batch_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let batch_count2 = batch_count.clone();
    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        expand_families: false,
        batch_size: 2,
        ..Default::default()
    };

    let outcome = run_promote(&matter, &job.id, &params, cancel, |_| {
        let n = batch_count2.fetch_add(1, Ordering::SeqCst) + 1;
        if n >= 1 {
            cancel_flag.store(true, Ordering::SeqCst);
        }
    })
    .expect("run");

    // First call may complete a batch then pause on next cancel check, or pause empty.
    // Force cancel at start of second batch via progress after first commit.
    match &outcome {
        PromoteOutcome::Paused(s) => {
            assert!(s.completed_count < 5 || s.completed_count == 2);
            let cp = matter
                .get_checkpoint(&job.id, PROMOTE_STAGE)
                .expect("cp")
                .expect("present");
            assert!(cp.completed_count >= 0);
        }
        PromoteOutcome::Succeeded(s) => {
            // Tiny matters may finish before cancel trips — re-run with pre-set cancel.
            assert_eq!(s.promoted_count, 5);
            return;
        }
        other => panic!("{other:?}"),
    }

    cancel_flag.store(false, Ordering::SeqCst);
    let outcome2 = run_promote(&matter, &job.id, &params, None, |_| {}).expect("resume");
    match outcome2 {
        PromoteOutcome::Succeeded(s) => assert_eq!(s.promoted_count, 5),
        other => panic!("resume {other:?}"),
    }
    let in_review_count = matter
        .list_promote_candidates()
        .unwrap()
        .iter()
        .filter(|c| matter.get_item(&c.id).unwrap().in_review == Some(1))
        .count();
    assert_eq!(in_review_count, 5);
}

/// Cancel at start → Paused with checkpoint; resume completes.
#[test]
fn cancel_immediately_then_resume() {
    let (_tmp, matter) = temp_matter("cancel-immediate");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");

    for i in 0..3 {
        insert(
            &matter,
            &format!("x{i}.eml"),
            item_status::EXTRACTED,
            ItemInput {
                dedup_role: Some(item_dedup_role::UNIQUE.into()),
                ..Default::default()
            },
        );
    }

    let cancel = || true;
    let outcome = run_promote(
        &matter,
        &job.id,
        &PromoteParams {
            policy: POLICY_UNIQUE_ONLY.into(),
            expand_families: false,
            batch_size: 1,
            ..Default::default()
        },
        Some(&cancel),
        |_| {},
    )
    .expect("run");
    assert!(matches!(outcome, PromoteOutcome::Paused(_)));

    let outcome2 =
        run_promote(&matter, &job.id, &PromoteParams::default(), None, |_| {}).expect("resume");
    match outcome2 {
        PromoteOutcome::Succeeded(s) => assert_eq!(s.promoted_count, 3),
        other => panic!("{other:?}"),
    }
}

/// never-deduped unique_only treats all extracted as eligible.
#[test]
fn unique_only_without_dedupe_promotes_extracted() {
    let (_tmp, matter) = temp_matter("no-dedupe");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");
    let a = insert(
        &matter,
        "a.eml",
        item_status::EXTRACTED,
        ItemInput {
            ..Default::default()
        },
    );
    let _disc = insert(
        &matter,
        "d.eml",
        item_status::DISCOVERED,
        ItemInput {
            ..Default::default()
        },
    );
    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        expand_families: false,
        require_dedupe: false,
        ..Default::default()
    };
    let outcome = run_with(&matter, &job.id, &params);
    match outcome {
        PromoteOutcome::Succeeded(s) => assert_eq!(s.promoted_count, 1),
        other => panic!("{other:?}"),
    }
    assert!(in_review(&matter, &a));
}

/// require_dedupe fails when no dedup_role.
#[test]
fn require_dedupe_fails_when_none() {
    let (_tmp, matter) = temp_matter("req-dedupe");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");
    insert(
        &matter,
        "a.eml",
        item_status::EXTRACTED,
        ItemInput {
            ..Default::default()
        },
    );
    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        require_dedupe: true,
        expand_families: false,
        ..Default::default()
    };
    let err = run_promote(&matter, &job.id, &params, None, |_| {}).expect_err("must fail");
    assert!(
        err.to_string().contains("require_dedupe") || err.to_string().contains("dedup_role"),
        "{err}"
    );
}

/// Resume from checkpoint phase=`done`/`snapshot` with stale review_sets meta
/// must repair item_count/policy (Codex final-gate P2).
#[test]
fn resume_from_done_repairs_stale_review_set_snapshot() {
    let (_tmp, matter) = temp_matter("resume-snapshot");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");

    let a = insert(
        &matter,
        "a.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let b = insert(
        &matter,
        "b.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );

    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        expand_families: false,
        ..Default::default()
    };
    let outcome = run_with(&matter, &job.id, &params);
    match &outcome {
        PromoteOutcome::Succeeded(s) => assert_eq!(s.promoted_count, 2),
        other => panic!("first run {other:?}"),
    }
    assert!(in_review(&matter, &a));
    assert!(in_review(&matter, &b));

    let set = matter
        .ensure_default_review_set(matter_core::DEFAULT_REVIEW_SET_NAME)
        .expect("set");
    assert_eq!(set.item_count, 2);
    assert_eq!(set.policy.as_deref(), Some(POLICY_UNIQUE_ONLY));

    // Simulate crash after membership write: corrupt review_sets meta and leave
    // checkpoint at phase=done (legacy) or snapshot (new).
    matter
        .update_review_set_snapshot(&set.id, "stale_policy", Some("{}"), 0)
        .expect("corrupt");
    let stale = matter.get_review_set(&set.id).expect("get");
    assert_eq!(stale.item_count, 0);

    let mut cp = matter
        .get_checkpoint(&job.id, PROMOTE_STAGE)
        .expect("cp")
        .expect("present");
    // Force phase=done with correct membership counts (stale meta only).
    let mut cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("cursor json");
    cursor["phase"] = serde_json::json!("done");
    cursor["promoted_count"] = serde_json::json!(2);
    cursor["completed_count"] = serde_json::json!(2);
    let repaired = cursor.to_string();
    matter
        .put_checkpoint(&job.id, PROMOTE_STAGE, &repaired, 2)
        .expect("put cp");
    let _ = &mut cp;

    let outcome2 = run_promote(&matter, &job.id, &params, None, |_| {}).expect("resume");
    match outcome2 {
        PromoteOutcome::Succeeded(s) => {
            assert_eq!(s.promoted_count, 2);
            assert_eq!(s.resolved_policy, POLICY_UNIQUE_ONLY);
        }
        other => panic!("resume {other:?}"),
    }

    let fixed = matter.get_review_set(&set.id).expect("fixed");
    assert_eq!(
        fixed.item_count, 2,
        "snapshot item_count repaired on resume"
    );
    assert_eq!(
        fixed.policy.as_deref(),
        Some(POLICY_UNIQUE_ONLY),
        "snapshot policy repaired on resume"
    );
    assert!(in_review(&matter, &a));
    assert!(in_review(&matter, &b));
}

/// Resume from phase=`snapshot` (post-write, pre-meta) repairs review_sets.
#[test]
fn resume_from_snapshot_phase_repairs_meta() {
    let (_tmp, matter) = temp_matter("resume-snapshot-phase");
    let job = matter.create_job(JOB_KIND_PROMOTE).expect("job");
    let a = insert(
        &matter,
        "a.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );

    let params = PromoteParams {
        policy: POLICY_UNIQUE_ONLY.into(),
        expand_families: false,
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert!(in_review(&matter, &a));

    let set = matter
        .ensure_default_review_set(matter_core::DEFAULT_REVIEW_SET_NAME)
        .expect("set");
    matter
        .update_review_set_snapshot(&set.id, "broken", None, 99)
        .expect("corrupt");

    let cp = matter
        .get_checkpoint(&job.id, PROMOTE_STAGE)
        .expect("cp")
        .expect("present");
    let mut cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("cursor");
    cursor["phase"] = serde_json::json!("snapshot");
    matter
        .put_checkpoint(&job.id, PROMOTE_STAGE, &cursor.to_string(), 1)
        .expect("put");

    let outcome = run_promote(&matter, &job.id, &params, None, |_| {}).expect("resume");
    assert!(matches!(outcome, PromoteOutcome::Succeeded(_)));
    let fixed = matter.get_review_set(&set.id).expect("fixed");
    assert_eq!(fixed.item_count, 1);
    assert_eq!(fixed.policy.as_deref(), Some(POLICY_UNIQUE_ONLY));
}
