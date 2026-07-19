//! Synthetic matter cull tests (spec §3.10).

#![allow(clippy::field_reassign_with_default)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use matter_core::{
    item_cull_status, item_dedup_role, item_near_dup_role, item_role, item_status, ItemInput,
    Matter,
};
use matter_cull::{
    reason, run_cull, CullOutcome, CullParams, CullRules, DateField, DateRule, EmptyRule,
    FamilyPolicy, ListMode, MissingDatePolicy, PathContainsRule, StringListRule, CULL_STAGE,
    JOB_KIND_CULL, PRESET_NOISE_LIGHT, PRESET_UNIQUE_ONLY,
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

fn run_with(matter: &Matter, job_id: &str, params: &CullParams) -> CullOutcome {
    run_cull(matter, job_id, params, None, |_| {}).expect("run")
}

fn reasons_of(matter: &Matter, id: &str) -> Vec<String> {
    let item = matter.get_item(id).unwrap();
    item.cull_reasons_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default()
}

/// 1. unique_only: exact dups culled; uniques included.
#[test]
fn unique_only_culls_exact_duplicates() {
    let (_tmp, matter) = temp_matter("unique-only");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let u = insert(
        &matter,
        "u.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            size_bytes: Some(100),
            ..Default::default()
        },
    );
    let d = insert(
        &matter,
        "d.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            size_bytes: Some(100),
            ..Default::default()
        },
    );

    let params = CullParams {
        preset_name: Some(PRESET_UNIQUE_ONLY.into()),
        ..Default::default()
    };
    let outcome = run_with(&matter, &job.id, &params);
    assert!(matches!(outcome, CullOutcome::Succeeded(_)), "{outcome:?}");

    let iu = matter.get_item(&u).unwrap();
    let id = matter.get_item(&d).unwrap();
    assert_eq!(iu.cull_status.as_deref(), Some(item_cull_status::INCLUDED));
    assert_eq!(id.cull_status.as_deref(), Some(item_cull_status::CULLED));
    assert!(reasons_of(&matter, &d).contains(&reason::EXACT_DUPLICATE.to_string()));
}

