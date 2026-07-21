//! Production QC integration tests (spec §3.11).

#![allow(clippy::field_reassign_with_default)]

use std::fs;

use matter_core::{
    item_role, item_status, CreateRedactionInput, ItemInput, Matter, UpsertItemPrivilegeInput,
    FAMILY_KIND_EMAIL_ATTACHMENTS, SCHEMA_VERSION,
};
use matter_qc::{
    evaluate_candidates_with_cancel, resolve_rules, run_production_qc, QcError, QcOutcome,
    QcParams, QcRuleConfig, QcSeverity, JOB_KIND_QC, RULE_BROKEN_FAMILY_INCOMPLETE_PARENT,
    RULE_BROKEN_FAMILY_ORPHAN_CHILD, RULE_EMPTY_SELECTION, RULE_MISSING_NATIVE, RULE_MISSING_TEXT,
    RULE_ONLY_WITHHELD, RULE_PDF_NEEDS_OCR, RULE_REDACTED_TEXT_MISSING,
    RULE_WITHHELD_FAMILY_MEMBER, RULE_WITHHELD_IN_SELECTION, RULE_ZERO_SIZE,
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

fn put_text(matter: &Matter, body: &str) -> String {
    matter.put_bytes(body.as_bytes()).expect("put text")
}

fn put_native(matter: &Matter, bytes: &[u8]) -> String {
    matter.put_bytes(bytes).expect("put native")
}

fn insert_review_item(matter: &Matter, mut input: ItemInput) -> String {
    input.status = item_status::EXTRACTED.into();
    if input.role.is_none() {
        input.role = Some(item_role::STANDALONE.into());
    }
    input.in_review = Some(1);
    matter.insert_item(input).expect("insert").id
}

fn run_qc(matter: &Matter, job_id: &str, params: &QcParams) -> matter_qc::QcReport {
    match run_production_qc(matter, job_id, params, None, |_| {}).expect("run") {
        QcOutcome::Succeeded(r) => r,
        other => panic!("expected Succeeded, got {other:?}"),
    }
}

fn findings_of<'a>(report: &'a matter_qc::QcReport, rule: &str) -> Vec<&'a matter_qc::QcFinding> {
    report
        .findings
        .iter()
        .filter(|f| f.rule_id == rule)
        .collect()
}

fn good_doc(matter: &Matter, path: &str) -> String {
    let n = put_native(matter, b"native-bytes");
    let t = put_text(matter, "plain text body");
    insert_review_item(
        matter,
        ItemInput {
            path: Some(path.into()),
            native_sha256: Some(n),
            text_sha256: Some(t),
            file_category: Some("document".into()),
            size_bytes: Some(12),
            ..Default::default()
        },
    )
}

/// Parent + children sharing a family_id (required by matter-core cohesion).
fn insert_family_parent(matter: &Matter, path: &str, in_review: i64) -> (String, String) {
    let family = matter
        .insert_family(FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("family");
    let n = put_native(matter, b"parent-native");
    let t = put_text(matter, "parent text body");
    let parent = matter
        .insert_item(ItemInput {
            path: Some(path.into()),
            native_sha256: Some(n),
            text_sha256: Some(t),
            file_category: Some("email".into()),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            in_review: Some(in_review),
            status: item_status::EXTRACTED.into(),
            size_bytes: Some(12),
            ..Default::default()
        })
        .expect("parent")
        .id;
    (parent, family.id)
}

fn insert_child(
    matter: &Matter,
    parent_id: &str,
    family_id: &str,
    path: &str,
    in_review: i64,
) -> String {
    let n = put_native(matter, path.as_bytes());
    let t = put_text(matter, "child text");
    matter
        .insert_item(ItemInput {
            path: Some(path.into()),
            native_sha256: Some(n),
            text_sha256: Some(t),
            file_category: Some("document".into()),
            role: Some(item_role::ATTACHMENT.into()),
            parent_item_id: Some(parent_id.into()),
            family_id: Some(family_id.into()),
            in_review: Some(in_review),
            status: item_status::EXTRACTED.into(),
            size_bytes: Some(4),
            ..Default::default()
        })
        .expect("child")
        .id
}

#[test]
fn schema_v21_qc_runs_table() {
    let (_tmp, matter) = temp_matter("schema-v21");
    assert_eq!(SCHEMA_VERSION, 36);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    let has: bool = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='qc_runs'",
            [],
            |row| row.get(0),
        )
        .expect("table");
    assert!(has);
}

