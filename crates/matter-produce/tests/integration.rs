//! Production export integration tests (spec §4.12).

#![allow(clippy::field_reassign_with_default)]

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use matter_core::{
    item_role, item_status, CreateRedactionInput, ItemInput, Matter, UpsertItemPrivilegeInput,
    UpsertNoteInput, REDACTED_TOKEN, SCHEMA_VERSION,
};
use matter_produce::{
    encode_dat_field, format_utc_datetime, run_produce, ProduceOutcome, ProduceParams, DAT_FIELDS,
    DAT_NEWLINE, JOB_KIND_PRODUCE, PRODUCE_STAGE, UTF8_BOM,
};
use sha2::{Digest, Sha256};

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

/// Packaging tests opt out of the 0041 QC gate (covered in dedicated gate tests).
fn no_qc_gate(mut params: ProduceParams) -> ProduceParams {
    params.require_qc_pass = false;
    params
}

fn run_ok(matter: &Matter, job_id: &str, params: &ProduceParams) -> matter_produce::ProduceSummary {
    let p = no_qc_gate(params.clone());
    match run_produce(matter, job_id, &p, None, |_| {}).expect("run") {
        ProduceOutcome::Succeeded(s) => s,
        other => panic!("expected Succeeded, got {other:?}"),
    }
}

/// Direct `run_produce` with QC gate disabled (0040 packaging tests).
fn run_produce_pkg(
    matter: &Matter,
    job_id: &str,
    params: &ProduceParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<ProduceOutcome, matter_produce::ProduceError> {
    let p = no_qc_gate(params.clone());
    run_produce(matter, job_id, &p, cancel, progress)
}

fn read_dat(root: &str) -> Vec<u8> {
    let path = camino::Utf8Path::new(root).join("DATA").join("load.dat");
    fs::read(path.as_std_path()).expect("read dat")
}

fn dat_text(root: &str) -> String {
    let bytes = read_dat(root);
    assert!(
        bytes.starts_with(&UTF8_BOM),
        "DAT must start with UTF-8 BOM"
    );
    String::from_utf8(bytes[3..].to_vec()).expect("utf8 after bom")
}

fn sha256_file(path: &std::path::Path) -> String {
    let bytes = fs::read(path).expect("read");
    let d = Sha256::digest(&bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Schema v20 tables present.
#[test]
fn schema_v20_production_tables() {
    let (_tmp, matter) = temp_matter("schema-v20");
    assert_eq!(SCHEMA_VERSION, 24);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    for table in ["production_sets", "production_items"] {
        let has: bool = matter
            .connection()
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("table");
        assert!(has, "missing {table}");
    }
}

/// 1. Happy path: 2–3 in_review → DAT/NATIVES/TEXT; SHA match.
#[test]
fn happy_path_natives_text_dat() {
    let (_tmp, matter) = temp_matter("happy");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");

    let n1 = put_native(&matter, b"native-one-bytes");
    let t1 = put_text(&matter, "text one body");
    let n2 = put_native(&matter, b"native-two-bytes");
    let t2 = put_text(&matter, "text two body");
    let n3 = put_native(&matter, b"native-three");
    let t3 = put_text(&matter, "text three");

    insert_review_item(
        &matter,
        ItemInput {
            path: Some("docs/one.pdf".into()),
            native_sha256: Some(n1),
            text_sha256: Some(t1),
            file_category: Some("document".into()),
            mime_type: Some("application/pdf".into()),
            subject: Some("One".into()),
            custodian: Some("Alice".into()),
            ..Default::default()
        },
    );
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("docs/two.pdf".into()),
            native_sha256: Some(n2),
            text_sha256: Some(t2),
            file_category: Some("document".into()),
            mime_type: Some("application/pdf".into()),
            subject: Some("Two".into()),
            ..Default::default()
        },
    );
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("docs/three.txt".into()),
            native_sha256: Some(n3),
            text_sha256: Some(t3),
            file_category: Some("document".into()),
            subject: Some("Three".into()),
            ..Default::default()
        },
    );

    let params = ProduceParams {
        name: Some("HappyProd".into()),
        bates_prefix: "PROD".into(),
        ..Default::default()
    };
    let s = run_ok(&matter, &job.id, &params);
    assert_eq!(s.produced_count, 3);
    assert_eq!(s.selected_count, 3);
    assert_eq!(s.skipped_withheld, 0);

    let root = &s.output_root;
    let dat = dat_text(root);
    assert!(dat.contains("BEGBATES"));
    assert!(dat.contains("PROD000001"));
    assert!(dat.contains("PROD000002"));
    assert!(dat.contains("PROD000003"));
    assert!(dat.contains("NATIVES\\PROD000001.pdf") || dat.contains("NATIVES\\PROD000001."));

    // SHA of produced native matches DAT and file on disk.
    let native_path = camino::Utf8Path::new(root)
        .join("NATIVES")
        .join("PROD000001.pdf");
    assert!(native_path.as_std_path().exists());
    let disk_sha = sha256_file(native_path.as_std_path());
    assert!(dat.contains(&disk_sha));
    assert_eq!(
        fs::read(native_path.as_std_path()).unwrap(),
        b"native-one-bytes"
    );

    let text_path = camino::Utf8Path::new(root)
        .join("TEXT")
        .join("PROD000001.txt");
    assert_eq!(
        fs::read_to_string(text_path.as_std_path()).unwrap(),
        "text one body"
    );

    // CSV twin
    let csv = camino::Utf8Path::new(root).join("DATA").join("load.csv");
    assert!(csv.as_std_path().exists());
    let csv_bytes = fs::read(csv.as_std_path()).unwrap();
    assert!(csv_bytes.starts_with(&UTF8_BOM));

    // README present
    assert!(camino::Utf8Path::new(root)
        .join("README.txt")
        .as_std_path()
        .exists());
}

/// 2. DAT BOM EF BB BF.
#[test]
fn dat_has_utf8_bom() {
    let (_tmp, matter) = temp_matter("bom");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"x");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.bin".into()),
            native_sha256: Some(n),
            ..Default::default()
        },
    );
    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Bom".into()),
            ..Default::default()
        },
    );
    let bytes = read_dat(&s.output_root);
    assert_eq!(&bytes[0..3], &UTF8_BOM);
}

