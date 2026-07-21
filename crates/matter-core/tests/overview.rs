//! Integration tests for case overview (schema v19 / track 0038).

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use matter_core::{
    item_cull_status, item_dedup_role, item_role, item_status, load_case_overview,
    load_case_overview_on, privilege_basis, privilege_status, ApplyCodesInput, CullFieldUpdate,
    DedupRoleUpdate, ItemErrorInput, ItemInput, JobState, Matter, OverviewOptions,
    UpsertItemPrivilegeInput, SCHEMA_VERSION,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

#[test]
fn schema_v19_on_create() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-v19");
    let matter = Matter::create(&root, "V19").expect("create");
    assert_eq!(SCHEMA_VERSION, 29);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    assert_eq!(matter.info().expect("info").schema_version, SCHEMA_VERSION);

    for idx in [
        "idx_items_matter_file_category",
        "idx_items_matter_custodian",
        "idx_items_matter_role",
    ] {
        let has: bool = matter
            .connection()
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name=?1",
                [idx],
                |row| row.get(0),
            )
            .expect("idx");
        assert!(has, "expected index {idx}");
    }
}

#[test]
fn empty_matter_overview_zeros() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-empty-ov");
    let matter = Matter::create(&root, "Empty").expect("create");

    let ov = load_case_overview_on(&matter, &OverviewOptions::default()).expect("ov");
    assert_eq!(ov.totals.items_total, 0);
    assert_eq!(ov.totals.size_bytes_top_level, 0);
    assert_eq!(ov.totals.sources_total, 0);
    assert_eq!(ov.totals.top_level_items, 0);
    assert_eq!(ov.totals.families_total, 0);
    assert!(ov.by_status.is_empty());
    assert!(ov.by_file_category.is_empty());
    assert!(ov.by_custodian.is_empty());
    assert_eq!(ov.dedup.unique, 0);
    assert!(ov.cull.never_run);
    assert_eq!(ov.review.in_review, 0);
    assert_eq!(ov.review.reviewed_count, 0);
    assert_eq!(ov.review.unreviewed_count, 0);
    assert_eq!(ov.privilege.claimed, 0);
    assert_eq!(ov.privilege.withhold, 0);
    assert_eq!(ov.ocr.pdf_needs_ocr, 0);
    assert_eq!(ov.errors.total, 0);
    assert!(ov.errors.by_code.is_empty());
    assert!(ov.jobs.recent.is_empty());
    assert!(!ov.generated_at.is_empty());

    // Concurrent path also works on empty.
    drop(matter);
    let ov2 = load_case_overview(&root, &OverviewOptions::default()).expect("fanout");
    assert_eq!(ov2.totals.items_total, 0);
}