/// 1. Orphan attachment → orphan error, passed=false
#[test]
fn orphan_attachment_error() {
    let (_tmp, matter) = temp_matter("orphan");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    // Parent not in review — only child is selected
    let (parent, family_id) = insert_family_parent(&matter, "parent.eml", 0);
    let child = insert_child(&matter, &parent, &family_id, "attach.pdf", 1);

    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.passed);
    let orphan = findings_of(&r, RULE_BROKEN_FAMILY_ORPHAN_CHILD);
    assert_eq!(orphan.len(), 1);
    assert_eq!(orphan[0].item_id.as_deref(), Some(child.as_str()));
    assert_eq!(orphan[0].severity, QcSeverity::Error);
}

/// 2. Parent + 0 of N non-withheld kids → incomplete_parent warn
#[test]
fn incomplete_parent_zero_of_n() {
    let (_tmp, matter) = temp_matter("inc-0");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let (parent, family_id) = insert_family_parent(&matter, "p.eml", 1);
    for i in 0..2 {
        insert_child(&matter, &parent, &family_id, &format!("c{i}.bin"), 0);
    }
    let r = run_qc(&matter, &job.id, &QcParams::default());
    let inc = findings_of(&r, RULE_BROKEN_FAMILY_INCOMPLETE_PARENT);
    assert_eq!(inc.len(), 1);
    assert_eq!(inc[0].severity, QcSeverity::Warn);
    assert_eq!(inc[0].item_id.as_deref(), Some(parent.as_str()));
    // warn only → still passed
    assert!(r.passed);
}

/// 3. Parent + 1 of 3 non-withheld kids → incomplete MUST fire
#[test]
fn incomplete_parent_one_of_three() {
    let (_tmp, matter) = temp_matter("inc-1of3");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let (parent, family_id) = insert_family_parent(&matter, "p.eml", 1);
    for i in 0..3 {
        // only first kid in review
        insert_child(
            &matter,
            &parent,
            &family_id,
            &format!("k{i}.bin"),
            if i == 0 { 1 } else { 0 },
        );
    }
    let r = run_qc(&matter, &job.id, &QcParams::default());
    let inc = findings_of(&r, RULE_BROKEN_FAMILY_INCOMPLETE_PARENT);
    assert!(
        !inc.is_empty(),
        "parent+1-of-3 must fire incomplete_parent; findings={:?}",
        r.findings
    );
    assert!(inc
        .iter()
        .any(|f| f.item_id.as_deref() == Some(parent.as_str())));
}

/// 4. Parent + all non-withheld kids, one withheld unselected → no incomplete; withheld_family_member
#[test]
fn withheld_child_not_incomplete_but_family_member() {
    let (_tmp, matter) = temp_matter("withheld-kid");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let (parent, family_id) = insert_family_parent(&matter, "p.eml", 1);
    // two non-withheld kids in review
    for i in 0..2 {
        insert_child(&matter, &parent, &family_id, &format!("ok{i}.bin"), 1);
    }
    // one withheld kid NOT in review
    let withheld = insert_child(&matter, &parent, &family_id, "priv.bin", 0);
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: withheld,
            basis: "attorney_client".into(),
            description: "privileged attachment".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "t".into(),
            expected_version: None,
        })
        .unwrap();

    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(
        findings_of(&r, RULE_BROKEN_FAMILY_INCOMPLETE_PARENT).is_empty(),
        "withheld unselected child must not cause incomplete_parent; findings={:?}",
        r.findings
    );
    let fam = findings_of(&r, RULE_WITHHELD_FAMILY_MEMBER);
    assert!(
        !fam.is_empty(),
        "expected withheld_family_member; findings={:?}",
        r.findings
    );
}