/// 3. Withheld skipped — not in DAT/files.
#[test]
fn withhold_skipped_not_in_volume() {
    let (_tmp, matter) = temp_matter("withhold-skip");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n_ok = put_native(&matter, b"ok-native");
    let n_hold = put_native(&matter, b"secret-native");

    let ok_id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("ok.pdf".into()),
            native_sha256: Some(n_ok),
            subject: Some("OK Doc".into()),
            ..Default::default()
        },
    );
    let hold_id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("secret.pdf".into()),
            native_sha256: Some(n_hold),
            subject: Some("SECRET PRIVILEGED".into()),
            ..Default::default()
        },
    );
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: hold_id.clone(),
            basis: "attorney_client".into(),
            description: "Client legal advice re merger".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "tester".into(),
        })
        .expect("withhold");

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Wh".into()),
            ..Default::default()
        },
    );
    assert_eq!(s.produced_count, 1);
    assert_eq!(s.skipped_withheld, 1);

    let dat = dat_text(&s.output_root);
    assert!(dat.contains(&ok_id));
    assert!(!dat.contains(&hold_id));
    assert!(!dat.contains("SECRET PRIVILEGED"));
    assert!(!dat.contains("Client legal advice"));

    let natives = camino::Utf8Path::new(&s.output_root).join("NATIVES");
    let entries: Vec<_> = fs::read_dir(natives.as_std_path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries.len(), 1);
    let secret_bytes = b"secret-native";
    for name in &entries {
        let p = natives.join(name);
        let b = fs::read(p.as_std_path()).unwrap();
        assert_ne!(b.as_slice(), secret_bytes);
    }
}

/// 4. fail_if_withheld aborts.
#[test]
fn fail_if_withheld_aborts() {
    let (_tmp, matter) = temp_matter("withhold-fail");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"held");
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("h.pdf".into()),
            native_sha256: Some(n),
            ..Default::default()
        },
    );
    // Second non-withheld item so selected_count > 1 is meaningful.
    let n2 = put_native(&matter, b"ok");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("ok.pdf".into()),
            native_sha256: Some(n2),
            ..Default::default()
        },
    );
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: id,
            basis: "work_product".into(),
            description: "WP".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "t".into(),
        })
        .expect("priv");

    let outcome = run_produce_pkg(
        &matter,
        &job.id,
        &ProduceParams {
            fail_if_withheld: true,
            name: Some("FailWh".into()),
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run");
    match outcome {
        ProduceOutcome::Failed { message, summary } => {
            assert!(message.contains("fail_if_withheld"), "{message}");
            assert!(
                summary.selected_count > 0,
                "fail_if_withheld pre-scan must report selected_count, got {}",
                summary.selected_count
            );
            assert_eq!(summary.selected_count, 2);
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// 5. Redaction TEXT is [REDACTED] not original.
#[test]
fn redaction_uses_redacted_text() {
    let (_tmp, matter) = temp_matter("rdx-ok");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let body = "Alpha SECRET beta";
    let text_sha = put_text(&matter, body);
    let native = put_native(&matter, b"n");
    let item_id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("r.txt".into()),
            native_sha256: Some(native),
            text_sha256: Some(text_sha.clone()),
            subject: Some("Redact me".into()),
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
    matter
        .regenerate_redacted_text(&item_id, body, "t")
        .expect("regen");

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Rdx".into()),
            ..Default::default()
        },
    );
    let text_path = camino::Utf8Path::new(&s.output_root)
        .join("TEXT")
        .join("PROD000001.txt");
    let produced = fs::read_to_string(text_path.as_std_path()).unwrap();
    assert!(produced.contains(REDACTED_TOKEN));
    assert!(!produced.contains("SECRET"));
    let dat = dat_text(&s.output_root);
    assert!(dat.contains("HAS_REDACTED_TEXT"));
    assert!(dat.contains("Y"));
}

/// 6. Missing redacted artifact → error; original not in TEXT/.
#[test]
fn redaction_missing_artifact_errors() {
    let (_tmp, matter) = temp_matter("rdx-miss");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let body = "Alpha SECRET beta";
    let text_sha = put_text(&matter, body);
    let native = put_native(&matter, b"n");
    let item_id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("r.txt".into()),
            native_sha256: Some(native),
            text_sha256: Some(text_sha.clone()),
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
            body_digest: text_sha.clone(),
            reason: "confidential".into(),
            label: None,
            actor: "t".into(),
        })
        .expect("redaction");
    // Intentionally do NOT regenerate redacted_text_sha256.
    let item = matter.get_item(&item_id).unwrap();
    assert!(item.redaction_count > 0);
    assert!(item.redacted_text_sha256.is_none());

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("RdxMiss".into()),
            ..Default::default()
        },
    );
    assert_eq!(s.error_count, 1);
    assert_eq!(s.produced_count, 0);

    let text_dir = camino::Utf8Path::new(&s.output_root).join("TEXT");
    if text_dir.as_std_path().exists() {
        for e in fs::read_dir(text_dir.as_std_path()).unwrap() {
            let p = e.unwrap().path();
            let content = fs::read_to_string(&p).unwrap_or_default();
            assert!(
                !content.contains("SECRET"),
                "original text must not appear in TEXT/: {content}"
            );
            assert_ne!(content, body);
        }
    }
    // Original CAS still intact.
    let orig = matter.get_bytes(&text_sha).unwrap();
    assert_eq!(orig, body.as_bytes());

    // F-002: no orphan native for the failed control under NATIVES/.
    let natives_dir = camino::Utf8Path::new(&s.output_root).join("NATIVES");
    if natives_dir.as_std_path().exists() {
        for e in fs::read_dir(natives_dir.as_std_path()).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            assert!(
                !name.starts_with("PROD"),
                "orphan native left for failed redacted item: {name}"
            );
        }
    }
}

/// 7. Privilege description not in DAT.
#[test]
fn privilege_description_not_in_dat() {
    let (_tmp, matter) = temp_matter("priv-desc");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"doc");
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("d.pdf".into()),
            native_sha256: Some(n),
            subject: Some("Public subject".into()),
            ..Default::default()
        },
    );
    // Privilege claim without withhold — description must still never appear in DAT.
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: id,
            basis: "attorney_client".into(),
            description: "UNIQUE_PRIV_DESC_TOKEN_XYZ".into(),
            status: "asserted".into(),
            withhold: false,
            include_on_log: true,
            actor: "t".into(),
        })
        .expect("priv");

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("PrivD".into()),
            ..Default::default()
        },
    );
    let dat = dat_text(&s.output_root);
    assert!(!dat.contains("UNIQUE_PRIV_DESC_TOKEN_XYZ"));
    assert!(!dat.contains("attorney_client"));
    for f in DAT_FIELDS {
        assert!(!f.to_ascii_lowercase().contains("privilege"));
        assert!(!f.to_ascii_lowercase().contains("description"));
    }
}

/// 8. Notes not in DAT.
#[test]
fn notes_not_in_dat() {
    let (_tmp, matter) = temp_matter("notes");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"doc");
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("d.pdf".into()),
            native_sha256: Some(n),
            subject: Some("Subj".into()),
            ..Default::default()
        },
    );
    matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: id,
            body: "UNIQUE_NOTE_BODY_TOKEN_ABC work product".into(),
            highlight_id: None,
            actor: "t".into(),
        })
        .expect("note");

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Notes".into()),
            ..Default::default()
        },
    );
    let dat = dat_text(&s.output_root);
    assert!(!dat.contains("UNIQUE_NOTE_BODY_TOKEN_ABC"));
    assert!(!dat.to_ascii_lowercase().contains("work product"));
}