#[test]
fn mixed_seed_matrix_rollups() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-mixed-ov");
    let matter = Matter::create(&root, "Mixed").expect("create");

    matter
        .insert_source(r"C:\exports\synth", "folder", "imported", None)
        .expect("source");

    // Parent email large + two attachments (size must exclude attaches).
    let family = matter.insert_family("").expect("family");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            file_category: Some("email".into()),
            custodian: Some("Alice".into()),
            size_bytes: Some(5_000_000),
            path: Some("alice/msg.eml".into()),
            ..Default::default()
        })
        .expect("parent");
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            file_category: Some("pdf".into()),
            custodian: Some("Alice".into()),
            size_bytes: Some(1_000_000),
            path: Some("alice/a.pdf".into()),
            ..Default::default()
        })
        .expect("att1");
    matter
        .insert_item(ItemInput {
            status: item_status::PARTIAL.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            file_category: Some("image".into()),
            custodian: Some("Alice".into()),
            size_bytes: Some(500_000),
            path: Some("alice/img.png".into()),
            ..Default::default()
        })
        .expect("att2");

    // Standalone with null category/custodian.
    let standalone = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            file_category: None,
            custodian: None,
            size_bytes: Some(100),
            path: Some("orphan.bin".into()),
            ..Default::default()
        })
        .expect("standalone");

    // Another standalone Bob / email, error status.
    let bob = matter
        .insert_item(ItemInput {
            status: item_status::ERROR.into(),
            role: Some(item_role::STANDALONE.into()),
            file_category: Some("email".into()),
            custodian: Some("Bob".into()),
            size_bytes: Some(200),
            path: Some("bob/err.eml".into()),
            ..Default::default()
        })
        .expect("bob");

    // Dedup roles
    matter
        .apply_dedup_batch_with_checkpoint(
            &matter.create_job("dedupe").expect("job").id,
            "dedupe",
            &[
                DedupRoleUpdate {
                    item_id: parent.id.clone(),
                    dedup_role: Some(item_dedup_role::UNIQUE.into()),
                    duplicate_of_item_id: None,
                    dedup_tier: None,
                    dedup_group_id: None,
                    deduped_at: None,
                    dedup_job_id: None,
                    extra_json: None,
                },
                DedupRoleUpdate {
                    item_id: bob.id.clone(),
                    dedup_role: Some(item_dedup_role::DUPLICATE.into()),
                    duplicate_of_item_id: Some(parent.id.clone()),
                    dedup_tier: None,
                    dedup_group_id: None,
                    deduped_at: None,
                    dedup_job_id: None,
                    extra_json: None,
                },
            ],
            "{}",
            2,
        )
        .expect("dedup");

    // Cull
    let cull_job = matter.create_job("cull").expect("cull job");
    matter
        .apply_cull_batch_with_checkpoint(
            &cull_job.id,
            "cull",
            &[
                CullFieldUpdate {
                    item_id: parent.id.clone(),
                    cull_status: Some(item_cull_status::INCLUDED.into()),
                    cull_reasons_json: None,
                    cull_preset_id: None,
                    cull_preset_name: None,
                    culled_at: None,
                    cull_job_id: None,
                },
                CullFieldUpdate {
                    item_id: bob.id.clone(),
                    cull_status: Some(item_cull_status::CULLED.into()),
                    cull_reasons_json: None,
                    cull_preset_id: None,
                    cull_preset_name: None,
                    culled_at: None,
                    cull_job_id: None,
                },
            ],
            "{}",
            2,
        )
        .expect("cull");

    // Promote to review: parent coded, standalone uncoded.
    matter
        .connection()
        .execute(
            "UPDATE items SET in_review = 1, review_order = 1 WHERE id = ?1",
            [&parent.id],
        )
        .expect("in_review parent");
    matter
        .connection()
        .execute(
            "UPDATE items SET in_review = 1, review_order = 2 WHERE id = ?1",
            [&standalone.id],
        )
        .expect("in_review standalone");

    let defs = matter.list_code_definitions().expect("defs");
    let responsive = defs
        .iter()
        .find(|d| d.key == "responsive")
        .expect("responsive");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![parent.id.clone()],
            add_code_ids: vec![responsive.id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("code");

    // pdf_needs_ocr + privilege withhold
    matter
        .connection()
        .execute(
            "UPDATE items SET pdf_needs_ocr = 1 WHERE id = ?1",
            [&standalone.id],
        )
        .expect("ocr flag");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: parent.id.clone(),
            basis: privilege_basis::ATTORNEY_CLIENT.into(),
            description: "synthetic claim".into(),
            status: privilege_status::ASSERTED.into(),
            withhold: true,
            include_on_log: true,
            actor: "tester".into(),
        })
        .expect("priv");

    // Item errors
    matter
        .record_item_error(ItemErrorInput {
            item_id: Some(bob.id.clone()),
            source_id: None,
            job_id: None,
            stage: "extract".into(),
            code: "encrypted_pdf".into(),
            message: "encrypted".into(),
            detail: None,
        })
        .expect("err1");
    matter
        .record_item_error(ItemErrorInput {
            item_id: Some(bob.id.clone()),
            source_id: None,
            job_id: None,
            stage: "extract".into(),
            code: "encrypted_pdf".into(),
            message: "encrypted again".into(),
            detail: None,
        })
        .expect("err2");
    matter
        .record_item_error(ItemErrorInput {
            item_id: Some(standalone.id.clone()),
            source_id: None,
            job_id: None,
            stage: "extract".into(),
            code: "corrupt_zip".into(),
            message: "zip".into(),
            detail: None,
        })
        .expect("err3");

    let ov = load_case_overview_on(&matter, &OverviewOptions::default()).expect("ov");

    // Totals: 5 items, top-level = parent + standalone + bob = 3
    assert_eq!(ov.totals.items_total, 5);
    assert_eq!(ov.totals.top_level_items, 3);
    assert_eq!(ov.totals.families_total, 1);
    assert_eq!(ov.totals.sources_total, 1);
    // Size: 5_000_000 + 100 + 200 = 5_000_300 (excludes 1M + 500k attaches)
    assert_eq!(ov.totals.size_bytes_top_level, 5_000_300);

    // Categories: email=2, pdf=1, image=1, uncategorized=1
    let cat = |name: &str| {
        ov.by_file_category
            .iter()
            .find(|c| c.label == name)
            .map(|c| c.count)
            .unwrap_or(0)
    };
    assert_eq!(cat("email"), 2);
    assert_eq!(cat("pdf"), 1);
    assert_eq!(cat("image"), 1);
    assert_eq!(cat(""), 1, "null category as empty label");

    // Custodians: Alice=3, Bob=1, none=1
    let cust = |name: &str| {
        ov.by_custodian
            .iter()
            .find(|c| c.label == name)
            .map(|c| c.count)
            .unwrap_or(0)
    };
    assert_eq!(cust("Alice"), 3);
    assert_eq!(cust("Bob"), 1);
    assert_eq!(cust(""), 1);

    // Status
    let st = |name: &str| {
        ov.by_status
            .iter()
            .find(|c| c.label == name)
            .map(|c| c.count)
            .unwrap_or(0)
    };
    assert_eq!(st(item_status::EXTRACTED), 3); // parent + att1 + standalone
    assert_eq!(st(item_status::PARTIAL), 1);
    assert_eq!(st(item_status::ERROR), 1);

    // Dedup
    assert_eq!(ov.dedup.unique, 1);
    assert_eq!(ov.dedup.duplicate, 1);

    // Cull
    assert!(!ov.cull.never_run);
    assert_eq!(ov.cull.included, 1);
    assert_eq!(ov.cull.culled, 1);

    // Review progress
    assert_eq!(ov.review.in_review, 2);
    assert_eq!(ov.review.reviewed_count, 1);
    assert_eq!(ov.review.unreviewed_count, 1);
    assert_eq!(
        ov.review.reviewed_count + ov.review.unreviewed_count,
        ov.review.in_review
    );

    // Privilege / OCR
    assert_eq!(ov.privilege.claimed, 1);
    assert_eq!(ov.privilege.withhold, 1);
    assert_eq!(ov.ocr.pdf_needs_ocr, 1);

    // Errors
    assert_eq!(ov.errors.total, 3);
    let enc = ov
        .errors
        .by_code
        .iter()
        .find(|c| c.label == "encrypted_pdf")
        .expect("enc");
    assert_eq!(enc.count, 2);
    let zip = ov
        .errors
        .by_code
        .iter()
        .find(|c| c.label == "corrupt_zip")
        .expect("zip");
    assert_eq!(zip.count, 1);
}

