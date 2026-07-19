//! Integration tests for matter progress/metrics report export (track 0039).

use std::fs;

use matter_core::{
    default_matter_report_dir, export_matter_report, item_role, item_status, load_case_overview_on,
    rfc3339_to_excel_utc, scrub_error_summary, ItemErrorInput, ItemInput, JobState, Matter,
    MatterReportParams, OverviewOptions, MATTER_REPORT_FORMAT_VERSION, SCHEMA_VERSION,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

fn pack_files(out: &camino::Utf8Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(out.as_std_path())
        .expect("read_dir")
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

fn read_pack_file(out: &camino::Utf8Path, name: &str) -> String {
    fs::read_to_string(out.join(name).as_std_path()).unwrap_or_else(|e| {
        panic!("read {name}: {e}");
    })
}

fn summary_value(summary: &str, metric: &str) -> String {
    for line in summary.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        // metric,value — metric has no commas in our fixed set
        if let Some((k, v)) = line.split_once(',') {
            if k == metric {
                return v.trim_matches('"').to_string();
            }
        }
    }
    panic!("metric {metric} not found in summary.csv:\n{summary}");
}

fn assert_not_zero_byte(out: &camino::Utf8Path, name: &str) {
    let meta = fs::metadata(out.join(name).as_std_path()).expect("meta");
    assert!(meta.len() > 0, "{name} must not be 0-byte");
}

#[test]
fn empty_matter_valid_pack_with_sentinels() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-empty-report");
    let matter = Matter::create(&root, "EmptyReport").expect("create");
    drop(matter);

    let out = base.join("report_empty");
    let result = export_matter_report(
        &root,
        MatterReportParams {
            output_dir: out.clone(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        },
    )
    .expect("export");

    assert!(!result.pdf_written);
    assert_eq!(result.overview.totals.items_total, 0);

    for name in [
        "summary.csv",
        "by_file_category.csv",
        "by_custodian.csv",
        "by_status.csv",
        "errors_by_code.csv",
        "jobs.csv",
        "README.txt",
    ] {
        assert!(out.join(name).exists(), "missing {name}");
        assert_not_zero_byte(&out, name);
    }

    let cat = read_pack_file(&out, "by_file_category.csv");
    assert!(cat.starts_with("label,count\n"));
    assert!(cat.contains("(none),0"));

    let cust = read_pack_file(&out, "by_custodian.csv");
    assert!(cust.contains("(none),0"));

    let status = read_pack_file(&out, "by_status.csv");
    assert!(status.contains("(none),0"));

    let errs = read_pack_file(&out, "errors_by_code.csv");
    assert!(errs.starts_with("code,count\n"));
    assert!(errs.contains("(none),0"), "zero-error still needs sentinel");

    let jobs = read_pack_file(&out, "jobs.csv");
    assert!(jobs.contains("job_id,kind,state"));
    assert!(jobs.contains("(none)"), "zero jobs sentinel");

    let summary = read_pack_file(&out, "summary.csv");
    assert_eq!(summary_value(&summary, "items_total"), "0");
    assert_eq!(
        summary_value(&summary, "report_format_version"),
        MATTER_REPORT_FORMAT_VERSION
    );
    assert_eq!(
        summary_value(&summary, "schema_version"),
        SCHEMA_VERSION.to_string()
    );
    assert_eq!(summary_value(&summary, "pdf_written"), "false");

    // Dual datetime
    let gen = summary_value(&summary, "generated_at");
    let gen_excel = summary_value(&summary, "generated_at_excel");
    assert!(
        gen.contains('T') || gen.ends_with('Z') || gen.contains('+'),
        "generated_at should look RFC3339: {gen}"
    );
    assert!(
        gen_excel.contains(" UTC") && gen_excel.contains('-') && gen_excel.contains(':'),
        "generated_at_excel missing: {gen_excel}"
    );
    // Excel form uses space, not the RFC3339 `T` date/time separator (UTC label is fine).
    assert!(
        !gen_excel.contains('T') || gen_excel.ends_with(" UTC"),
        "excel form should use space separator: {gen_excel}"
    );
    assert!(
        gen_excel.chars().filter(|c| *c == 'T').count() <= 1,
        "unexpected T in excel datetime: {gen_excel}"
    );
}

#[test]
fn seeded_overview_metrics_match_summary() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-seeded-report");
    let matter = Matter::create(&root, "SeededReport").expect("create");

    matter
        .insert_source(r"C:\exports\synth", "folder", "imported", None)
        .expect("source");

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
            subject: Some("CONFIDENTIAL_SUBJECT_XYZ_PRIVACY".into()),
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
        .expect("att");
    matter
        .insert_item(ItemInput {
            status: item_status::PARTIAL.into(),
            file_category: Some("email".into()),
            custodian: Some("Bob".into()),
            size_bytes: Some(100),
            path: Some("bob/msg.eml".into()),
            subject: Some("CONFIDENTIAL_SUBJECT_XYZ_PRIVACY".into()),
            ..Default::default()
        })
        .expect("bob");

    matter
        .record_item_error(ItemErrorInput {
            item_id: Some(parent.id.clone()),
            source_id: None,
            job_id: None,
            stage: "extract".into(),
            code: "parse_failed".into(),
            message: "synthetic".into(),
            detail: None,
        })
        .expect("err");

    // Distinctive path in job error_summary for scrub test
    let job = matter.create_job("extract").expect("job");
    matter
        .set_job_state(&job.id, JobState::Running, None)
        .expect("run");
    matter
        .put_checkpoint(&job.id, "stage", "{}", 7)
        .expect("cp");
    matter
        .set_job_state(
            &job.id,
            JobState::Failed,
            Some(r"Failed to extract C:\client_data\super_secret_merger.pdf"),
        )
        .expect("fail");

    let opts = OverviewOptions::default();
    let ov = load_case_overview_on(&matter, &opts).expect("ov");

    let out = base.join("report_seeded");
    let result = matter
        .export_matter_report(MatterReportParams {
            output_dir: out.clone(),
            overview_opts: opts,
            include_pdf: true, // must still leave pdf_written=false
            export_all_jobs: true,
        })
        .expect("export");

    assert!(!result.pdf_written, "PDF deferred");
    assert_eq!(result.overview.totals.items_total, ov.totals.items_total);

    let summary = read_pack_file(&out, "summary.csv");
    assert_eq!(
        summary_value(&summary, "items_total"),
        ov.totals.items_total.to_string()
    );
    assert_eq!(
        summary_value(&summary, "top_level_items"),
        ov.totals.top_level_items.to_string()
    );
    assert_eq!(
        summary_value(&summary, "size_bytes_top_level"),
        ov.totals.size_bytes_top_level.to_string()
    );
    assert_eq!(
        summary_value(&summary, "sources_total"),
        ov.totals.sources_total.to_string()
    );
    assert_eq!(
        summary_value(&summary, "families_total"),
        ov.totals.families_total.to_string()
    );
    assert_eq!(
        summary_value(&summary, "item_errors_total"),
        ov.errors.total.to_string()
    );
    assert_eq!(
        summary_value(&summary, "jobs_failed"),
        ov.jobs.failed.to_string()
    );
    assert_eq!(summary_value(&summary, "matter_name"), "SeededReport");

    // Rollups match overview top-N
    let cat = read_pack_file(&out, "by_file_category.csv");
    for r in &ov.by_file_category {
        let label = if r.label.is_empty() {
            "(uncategorized)"
        } else {
            r.label.as_str()
        };
        assert!(
            cat.contains(&format!("{label},{}", r.count)),
            "missing category row {label}:\n{cat}"
        );
    }
    let cust = read_pack_file(&out, "by_custodian.csv");
    for r in &ov.by_custodian {
        let label = if r.label.is_empty() {
            "(none)"
        } else {
            r.label.as_str()
        };
        assert!(
            cust.contains(&format!("{label},{}", r.count)),
            "missing custodian {label}:\n{cust}"
        );
    }
    let status = read_pack_file(&out, "by_status.csv");
    for r in &ov.by_status {
        assert!(status.contains(&r.count.to_string()));
    }
    let errs = read_pack_file(&out, "errors_by_code.csv");
    assert!(errs.contains("parse_failed"));
    assert!(!errs.contains("(none),0"), "non-empty should omit sentinel");

    // jobs.csv has seeded job + scrubbed path
    let jobs = read_pack_file(&out, "jobs.csv");
    assert!(jobs.contains(&job.id));
    assert!(jobs.contains("extract"));
    assert!(jobs.contains("failed"));
    assert!(
        jobs.contains(",7,")
            || jobs.contains(",7\n")
            || jobs.lines().any(|l| l.contains(&job.id) && l.contains("7"))
    );
    assert!(
        !jobs.contains(r"C:\client_data"),
        "path leaked in jobs.csv:\n{jobs}"
    );
    assert!(
        !jobs.contains("super_secret_merger"),
        "filename leaked in jobs.csv:\n{jobs}"
    );
    assert!(!jobs.contains("super_secret_merger.pdf"), "filename leaked");

    // Privacy: subject must not appear in any pack file
    let secret = "CONFIDENTIAL_SUBJECT_XYZ_PRIVACY";
    for name in pack_files(&out) {
        let body = read_pack_file(&out, &name);
        assert!(!body.contains(secret), "subject leaked in {name}:\n{body}");
    }

    // Dual datetime on summary
    let gen = summary_value(&summary, "generated_at");
    let gen_excel = summary_value(&summary, "generated_at_excel");
    assert!(!gen.is_empty());
    assert!(gen_excel.ends_with(" UTC"));
    // Job times: failed job should have started_at RFC3339 and excel twin columns present
    assert!(jobs.contains("started_at_excel"));
    assert!(jobs.contains("started_at_rfc3339"));

    // Audit complete event
    let count: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'report.export.complete'",
            [],
            |row| row.get(0),
        )
        .expect("audit count");
    assert_eq!(count, 1);

    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'report.export.complete' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("params");
    assert!(params_json.contains(MATTER_REPORT_FORMAT_VERSION));
    assert!(params_json.contains("items_total"));
    assert!(!params_json.contains(secret));
}