/// 5. Withheld in selection → error
#[test]
fn withheld_in_selection_error() {
    let (_tmp, matter) = temp_matter("withheld-sel");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let id = good_doc(&matter, "h.pdf");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: id.clone(),
            basis: "attorney_client".into(),
            description: "hold me".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "t".into(),
            expected_version: None,
        })
        .unwrap();
    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.passed);
    let f = findings_of(&r, RULE_WITHHELD_IN_SELECTION);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].item_id.as_deref(), Some(id.as_str()));
}

/// 6. Redaction without artifact → redacted_text_missing error
#[test]
fn redacted_text_missing_error() {
    let (_tmp, matter) = temp_matter("rdx-miss");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let body = "Alpha SECRET beta";
    let text_sha = put_text(&matter, body);
    let native = put_native(&matter, b"n");
    let item_id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("r.txt".into()),
            native_sha256: Some(native),
            text_sha256: Some(text_sha.clone()),
            file_category: Some("document".into()),
            size_bytes: Some(10),
            ..Default::default()
        },
    );
    matter
        .create_redaction(CreateRedactionInput {
            item_id: item_id.clone(),
            start_utf8: 6,
            end_utf8: 12,
            exact_quote: "SECRET".into(),
            display_body: body.into(),
            body_digest: text_sha,
            reason: "confidential".into(),
            label: None,
            actor: "t".into(),
        })
        .expect("redaction");
    let item = matter.get_item(&item_id).unwrap();
    assert!(item.redaction_count > 0);
    assert!(item.redacted_text_sha256.is_none());

    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.passed);
    let f = findings_of(&r, RULE_REDACTED_TEXT_MISSING);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].severity, QcSeverity::Error);
}

/// 7. Missing native non-email → error
#[test]
fn missing_native_non_email() {
    let (_tmp, matter) = temp_matter("miss-nat");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let t = put_text(&matter, "doc text");
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("doc.pdf".into()),
            text_sha256: Some(t),
            file_category: Some("document".into()),
            size_bytes: Some(1),
            ..Default::default()
        },
    );
    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.passed);
    let f = findings_of(&r, RULE_MISSING_NATIVE);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].item_id.as_deref(), Some(id.as_str()));
}

/// 8. Missing text document/email → error; image → warn
#[test]
fn missing_text_taxonomy() {
    let (_tmp, matter) = temp_matter("miss-text");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let n1 = put_native(&matter, b"doc");
    let doc = insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.docx".into()),
            native_sha256: Some(n1),
            file_category: Some("document".into()),
            size_bytes: Some(3),
            ..Default::default()
        },
    );
    let n2 = put_native(&matter, b"img");
    let img = insert_review_item(
        &matter,
        ItemInput {
            path: Some("b.png".into()),
            native_sha256: Some(n2),
            file_category: Some("image".into()),
            size_bytes: Some(3),
            ..Default::default()
        },
    );
    let r = run_qc(&matter, &job.id, &QcParams::default());
    let texts = findings_of(&r, RULE_MISSING_TEXT);
    let doc_f = texts
        .iter()
        .find(|f| f.item_id.as_deref() == Some(doc.as_str()))
        .expect("doc missing_text");
    assert_eq!(doc_f.severity, QcSeverity::Error);
    let img_f = texts
        .iter()
        .find(|f| f.item_id.as_deref() == Some(img.as_str()))
        .expect("img missing_text");
    assert_eq!(img_f.severity, QcSeverity::Warn);
    assert!(!r.passed);
}

