//! Integration tests for matter progress/metrics report export (track 0039).

use std::fs;
use std::thread;
use std::time::Duration;

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

/// Parse a simple CSV line respecting double-quoted fields (no multi-line fields).
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut cur));
            }
            other => cur.push(other),
        }
    }
    fields.push(cur);
    fields
}

fn jobs_header_index(header: &str, col: &str) -> usize {
    let cols = parse_csv_line(header);
    cols.iter()
        .position(|c| c == col)
        .unwrap_or_else(|| panic!("column {col} missing in jobs header: {header}"))
}

fn assert_not_zero_byte(out: &camino::Utf8Path, name: &str) {
    let meta = fs::metadata(out.join(name).as_std_path()).expect("meta");
    assert!(meta.len() > 0, "{name} must not be 0-byte");
}

/// Assert CSV body is header + exact ordered `label,count` rows matching `rows`,
/// plus `(other),N` when `other_count > 0` (and no extra data rows).
fn assert_exact_label_count_csv(
    csv: &str,
    rows: &[matter_core::LabelCount],
    label_fn: fn(&str) -> &str,
    other_count: u64,
) {
    let mut lines = csv.lines().filter(|l| !l.is_empty());
    let header = lines.next().expect("header");
    assert!(
        header.ends_with(",count") || header.contains("count"),
        "unexpected header: {header}"
    );

    let mut expected: Vec<String> = rows
        .iter()
        .map(|r| {
            let label = label_fn(&r.label);
            // Match pack CSV escaping for labels that need quotes.
            format!("{},{}", csv_escape_for_test(label), r.count)
        })
        .collect();
    if other_count > 0 {
        expected.push(format!("(other),{other_count}"));
    }
    if expected.is_empty() {
        expected.push("(none),0".into());
    }

    let actual: Vec<&str> = lines.collect();
    assert_eq!(
        actual.len(),
        expected.len(),
        "row count mismatch.\nexpected:\n{}\nactual:\n{}",
        expected.join("\n"),
        actual.join("\n")
    );
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(
            *a,
            e,
            "row {i} mismatch.\nexpected:\n{}\nactual csv:\n{csv}",
            expected.join("\n")
        );
    }
}

/// Minimal CSV field escape matching matter-core's `csv_escape_field` rules
/// (quote when comma/quote/newline present; double internal quotes).
fn csv_escape_for_test(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
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

    // No leftover .tmp sibling after successful export
    assert!(
        !base.join("report_empty.tmp").exists(),
        "temp pack dir must not remain after success"
    );
}

