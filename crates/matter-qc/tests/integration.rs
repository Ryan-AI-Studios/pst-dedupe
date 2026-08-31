//! Production QC integration tests (spec §3.11).

#![allow(clippy::field_reassign_with_default)]

use std::fs;

use matter_core::{
    geom_source, item_role, item_status, CreateGeomRedactionInput, CreateRedactionInput, ItemInput,
    Matter, UpsertItemPrivilegeInput, FAMILY_KIND_EMAIL_ATTACHMENTS, SCHEMA_VERSION,
};
use matter_qc::{
    evaluate_candidates_with_cancel, resolve_rules, run_production_qc, QcError, QcOutcome,
    QcParams, QcRuleConfig, QcSeverity, JOB_KIND_QC, PACK_IMAGE_OPT_V1,
    RULE_BROKEN_FAMILY_INCOMPLETE_PARENT, RULE_BROKEN_FAMILY_ORPHAN_CHILD,
    RULE_BURNED_NATIVE_MISSING, RULE_EMPTY_SELECTION, RULE_IMAGE_PAGE_MISSING, RULE_MISSING_NATIVE,
    RULE_MISSING_TEXT, RULE_ONLY_WITHHELD, RULE_OPT_ROW_COUNT_MISMATCH, RULE_PDF_NEEDS_OCR,
    RULE_REDACTED_TEXT_MISSING, RULE_TEXT_REDACT_UNMAPPED_ON_PDF, RULE_WITHHELD_FAMILY_MEMBER,
    RULE_WITHHELD_IN_SELECTION, RULE_ZERO_SIZE,
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
    assert_eq!(SCHEMA_VERSION, 41);
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

// ---------------------------------------------------------------------------
// 0060 QC packs
// ---------------------------------------------------------------------------

#[test]
fn strict_pack_escalates_withheld_family_to_error() {
    use matter_qc::{
        resolve_rules_for_pack, PACK_DEFAULT_V1, PACK_STRICT_PRIVILEGE_V1,
        RULE_BROKEN_FAMILY_INCOMPLETE_PARENT, RULE_WITHHELD_FAMILY_MEMBER,
    };

    let def = resolve_rules_for_pack(PACK_DEFAULT_V1, &[]);
    assert_eq!(def.severity(RULE_WITHHELD_FAMILY_MEMBER), QcSeverity::Warn);
    assert_eq!(
        def.severity(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT),
        QcSeverity::Warn
    );

    let strict = resolve_rules_for_pack(PACK_STRICT_PRIVILEGE_V1, &[]);
    assert_eq!(
        strict.severity(RULE_WITHHELD_FAMILY_MEMBER),
        QcSeverity::Error
    );
    assert_eq!(
        strict.severity(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT),
        QcSeverity::Error
    );
}

#[test]
fn strict_pack_fails_where_default_warns_incomplete_family() {
    use matter_qc::{PACK_DEFAULT_V1, PACK_STRICT_PRIVILEGE_V1};

    let (_tmp, matter) = temp_matter("strict-family");
    // Parent + 0 of 2 non-withheld kids → incomplete (warn under default).
    let (parent, family_id) = insert_family_parent(&matter, "p.eml", 1);
    for i in 0..2 {
        insert_child(&matter, &parent, &family_id, &format!("c{i}.bin"), 0);
    }

    let job_def = matter.create_job(JOB_KIND_QC).expect("job");
    let r_def = run_qc(
        &matter,
        &job_def.id,
        &QcParams {
            pack_id: Some(PACK_DEFAULT_V1.into()),
            ..Default::default()
        },
    );
    let inc_def = findings_of(&r_def, RULE_BROKEN_FAMILY_INCOMPLETE_PARENT);
    assert!(!inc_def.is_empty(), "default pack should find incomplete");
    assert_eq!(inc_def[0].severity, QcSeverity::Warn);
    assert!(r_def.passed, "warns alone still pass");

    let job_strict = matter.create_job(JOB_KIND_QC).expect("job2");
    let r_strict = run_qc(
        &matter,
        &job_strict.id,
        &QcParams {
            pack_id: Some(PACK_STRICT_PRIVILEGE_V1.into()),
            ..Default::default()
        },
    );
    let inc_s = findings_of(&r_strict, RULE_BROKEN_FAMILY_INCOMPLETE_PARENT);
    assert!(!inc_s.is_empty());
    assert_eq!(inc_s[0].severity, QcSeverity::Error);
    assert!(!r_strict.passed, "strict pack must fail produce gate");
}

#[test]
fn fingerprint_includes_pack_id() {
    use matter_core::{
        selection_fingerprint_with_pack, QC_PACK_DEFAULT_V1, QC_PACK_STRICT_PRIVILEGE_V1,
    };

    let ids = vec!["a".into(), "b".into()];
    let fp_def = selection_fingerprint_with_pack(&ids, QC_PACK_DEFAULT_V1);
    let fp_strict = selection_fingerprint_with_pack(&ids, QC_PACK_STRICT_PRIVILEGE_V1);
    assert_ne!(fp_def, fp_strict);
    let fp_empty = selection_fingerprint_with_pack(&ids, "");
    assert_ne!(fp_def, fp_empty);
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

#[test]
fn burned_native_missing_error() {
    let (_tmp, matter) = temp_matter("qc-burn-miss");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let native = put_native(&matter, b"%PDF-1.4 SECRET_TOKEN_0114");
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("s.pdf".into()),
            native_sha256: Some(native),
            mime_type: Some("application/pdf".into()),
            file_category: Some("pdf".into()),
            size_bytes: Some(10),
            ..Default::default()
        },
    );
    matter
        .create_geom_redaction(CreateGeomRedactionInput {
            item_id: id.clone(),
            page_index: 0,
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
            reason: "privilege".into(),
            label: None,
            source: geom_source::DRAW.into(),
            actor: "t".into(),
        })
        .expect("geom");
    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.passed);
    let f = findings_of(&r, RULE_BURNED_NATIVE_MISSING);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].severity, QcSeverity::Error);
    assert_eq!(f[0].item_id.as_deref(), Some(id.as_str()));
}