#[test]
fn size_excludes_attachments() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-size-ov");
    let matter = Matter::create(&root, "Size").expect("create");

    let family = matter.insert_family("").expect("family");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            size_bytes: Some(10_000),
            path: Some("p.pst".into()),
            ..Default::default()
        })
        .expect("parent");
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            size_bytes: Some(9_999),
            path: Some("child.bin".into()),
            ..Default::default()
        })
        .expect("child");

    let ov = load_case_overview_on(&matter, &OverviewOptions::default()).expect("ov");
    assert_eq!(ov.totals.size_bytes_top_level, 10_000);
    assert_eq!(ov.totals.items_total, 2);
    assert_eq!(ov.totals.top_level_items, 1);
}

#[test]
fn review_progress_coded_vs_uncoded() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-review-ov");
    let matter = Matter::create(&root, "Review").expect("create");

    let a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            path: Some("a".into()),
            ..Default::default()
        })
        .expect("a");
    let b = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            path: Some("b".into()),
            ..Default::default()
        })
        .expect("b");
    let c = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            path: Some("c".into()),
            ..Default::default()
        })
        .expect("c");

    for (id, order) in [(&a.id, 1i64), (&b.id, 2), (&c.id, 3)] {
        matter
            .connection()
            .execute(
                "UPDATE items SET in_review = 1, review_order = ?1 WHERE id = ?2",
                rusqlite::params![order, id],
            )
            .expect("promote");
    }

    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![a.id.clone(), b.id.clone()],
            add_code_ids: vec![hot.id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("codes");

    let ov = load_case_overview_on(&matter, &OverviewOptions::default()).expect("ov");
    assert_eq!(ov.review.in_review, 3);
    assert_eq!(ov.review.reviewed_count, 2);
    assert_eq!(ov.review.unreviewed_count, 1);
}