#[test]
fn target_exists_fails_closed() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-exists");
    Matter::create(&root, "Exists").expect("create");

    let out = base.join("report_exists");
    fs::create_dir_all(out.as_std_path()).expect("precreate");

    let err = export_matter_report(
        &root,
        MatterReportParams {
            output_dir: out.clone(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        },
    )
    .expect_err("must refuse overwrite");
    let msg = err.to_string();
    assert!(
        msg.contains("already exists") || msg.contains("refusing"),
        "unexpected error: {msg}"
    );

    // Fail audit should be recorded when matter opened successfully
    let matter = Matter::open(&root).expect("open");
    let fail_count: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'report.export.fail'",
            [],
            |row| row.get(0),
        )
        .expect("fail audit");
    assert!(fail_count >= 1, "expected report.export.fail audit");
}

#[test]
fn default_report_dir_under_exports_reports() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-default-path");
    Matter::create(&root, "DefaultPath").expect("create");

    let dir = default_matter_report_dir(&root);
    let s = dir.as_str().replace('\\', "/");
    assert!(s.contains("/exports/reports/matter_report_"));
    // stamp is YYYYMMDD_HHMMSS
    let stamp = dir.file_name().expect("name");
    assert!(stamp.starts_with("matter_report_"));
    assert!(stamp.len() > "matter_report_".len() + 8);
}

