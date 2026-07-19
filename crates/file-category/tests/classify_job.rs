//! Integration tests for the resumable `classify` job (track 0037).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use file_category::{
    run_classify, Category, ClassifyOutcome, ClassifyParams, JOB_KIND_CLASSIFY, TAXONOMY_V1,
};
use matter_core::{item_role, item_status, ApplyClassificationInput, ItemInput, Matter};

fn temp_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    let matter = Matter::create(&root, name).expect("create");
    (tmp, matter)
}

fn insert(matter: &Matter, path: &str, category: Option<&str>) -> String {
    let item = matter
        .insert_item(ItemInput {
            path: Some(path.into()),
            status: item_status::EXTRACTED.into(),
            file_category: category.map(|s| s.into()),
            role: Some(item_role::ATTACHMENT.into()),
            size_bytes: Some(10),
            ..Default::default()
        })
        .expect("insert");
    item.id
}

#[test]
fn legacy_attachment_classified() {
    let (_tmp, matter) = temp_matter("attach");
    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let id = insert(&matter, "report.pdf", Some("attachment"));

    let outcome =
        run_classify(&matter, &job.id, &ClassifyParams::default(), None, |_| {}).expect("run");
    assert!(matches!(outcome, ClassifyOutcome::Succeeded(_)));

    let item = matter.get_item(&id).expect("get");
    assert_eq!(item.file_category.as_deref(), Some(Category::Pdf.as_str()));
    assert_eq!(item.category_taxonomy.as_deref(), Some(TAXONOMY_V1));
    assert_eq!(item.role.as_deref(), Some(item_role::ATTACHMENT));
    assert_eq!(item.category_status.as_deref(), Some("ok"));
}

#[test]
fn idempotent_skip_when_taxonomy_v1() {
    let (_tmp, matter) = temp_matter("idem");
    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let id = insert(&matter, "notes.txt", Some("attachment"));

    let o1 =
        run_classify(&matter, &job.id, &ClassifyParams::default(), None, |_| {}).expect("run1");
    assert!(matches!(o1, ClassifyOutcome::Succeeded(_)));
    let item1 = matter.get_item(&id).expect("get1");
    assert_eq!(
        item1.file_category.as_deref(),
        Some(Category::Document.as_str())
    );
    let method1 = item1.category_method.clone();

    // Non-force list omits already taxonomy_v1 decisive categories — second run
    // has nothing to do (no full-table walk / no synthetic skip counts).
    let job2 = matter.create_job(JOB_KIND_CLASSIFY).expect("job2");
    let o2 =
        run_classify(&matter, &job2.id, &ClassifyParams::default(), None, |_| {}).expect("run2");
    match o2 {
        ClassifyOutcome::Succeeded(s) => {
            assert_eq!(s.classified_count, 0, "should not reclassify");
            assert_eq!(s.completed_count, 0, "decisive taxonomy_v1 not listed");
        }
        other => panic!("unexpected {other:?}"),
    }
    let item2 = matter.get_item(&id).expect("get2");
    assert_eq!(item2.category_method, method1);
    assert_eq!(
        item2.file_category.as_deref(),
        Some(Category::Document.as_str())
    );
}