#[test]
fn free_export_preserves_workspace_temp_marker() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-temp-marker");
    let matter = Matter::create(&root, "TempMarker").expect("create");
    let marker = matter.workspace_temp_dir().join("marker.bin");
    fs::create_dir_all(matter.workspace_temp_dir().as_std_path()).expect("temp dir");
    fs::write(marker.as_std_path(), b"live extract residue").expect("write marker");
    drop(matter);

    let out = base.join("report_marker");
    export_matter_report(
        &root,
        MatterReportParams {
            output_dir: out.clone(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        },
    )
    .expect("export via free fn (open_for_read)");

    assert!(
        marker.as_std_path().is_file(),
        "export_matter_report must not wipe workspace/temp (open_for_read)"
    );
    let body = fs::read(marker.as_std_path()).expect("read marker");
    assert_eq!(body, b"live extract residue");
    assert!(out.join("summary.csv").exists());
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
    let known_started = "2026-07-19T10:00:00Z";
    let job = matter.create_job("extract").expect("job");
    matter
        .set_job_state(&job.id, JobState::Running, None)
        .expect("run");
    // Pin started_at so dual-datetime excel twin is exact and stable.
    matter
        .connection()
        .execute(
            "UPDATE jobs SET started_at = ?1 WHERE id = ?2",
            rusqlite::params![known_started, job.id],
        )
        .expect("pin started_at");
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
    // set_job_state keeps prior started_at on Pending→Running only; re-pin after fail path.
    matter
        .connection()
        .execute(
            "UPDATE jobs SET started_at = ?1 WHERE id = ?2",
            rusqlite::params![known_started, job.id],
        )
        .expect("re-pin started_at");

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

    // Summary KPI: review / dedup / cull / privilege / ocr match overview
    assert_eq!(
        summary_value(&summary, "in_review"),
        ov.review.in_review.to_string()
    );
    assert_eq!(
        summary_value(&summary, "reviewed_count"),
        ov.review.reviewed_count.to_string()
    );
    assert_eq!(
        summary_value(&summary, "unreviewed_count"),
        ov.review.unreviewed_count.to_string()
    );
    assert_eq!(
        summary_value(&summary, "dedup_unique"),
        ov.dedup.unique.to_string()
    );
    assert_eq!(
        summary_value(&summary, "dedup_duplicate"),
        ov.dedup.duplicate.to_string()
    );
    assert_eq!(
        summary_value(&summary, "dedup_skipped"),
        ov.dedup.skipped.to_string()
    );
    assert_eq!(
        summary_value(&summary, "dedup_null"),
        ov.dedup.null_role.to_string()
    );
    assert_eq!(
        summary_value(&summary, "cull_never_run"),
        if ov.cull.never_run { "true" } else { "false" }
    );
    assert_eq!(
        summary_value(&summary, "cull_included"),
        ov.cull.included.to_string()
    );
    assert_eq!(
        summary_value(&summary, "cull_culled"),
        ov.cull.culled.to_string()
    );
    assert_eq!(
        summary_value(&summary, "cull_other"),
        ov.cull.other.to_string()
    );
    assert_eq!(
        summary_value(&summary, "privilege_claimed"),
        ov.privilege.claimed.to_string()
    );
    assert_eq!(
        summary_value(&summary, "privilege_withhold"),
        ov.privilege.withhold.to_string()
    );
    assert_eq!(
        summary_value(&summary, "pdf_needs_ocr"),
        ov.ocr.pdf_needs_ocr.to_string()
    );
    assert_eq!(
        summary_value(&summary, "has_text"),
        ov.ocr.has_text.to_string()
    );
    assert_eq!(
        summary_value(&summary, "has_native"),
        ov.ocr.has_native.to_string()
    );

    // Rollups: exact ordered label,count rows match overview (+ remainder when present)
    let cat = read_pack_file(&out, "by_file_category.csv");
    assert_exact_label_count_csv(
        &cat,
        &ov.by_file_category,
        |raw| {
            if raw.is_empty() {
                "(uncategorized)"
            } else {
                raw
            }
        },
        ov.other_categories_count,
    );
    let cust = read_pack_file(&out, "by_custodian.csv");
    assert_exact_label_count_csv(
        &cust,
        &ov.by_custodian,
        |raw| {
            if raw.is_empty() {
                "(none)"
            } else {
                raw
            }
        },
        ov.other_custodians_count,
    );
    let status = read_pack_file(&out, "by_status.csv");
    assert_exact_label_count_csv(
        &status,
        &ov.by_status,
        |raw| {
            if raw.is_empty() {
                "(none)"
            } else {
                raw
            }
        },
        0,
    );
    let errs = read_pack_file(&out, "errors_by_code.csv");
    assert!(errs.contains("parse_failed"));
    assert!(!errs.contains("(none),0"), "non-empty should omit sentinel");

    // jobs.csv: parse columns for completed_count=7 and dual datetime values
    let jobs = read_pack_file(&out, "jobs.csv");
    let mut job_lines = jobs.lines();
    let header = job_lines.next().expect("jobs header");
    let idx_id = jobs_header_index(header, "job_id");
    let idx_completed = jobs_header_index(header, "completed_count");
    let idx_started_rfc = jobs_header_index(header, "started_at_rfc3339");
    let idx_started_excel = jobs_header_index(header, "started_at_excel");
    let job_row = job_lines
        .find(|l| {
            let cols = parse_csv_line(l);
            cols.get(idx_id).map(|s| s.as_str()) == Some(job.id.as_str())
        })
        .expect("job row");
    let cols = parse_csv_line(job_row);
    assert_eq!(
        cols.get(idx_completed).map(|s| s.as_str()),
        Some("7"),
        "completed_count must be 7; row={job_row}"
    );
    assert_eq!(
        cols.get(idx_started_rfc).map(|s| s.as_str()),
        Some(known_started),
        "started_at_rfc3339 mismatch; row={job_row}"
    );
    assert_eq!(
        cols.get(idx_started_excel).map(|s| s.as_str()),
        Some("2026-07-19 10:00:00 UTC"),
        "started_at_excel twin mismatch; row={job_row}"
    );

    assert!(jobs.contains("extract"));
    assert!(jobs.contains("failed"));
    assert!(
        !jobs.contains(r"C:\client_data"),
        "path leaked in jobs.csv:\n{jobs}"
    );
    assert!(
        !jobs.contains("super_secret_merger"),
        "filename leaked in jobs.csv:\n{jobs}"
    );
    assert!(!jobs.contains("super_secret_merger.pdf"), "filename leaked");
    // Path error free-text residues (e.g. "to extract") must not survive scrub allowlist.
    let idx_err = jobs_header_index(header, "error_summary_safe");
    let err_cell = cols.get(idx_err).map(|s| s.as_str()).unwrap_or("");
    assert!(
        !err_cell.to_ascii_lowercase().contains("extract"),
        "free text leaked in error_summary_safe: {err_cell}"
    );
    assert!(
        err_cell.eq_ignore_ascii_case("failed") || err_cell == "(redacted)",
        "expected allowlisted scrub result, got: {err_cell}"
    );

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
fn jobs_newest_first_in_csv() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-jobs-order");
    let matter = Matter::create(&root, "JobsOrder").expect("create");

    let older = matter.create_job("extract").expect("older");
    // Ensure distinct created_at ordering (list_jobs ORDER BY created_at DESC).
    thread::sleep(Duration::from_millis(15));
    let newer = matter.create_job("cull").expect("newer");

    // Pin created_at so ordering is deterministic even if clocks are coarse.
    matter
        .connection()
        .execute(
            "UPDATE jobs SET created_at = '2026-01-01T00:00:00Z' WHERE id = ?1",
            rusqlite::params![older.id],
        )
        .expect("pin older");
    matter
        .connection()
        .execute(
            "UPDATE jobs SET created_at = '2026-01-02T00:00:00Z' WHERE id = ?1",
            rusqlite::params![newer.id],
        )
        .expect("pin newer");

    let out = base.join("report_jobs_order");
    matter
        .export_matter_report(MatterReportParams {
            output_dir: out.clone(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        })
        .expect("export");

    let jobs = read_pack_file(&out, "jobs.csv");
    let mut lines = jobs.lines();
    let header = lines.next().expect("header");
    let idx_id = jobs_header_index(header, "job_id");
    let first = parse_csv_line(lines.next().expect("first data row"));
    let second = parse_csv_line(lines.next().expect("second data row"));
    assert_eq!(
        first.get(idx_id).map(|s| s.as_str()),
        Some(newer.id.as_str()),
        "first data row must be newest job; jobs.csv:\n{jobs}"
    );
    assert_eq!(
        second.get(idx_id).map(|s| s.as_str()),
        Some(older.id.as_str()),
        "second data row must be older job; jobs.csv:\n{jobs}"
    );
}

#[test]
fn custodian_label_with_comma_csv_escaped() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-comma-cust");
    let matter = Matter::create(&root, "CommaCust").expect("create");
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            file_category: Some("pdf".into()),
            custodian: Some("Smith, Jane".into()),
            size_bytes: Some(10),
            ..Default::default()
        })
        .expect("item");

    let out = base.join("report_comma");
    matter
        .export_matter_report(MatterReportParams {
            output_dir: out.clone(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        })
        .expect("export");

    let cust = read_pack_file(&out, "by_custodian.csv");
    assert!(
        cust.contains("\"Smith, Jane\",1")
            || cust.lines().any(|l| {
                let cols = parse_csv_line(l);
                cols.first().map(|s| s.as_str()) == Some("Smith, Jane")
                    && cols.get(1).map(|s| s.as_str()) == Some("1")
            }),
        "custodian with comma must be quoted correctly:\n{cust}"
    );
    // Raw unquoted form would split into three CSV fields.
    assert!(
        !cust.lines().any(|l| l.starts_with("Smith, Jane,1")),
        "unquoted comma label would break CSV:\n{cust}"
    );
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
    // Free text "boom"/"at" dropped by allowlist → (redacted)
    assert_eq!(safe, "(redacted)");
    assert_eq!(
        rfc3339_to_excel_utc("2026-01-02T03:04:05Z"),
        "2026-01-02 03:04:05 UTC"
    );

    let spaced = scrub_error_summary(r"Failed to extract C:\client data\super_secret_merger.pdf");
    assert!(!spaced.contains("client"));
    assert!(!spaced.contains("super_secret"));
    assert!(!spaced.contains("merger"));
    assert!(!spaced.to_ascii_lowercase().contains("pdf"));
    assert!(spaced.eq_ignore_ascii_case("failed") || spaced == "(redacted)");

    let relative = scrub_error_summary(r"err client_data\acme_deal\memo.eml");
    assert!(!relative.contains("client_data"));
    assert!(!relative.contains("acme_deal"));
    assert!(!relative.contains("memo"));
    // Bare "err" is free text (codes require snake_case `_`); path redacted → (redacted).
    assert_eq!(relative, "(redacted)");

    let subjectish = scrub_error_summary("failed while processing CONFIDENTIAL_SUBJECT_XYZ");
    assert!(!subjectish.to_ascii_uppercase().contains("CONFIDENTIAL"));
    assert!(!subjectish.to_ascii_uppercase().contains("SUBJECT"));
    assert!(!subjectish.contains("XYZ"));
    assert!(subjectish.eq_ignore_ascii_case("failed") || subjectish == "(redacted)");

    let root_unix = scrub_error_summary("failed /client_secret");
    assert!(!root_unix.contains("client_secret"));
    assert!(root_unix.eq_ignore_ascii_case("failed") || root_unix == "(redacted)");
}