/// 2. Date window: in/out/missing with default include.
#[test]
fn date_window_in_out_missing() {
    let (_tmp, matter) = temp_matter("date-win");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let mut rules = CullRules::default();
    rules.exclude_exact_duplicates = false;
    rules.date = DateRule {
        enabled: true,
        field: DateField::SentAt,
        // Eastern midnight 2023-01-01 → 2023-01-01T05:00:00Z
        start: Some("2023-01-01T00:00:00-05:00".into()),
        end: Some("2023-02-01T00:00:00-05:00".into()),
        missing_policy: MissingDatePolicy::Include,
    };

    let in_range = insert(
        &matter,
        "in.eml",
        item_status::EXTRACTED,
        ItemInput {
            sent_at: Some("2023-01-15T12:00:00Z".into()),
            size_bytes: Some(1),
            ..Default::default()
        },
    );
    let out_range = insert(
        &matter,
        "out.eml",
        item_status::EXTRACTED,
        ItemInput {
            // 2022-12-31 20:00 EST = 2023-01-01T01:00:00Z — before start (05:00Z)
            sent_at: Some("2023-01-01T01:00:00Z".into()),
            size_bytes: Some(1),
            ..Default::default()
        },
    );
    let missing = insert(
        &matter,
        "miss.eml",
        item_status::EXTRACTED,
        ItemInput {
            size_bytes: Some(1),
            ..Default::default()
        },
    );

    let params = CullParams {
        rules: Some(rules),
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);

    assert_eq!(
        matter.get_item(&in_range).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
    assert_eq!(
        matter.get_item(&out_range).unwrap().cull_status.as_deref(),
        Some(item_cull_status::CULLED)
    );
    assert!(reasons_of(&matter, &out_range).contains(&reason::DATE_OUT_OF_RANGE.to_string()));
    assert_eq!(
        matter.get_item(&missing).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
}

/// 3. Date TZ: offset works; naive bound → validation error.
#[test]
fn date_tz_offset_and_naive_reject() {
    let (_tmp, matter) = temp_matter("date-tz");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let mut bad = CullRules::default();
    bad.date.enabled = true;
    bad.date.start = Some("2023-01-01T00:00:00".into());
    let params = CullParams {
        rules: Some(bad),
        ..Default::default()
    };
    let err = run_cull(&matter, &job.id, &params, None, |_| {});
    assert!(err.is_err(), "naive bound must fail");
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("naive") || msg.contains("offset") || msg.contains("Invalid"),
        "{msg}"
    );

    // Offset path covered by date_window_in_out_missing.
}

/// 4. Empty zero_size culled when enabled.
#[test]
fn empty_zero_size_culled() {
    let (_tmp, matter) = temp_matter("empty");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let z = insert(
        &matter,
        "z.bin",
        item_status::EXTRACTED,
        ItemInput {
            size_bytes: Some(0),
            ..Default::default()
        },
    );
    let ok = insert(
        &matter,
        "ok.bin",
        item_status::EXTRACTED,
        ItemInput {
            size_bytes: Some(10),
            ..Default::default()
        },
    );

    let mut rules = CullRules::default();
    rules.exclude_exact_duplicates = false;
    rules.empty = EmptyRule {
        enabled: true,
        zero_size: true,
        no_text_and_no_native: false,
    };
    let params = CullParams {
        rules: Some(rules),
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert_eq!(
        matter.get_item(&z).unwrap().cull_status.as_deref(),
        Some(item_cull_status::CULLED)
    );
    assert!(reasons_of(&matter, &z).contains(&reason::EMPTY.to_string()));
    assert_eq!(
        matter.get_item(&ok).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
}

/// 5. Path exclude pattern.
#[test]
fn path_exclude_pattern() {
    let (_tmp, matter) = temp_matter("path");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let bad = insert(
        &matter,
        r"C:\Windows\System32\foo.dll",
        item_status::EXTRACTED,
        ItemInput {
            size_bytes: Some(1),
            ..Default::default()
        },
    );
    let good = insert(
        &matter,
        r"C:\Evidence\mail.pst",
        item_status::EXTRACTED,
        ItemInput {
            size_bytes: Some(1),
            ..Default::default()
        },
    );

    let mut rules = CullRules::default();
    rules.exclude_exact_duplicates = false;
    rules.path_contains = PathContainsRule {
        enabled: true,
        mode: ListMode::Exclude,
        patterns: vec![r"\Windows\".into()],
    };
    let params = CullParams {
        rules: Some(rules),
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert_eq!(
        matter.get_item(&bad).unwrap().cull_status.as_deref(),
        Some(item_cull_status::CULLED)
    );
    assert!(reasons_of(&matter, &bad).contains(&reason::PATH.to_string()));
    assert_eq!(
        matter.get_item(&good).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
}

/// 6. File category include mode.
#[test]
fn file_category_include_mode() {
    let (_tmp, matter) = temp_matter("fcat");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let email = insert(
        &matter,
        "a.eml",
        item_status::EXTRACTED,
        ItemInput {
            file_category: Some("email".into()),
            size_bytes: Some(1),
            ..Default::default()
        },
    );
    let other = insert(
        &matter,
        "b.bin",
        item_status::EXTRACTED,
        ItemInput {
            file_category: Some("executable".into()),
            size_bytes: Some(1),
            ..Default::default()
        },
    );

    let mut rules = CullRules::default();
    rules.exclude_exact_duplicates = false;
    rules.file_categories = StringListRule {
        enabled: true,
        mode: ListMode::Include,
        values: vec!["email".into()],
    };
    let params = CullParams {
        rules: Some(rules),
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert_eq!(
        matter.get_item(&email).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
    assert_eq!(
        matter.get_item(&other).unwrap().cull_status.as_deref(),
        Some(item_cull_status::CULLED)
    );
    assert!(reasons_of(&matter, &other).contains(&reason::FILE_CATEGORY.to_string()));
}

/// noise_light excludes file_category=executable (taxonomy_v1 / 0037).
#[test]
fn noise_light_excludes_executable_category() {
    let (_tmp, matter) = temp_matter("noise_exe");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let good = insert(
        &matter,
        "doc.pdf",
        item_status::EXTRACTED,
        ItemInput {
            file_category: Some("pdf".into()),
            size_bytes: Some(100),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );
    let exe = insert(
        &matter,
        "tool.exe",
        item_status::EXTRACTED,
        ItemInput {
            file_category: Some("executable".into()),
            size_bytes: Some(100),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            ..Default::default()
        },
    );

    let params = CullParams {
        preset_name: Some(PRESET_NOISE_LIGHT.into()),
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert_eq!(
        matter.get_item(&good).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
    assert_eq!(
        matter.get_item(&exe).unwrap().cull_status.as_deref(),
        Some(item_cull_status::CULLED)
    );
    assert!(reasons_of(&matter, &exe).contains(&reason::FILE_CATEGORY.to_string()));
}

/// 7. Near-dup member NOT culled by default.
#[test]
fn near_dup_member_not_culled_by_default() {
    let (_tmp, matter) = temp_matter("neardup");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let m = insert(
        &matter,
        "m.txt",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            near_dup_role: Some(item_near_dup_role::MEMBER.into()),
            size_bytes: Some(10),
            ..Default::default()
        },
    );
    let params = CullParams {
        preset_name: Some(PRESET_UNIQUE_ONLY.into()),
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);
    assert_eq!(
        matter.get_item(&m).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
}

/// 8. Family absolute: included parent + child exact_duplicate → child INCLUDED.
#[test]
fn family_absolute_include_duplicate_child() {
    let (_tmp, matter) = temp_matter("family");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let fam = matter
        .insert_family(matter_core::FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            path: Some("parent.eml".into()),
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            size_bytes: Some(100),
            ..Default::default()
        })
        .expect("parent");
    let child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            path: Some("attach.pdf".into()),
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            size_bytes: Some(50),
            ..Default::default()
        })
        .expect("child");

    let mut rules = CullRules::default();
    rules.exclude_exact_duplicates = true;
    rules.family_policy = FamilyPolicy::KeepChildrenWithIncludedParent;
    let params = CullParams {
        rules: Some(rules),
        ..Default::default()
    };
    let _ = run_with(&matter, &job.id, &params);

    assert_eq!(
        matter.get_item(&parent.id).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
    let ch = matter.get_item(&child.id).unwrap();
    assert_eq!(
        ch.cull_status.as_deref(),
        Some(item_cull_status::INCLUDED),
        "child must be included despite exact_duplicate"
    );
    assert!(
        ch.cull_reasons_json
            .as_deref()
            .map(|j| j == "[]" || j.is_empty())
            .unwrap_or(true),
        "reasons cleared: {:?}",
        ch.cull_reasons_json
    );
}

/// 9. reset:true deterministic recompute.
#[test]
fn reset_true_deterministic_recompute() {
    let (_tmp, matter) = temp_matter("reset");
    let job1 = matter.create_job(JOB_KIND_CULL).expect("job1");
    let job2 = matter.create_job(JOB_KIND_CULL).expect("job2");

    let d = insert(
        &matter,
        "d.eml",
        item_status::EXTRACTED,
        ItemInput {
            dedup_role: Some(item_dedup_role::DUPLICATE.into()),
            size_bytes: Some(1),
            ..Default::default()
        },
    );
    let params = CullParams {
        preset_name: Some(PRESET_UNIQUE_ONLY.into()),
        reset: true,
        ..Default::default()
    };
    let _ = run_with(&matter, &job1.id, &params);
    assert_eq!(
        matter.get_item(&d).unwrap().cull_status.as_deref(),
        Some(item_cull_status::CULLED)
    );
    // Mutate role then re-run — should recompute.
    matter
        .update_item(
            &d,
            matter_core::ItemUpdate {
                dedup_role: Some(Some(item_dedup_role::UNIQUE.into())),
                ..Default::default()
            },
        )
        .expect("upd");
    let _ = run_with(&matter, &job2.id, &params);
    assert_eq!(
        matter.get_item(&d).unwrap().cull_status.as_deref(),
        Some(item_cull_status::INCLUDED)
    );
}

/// 10. Cancel → Paused; resume; same-txn checkpoint.
#[test]
fn cancel_pause_resume_checkpoint() {
    let (_tmp, matter) = temp_matter("cancel");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    // Large n + small batch so cancel after first progress is always mid-run.
    const N: u64 = 50;
    for i in 0..N {
        insert(
            &matter,
            &format!("f{i:03}.bin"),
            item_status::EXTRACTED,
            ItemInput {
                dedup_role: Some(item_dedup_role::UNIQUE.into()),
                size_bytes: Some(1),
                ..Default::default()
            },
        );
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let params = CullParams {
        preset_name: Some(PRESET_UNIQUE_ONLY.into()),
        batch_size: 2,
        ..Default::default()
    };
    let outcome = run_cull(
        &matter,
        &job.id,
        &params,
        Some(&|| cancel_flag2.load(Ordering::SeqCst)),
        |_| {
            // Cancel after first committed batch — next item/batch sees cancel.
            cancel_flag.store(true, Ordering::SeqCst);
        },
    )
    .expect("run");

    let CullOutcome::Paused(s) = outcome else {
        panic!("expected Paused after cancel, got {outcome:?}");
    };
    assert!(
        s.completed_count > 0 && s.completed_count < N,
        "partial progress required for pause: {s:?}"
    );
    let cp = matter
        .get_checkpoint(&job.id, CULL_STAGE)
        .expect("cp")
        .expect("present");
    assert_eq!(cp.completed_count as u64, s.completed_count);

    // Resume with cancel off → Succeeded and every eligible item has cull_status.
    let outcome2 = run_cull(&matter, &job.id, &params, None, |_| {}).expect("resume");
    assert!(
        matches!(outcome2, CullOutcome::Succeeded(_)),
        "{outcome2:?}"
    );
    let all = matter.list_cull_candidates(true).unwrap();
    assert_eq!(all.len() as u64, N);
    for c in all {
        let item = matter.get_item(&c.id).unwrap();
        assert!(
            item.cull_status.is_some(),
            "item {} missing cull_status after resume",
            c.id
        );
    }
}

/// 13. DeNIST: SHA-256 match; missing path fail; MD5-only fail.
#[test]
fn denist_sha256_match_and_format_fail() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("denist");
    let matter = Matter::create(&root, "denist").expect("create");
    let job = matter.create_job(JOB_KIND_CULL).expect("job");

    let digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let id = insert(
        &matter,
        "known.bin",
        item_status::EXTRACTED,
        ItemInput {
            native_sha256: Some(digest.into()),
            size_bytes: Some(1),
            ..Default::default()
        },
    );

    // Missing path when enabled.
    let mut rules = CullRules::default();
    rules.exclude_exact_duplicates = false;
    rules.denist.enabled = true;
    rules.denist.hash_list_path = None;
    let err = run_cull(
        &matter,
        &job.id,
        &CullParams {
            rules: Some(rules.clone()),
            ..Default::default()
        },
        None,
        |_| {},
    );
    assert!(err.is_err(), "missing path must fail");

    // MD5-only list fail.
    let md5_path = base.join("md5.txt");
    std::fs::write(
        md5_path.as_std_path(),
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\ncccccccccccccccccccccccccccccccc\n",
    )
    .unwrap();
    rules.denist.hash_list_path = Some(md5_path.as_str().to_string());
    let job2 = matter.create_job(JOB_KIND_CULL).expect("job2");
    let err = run_cull(
        &matter,
        &job2.id,
        &CullParams {
            rules: Some(rules.clone()),
            reset: true,
            ..Default::default()
        },
        None,
        |_| {},
    );
    assert!(err.is_err());
    let msg = err.unwrap_err().to_string();
    assert!(
        msg.contains("denist_hash_format") || msg.contains("SHA-256"),
        "{msg}"
    );

    // Valid SHA-256 list match.
    let sha_path = base.join("sha.txt");
    std::fs::write(sha_path.as_std_path(), format!("{digest}\n")).unwrap();
    rules.denist.hash_list_path = Some(sha_path.as_str().to_string());
    let job3 = matter.create_job(JOB_KIND_CULL).expect("job3");
    let outcome = run_with(
        &matter,
        &job3.id,
        &CullParams {
            rules: Some(rules),
            reset: true,
            ..Default::default()
        },
    );
    assert!(matches!(outcome, CullOutcome::Succeeded(_)), "{outcome:?}");
    assert_eq!(
        matter.get_item(&id).unwrap().cull_status.as_deref(),
        Some(item_cull_status::CULLED)
    );
    assert!(reasons_of(&matter, &id).contains(&reason::DENIST.to_string()));
}