#[test]
fn text_redact_unmapped_on_pdf_error() {
    let (_tmp, matter) = temp_matter("qc-unmap");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let body = "Alpha SECRET beta";
    let text_sha = put_text(&matter, body);
    let native = put_native(&matter, b"%PDF-1.4 SECRET");
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("s.pdf".into()),
            native_sha256: Some(native),
            text_sha256: Some(text_sha.clone()),
            mime_type: Some("application/pdf".into()),
            file_category: Some("pdf".into()),
            size_bytes: Some(10),
            ..Default::default()
        },
    );
    matter
        .create_redaction(CreateRedactionInput {
            item_id: id.clone(),
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
    let r = run_qc(&matter, &job.id, &QcParams::default());
    assert!(!r.passed);
    let f = findings_of(&r, RULE_TEXT_REDACT_UNMAPPED_ON_PDF);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].severity, QcSeverity::Error);
}

#[test]
fn run_production_qc_image_pack_finds_missing_tiff() {
    let (_tmp, matter) = temp_matter("0115-qc-img");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let id = good_doc(&matter, "scan.pdf");
    let vol = matter
        .root()
        .join("exports")
        .join("productions")
        .join("QCIMG");
    fs::create_dir_all(vol.as_std_path()).expect("vol");
    let now = "2020-01-01T00:00:00Z";
    matter
        .connection()
        .execute(
            "INSERT INTO production_sets \
             (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status, output_root, profile_slug) \
             VALUES ('set_qc_img', ?1, 'QCIMG', ?2, ?2, 'PROD', 2, 'complete', ?3, 'us_concordance_image_opt_v1')",
            rusqlite::params![matter.id(), now, vol.as_str()],
        )
        .expect("set");
    matter
        .connection()
        .execute(
            "INSERT INTO production_items \
             (production_set_id, item_id, control_number, status, produced_at, end_bates, page_count) \
             VALUES ('set_qc_img', ?1, 'PROD000001', 'ok', ?2, 'PROD000001', 1)",
            rusqlite::params![&id, now],
        )
        .expect("item");
    matter
        .connection()
        .execute(
            "INSERT INTO production_image_pages \
             (production_set_id, item_id, page_index, bates, relpath, sha256) \
             VALUES ('set_qc_img', ?1, 0, 'PROD000001', 'IMAGES\\001\\PROD000001.TIF', \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')",
            rusqlite::params![&id],
        )
        .expect("page");
    let r = run_qc(
        &matter,
        &job.id,
        &QcParams {
            pack_id: Some(PACK_IMAGE_OPT_V1.into()),
            ..Default::default()
        },
    );
    assert!(!r.passed, "missing TIFF must fail QC");
    let f = findings_of(&r, RULE_IMAGE_PAGE_MISSING);
    assert!(
        !f.is_empty(),
        "run_production_qc must evaluate image_page_missing, findings={:?}",
        r.findings
    );
}