/// 9. ICS child uses child native (not parent archive).
#[test]
fn ics_child_uses_child_native() {
    let (_tmp, matter) = temp_matter("ics-child");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let parent_native = put_native(&matter, b"PARENT_MULTI_EVENT_ICS");
    let child_native = put_native(&matter, b"CHILD_SINGLE_EVENT");
    let family = matter.insert_family("email_attachments").expect("family");

    let parent_id = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            path: Some("calendar.ics".into()),
            native_sha256: Some(parent_native),
            file_category: Some("calendar".into()),
            family_id: Some(family.id.clone()),
            in_review: Some(0),
            ..Default::default()
        })
        .expect("parent")
        .id;

    insert_review_item(
        &matter,
        ItemInput {
            path: Some("event.ics".into()),
            native_sha256: Some(child_native),
            parent_item_id: Some(parent_id),
            family_id: Some(family.id),
            role: Some(item_role::ATTACHMENT.into()),
            file_category: Some("calendar".into()),
            mime_type: Some("text/calendar".into()),
            subject: Some("Standup".into()),
            ..Default::default()
        },
    );

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Ics".into()),
            expand_family: false,
            ..Default::default()
        },
    );
    assert_eq!(s.produced_count, 1);
    let natives = camino::Utf8Path::new(&s.output_root).join("NATIVES");
    let mut found_child = false;
    for e in fs::read_dir(natives.as_std_path()).unwrap() {
        let b = fs::read(e.unwrap().path()).unwrap();
        assert_ne!(b.as_slice(), b"PARENT_MULTI_EVENT_ICS");
        if b.as_slice() == b"CHILD_SINGLE_EVENT" {
            found_child = true;
        }
    }
    assert!(found_child, "child native bytes must be produced");
}

/// 10. Synthetic EML → .eml + FILE_EXT match.
#[test]
fn synthetic_eml_ext_matches() {
    let (_tmp, matter) = temp_matter("eml");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let text = put_text(&matter, "Hello body");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("inbox/msg.msg".into()), // stale path ext
            // no native_sha256
            text_sha256: Some(text),
            file_category: Some("email".into()),
            mime_type: Some("application/vnd.ms-outlook".into()),
            from_addr: Some("a@x.com".into()),
            to_addrs_json: Some(r#"["b@y.com"]"#.into()),
            subject: Some("Hello".into()),
            sent_at: Some("2026-07-19T12:00:00Z".into()),
            message_id: Some("mid-1".into()),
            ..Default::default()
        },
    );

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Eml".into()),
            export_eml_if_missing_native: true,
            ..Default::default()
        },
    );
    assert_eq!(s.produced_count, 1);
    let eml = camino::Utf8Path::new(&s.output_root)
        .join("NATIVES")
        .join("PROD000001.eml");
    assert!(eml.as_std_path().exists(), "expected .eml on disk");
    let dat = dat_text(&s.output_root);
    assert!(dat.contains("NATIVES\\PROD000001.eml"));
    // FILE_EXT field is `eml` not msg
    assert!(dat.contains("þemlþ") || dat.contains("eml"));
    assert!(!dat.contains("NATIVES\\PROD000001.msg"));
}

/// 11. UTC dates.
#[test]
fn utc_dates_in_dat() {
    let (_tmp, matter) = temp_matter("utc");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"x");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.eml".into()),
            native_sha256: Some(n),
            sent_at: Some("2026-07-19T15:00:00+03:00".into()),
            received_at: Some("2026-07-19T12:30:00Z".into()),
            created_at: Some("2026-07-18T00:00:00Z".into()),
            subject: Some("dated".into()),
            file_category: Some("email".into()),
            ..Default::default()
        },
    );
    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Utc".into()),
            ..Default::default()
        },
    );
    let dat = dat_text(&s.output_root);
    assert!(dat.contains("2026-07-19T12:00:00Z"), "offset→UTC: {dat}");
    assert!(dat.contains("2026-07-19T12:30:00Z"));
    // No zone-less local float for our known fields
    assert!(!dat.contains("2026-07-19T15:00:00¶") && !dat.contains("2026-07-19T15:00:00þ"));
    assert_eq!(
        format_utc_datetime(Some("2026-07-19T15:00:00+03:00")),
        "2026-07-19T12:00:00Z"
    );
    // Zone-less inputs must remain empty (never invent Z).
    assert_eq!(format_utc_datetime(Some("2026-07-19T12:00:00")), "");
    assert_eq!(format_utc_datetime(Some("2026-07-19 12:00:00")), "");
}

/// 12. Multi-line → ® in DAT.
#[test]
fn multiline_becomes_registered_mark_in_dat() {
    let (_tmp, matter) = temp_matter("ml");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"x");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.eml".into()),
            native_sha256: Some(n),
            subject: Some("Line1\nLine2".into()),
            to_addrs_json: Some(r#"["a@x.com","b@y.com"]"#.into()),
            file_category: Some("email".into()),
            ..Default::default()
        },
    );
    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Ml".into()),
            ..Default::default()
        },
    );
    let dat = dat_text(&s.output_root);
    let encoded = encode_dat_field("Line1\nLine2");
    assert!(encoded.contains(DAT_NEWLINE));
    assert!(dat.contains(&encoded), "dat missing ® subject: {dat}");
    assert!(!dat.contains("Line1\nLine2"));
}