#[test]
fn errors_by_code_top_n() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-err-ov");
    let matter = Matter::create(&root, "Errors").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::ERROR.into(),
            path: Some("x".into()),
            ..Default::default()
        })
        .expect("item");

    for (code, n) in [("aaa", 5usize), ("bbb", 3), ("ccc", 1), ("ddd", 1)] {
        for _ in 0..n {
            matter
                .record_item_error(ItemErrorInput {
                    item_id: Some(item.id.clone()),
                    source_id: None,
                    job_id: None,
                    stage: "s".into(),
                    code: code.into(),
                    message: "m".into(),
                    detail: None,
                })
                .expect("err");
        }
    }

    let opts = OverviewOptions {
        top_error_codes: 2,
        ..Default::default()
    };
    let ov = load_case_overview_on(&matter, &opts).expect("ov");
    assert_eq!(ov.errors.total, 10);
    assert_eq!(ov.errors.by_code.len(), 2);
    assert_eq!(ov.errors.by_code[0].label, "aaa");
    assert_eq!(ov.errors.by_code[0].count, 5);
    assert_eq!(ov.errors.by_code[1].label, "bbb");
    assert_eq!(ov.errors.by_code[1].count, 3);
    assert_eq!(ov.errors.other_codes_count, 2); // ccc + ddd
}

#[test]
fn top_n_zero_honors_empty_lists() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-topn-zero");
    let matter = Matter::create(&root, "TopNZero").expect("create");

    for i in 0..3 {
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                file_category: Some(format!("cat{i}")),
                path: Some(format!("f{i}")),
                ..Default::default()
            })
            .expect("item");
    }
    let item = matter
        .insert_item(ItemInput {
            status: item_status::ERROR.into(),
            path: Some("err".into()),
            ..Default::default()
        })
        .expect("err item");
    matter
        .record_item_error(ItemErrorInput {
            item_id: Some(item.id.clone()),
            source_id: None,
            job_id: None,
            stage: "s".into(),
            code: "code_a".into(),
            message: "m".into(),
            detail: None,
        })
        .expect("err");
    matter
        .record_item_error(ItemErrorInput {
            item_id: Some(item.id),
            source_id: None,
            job_id: None,
            stage: "s".into(),
            code: "code_b".into(),
            message: "m".into(),
            detail: None,
        })
        .expect("err2");

    let opts = OverviewOptions {
        top_categories: 0,
        top_custodians: 0,
        top_error_codes: 0,
        ..Default::default()
    };
    let ov = load_case_overview_on(&matter, &opts).expect("ov");

    // Categories: empty top list, remainder = all items (4).
    assert!(ov.by_file_category.is_empty());
    assert_eq!(ov.other_categories_count, 4);

    // Error codes: empty top list, remainder = total errors (2).
    assert_eq!(ov.errors.total, 2);
    assert!(ov.errors.by_code.is_empty());
    assert_eq!(ov.errors.other_codes_count, 2);
}