#[test]
fn free_function_and_matter_method_agree() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-free-fn");
    let matter = Matter::create(&root, "FreeFn").expect("create");
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            file_category: Some("pdf".into()),
            custodian: Some("Carol".into()),
            size_bytes: Some(42),
            ..Default::default()
        })
        .expect("item");
    drop(matter);

    let out = base.join("report_free");
    let r = export_matter_report(
        &root,
        MatterReportParams {
            output_dir: out.clone(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        },
    )
    .expect("export");
    assert_eq!(r.overview.totals.items_total, 1);
    assert!(out.join("summary.csv").exists());
}

#[test]
fn scrub_and_excel_helpers_public() {
    let safe = scrub_error_summary(r"boom at D:\cases\acme\file.msg");
    assert!(!safe.contains("acme"));
    assert!(!safe.contains("file.msg"));
    assert_eq!(
        rfc3339_to_excel_utc("2026-01-02T03:04:05Z"),
        "2026-01-02 03:04:05 UTC"
    );
}

#[test]
fn remainder_other_row_when_other_count() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-other");
    let matter = Matter::create(&root, "Other").expect("create");
    // Seed more categories than top_n=2
    for (i, cat) in ["a", "b", "c", "d"].iter().enumerate() {
        for _ in 0..=i {
            matter
                .insert_item(ItemInput {
                    status: item_status::EXTRACTED.into(),
                    file_category: Some((*cat).into()),
                    size_bytes: Some(1),
                    ..Default::default()
                })
                .expect("item");
        }
    }
    let out = base.join("report_other");
    matter
        .export_matter_report(MatterReportParams {
            output_dir: out.clone(),
            overview_opts: OverviewOptions {
                top_categories: 2,
                top_custodians: 25,
                top_error_codes: 15,
                recent_jobs: 5,
            },
            include_pdf: false,
            export_all_jobs: true,
        })
        .expect("export");
    let cat = read_pack_file(&out, "by_file_category.csv");
    assert!(
        cat.contains("(other),"),
        "expected (other) remainder:\n{cat}"
    );
}