#[test]
fn remainder_other_row_when_other_count() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-other");
    let matter = Matter::create(&root, "Other").expect("create");
    // Seed more categories than top_n=2
    // counts: a=1, b=2, c=3, d=4 → top 2 by count desc: d,c; other = a+b = 3
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
    let opts = OverviewOptions {
        top_categories: 2,
        top_custodians: 25,
        top_error_codes: 15,
        recent_jobs: 5,
    };
    let ov = load_case_overview_on(&matter, &opts).expect("overview");
    let out = base.join("report_other");
    matter
        .export_matter_report(MatterReportParams {
            output_dir: out.clone(),
            overview_opts: opts,
            include_pdf: false,
            export_all_jobs: true,
        })
        .expect("export");
    let cat = read_pack_file(&out, "by_file_category.csv");
    assert!(
        ov.other_categories_count > 0,
        "fixture must produce remainder"
    );
    assert_exact_label_count_csv(
        &cat,
        &ov.by_file_category,
        |raw| {
            if raw.is_empty() {
                "(uncategorized)"
            } else {
                raw
            }
        },
        ov.other_categories_count,
    );
    // Explicit remainder-only shape check for this fixture.
    assert!(
        cat.lines()
            .any(|l| l == format!("(other),{}", ov.other_categories_count)),
        "expected exact (other),{} row:\n{cat}",
        ov.other_categories_count
    );
}