#[test]
fn withhold_counts_privilege_table_when_item_cache_drifted() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-withhold-drift");
    let matter = Matter::create(&root, "WithholdDrift").expect("create");

    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            path: Some("drifted.eml".into()),
            ..Default::default()
        })
        .expect("item");

    // Seed privilege-table withhold without using upsert (which maintains the cache).
    let now = "2020-01-01T00:00:00Z";
    matter
        .connection()
        .execute(
            "INSERT INTO item_privilege (\
               item_id, matter_id, basis, description, status, withhold, \
               include_on_log, asserted_at, asserted_by, updated_at, updated_by) \
             VALUES (?1, ?2, ?3, 'drift claim', ?4, 1, 1, ?5, 'tester', ?5, 'tester')",
            rusqlite::params![
                item.id,
                matter.id(),
                privilege_basis::ATTORNEY_CLIENT,
                privilege_status::ASSERTED,
                now,
            ],
        )
        .expect("insert privilege row");

    // Force denormalized cache to 0 (drifted vs privilege table).
    matter
        .connection()
        .execute(
            "UPDATE items SET privilege_withhold = 0 WHERE id = ?1",
            [&item.id],
        )
        .expect("clear cache flag");

    let flag: i64 = matter
        .connection()
        .query_row(
            "SELECT privilege_withhold FROM items WHERE id = ?1",
            [&item.id],
            |row| row.get(0),
        )
        .expect("flag");
    assert_eq!(flag, 0, "precondition: item cache must be drifted to 0");

    let priv_ov = matter.overview_privilege().expect("privilege overview");
    assert!(
        priv_ov.withhold >= 1,
        "withhold must count privilege-table withhold even when item flag is 0; got {}",
        priv_ov.withhold
    );
    assert_eq!(priv_ov.claimed, 1);
}

#[test]
fn top_n_categories_capped() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-topn-ov");
    let matter = Matter::create(&root, "TopN").expect("create");

    for i in 0..10 {
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                file_category: Some(format!("cat{i:02}")),
                path: Some(format!("f{i}")),
                // Vary counts: cat00 gets 10 items, cat01 gets 9, ...
                ..Default::default()
            })
            .expect("item");
        // Add extras for lower index to ensure stable order.
        for _ in 0..(10 - i) {
            matter
                .insert_item(ItemInput {
                    status: item_status::EXTRACTED.into(),
                    file_category: Some(format!("cat{i:02}")),
                    path: Some(format!("f{i}-extra")),
                    ..Default::default()
                })
                .expect("extra");
        }
    }

    let opts = OverviewOptions {
        top_categories: 3,
        ..Default::default()
    };
    let ov = load_case_overview_on(&matter, &opts).expect("ov");
    assert_eq!(ov.by_file_category.len(), 3);
    assert!(ov.other_categories_count > 0);
    // Highest count first
    assert!(ov.by_file_category[0].count >= ov.by_file_category[1].count);
    assert!(ov.by_file_category[1].count >= ov.by_file_category[2].count);
}

#[test]
fn null_category_and_custodian_labels() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-null-labels");
    let matter = Matter::create(&root, "Nulls").expect("create");
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            file_category: None,
            custodian: None,
            path: Some("n".into()),
            ..Default::default()
        })
        .expect("item");

    let ov = load_case_overview_on(&matter, &OverviewOptions::default()).expect("ov");
    assert_eq!(ov.by_file_category.len(), 1);
    assert_eq!(ov.by_file_category[0].label, "");
    assert_eq!(ov.by_file_category[0].count, 1);
    assert_eq!(ov.by_custodian.len(), 1);
    assert_eq!(ov.by_custodian[0].label, "");
    assert_eq!(ov.by_custodian[0].count, 1);
}