/// Crash window: rows.jsonl has the row but checkpoint missing the item id.
/// Resume must not double the DAT row or re-assign a new control.
#[test]
fn crash_resume_jsonl_without_checkpoint_no_duplicate() {
    let (_tmp, matter) = temp_matter("crash-jsonl");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let mut ids = Vec::new();
    for i in 0..2 {
        let n = put_native(&matter, format!("native-crash-{i}").as_bytes());
        let id = insert_review_item(
            &matter,
            ItemInput {
                path: Some(format!("c{i}.bin")),
                native_sha256: Some(n),
                subject: Some(format!("Crash{i}")),
                ..Default::default()
            },
        );
        ids.push(id);
    }

    // Produce first item only, then pause.
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let outcome = run_produce_pkg(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("CrashJsonl".into()),
            ..Default::default()
        },
        Some(&|| cancel_flag.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");
    let paused = match outcome {
        ProduceOutcome::Paused(s) => s,
        other => panic!("expected Paused, got {other:?}"),
    };
    assert!(paused.produced_count >= 1);

    // Simulate crash after rows.jsonl append but before done_item_ids checkpoint:
    // leave JSONL + production_items intact; rewind checkpoint as if the item
    // was never marked done and next_seq was not advanced.
    let cp = matter
        .get_checkpoint(&job.id, PRODUCE_STAGE)
        .unwrap()
        .expect("checkpoint");
    let mut cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("cursor json");
    let first_id = ids[0].clone();
    let done = cursor
        .get_mut("done_item_ids")
        .and_then(|v| v.as_array_mut())
        .expect("done_item_ids");
    done.retain(|v| v.as_str() != Some(first_id.as_str()));
    cursor["next_seq"] = serde_json::json!(1);
    cursor["produced_count"] = serde_json::json!(0);
    cursor["cursor_index"] = serde_json::json!(0);
    cursor["completed_count"] = serde_json::json!(0);
    cursor["phase"] = serde_json::json!("work");
    let rewritten = cursor.to_string();
    matter
        .put_checkpoint(&job.id, PRODUCE_STAGE, &rewritten, 0)
        .expect("put checkpoint");

    // JSONL must still contain the first item's row (durable side effect).
    let jsonl_path = camino::Utf8Path::new(&paused.output_root)
        .join("DATA")
        .join("rows.jsonl");
    let jsonl = fs::read_to_string(jsonl_path.as_std_path()).expect("jsonl");
    assert!(
        jsonl.contains(&first_id),
        "rows.jsonl should still hold first item before resume"
    );

    // Resume: must not re-append a second row for the first item.
    let s2 = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("CrashJsonl".into()),
            ..Default::default()
        },
    );
    assert_eq!(s2.produced_count, 2);
    let dat = dat_text(&s2.output_root);
    // Header + one data line per produced item.
    let data_lines: Vec<&str> = dat
        .lines()
        .filter(|l| !l.trim().is_empty())
        .skip(1)
        .collect();
    assert_eq!(
        data_lines.len(),
        2,
        "DAT must have exactly 2 data rows (no JSONL duplicate): {dat}"
    );
    let item_hits: Vec<_> = data_lines
        .iter()
        .filter(|l| l.contains(&first_id))
        .collect();
    assert_eq!(
        item_hits.len(),
        1,
        "first ITEM_ID must appear in exactly one DAT row: {dat}"
    );
    assert!(dat.contains(&ids[1]));
    assert!(dat.contains("PROD000001"));
    assert!(dat.contains("PROD000002"));
    // No PROD000003 renumber of the recovered item.
    assert!(
        !dat.contains("PROD000003"),
        "must not burn a third control on resume: {dat}"
    );
}

/// 13. Cancel/resume partial consistency.
#[test]
fn cancel_resume_no_renumber() {
    let (_tmp, matter) = temp_matter("resume");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    for i in 0..3 {
        let n = put_native(&matter, format!("native-{i}").as_bytes());
        insert_review_item(
            &matter,
            ItemInput {
                path: Some(format!("f{i}.bin")),
                native_sha256: Some(n),
                subject: Some(format!("Item{i}")),
                ..Default::default()
            },
        );
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let outcome = run_produce_pkg(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Resume".into()),
            ..Default::default()
        },
        Some(&|| cancel_flag.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");

    match outcome {
        ProduceOutcome::Paused(s) => {
            assert!(s.produced_count >= 1);
            assert!(s.produced_count < 3);
            // status partial
            let status: String = matter
                .connection()
                .query_row(
                    "SELECT status FROM production_sets WHERE id = ?1",
                    [s.production_set_id.as_str()],
                    |row| row.get(0),
                )
                .expect("status");
            assert_eq!(status, "partial");
        }
        other => panic!("expected Paused, got {other:?}"),
    }

    // Resume
    let s2 = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Resume".into()),
            ..Default::default()
        },
    );
    assert_eq!(s2.produced_count, 3);
    let dat = dat_text(&s2.output_root);
    assert!(dat.contains("PROD000001"));
    assert!(dat.contains("PROD000002"));
    assert!(dat.contains("PROD000003"));
    // Checkpoint stage present
    assert!(matter
        .get_checkpoint(&job.id, PRODUCE_STAGE)
        .unwrap()
        .is_some());
}

/// 14. Empty selection honest fail.
#[test]
fn empty_selection_fails() {
    let (_tmp, matter) = temp_matter("empty");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let outcome = run_produce_pkg(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Empty".into()),
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run");
    match outcome {
        ProduceOutcome::Failed { message, .. } => {
            assert!(message.to_ascii_lowercase().contains("empty"), "{message}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// 15. Workspace / matter-root gate: produce writes under matter exports by default.
#[test]
fn default_output_under_exports_productions() {
    let (_tmp, matter) = temp_matter("gate");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"x");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.bin".into()),
            native_sha256: Some(n),
            ..Default::default()
        },
    );
    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("GateTest".into()),
            output_dir: None,
            ..Default::default()
        },
    );
    let root = matter.root().as_str();
    assert!(
        s.output_root.starts_with(root),
        "output {} not under matter {}",
        s.output_root,
        root
    );
    assert!(
        s.output_root.contains("exports") && s.output_root.contains("productions"),
        "{}",
        s.output_root
    );
}

#[test]
fn item_ids_scope() {
    let (_tmp, matter) = temp_matter("ids");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n1 = put_native(&matter, b"a");
    let n2 = put_native(&matter, b"b");
    let id1 = insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.bin".into()),
            native_sha256: Some(n1),
            in_review: Some(0), // not in review
            ..Default::default()
        },
    );
    // Force not in review after insert helper sets 1
    matter
        .connection()
        .execute(
            "UPDATE items SET in_review = 0 WHERE id = ?1",
            [id1.as_str()],
        )
        .unwrap();
    let _id2 = insert_review_item(
        &matter,
        ItemInput {
            path: Some("b.bin".into()),
            native_sha256: Some(n2),
            ..Default::default()
        },
    );

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            scope: "item_ids".into(),
            item_ids: vec![id1.clone()],
            name: Some("Ids".into()),
            ..Default::default()
        },
    );
    assert_eq!(s.produced_count, 1);
    let dat = dat_text(&s.output_root);
    assert!(dat.contains(&id1));
}