/// 9. pdf_needs_ocr → warn
#[test]
fn pdf_needs_ocr_warn() {
    let (_tmp, matter) = temp_matter("pdf-ocr");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let id = good_doc(&matter, "scan.pdf");
    matter
        .connection()
        .execute("UPDATE items SET pdf_needs_ocr = 1 WHERE id = ?1", [&id])
        .unwrap();
    let r = run_qc(&matter, &job.id, &QcParams::default());
    let f = findings_of(&r, RULE_PDF_NEEDS_OCR);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].severity, QcSeverity::Warn);
    assert!(r.passed);
}

/// 10. Severity off disables rule
#[test]
fn severity_off_disables_rule() {
    let (_tmp, matter) = temp_matter("off");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let id = good_doc(&matter, "z.bin");
    matter
        .connection()
        .execute("UPDATE items SET size_bytes = 0 WHERE id = ?1", [&id])
        .unwrap();

    let with_warn = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!findings_of(&with_warn, RULE_ZERO_SIZE).is_empty());

    let job2 = matter.create_job(JOB_KIND_QC).expect("job2");
    let off = run_qc(
        &matter,
        &job2.id,
        &QcParams {
            rules: vec![QcRuleConfig {
                id: RULE_ZERO_SIZE.into(),
                severity: QcSeverity::Off,
            }],
            ..Default::default()
        },
    );
    assert!(findings_of(&off, RULE_ZERO_SIZE).is_empty());
}

/// 11. Empty selection → error
#[test]
fn empty_selection_error() {
    let (_tmp, matter) = temp_matter("empty");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.passed);
    let f = findings_of(&r, RULE_EMPTY_SELECTION);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].severity, QcSeverity::Error);
}

/// 12. Findings CSV written; no subject leak
#[test]
fn findings_csv_no_subject_leak() {
    let (_tmp, matter) = temp_matter("csv-priv");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let secret_subject = "ULTRA_SECRET_SUBJECT_TOKEN_ZZZ";
    let n = put_native(&matter, b"x");
    // missing text → finding, subject must not appear in CSV
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("C:\\client\\secret\\path\\file.docx".into()),
            native_sha256: Some(n),
            file_category: Some("document".into()),
            subject: Some(secret_subject.into()),
            size_bytes: Some(1),
            ..Default::default()
        },
    );
    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.report_path.is_empty());
    let findings_path = camino::Utf8Path::new(&r.report_path).join("findings.csv");
    let body = fs::read_to_string(findings_path.as_std_path()).expect("findings");
    assert!(body.contains("rule_id"));
    assert!(!body.contains(secret_subject));
    assert!(!body.contains("C:\\client"));
    assert!(!body.contains("secret\\path"));
    let summary_path = camino::Utf8Path::new(&r.report_path).join("summary.csv");
    assert!(summary_path.as_std_path().exists());

    // qc_runs row
    let latest = matter.load_latest_qc_run().unwrap().expect("qc_run");
    assert_eq!(latest.selection_fingerprint, r.selection_fingerprint);
    assert_eq!(latest.passed, r.passed);
}

/// Dangling parent_item_id must not abort withheld_family_member / incomplete checks.
#[test]
fn dangling_parent_does_not_abort_qc() {
    let (_tmp, matter) = temp_matter("dangle-parent");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let id = good_doc(&matter, "orphanish.pdf");
    // Simulate broken parent pointer (insert APIs refuse missing parents).
    matter
        .connection()
        .execute(
            "UPDATE items SET parent_item_id = 'itm_missing_parent' WHERE id = ?1",
            [&id],
        )
        .expect("sql");
    let r = run_qc(&matter, &job.id, &QcParams::default());
    // Orphan rule fires; no hard error from item_is_withheld on missing parent.
    let orphan = findings_of(&r, RULE_BROKEN_FAMILY_ORPHAN_CHILD);
    assert_eq!(orphan.len(), 1);
    assert!(!r.passed);
}