#[test]
fn error_summary_free_text_absent_from_pack() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-scrub-freetext");
    let matter = Matter::create(&root, "ScrubFree").expect("create");
    let job = matter.create_job("extract").expect("job");
    matter
        .set_job_state(&job.id, JobState::Running, None)
        .expect("run");
    matter
        .set_job_state(
            &job.id,
            JobState::Failed,
            Some("failed while processing CONFIDENTIAL_SUBJECT_XYZ"),
        )
        .expect("fail");

    let out = base.join("report_freetext");
    matter
        .export_matter_report(MatterReportParams {
            output_dir: out.clone(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        })
        .expect("export");

    // Distinctive free-text tokens from the seeded job error (not README words
    // like "subjects").
    let forbidden = [
        "CONFIDENTIAL",
        "CONFIDENTIAL_SUBJECT_XYZ",
        "SUBJECT_XYZ",
        "processing",
    ];
    for name in pack_files(&out) {
        let body = read_pack_file(&out, &name);
        let upper = body.to_ascii_uppercase();
        let lower = body.to_ascii_lowercase();
        for token in forbidden {
            if token.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                assert!(
                    !upper.contains(&token.to_ascii_uppercase()),
                    "{token} leaked in {name}:\n{body}"
                );
            } else {
                assert!(
                    !lower.contains(&token.to_ascii_lowercase()),
                    "{token} leaked in {name}:\n{body}"
                );
            }
        }
        // "while" only as free-text residue — skip README which has no "while".
        if name == "jobs.csv" {
            assert!(
                !lower.contains("while"),
                "free text 'while' leaked in jobs.csv:\n{body}"
            );
        }
    }

    let jobs = read_pack_file(&out, "jobs.csv");
    let mut lines = jobs.lines();
    let header = lines.next().expect("header");
    let idx_err = jobs_header_index(header, "error_summary_safe");
    let row = lines
        .find(|l| l.contains(job.id.as_str()))
        .expect("job row");
    let cols = parse_csv_line(row);
    let err = cols.get(idx_err).map(|s| s.as_str()).unwrap_or("");
    assert!(
        err.eq_ignore_ascii_case("failed") || err == "(redacted)",
        "error_summary_safe should be allowlisted only: {err}"
    );
}