/// Redacted email without native: synthetic .eml must use redacted body, not original.
#[test]
fn redacted_email_synthetic_eml_uses_redacted_body() {
    let (_tmp, matter) = temp_matter("rdx-eml");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let body = "Hello SECRET world";
    let text_sha = put_text(&matter, body);
    let item_id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("inbox/msg.msg".into()),
            // no native_sha256 — force export-only EML
            text_sha256: Some(text_sha.clone()),
            file_category: Some("email".into()),
            mime_type: Some("message/rfc822".into()),
            from_addr: Some("a@x.com".into()),
            to_addrs_json: Some(r#"["b@y.com"]"#.into()),
            subject: Some("Redacted mail".into()),
            message_id: Some("mid-rdx".into()),
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
    matter
        .regenerate_redacted_text(&item_id, body, "t")
        .expect("regen");

    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("RdxEml".into()),
            export_eml_if_missing_native: true,
            ..Default::default()
        },
    );
    assert_eq!(s.produced_count, 1);

    let eml = camino::Utf8Path::new(&s.output_root)
        .join("NATIVES")
        .join("PROD000001.eml");
    assert!(eml.as_std_path().exists(), "expected .eml");
    let eml_body = fs::read_to_string(eml.as_std_path()).expect("read eml");
    assert!(
        eml_body.contains(REDACTED_TOKEN),
        "EML must contain redacted token: {eml_body}"
    );
    assert!(
        !eml_body.contains("SECRET"),
        "EML must not leak original secret: {eml_body}"
    );

    let text_path = camino::Utf8Path::new(&s.output_root)
        .join("TEXT")
        .join("PROD000001.txt");
    let text_body = fs::read_to_string(text_path.as_std_path()).unwrap();
    assert!(text_body.contains(REDACTED_TOKEN));
    assert!(!text_body.contains("SECRET"));

    let dat = dat_text(&s.output_root);
    assert!(dat.contains("NATIVES\\PROD000001.eml"));
    assert!(dat.contains("þemlþ") || dat.contains("eml"));
    assert!(dat.contains("HAS_REDACTED_TEXT"));
}

/// Produce, assert withhold on produced item, resume → withheld not in final DAT/NATIVES.
#[test]
fn late_withhold_on_resume_excludes_from_volume() {
    let (_tmp, matter) = temp_matter("late-withhold");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let mut ids = Vec::new();
    for i in 0..2 {
        let n = put_native(&matter, format!("native-late-{i}").as_bytes());
        let id = insert_review_item(
            &matter,
            ItemInput {
                path: Some(format!("l{i}.bin")),
                native_sha256: Some(n),
                subject: Some(format!("Late{i}")),
                ..Default::default()
            },
        );
        ids.push(id);
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let outcome = run_produce_pkg(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("LateHold".into()),
            ..Default::default()
        },
        Some(&|| cancel_flag.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");
    let paused = match outcome {
        ProduceOutcome::Paused(s) => s,
        other => panic!("expected Paused, got {other:?}"),
    };
    assert!(paused.produced_count >= 1);
    let first_id = ids[0].clone();

    // Assert privilege hold after the first item was packaged.
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: first_id.clone(),
            basis: "attorney_client".into(),
            description: "hold after produce".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "t".into(),
        })
        .expect("withhold");
    assert!(matter.item_is_withheld(&first_id).unwrap());

    let s2 = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("LateHold".into()),
            ..Default::default()
        },
    );
    assert_eq!(s2.skipped_withheld, 1);
    assert_eq!(
        s2.produced_count, 1,
        "only the second item should remain produced"
    );

    // Extra resume must not re-inflate skipped_withheld (idempotent audit counts).
    let s3 = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("LateHold".into()),
            ..Default::default()
        },
    );
    assert_eq!(
        s3.skipped_withheld, 1,
        "multi-resume must not double-count withheld skips"
    );
    assert_eq!(s3.produced_count, 1);

    let dat = dat_text(&s2.output_root);
    assert!(
        !dat.contains(&first_id),
        "withheld item must not appear in DAT: {dat}"
    );
    assert!(dat.contains(&ids[1]));

    let natives = camino::Utf8Path::new(&s2.output_root).join("NATIVES");
    if natives.as_std_path().exists() {
        for e in fs::read_dir(natives.as_std_path()).unwrap() {
            let bytes = fs::read(e.unwrap().path()).unwrap();
            assert_ne!(
                bytes.as_slice(),
                b"native-late-0",
                "withheld native must not remain under NATIVES/"
            );
        }
    }
    let text_dir = camino::Utf8Path::new(&s2.output_root).join("TEXT");
    if text_dir.as_std_path().exists() {
        for e in fs::read_dir(text_dir.as_std_path()).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            // First control should not leave a text file (none for binary, but guard anyway).
            assert!(
                !name.starts_with("PROD000001"),
                "stale text for withheld control: {name}"
            );
        }
    }
}

/// After produce + rewound checkpoint + JSONL present, delete native → resume rebuilds.
#[test]
fn resume_rebuilds_when_recovered_native_missing() {
    let (_tmp, matter) = temp_matter("rebuild-native");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let mut ids = Vec::new();
    for i in 0..2 {
        let n = put_native(&matter, format!("native-rebuild-{i}").as_bytes());
        let id = insert_review_item(
            &matter,
            ItemInput {
                path: Some(format!("r{i}.bin")),
                native_sha256: Some(n),
                subject: Some(format!("Rebuild{i}")),
                ..Default::default()
            },
        );
        ids.push(id);
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let outcome = run_produce_pkg(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Rebuild".into()),
            ..Default::default()
        },
        Some(&|| cancel_flag.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");
    let paused = match outcome {
        ProduceOutcome::Paused(s) => s,
        other => panic!("expected Paused, got {other:?}"),
    };
    let first_id = ids[0].clone();
    let root = paused.output_root.clone();

    // Rewind checkpoint as if crash after JSONL append.
    let cp = matter
        .get_checkpoint(&job.id, PRODUCE_STAGE)
        .unwrap()
        .expect("checkpoint");
    let mut cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("cursor");
    let done = cursor
        .get_mut("done_item_ids")
        .and_then(|v| v.as_array_mut())
        .expect("done_item_ids");
    done.retain(|v| v.as_str() != Some(first_id.as_str()));
    cursor["next_seq"] = serde_json::json!(1);
    cursor["produced_count"] = serde_json::json!(0);
    cursor["cursor_index"] = serde_json::json!(0);
    cursor["completed_count"] = serde_json::json!(0);
    cursor["phase"] = serde_json::json!("work");
    matter
        .put_checkpoint(&job.id, PRODUCE_STAGE, &cursor.to_string(), 0)
        .expect("put checkpoint");

    // Damage volume: delete the recovered native.
    let native_path = camino::Utf8Path::new(&root)
        .join("NATIVES")
        .join("PROD000001.bin");
    assert!(
        native_path.as_std_path().exists(),
        "precondition: native exists before delete"
    );
    fs::remove_file(native_path.as_std_path()).expect("delete native");

    let s2 = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Rebuild".into()),
            ..Default::default()
        },
    );
    assert_eq!(s2.produced_count, 2);
    assert!(
        native_path.as_std_path().exists(),
        "native must be rebuilt on resume"
    );
    assert_eq!(
        fs::read(native_path.as_std_path()).unwrap(),
        b"native-rebuild-0"
    );
    let dat = dat_text(&s2.output_root);
    assert!(dat.contains(&first_id));
    assert!(dat.contains("NATIVES\\PROD000001.bin") || dat.contains("PROD000001"));
    // DAT SHA must match the rebuilt file.
    let disk_sha = sha256_file(native_path.as_std_path());
    assert!(dat.contains(&disk_sha), "DAT must reference rebuilt SHA");
}