/// All candidates withheld → only_withheld set-level error.
#[test]
fn only_withheld_set_level_error() {
    let (_tmp, matter) = temp_matter("only-withheld");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let id = good_doc(&matter, "priv.pdf");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: id,
            basis: "attorney_client".into(),
            description: "all withheld".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "t".into(),
            expected_version: None,
        })
        .unwrap();
    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.passed);
    let f = findings_of(&r, RULE_ONLY_WITHHELD);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].severity, QcSeverity::Error);
    assert!(f[0].item_id.is_none());
}

/// Cancel callback during evaluate returns Cancelled.
#[test]
fn evaluate_cancel_between_items() {
    let (_tmp, matter) = temp_matter("eval-cancel");
    let a = good_doc(&matter, "a.pdf");
    let b = good_doc(&matter, "b.pdf");
    let rules = resolve_rules(&[]);
    let cancel = || true;
    let err = evaluate_candidates_with_cancel(
        &matter,
        &[a, b],
        &rules,
        Some(&cancel as &dyn Fn() -> bool),
    )
    .expect_err("must cancel");
    assert!(matches!(err, QcError::Cancelled));
}

/// Cancel mid-QC → Paused with checkpoint; resume completes without re-eval from 0 only.
#[test]
fn cancel_pause_resume_checkpoint() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use matter_qc::QC_STAGE;

    let (_tmp, matter) = temp_matter("qc-cancel-resume");
    let job = matter.create_job(JOB_KIND_QC).expect("job");

    const N: u64 = 20;
    for i in 0..N {
        good_doc(&matter, &format!("doc{i:03}.pdf"));
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let params = QcParams::default();
    let outcome = run_production_qc(
        &matter,
        &job.id,
        &params,
        Some(&|| cancel_flag2.load(Ordering::SeqCst)),
        |completed| {
            // Cancel after first item progress so we always pause mid-scan.
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");

    let QcOutcome::Paused(s) = outcome else {
        panic!("expected Paused after cancel, got {outcome:?}");
    };
    assert!(
        s.completed_count > 0 && s.completed_count < N,
        "partial progress required for pause: {s:?}"
    );
    // Partial cancel must NOT write a authorizing qc_runs row.
    assert!(
        matter.load_latest_qc_run().expect("load").is_none(),
        "partial cancel must not insert qc_runs"
    );

    let cp = matter
        .get_checkpoint(&job.id, QC_STAGE)
        .expect("cp")
        .expect("checkpoint present after pause");
    assert_eq!(cp.completed_count as u64, s.completed_count);
    let cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("checkpoint json");
    let paused_cursor = cursor["cursor_index"].as_u64().unwrap_or(0);
    assert_eq!(paused_cursor, s.completed_count);
    assert!(
        cursor["ordered_ids"]
            .as_array()
            .map(|a| a.len() as u64 == N)
            .unwrap_or(false),
        "frozen ordered_ids required: {}",
        cp.cursor_json
    );

    // Resume with cancel off → Succeeded; cursor advances; qc_runs written once.
    let outcome2 = run_production_qc(&matter, &job.id, &params, None, |_| {}).expect("resume");
    let QcOutcome::Succeeded(r) = outcome2 else {
        panic!("expected Succeeded on resume, got {outcome2:?}");
    };
    assert_eq!(r.candidate_count, N);
    assert!(r.passed, "good docs should pass: {r:?}");
    assert!(!r.qc_run_id.is_empty());

    let cp2 = matter
        .get_checkpoint(&job.id, QC_STAGE)
        .expect("cp2")
        .expect("final checkpoint");
    let cursor2: serde_json::Value =
        serde_json::from_str(&cp2.cursor_json).expect("checkpoint json");
    assert_eq!(cursor2["cursor_index"].as_u64().unwrap_or(0), N);
    assert_eq!(cursor2["phase"].as_str(), Some("done"));
    assert_eq!(cp2.completed_count as u64, N);

    let run = matter
        .load_latest_qc_run()
        .expect("load")
        .expect("qc_runs after full success");
    assert_eq!(run.candidate_count, N);
    assert!(run.passed);
}