#[test]
fn run_production_qc_image_pack_respects_item_scope() {
    let (_tmp, matter) = temp_matter("0115-qc-scope");
    let broken = good_doc(&matter, "broken.pdf");
    let other = good_doc(&matter, "other.pdf");
    let vol = matter
        .root()
        .join("exports")
        .join("productions")
        .join("QCSCOPE");
    fs::create_dir_all(vol.as_std_path()).expect("vol");
    let now = "2020-01-01T00:00:00Z";
    matter
        .connection()
        .execute(
            "INSERT INTO production_sets \
             (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status, output_root, profile_slug) \
             VALUES ('set_qc_scope', ?1, 'QCSCOPE', ?2, ?2, 'PROD', 2, 'complete', ?3, 'us_concordance_image_opt_v1')",
            rusqlite::params![matter.id(), now, vol.as_str()],
        )
        .expect("set");
    matter
        .connection()
        .execute(
            "INSERT INTO production_items \
             (production_set_id, item_id, control_number, status, produced_at, end_bates, page_count) \
             VALUES ('set_qc_scope', ?1, 'PROD000001', 'ok', ?2, 'PROD000001', 1)",
            rusqlite::params![&broken, now],
        )
        .expect("item");
    matter
        .connection()
        .execute(
            "INSERT INTO production_image_pages \
             (production_set_id, item_id, page_index, bates, relpath, sha256) \
             VALUES ('set_qc_scope', ?1, 0, 'PROD000001', 'IMAGES\\001\\PROD000001.TIF', \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')",
            rusqlite::params![&broken],
        )
        .expect("page");

    let job_other = matter.create_job(JOB_KIND_QC).expect("job other");
    let r_other = run_qc(
        &matter,
        &job_other.id,
        &QcParams {
            scope: "item_ids".into(),
            item_ids: vec![other],
            pack_id: Some(PACK_IMAGE_OPT_V1.into()),
            ..Default::default()
        },
    );
    assert!(
        findings_of(&r_other, RULE_IMAGE_PAGE_MISSING).is_empty(),
        "scoped QC of unrelated item must not inherit another item's missing TIFF: {:?}",
        r_other.findings
    );

    let job_broken = matter.create_job(JOB_KIND_QC).expect("job broken");
    let r_broken = run_qc(
        &matter,
        &job_broken.id,
        &QcParams {
            scope: "item_ids".into(),
            item_ids: vec![broken],
            pack_id: Some(PACK_IMAGE_OPT_V1.into()),
            ..Default::default()
        },
    );
    assert!(
        !findings_of(&r_broken, RULE_IMAGE_PAGE_MISSING).is_empty(),
        "scoped QC of the broken item must still find the missing TIFF"
    );
}

#[test]
fn run_production_qc_image_pack_empty_selection_skips_volume() {
    let (_tmp, matter) = temp_matter("0115-qc-empty-vol");
    let vol = matter
        .root()
        .join("exports")
        .join("productions")
        .join("QCEMPTY");
    fs::create_dir_all(vol.as_std_path()).expect("vol");
    fs::write(
        vol.join("IMAGE.opt").as_std_path(),
        "VOL001,IMAGES\\001\\PROD000001.TIF,Y,,,PROD000001,\r\n",
    )
    .expect("opt");
    let now = "2020-01-01T00:00:00Z";
    matter
        .connection()
        .execute(
            "INSERT INTO production_sets \
             (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status, output_root, profile_slug) \
             VALUES ('set_qc_empty', ?1, 'QCEMPTY', ?2, ?2, 'PROD', 2, 'complete', ?3, 'us_concordance_image_opt_v1')",
            rusqlite::params![matter.id(), now, vol.as_str()],
        )
        .expect("set");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let r = run_qc(
        &matter,
        &job.id,
        &QcParams {
            pack_id: Some(PACK_IMAGE_OPT_V1.into()),
            ..Default::default()
        },
    );
    assert_eq!(
        findings_of(&r, RULE_EMPTY_SELECTION).len(),
        1,
        "empty corpus must still report empty_selection"
    );
    assert!(
        findings_of(&r, RULE_IMAGE_PAGE_MISSING).is_empty(),
        "empty selection must not inherit unrelated volume TIFF findings: {:?}",
        r.findings
    );
    assert!(
        findings_of(&r, RULE_OPT_ROW_COUNT_MISMATCH).is_empty(),
        "empty selection must not inherit unrelated OPT count findings: {:?}",
        r.findings
    );
}