/// Default named production colliding with a complete volume gets a unique path;
/// explicit non-empty output_dir is rejected.
#[test]
fn output_dir_collision_unique_or_reject() {
    let (_tmp, matter) = temp_matter("collision");
    let job1 = matter.create_job(JOB_KIND_PRODUCE).expect("job1");
    let n = put_native(&matter, b"first-volume-native");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.bin".into()),
            native_sha256: Some(n),
            subject: Some("One".into()),
            ..Default::default()
        },
    );

    let s1 = run_ok(
        &matter,
        &job1.id,
        &ProduceParams {
            name: Some("SameName".into()),
            ..Default::default()
        },
    );
    let first_root = s1.output_root.clone();
    let first_native = camino::Utf8Path::new(&first_root)
        .join("NATIVES")
        .join("PROD000001.bin");
    assert_eq!(
        fs::read(first_native.as_std_path()).unwrap(),
        b"first-volume-native"
    );

    // Second production with same default name must not overwrite the first volume.
    let job2 = matter.create_job(JOB_KIND_PRODUCE).expect("job2");
    // Need another in_review item (or same corpus still has the item).
    let s2 = run_ok(
        &matter,
        &job2.id,
        &ProduceParams {
            name: Some("SameName".into()),
            ..Default::default()
        },
    );
    assert_ne!(
        s1.output_root, s2.output_root,
        "colliding default name must allocate a unique output root"
    );
    // First volume intact.
    assert_eq!(
        fs::read(first_native.as_std_path()).unwrap(),
        b"first-volume-native",
        "prior volume must not be clobbered"
    );

    // Explicit output_dir pointing at the first complete volume must fail closed.
    let job3 = matter.create_job(JOB_KIND_PRODUCE).expect("job3");
    let err = run_produce_pkg(
        &matter,
        &job3.id,
        &ProduceParams {
            name: Some("ExplicitCollide".into()),
            output_dir: Some(first_root.clone()),
            ..Default::default()
        },
        None,
        |_| {},
    );
    match err {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.to_ascii_lowercase().contains("non-empty")
                    || msg.to_ascii_lowercase().contains("overwrite")
                    || msg.to_ascii_lowercase().contains("output_dir"),
                "expected collision error, got: {msg}"
            );
        }
        Ok(other) => panic!("expected hard error for non-empty output_dir, got {other:?}"),
    }
    // Selection was checkpointed before the hard layout failure so fail audit
    // can report selected > 0 (not ProduceSummary::default zeros).
    let cp = matter
        .get_checkpoint(&job3.id, PRODUCE_STAGE)
        .expect("cp")
        .expect("checkpoint after pre-layout fail");
    let v: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("json");
    let selected = v
        .get("selected_count")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    assert!(
        selected >= 1,
        "hard fail before layout must still checkpoint selected_count, got {selected}"
    );
    // Still intact after rejected collision.
    assert_eq!(
        fs::read(first_native.as_std_path()).unwrap(),
        b"first-volume-native"
    );

    // Resume after pre-layout hard fail re-enters setup (does not write under empty root).
    // Checkpoint freezes the prior colliding output_dir, so resume fails closed again.
    let err_resume = run_produce_pkg(
        &matter,
        &job3.id,
        &ProduceParams {
            name: Some("ExplicitCollide".into()),
            output_dir: Some(first_root.clone()),
            ..Default::default()
        },
        None,
        |_| {},
    );
    assert!(
        err_resume.is_err(),
        "resume with frozen non-empty output_dir must fail closed, got {err_resume:?}"
    );
    // Must not have materialized a volume at the empty-string root (cwd).
    assert!(
        !std::path::Path::new("DATA").join("load.dat").exists(),
        "must not write load.dat under empty/cwd root on bad resume"
    );

    // Explicit dir with only unrelated content (no production markers) is still rejected.
    let (tmp_out, out_base) = utf8_tempdir();
    let cluttered = out_base.join("cluttered_out");
    fs::create_dir_all(cluttered.as_std_path()).unwrap();
    fs::write(cluttered.join("README.txt").as_std_path(), b"not empty").unwrap();
    let job4 = matter.create_job(JOB_KIND_PRODUCE).expect("job4");
    let err = run_produce_pkg(
        &matter,
        &job4.id,
        &ProduceParams {
            name: Some("Clutter".into()),
            output_dir: Some(cluttered.to_string()),
            ..Default::default()
        },
        None,
        |_| {},
    );
    match err {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.to_ascii_lowercase().contains("non-empty"),
                "expected non-empty rejection for unrelated files, got: {msg}"
            );
        }
        Ok(other) => {
            panic!("expected hard error for non-empty unrelated output_dir, got {other:?}")
        }
    }
    drop(tmp_out);
}