#[test]
fn concurrent_fanout_matches_sequential() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-fanout");
    let matter = Matter::create(&root, "Fanout").expect("create");
    for i in 0..5 {
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                file_category: Some(if i % 2 == 0 { "email" } else { "pdf" }.into()),
                custodian: Some(if i < 3 { "A" } else { "B" }.into()),
                size_bytes: Some(100 * i),
                path: Some(format!("p{i}")),
                ..Default::default()
            })
            .expect("item");
    }
    let sequential = load_case_overview_on(&matter, &OverviewOptions::default()).expect("seq");
    drop(matter);
    let concurrent = load_case_overview(&root, &OverviewOptions::default()).expect("conc");

    assert_eq!(concurrent.totals, sequential.totals);
    assert_eq!(concurrent.by_status, sequential.by_status);
    assert_eq!(concurrent.by_file_category, sequential.by_file_category);
    assert_eq!(concurrent.by_custodian, sequential.by_custodian);
    assert_eq!(concurrent.dedup, sequential.dedup);
    assert_eq!(concurrent.review, sequential.review);
    assert_eq!(concurrent.errors.total, sequential.errors.total);
}

#[test]
fn overview_while_writer_connected() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-wal-ov");
    {
        let matter = Matter::create(&root, "WalOv").expect("create");
        matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                path: Some("seed".into()),
                size_bytes: Some(1),
                ..Default::default()
            })
            .expect("seed");
    }

    let barrier = Arc::new(Barrier::new(2));
    let writer_root = root.clone();
    let b_w = Arc::clone(&barrier);
    let writer = thread::spawn(move || {
        let matter = Matter::open(&writer_root).expect("writer");
        b_w.wait();
        for i in 0..20 {
            let _ = matter.insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                path: Some(format!("w{i}")),
                size_bytes: Some(10),
                ..Default::default()
            });
            thread::sleep(Duration::from_millis(5));
        }
        drop(matter);
    });

    let reader_root = root.clone();
    let b_r = Arc::clone(&barrier);
    let reader = thread::spawn(move || {
        b_r.wait();
        let mut ok = false;
        for _ in 0..30 {
            match load_case_overview(&reader_root, &OverviewOptions::default()) {
                Ok(ov) => {
                    assert!(ov.totals.items_total >= 1);
                    assert!(!ov.generated_at.is_empty());
                    ok = true;
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    assert!(
                        msg.to_ascii_lowercase().contains("locked")
                            || msg.to_ascii_lowercase().contains("busy"),
                        "unexpected overview error under writer: {msg}"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        assert!(ok, "overview never succeeded under writer");
    });

    writer.join().expect("writer");
    reader.join().expect("reader");
}

#[test]
fn jobs_summary_counts_states() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-jobs-ov");
    let matter = Matter::create(&root, "Jobs").expect("create");
    let j1 = matter.create_job("ingest").expect("j1");
    let j2 = matter.create_job("extract").expect("j2");
    matter
        .set_job_state(&j1.id, JobState::Running, None)
        .expect("run");
    matter
        .put_checkpoint(&j1.id, "stage", "{}", 42)
        .expect("cp");
    matter
        .set_job_state(&j2.id, JobState::Running, None)
        .expect("run2");
    matter
        .set_job_state(&j2.id, JobState::Succeeded, None)
        .expect("ok");

    let ov = load_case_overview_on(&matter, &OverviewOptions::default()).expect("ov");
    assert_eq!(ov.jobs.running, 1);
    assert_eq!(ov.jobs.succeeded, 1);
    assert!(!ov.jobs.recent.is_empty());
    let running = ov
        .jobs
        .recent
        .iter()
        .find(|j| j.id == j1.id)
        .expect("running job in recent");
    assert_eq!(running.completed_count, Some(42));
}