#[test]
fn run_production_qc_image_pack_missing_opt_on_completed_volume() {
    use sha2::{Digest, Sha256};
    let (_tmp, matter) = temp_matter("0115-qc-no-opt");
    let id = good_doc(&matter, "scan.pdf");
    let vol = matter
        .root()
        .join("exports")
        .join("productions")
        .join("QCNOOPT");
    let tif_dir = vol.join("IMAGES").join("001");
    fs::create_dir_all(tif_dir.as_std_path()).expect("dir");
    let tif_bytes = b"II*\0fake-tif";
    fs::write(tif_dir.join("PROD000001.TIF").as_std_path(), tif_bytes).expect("tif");
    let sha = Sha256::digest(tif_bytes);
    let sha_hex: String = sha.iter().map(|b| format!("{b:02x}")).collect();
    let now = "2020-01-01T00:00:00Z";
    matter
        .connection()
        .execute(
            "INSERT INTO production_sets \
             (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status, output_root, profile_slug) \
             VALUES ('set_qc_noopt', ?1, 'QCNOOPT', ?2, ?2, 'PROD', 2, 'complete', ?3, 'us_concordance_image_opt_v1')",
            rusqlite::params![matter.id(), now, vol.as_str()],
        )
        .expect("set");
    matter
        .connection()
        .execute(
            "INSERT INTO production_items \
             (production_set_id, item_id, control_number, status, produced_at, end_bates, page_count) \
             VALUES ('set_qc_noopt', ?1, 'PROD000001', 'ok', ?2, 'PROD000001', 1)",
            rusqlite::params![&id, now],
        )
        .expect("item");
    matter
        .connection()
        .execute(
            "INSERT INTO production_image_pages \
             (production_set_id, item_id, page_index, bates, relpath, sha256) \
             VALUES ('set_qc_noopt', ?1, 0, 'PROD000001', 'IMAGES\\001\\PROD000001.TIF', ?2)",
            rusqlite::params![&id, sha_hex],
        )
        .expect("page");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let r = run_qc(
        &matter,
        &job.id,
        &QcParams {
            pack_id: Some(PACK_IMAGE_OPT_V1.into()),
            ..Default::default()
        },
    );
    assert!(
        !r.passed,
        "missing IMAGE.opt on a completed image volume must fail"
    );
    assert!(
        !findings_of(&r, RULE_OPT_ROW_COUNT_MISMATCH).is_empty(),
        "must emit opt_row_count_mismatch when OPT is absent: {:?}",
        r.findings
    );
}

#[test]
fn run_production_qc_image_pack_zero_page_pdf_is_missing() {
    let (_tmp, matter) = temp_matter("0115-qc-zero-pc");
    let id = good_doc(&matter, "scan.pdf");
    let vol = matter
        .root()
        .join("exports")
        .join("productions")
        .join("QCZERO");
    fs::create_dir_all(vol.as_std_path()).expect("vol");
    let now = "2020-01-01T00:00:00Z";
    matter
        .connection()
        .execute(
            "INSERT INTO production_sets \
             (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status, output_root, profile_slug) \
             VALUES ('set_qc_zero', ?1, 'QCZERO', ?2, ?2, 'PROD', 2, 'complete', ?3, 'us_concordance_image_opt_v1')",
            rusqlite::params![matter.id(), now, vol.as_str()],
        )
        .expect("set");
    matter
        .connection()
        .execute(
            "INSERT INTO production_items \
             (production_set_id, item_id, control_number, status, produced_at, end_bates, page_count) \
             VALUES ('set_qc_zero', ?1, 'PROD000001', 'ok', ?2, 'PROD000001', 0)",
            rusqlite::params![&id, now],
        )
        .expect("item");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let r = run_qc(
        &matter,
        &job.id,
        &QcParams {
            pack_id: Some(PACK_IMAGE_OPT_V1.into()),
            ..Default::default()
        },
    );
    assert!(
        !findings_of(&r, RULE_IMAGE_PAGE_MISSING).is_empty(),
        "image-eligible page_count=0 must fail image QC: {:?}",
        r.findings
    );
}