/// Crash window: production_items=ok + files on disk but no JSONL row; late withhold
/// must purge control-derived NATIVES/TEXT bytes (not only JSONL-linked artifacts).
#[test]
fn late_withhold_after_db_ok_without_jsonl_purges_bytes() {
    let (_tmp, matter) = temp_matter("late-wh-no-jsonl");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let mut ids = Vec::new();
    for i in 0..2 {
        let n = put_native(&matter, format!("native-dbok-{i}").as_bytes());
        let t = put_text(&matter, &format!("text-dbok-{i}"));
        let id = insert_review_item(
            &matter,
            ItemInput {
                path: Some(format!("d{i}.bin")),
                native_sha256: Some(n),
                text_sha256: Some(t),
                subject: Some(format!("DbOk{i}")),
                ..Default::default()
            },
        );
        ids.push(id);
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let outcome = run_produce_pkg(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("DbOkWh".into()),
            ..Default::default()
        },
        Some(&|| cancel_flag.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");
    let paused = match outcome {
        ProduceOutcome::Paused(s) => s,
        other => panic!("expected Paused, got {other:?}"),
    };
    assert!(paused.produced_count >= 1);
    let root = paused.output_root.clone();
    let first_id = ids[0].clone();

    // Precondition: control files exist.
    let native_path = camino::Utf8Path::new(&root)
        .join("NATIVES")
        .join("PROD000001.bin");
    let text_path = camino::Utf8Path::new(&root)
        .join("TEXT")
        .join("PROD000001.txt");
    assert!(native_path.as_std_path().exists());
    assert!(text_path.as_std_path().exists());

    // Simulate crash after production_items ok + files written, before durable JSONL:
    // strip JSONL entry while leaving production_items and volume files.
    let jsonl = camino::Utf8Path::new(&root).join("DATA").join("rows.jsonl");
    if jsonl.as_std_path().exists() {
        let content = fs::read_to_string(jsonl.as_std_path()).unwrap();
        let kept: Vec<&str> = content
            .lines()
            .filter(|line| !line.contains(&first_id))
            .collect();
        fs::write(jsonl.as_std_path(), kept.join("\n")).unwrap();
    }
    // Rewind checkpoint so first item is not done (still selected for resume).
    let cp = matter
        .get_checkpoint(&job.id, PRODUCE_STAGE)
        .unwrap()
        .expect("checkpoint");
    let mut cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("cursor");
    let done = cursor
        .get_mut("done_item_ids")
        .and_then(|v| v.as_array_mut())
        .expect("done_item_ids");
    done.retain(|v| v.as_str() != Some(first_id.as_str()));
    cursor["cursor_index"] = serde_json::json!(0);
    cursor["completed_count"] = serde_json::json!(0);
    // Keep produced_count / production_items so was_ok path is exercised.
    cursor["phase"] = serde_json::json!("work");
    matter
        .put_checkpoint(&job.id, PRODUCE_STAGE, &cursor.to_string(), 0)
        .expect("put checkpoint");

    // Confirm production_items still has ok for first item.
    let ok_row: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM production_items WHERE item_id = ?1 AND status = 'ok'",
            [&first_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ok_row, 1, "precondition: production_items ok without JSONL");

    // Late withhold.
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: first_id.clone(),
            basis: "attorney_client".into(),
            description: "hold after db-ok crash window".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "t".into(),
        })
        .expect("withhold");

    let s2 = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("DbOkWh".into()),
            ..Default::default()
        },
    );
    assert_eq!(s2.skipped_withheld, 1);
    assert_eq!(s2.produced_count, 1);

    let dat = dat_text(&s2.output_root);
    assert!(!dat.contains(&first_id), "withheld must not be in DAT");

    // No bytes for withheld control under NATIVES/TEXT.
    assert!(
        !native_path.as_std_path().exists(),
        "withheld native must be purged even without JSONL row"
    );
    assert!(
        !text_path.as_std_path().exists(),
        "withheld text must be purged even without JSONL row"
    );
    let natives = camino::Utf8Path::new(&s2.output_root).join("NATIVES");
    if natives.as_std_path().exists() {
        for e in fs::read_dir(natives.as_std_path()).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            assert!(
                !name.starts_with("PROD000001"),
                "stale native for withheld control: {name}"
            );
        }
    }
    let text_dir = camino::Utf8Path::new(&s2.output_root).join("TEXT");
    if text_dir.as_std_path().exists() {
        for e in fs::read_dir(text_dir.as_std_path()).unwrap() {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            assert!(
                !name.starts_with("PROD000001"),
                "stale text for withheld control: {name}"
            );
        }
    }
}

/// Truncated TEXT after produce + rewound checkpoint → resume rebuilds TEXT.
#[test]
fn resume_rebuilds_when_recovered_text_truncated() {
    let (_tmp, matter) = temp_matter("rebuild-text");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let mut ids = Vec::new();
    for i in 0..2 {
        let n = put_native(&matter, format!("native-txt-{i}").as_bytes());
        let t = put_text(&matter, &format!("full-text-body-for-item-{i}"));
        let id = insert_review_item(
            &matter,
            ItemInput {
                path: Some(format!("t{i}.bin")),
                native_sha256: Some(n),
                text_sha256: Some(t),
                subject: Some(format!("Txt{i}")),
                ..Default::default()
            },
        );
        ids.push(id);
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let outcome = run_produce_pkg(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("TxtRebuild".into()),
            ..Default::default()
        },
        Some(&|| cancel_flag.load(Ordering::SeqCst)),
        |completed| {
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("run");
    let paused = match outcome {
        ProduceOutcome::Paused(s) => s,
        other => panic!("expected Paused, got {other:?}"),
    };
    let first_id = ids[0].clone();
    let root = paused.output_root.clone();

    let text_path = camino::Utf8Path::new(&root)
        .join("TEXT")
        .join("PROD000001.txt");
    assert!(text_path.as_std_path().exists());
    let original = fs::read_to_string(text_path.as_std_path()).unwrap();
    assert!(!original.is_empty());

    // Rewind checkpoint as if crash after JSONL append.
    let cp = matter
        .get_checkpoint(&job.id, PRODUCE_STAGE)
        .unwrap()
        .expect("checkpoint");
    let mut cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("cursor");
    let done = cursor
        .get_mut("done_item_ids")
        .and_then(|v| v.as_array_mut())
        .expect("done_item_ids");
    done.retain(|v| v.as_str() != Some(first_id.as_str()));
    cursor["next_seq"] = serde_json::json!(1);
    cursor["produced_count"] = serde_json::json!(0);
    cursor["cursor_index"] = serde_json::json!(0);
    cursor["completed_count"] = serde_json::json!(0);
    cursor["phase"] = serde_json::json!("work");
    matter
        .put_checkpoint(&job.id, PRODUCE_STAGE, &cursor.to_string(), 0)
        .expect("put checkpoint");

    // Truncate TEXT to empty (fails size>0 and sha integrity).
    fs::write(text_path.as_std_path(), b"").expect("truncate text");

    let s2 = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("TxtRebuild".into()),
            ..Default::default()
        },
    );
    assert_eq!(s2.produced_count, 2);
    assert!(
        text_path.as_std_path().exists(),
        "TEXT must be rebuilt on resume"
    );
    let rebuilt = fs::read_to_string(text_path.as_std_path()).unwrap();
    assert_eq!(rebuilt, "full-text-body-for-item-0");
    assert_ne!(rebuilt, "", "rebuilt TEXT must be non-empty");
}

// ---------------------------------------------------------------------------
// 0041 QC gate
// ---------------------------------------------------------------------------