#[test]
fn force_overwrites() {
    let (_tmp, matter) = temp_matter("force");
    // Seed a decisive wrong category: taxonomy_v1 document on a spreadsheet path.
    // Without force this would be skipped; force must reclassify to spreadsheet.
    let id = insert(&matter, "sheet.xlsx", Some(Category::Document.as_str()));
    matter
        .apply_classification(ApplyClassificationInput {
            item_id: id.clone(),
            force: true,
            category: Category::Document.as_str().into(),
            method: "extension".into(),
            taxonomy: TAXONOMY_V1.into(),
            mime_type: None,
            status: Some("ok".into()),
            error: None,
        })
        .expect("seed wrong category");
    let seeded = matter.get_item(&id).expect("seeded");
    assert_eq!(
        seeded.file_category.as_deref(),
        Some(Category::Document.as_str())
    );
    assert_eq!(seeded.category_taxonomy.as_deref(), Some(TAXONOMY_V1));

    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let params = ClassifyParams {
        force: true,
        ..Default::default()
    };
    let o = run_classify(&matter, &job.id, &params, None, |_| {}).expect("force");
    match o {
        ClassifyOutcome::Succeeded(s) => {
            assert!(
                s.classified_count >= 1,
                "force must reclassify decisive wrong category"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let item = matter.get_item(&id).expect("get");
    assert_eq!(
        item.file_category.as_deref(),
        Some(Category::Spreadsheet.as_str()),
        "force must overwrite document → spreadsheet from sheet.xlsx"
    );
    assert_eq!(item.category_taxonomy.as_deref(), Some(TAXONOMY_V1));
}

#[test]
fn cancel_between_items() {
    let (_tmp, matter) = temp_matter("cancel");
    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let _a = insert(&matter, "a.pdf", Some("attachment"));
    let _b = insert(&matter, "b.docx", Some("attachment"));
    let _c = insert(&matter, "c.xlsx", Some("attachment"));

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel_flag);
    let outcome = run_classify(
        &matter,
        &job.id,
        &ClassifyParams {
            batch_size: 1,
            ..Default::default()
        },
        Some(&|| flag.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");

    match outcome {
        ClassifyOutcome::Paused(s) => {
            assert!(s.completed_count >= 1);
            assert!(s.completed_count < 3);
        }
        ClassifyOutcome::Succeeded(s) => {
            // Cancel might race after last item; accept success if all done.
            assert_eq!(s.completed_count, 3);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn no_cas_or_text_mutation() {
    let (_tmp, matter) = temp_matter("nocas");
    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let digest = matter.put_bytes(b"%PDF-1.4 hello").expect("put");
    let item = matter
        .insert_item(ItemInput {
            path: Some("invoice.docx".into()),
            native_sha256: Some(digest.clone()),
            text_sha256: Some(digest.clone()), // dummy text pointer
            status: item_status::EXTRACTED.into(),
            file_category: Some("attachment".into()),
            role: Some(item_role::ATTACHMENT.into()),
            size_bytes: Some(14),
            ..Default::default()
        })
        .expect("insert");

    let _ = run_classify(
        &matter,
        &job.id,
        &ClassifyParams {
            use_magic: true,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run");

    let after = matter.get_item(&item.id).expect("get");
    assert_eq!(after.native_sha256.as_deref(), Some(digest.as_str()));
    assert_eq!(after.text_sha256.as_deref(), Some(digest.as_str()));
    assert_eq!(after.role.as_deref(), Some(item_role::ATTACHMENT));
    // Magic PDF beats lying .docx extension.
    assert_eq!(after.file_category.as_deref(), Some(Category::Pdf.as_str()));
}

#[test]
fn msg_extension_is_email() {
    let (_tmp, matter) = temp_matter("msg");
    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let id = insert(&matter, "note.msg", Some("attachment"));
    let _ = run_classify(&matter, &job.id, &ClassifyParams::default(), None, |_| {}).expect("run");
    assert_eq!(
        matter.get_item(&id).unwrap().file_category.as_deref(),
        Some(Category::Email.as_str())
    );
}

#[test]
fn resume_after_last_item_id_does_not_reprocess() {
    use file_category::CLASSIFY_STAGE;

    let (_tmp, matter) = temp_matter("resume");
    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let a = insert(&matter, "a.pdf", Some("attachment"));
    let b = insert(&matter, "b.docx", Some("attachment"));
    let c = insert(&matter, "c.xlsx", Some("attachment"));

    // Order by id for keyset; pause after first completed item.
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel_flag);
    let paused = run_classify(
        &matter,
        &job.id,
        &ClassifyParams {
            batch_size: 1,
            ..Default::default()
        },
        Some(&|| flag.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("pause run");

    // If cancel raced past everything, skip resume assertions.
    if matches!(paused, ClassifyOutcome::Succeeded(_)) {
        return;
    }
    let ClassifyOutcome::Paused(s) = paused else {
        panic!("expected pause, got {paused:?}");
    };
    assert!(s.completed_count >= 1);

    let cp = matter
        .get_checkpoint(&job.id, CLASSIFY_STAGE)
        .expect("cp")
        .expect("checkpoint present");
    let cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("cursor json");
    let last = cursor["last_item_id"]
        .as_str()
        .expect("last_item_id set")
        .to_string();

    // Finish job from checkpoint.
    let resumed =
        run_classify(&matter, &job.id, &ClassifyParams::default(), None, |_| {}).expect("resume");
    match resumed {
        ClassifyOutcome::Succeeded(s) => {
            assert_eq!(
                s.completed_count, 3,
                "all three processed across pause+resume"
            );
        }
        other => panic!("expected success on resume, got {other:?}"),
    }

    // All three classified; the item at last_item_id was not re-listed as pending after resume
    // (already taxonomy_v1 decisive after first pass).
    for id in [&a, &b, &c] {
        let item = matter.get_item(id).expect("get");
        assert_eq!(item.category_taxonomy.as_deref(), Some(TAXONOMY_V1));
        assert!(item
            .file_category
            .as_deref()
            .is_some_and(|c| { c != "attachment" && !c.is_empty() }));
    }
    // Keyset: listing after checkpoint last_id with non-force should not return last itself.
    let after = matter
        .list_classify_candidates(Some(&last), 100, false, false)
        .expect("after");
    assert!(!after.iter().any(|c| c.id == last));
}

#[test]
fn classify_fail_audit_includes_summary_keys() {
    use file_category::CLASSIFY_STAGE;

    let (_tmp, matter) = temp_matter("fail-audit");
    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let _ = insert(&matter, "x.pdf", Some("attachment"));

    // Corrupt checkpoint → load_prior fails → classify.fail with default summary keys.
    matter
        .put_checkpoint(&job.id, CLASSIFY_STAGE, "{not-json", 0)
        .expect("put corrupt cp");

    let err = run_classify(&matter, &job.id, &ClassifyParams::default(), None, |_| {});
    assert!(err.is_err(), "corrupt checkpoint must fail");

    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events WHERE action = 'classify.fail' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("fail audit row");
    let v: serde_json::Value = serde_json::from_str(&params_json).expect("params json");
    for key in [
        "error",
        "completed_count",
        "classified_count",
        "skipped_count",
        "error_count",
        "by_category",
        "by_method",
    ] {
        assert!(v.get(key).is_some(), "fail audit missing key {key}: {v}");
    }
}

/// Mid-run operational error must keep partial by_category/method counts in fail audit.
#[test]
fn mid_run_fail_preserves_partial_counts() {
    use std::sync::atomic::AtomicU64;

    let (_tmp, matter) = temp_matter("mid-fail");
    let job = matter.create_job(JOB_KIND_CLASSIFY).expect("job");
    let _a = insert(&matter, "a.pdf", Some("attachment"));
    let b = insert(&matter, "b.pdf", Some("attachment"));

    // After first item succeeds, delete the second so process_one fails mid-batch.
    let progress_hits = AtomicU64::new(0);
    let outcome = run_classify(
        &matter,
        &job.id,
        &ClassifyParams {
            batch_size: 10,
            ..ClassifyParams::default()
        },
        None,
        |_| {
            let n = progress_hits.fetch_add(1, Ordering::SeqCst) + 1;
            if n == 1 {
                let _ = matter
                    .connection()
                    .execute("DELETE FROM items WHERE id = ?1", [&b]);
            }
        },
    )
    .expect("run returns Ok(Failed) not Err");

    match outcome {
        ClassifyOutcome::Failed { summary, .. } => {
            assert!(
                summary.completed_count >= 1 && summary.classified_count >= 1,
                "expected partial progress: {summary:?}"
            );
            assert!(
                summary.by_category.values().any(|&c| c >= 1),
                "by_category should retain partial counts: {:?}",
                summary.by_category
            );
        }
        other => panic!("expected Failed with partial summary, got {other:?}"),
    }

    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events WHERE action = 'classify.fail' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("fail audit");
    let v: serde_json::Value = serde_json::from_str(&params_json).expect("json");
    assert!(
        v["completed_count"].as_u64().unwrap_or(0) >= 1,
        "fail audit must not zero out partial completed_count: {v}"
    );
    assert!(
        v["by_category"].as_object().is_some_and(|m| !m.is_empty()),
        "fail audit by_category must retain partial counts: {v}"
    );
}