#[test]
fn run_production_qc_image_pack_magic_only_pdf_zero_pages() {
    let (_tmp, matter) = temp_matter("0115-qc-magic");
    let pdf = b"%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF\n";
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: None,
            native_sha256: Some(put_native(&matter, pdf)),
            text_sha256: Some(put_text(&matter, "p")),
            mime_type: None,
            file_category: Some("document".into()),
            size_bytes: Some(pdf.len() as i64),
            ..Default::default()
        },
    );
    let vol = matter
        .root()
        .join("exports")
        .join("productions")
        .join("QCMAGIC");
    fs::create_dir_all(vol.as_std_path()).expect("vol");
    let now = "2020-01-01T00:00:00Z";
    matter
        .connection()
        .execute(
            "INSERT INTO production_sets \
             (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status, output_root, profile_slug) \
             VALUES ('set_qc_magic', ?1, 'QCMAGIC', ?2, ?2, 'PROD', 2, 'complete', ?3, 'us_concordance_image_opt_v1')",
            rusqlite::params![matter.id(), now, vol.as_str()],
        )
        .expect("set");
    matter
        .connection()
        .execute(
            "INSERT INTO production_items \
             (production_set_id, item_id, control_number, status, produced_at, end_bates, page_count) \
             VALUES ('set_qc_magic', ?1, 'PROD000001', 'ok', ?2, 'PROD000001', 0)",
            rusqlite::params![&id, now],
        )
        .expect("item");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let r = run_qc(
        &matter,
        &job.id,
        &QcParams {
            pack_id: Some(PACK_IMAGE_OPT_V1.into()),
            ..Default::default()
        },
    );
    assert!(
        !findings_of(&r, RULE_IMAGE_PAGE_MISSING).is_empty(),
        "magic-only PDF with page_count=0 must fail image QC: {:?}",
        r.findings
    );
}

#[test]
fn run_production_qc_image_pack_skips_dat_only_sets() {
    use matter_core::BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1;
    let (_tmp, matter) = temp_matter("0115-qc-dat-only");
    let id = good_doc(&matter, "scan.pdf");
    let vol = matter
        .root()
        .join("exports")
        .join("productions")
        .join("QCDAT");
    fs::create_dir_all(vol.as_std_path()).expect("vol");
    let now = "2020-01-01T00:00:00Z";
    matter
        .connection()
        .execute(
            "INSERT INTO production_sets \
             (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status, output_root, profile_slug) \
             VALUES ('set_qc_dat', ?1, 'QCDAT', ?2, ?2, 'PROD', 2, 'complete', ?3, ?4)",
            rusqlite::params![
                matter.id(),
                now,
                vol.as_str(),
                BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1
            ],
        )
        .expect("set");
    matter
        .connection()
        .execute(
            "INSERT INTO production_items \
             (production_set_id, item_id, control_number, status, produced_at, end_bates, page_count) \
             VALUES ('set_qc_dat', ?1, 'PROD000001', 'ok', ?2, 'PROD000001', 0)",
            rusqlite::params![&id, now],
        )
        .expect("item");
    let job = matter.create_job(JOB_KIND_QC).expect("job");
    let r = run_qc(
        &matter,
        &job.id,
        &QcParams {
            pack_id: Some(PACK_IMAGE_OPT_V1.into()),
            ..Default::default()
        },
    );
    assert!(
        findings_of(&r, RULE_IMAGE_PAGE_MISSING).is_empty(),
        "DAT-only PDF rows must not inherit image_page_missing: {:?}",
        r.findings
    );
}