/// 13. Produce gate: failed QC prevents produce.
#[test]
fn qc_gate_failed_blocks_produce() {
    use matter_core::{selection_fingerprint, InsertQcRunInput};

    let (_tmp, matter) = temp_matter("qc-fail-gate");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"n");
    let t = put_text(&matter, "body");
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.pdf".into()),
            native_sha256: Some(n),
            text_sha256: Some(t),
            file_category: Some("document".into()),
            ..Default::default()
        },
    );
    let ids = vec![id];
    let fp = selection_fingerprint(&ids);
    matter
        .insert_qc_run(InsertQcRunInput {
            profile: "default_production_qc_v1".into(),
            passed: false,
            error_count: 2,
            warn_count: 0,
            candidate_count: 1,
            selection_fingerprint: fp,
            scope: "review_corpus".into(),
            scope_json: None,
            report_path: None,
            job_id: None,
            rules_json: None,
        })
        .expect("qc_run");

    let outcome = run_produce(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Blocked".into()),
            require_qc_pass: true,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run");
    match outcome {
        ProduceOutcome::Failed { message, .. } => {
            assert!(
                message.contains("QC failed") || message.contains("QC required"),
                "msg={message}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// 14. Stale QC: pass QC then add item → produce blocked.
#[test]
fn qc_gate_stale_blocks_produce() {
    use matter_core::{selection_fingerprint, InsertQcRunInput};

    let (_tmp, matter) = temp_matter("qc-stale-gate");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"n1");
    let t = put_text(&matter, "body1");
    let id1 = insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.pdf".into()),
            native_sha256: Some(n),
            text_sha256: Some(t),
            file_category: Some("document".into()),
            ..Default::default()
        },
    );
    let ids = vec![id1];
    let fp = selection_fingerprint(&ids);
    matter
        .insert_qc_run(InsertQcRunInput {
            profile: "default_production_qc_v1".into(),
            passed: true,
            error_count: 0,
            warn_count: 0,
            candidate_count: 1,
            selection_fingerprint: fp,
            scope: "review_corpus".into(),
            scope_json: None,
            report_path: None,
            job_id: None,
            rules_json: None,
        })
        .expect("qc_run");

    // Selection change: add another in_review item.
    let n2 = put_native(&matter, b"n2");
    let t2 = put_text(&matter, "body2");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("b.pdf".into()),
            native_sha256: Some(n2),
            text_sha256: Some(t2),
            file_category: Some("document".into()),
            ..Default::default()
        },
    );

    let outcome = run_produce(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Stale".into()),
            require_qc_pass: true,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run");
    match outcome {
        ProduceOutcome::Failed { message, .. } => {
            assert!(
                message.contains("QC stale") || message.contains("selection changed"),
                "msg={message}"
            );
        }
        other => panic!("expected Failed stale, got {other:?}"),
    }
}

/// require_qc_pass=false allows produce without QC.
#[test]
fn qc_gate_disabled_allows_produce() {
    let (_tmp, matter) = temp_matter("qc-off");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"n");
    let t = put_text(&matter, "body");
    insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.pdf".into()),
            native_sha256: Some(n),
            text_sha256: Some(t),
            file_category: Some("document".into()),
            ..Default::default()
        },
    );
    let s = run_ok(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("NoQc".into()),
            require_qc_pass: false,
            ..Default::default()
        },
    );
    assert_eq!(s.produced_count, 1);
}

/// Fresh passed QC allows produce when require_qc_pass=true.
#[test]
fn qc_gate_fresh_pass_allows_produce() {
    use matter_core::{selection_fingerprint, InsertQcRunInput};

    let (_tmp, matter) = temp_matter("qc-fresh-ok");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let n = put_native(&matter, b"n");
    let t = put_text(&matter, "body");
    let id = insert_review_item(
        &matter,
        ItemInput {
            path: Some("a.pdf".into()),
            native_sha256: Some(n),
            text_sha256: Some(t),
            file_category: Some("document".into()),
            ..Default::default()
        },
    );
    let ids = vec![id];
    let fp = selection_fingerprint(&ids);
    matter
        .insert_qc_run(InsertQcRunInput {
            profile: "default_production_qc_v1".into(),
            passed: true,
            error_count: 0,
            warn_count: 1,
            candidate_count: 1,
            selection_fingerprint: fp,
            scope: "review_corpus".into(),
            scope_json: None,
            report_path: None,
            job_id: None,
            rules_json: None,
        })
        .expect("qc_run");

    let outcome = run_produce(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("Ok".into()),
            require_qc_pass: true,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run");
    match outcome {
        ProduceOutcome::Succeeded(s) => assert_eq!(s.produced_count, 1),
        other => panic!("expected Succeeded, got {other:?}"),
    }
}

/// QC expand=false fingerprint vs produce expand=true (family members added) → stale gate.
#[test]
fn qc_expand_false_produce_expand_true_is_stale() {
    use matter_core::{
        item_role, selection_fingerprint, InsertQcRunInput, FAMILY_KIND_EMAIL_ATTACHMENTS,
    };

    let (_tmp, matter) = temp_matter("qc-expand-mismatch");
    let job = matter.create_job(JOB_KIND_PRODUCE).expect("job");
    let family = matter
        .insert_family(FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("family");
    let n_p = put_native(&matter, b"parent");
    let t_p = put_text(&matter, "parent body");
    // Parent NOT in review — only child selected when expand=false.
    let parent = matter
        .insert_item(ItemInput {
            path: Some("parent.eml".into()),
            native_sha256: Some(n_p),
            text_sha256: Some(t_p),
            file_category: Some("email".into()),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            in_review: Some(0),
            status: item_status::EXTRACTED.into(),
            size_bytes: Some(4),
            ..Default::default()
        })
        .expect("parent")
        .id;
    let n_c = put_native(&matter, b"child");
    let t_c = put_text(&matter, "child body");
    let child = matter
        .insert_item(ItemInput {
            path: Some("child.pdf".into()),
            native_sha256: Some(n_c),
            text_sha256: Some(t_c),
            file_category: Some("document".into()),
            role: Some(item_role::ATTACHMENT.into()),
            parent_item_id: Some(parent.clone()),
            family_id: Some(family.id),
            in_review: Some(1),
            status: item_status::EXTRACTED.into(),
            size_bytes: Some(4),
            ..Default::default()
        })
        .expect("child")
        .id;

    // Simulate QC run with expand_family_for_scan=false (child only).
    let qc_ids = vec![child];
    let fp = selection_fingerprint(&qc_ids);
    matter
        .insert_qc_run(InsertQcRunInput {
            profile: "default_production_qc_v1".into(),
            passed: true,
            error_count: 0,
            warn_count: 0,
            candidate_count: 1,
            selection_fingerprint: fp,
            scope: "review_corpus".into(),
            scope_json: Some(r#"{"expand_family_for_scan":false}"#.into()),
            report_path: None,
            job_id: None,
            rules_json: None,
        })
        .expect("qc_run");

    let outcome = run_produce(
        &matter,
        &job.id,
        &ProduceParams {
            name: Some("ExpandMismatch".into()),
            require_qc_pass: true,
            expand_family: true,
            ..Default::default()
        },
        None,
        |_| {},
    )
    .expect("run");
    match outcome {
        ProduceOutcome::Failed { message, .. } => {
            assert!(
                message.contains("QC stale") || message.contains("selection changed"),
                "expand mismatch must be stale; msg={message}"
            );
        }
        other => panic!("expected Failed stale for expand mismatch, got {other:?}"),
    }
}
