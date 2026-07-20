//! Gap analysis integration tests (spec §3.9).

#![allow(clippy::field_reassign_with_default)]

use std::fs;
use std::path::PathBuf;

use matter_core::{item_role, item_status, ItemInput, Matter, SCHEMA_VERSION};
use matter_gap::{
    allowed_buckets, analyze_date_coverage, check_bytes_size, import_opposing_dat, parse_dat_bytes,
    run_collection_gap, run_gap, run_opposing_gap, CollectionGapParams, DatCaps, DatColumnMap,
    GapError, GapOutcome, GapParams, MappedField, OpposingGapParams, FINDING_DATE_WINDOW_EMPTY,
    FINDING_MISSING_CUSTODIAN, FINDING_UNEXPECTED_CUSTODIAN, JOB_KIND_GAP, KIND_COLLECTION,
    KIND_OPPOSING,
};
use matter_produce::{encode_dat_field, DAT_QUALIFIER, DAT_SEPARATOR, UTF8_BOM};

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

fn put_native(matter: &Matter, bytes: &[u8]) -> String {
    matter.put_bytes(bytes).expect("put native")
}

fn insert_item(matter: &Matter, mut input: ItemInput) -> String {
    input.status = item_status::EXTRACTED.into();
    if input.role.is_none() {
        input.role = Some(item_role::STANDALONE.into());
    }
    matter.insert_item(input).expect("insert").id
}

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("..");
    p.push("fixtures");
    p.push("gap");
    p.push(name);
    p
}

#[test]
fn schema_v22_gap_tables() {
    let (_tmp, matter) = temp_matter("schema-v22");
    assert_eq!(SCHEMA_VERSION, 28);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    for table in [
        "expected_custodians",
        "gap_imports",
        "gap_expected_docs",
        "gap_runs",
    ] {
        let has: bool = matter
            .connection()
            .query_row(
                &format!(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='{table}'"
                ),
                [],
                |row| row.get(0),
            )
            .expect("table");
        assert!(has, "missing {table}");
    }
}

/// 1. Roster import + missing_custodian severity=warn
#[test]
fn roster_missing_custodian_is_warn() {
    let (_tmp, matter) = temp_matter("missing-warn");
    let csv = b"custodian\nAlice\nMissing Person\n";
    matter
        .import_expected_custodians_csv_bytes(csv)
        .expect("import");
    let n = put_native(&matter, b"a");
    insert_item(
        &matter,
        ItemInput {
            path: Some("a.eml".into()),
            native_sha256: Some(n),
            custodian: Some("Alice".into()),
            ..Default::default()
        },
    );
    let report = run_collection_gap(&matter, &CollectionGapParams::default(), Some("job-miss"))
        .expect("run");
    let missing: Vec<_> = report
        .roster
        .missing
        .iter()
        .filter(|f| f.finding_id == FINDING_MISSING_CUSTODIAN)
        .collect();
    assert!(!missing.is_empty());
    assert!(missing.iter().all(|f| f.severity.as_str() == "warn"));
    assert!(missing.iter().any(|f| f.name_norm == "missing person"));
}

/// 2. Present custodian not flagged missing
#[test]
fn present_custodian_not_missing() {
    let (_tmp, matter) = temp_matter("present");
    matter
        .import_expected_custodians_csv_bytes(b"custodian\nAlice\n")
        .unwrap();
    let n = put_native(&matter, b"x");
    insert_item(
        &matter,
        ItemInput {
            path: Some("x.pdf".into()),
            native_sha256: Some(n),
            custodian: Some("Alice".into()),
            ..Default::default()
        },
    );
    let report = run_collection_gap(&matter, &CollectionGapParams::default(), None).unwrap();
    assert!(report.roster.missing.is_empty());
}

/// 3. unexpected_custodian when enabled
#[test]
fn unexpected_custodian_warn() {
    let (_tmp, matter) = temp_matter("unexpected");
    matter
        .import_expected_custodians_csv_bytes(b"custodian\nAlice\n")
        .unwrap();
    let n = put_native(&matter, b"y");
    insert_item(
        &matter,
        ItemInput {
            path: Some("y.pdf".into()),
            native_sha256: Some(n),
            custodian: Some("Bob".into()),
            ..Default::default()
        },
    );
    let mut params = CollectionGapParams::default();
    params.flag_unexpected_custodian = true;
    let report = run_collection_gap(&matter, &params, None).unwrap();
    assert!(report
        .roster
        .unexpected
        .iter()
        .any(|f| f.finding_id == FINDING_UNEXPECTED_CUSTODIAN));
}

/// 4. Date window empty → finding error
#[test]
fn date_window_empty_error() {
    let (_tmp, matter) = temp_matter("date-empty");
    let mut params = CollectionGapParams::default();
    params.window_start = Some("2020-01-01T00:00:00Z".into());
    params.window_end = Some("2020-12-31T23:59:59Z".into());
    let report = run_collection_gap(&matter, &params, None).unwrap();
    assert!(report
        .date_findings
        .iter()
        .any(|f| f.finding_id == FINDING_DATE_WINDOW_EMPTY && f.severity.as_str() == "error"));
    assert!(report.error_count >= 1);
}

/// 5. Date hole: week/month only; no day default
#[test]
fn date_buckets_week_month_only() {
    assert!(allowed_buckets().contains(&"week"));
    assert!(allowed_buckets().contains(&"month"));
    assert!(!allowed_buckets().contains(&"day"));
    let err =
        analyze_date_coverage(&[], Some("2020-01-01"), Some("2020-01-31"), "day").unwrap_err();
    assert!(err.to_string().contains("day"));
    let mut p = GapParams::default();
    p.bucket = "day".into();
    assert!(p.validate_shape().is_err());
}

/// 6. Synthetic 0040-format DAT import → row count; BOM handled
#[test]
fn concordance_dat_import_with_bom() {
    let (_tmp, matter) = temp_matter("dat-bom");
    let mut bytes = Vec::from(UTF8_BOM);
    let headers = [
        "CONTROL_NUMBER",
        "SHA256",
        "ITEM_ID",
        "CUSTODIAN",
        "FILE_NAME",
    ];
    let mut line = String::new();
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            line.push(DAT_SEPARATOR);
        }
        line.push(DAT_QUALIFIER);
        line.push_str(h);
        line.push(DAT_QUALIFIER);
    }
    bytes.extend(line.as_bytes());
    bytes.push(b'\n');
    let vals = ["C1", "deadbeef", "i1", "Alice", "a.pdf"];
    let mut data = String::new();
    for (i, v) in vals.iter().enumerate() {
        if i > 0 {
            data.push(DAT_SEPARATOR);
        }
        data.push(DAT_QUALIFIER);
        data.push_str(&encode_dat_field(v));
        data.push(DAT_QUALIFIER);
    }
    bytes.extend(data.as_bytes());
    bytes.push(b'\n');

    let map = DatColumnMap::default_produce_v1();
    let parsed = parse_dat_bytes(&bytes, &map, DatCaps::default()).unwrap();
    assert_eq!(parsed.rows.len(), 1);
    assert_eq!(parsed.rows[0].control_number.as_deref(), Some("C1"));

    // write temp file and import via API
    let (td, base) = utf8_tempdir();
    let path = base.join("sample.dat");
    fs::write(path.as_std_path(), &bytes).unwrap();
    let import_id =
        import_opposing_dat(&matter, path.as_std_path(), None, DatCaps::default()).expect("import");
    let docs = matter.list_gap_expected_docs(&import_id).unwrap();
    assert_eq!(docs.len(), 1);
    drop(td);
}

/// 7. Non-email SHA256 match / non-match
#[test]
fn non_email_sha256_match_and_gap() {
    let (_tmp, matter) = temp_matter("sha-match");
    let sha = put_native(&matter, b"native-content-xyz");
    insert_item(
        &matter,
        ItemInput {
            path: Some("doc.pdf".into()),
            native_sha256: Some(sha.clone()),
            file_category: Some("document".into()),
            ..Default::default()
        },
    );

    let (td, base) = utf8_tempdir();
    let path = base.join("opp.csv");
    let csv = format!(
        "CONTROL_NUMBER,SHA256,ITEM_ID,CUSTODIAN,FILE_NAME\n\
         P1,{sha},,Alice,doc.pdf\n\
         P2,ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff,,Alice,other.pdf\n"
    );
    fs::write(path.as_std_path(), csv).unwrap();
    let import_id =
        import_opposing_dat(&matter, path.as_std_path(), None, DatCaps::default()).unwrap();
    let report = run_opposing_gap(
        &matter,
        &OpposingGapParams {
            import_id,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(report.matched_count, 1);
    assert_eq!(report.expected_not_in_matter_count, 1);
    drop(td);
}

/// 8. Email: different native hashes, same Message-ID → matched
#[test]
fn email_same_message_id_different_hash_matches() {
    let (_tmp, matter) = temp_matter("email-mid");
    let sha_matter = put_native(&matter, b"matter-native-bytes");
    insert_item(
        &matter,
        ItemInput {
            path: Some("mail.eml".into()),
            native_sha256: Some(sha_matter),
            message_id: Some("<ABC@example.com>".into()),
            file_category: Some("email".into()),
            mime_type: Some("message/rfc822".into()),
            ..Default::default()
        },
    );

    // Expected has different sha but same MID
    let (td, base) = utf8_tempdir();
    let path = base.join("email.csv");
    // Custom map including MESSAGE_ID
    let csv = "CONTROL_NUMBER,SHA256,ITEM_ID,CUSTODIAN,FILE_NAME,FILE_EXT,FILE_CATEGORY,MIME_TYPE,MESSAGE_ID\n\
               E1,1111111111111111111111111111111111111111111111111111111111111111,,Alice,mail.eml,eml,email,message/rfc822,<ABC@example.com>\n";
    fs::write(path.as_std_path(), csv).unwrap();

    let mut raw = std::collections::HashMap::new();
    raw.insert("CONTROL_NUMBER".into(), "control_number".into());
    raw.insert("SHA256".into(), "sha256".into());
    raw.insert("ITEM_ID".into(), "item_id".into());
    raw.insert("CUSTODIAN".into(), "custodian".into());
    raw.insert("FILE_NAME".into(), "file_name".into());
    raw.insert("FILE_EXT".into(), "file_ext".into());
    raw.insert("FILE_CATEGORY".into(), "file_category".into());
    raw.insert("MIME_TYPE".into(), "mime_type".into());
    raw.insert("MESSAGE_ID".into(), "message_id".into());
    let map = DatColumnMap::from_string_map(&raw).unwrap();

    let import_id =
        import_opposing_dat(&matter, path.as_std_path(), Some(&map), DatCaps::default()).unwrap();
    let report = run_opposing_gap(
        &matter,
        &OpposingGapParams {
            import_id,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(report.matched_count, 1, "same MID must match despite hash");
    assert_eq!(report.expected_not_in_matter_count, 0);
    drop(td);
}

/// Empty/whitespace Message-ID on expected must NOT match a matter item that
/// also has empty/None message_id via MatchKey::MessageId (empty never matches).
#[test]
fn empty_message_id_never_matches_via_mid() {
    use matter_core::GapExpectedDoc;
    use matter_gap::{match_expected_to_matter, MatchKey};

    let (_tmp, matter) = temp_matter("empty-mid");
    let sha = put_native(&matter, b"empty-mid-native");
    // Matter item with no message_id
    insert_item(
        &matter,
        ItemInput {
            path: Some("blank.eml".into()),
            native_sha256: Some(sha),
            message_id: None,
            file_category: Some("email".into()),
            mime_type: Some("message/rfc822".into()),
            ..Default::default()
        },
    );
    // Expected with whitespace-only Message-ID and a different hash (no SHA path)
    let doc = GapExpectedDoc {
        id: "exp-empty-mid".into(),
        message_id: Some("   ".into()),
        sha256: Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".into()),
        file_category: Some("email".into()),
        mime_type: Some("message/rfc822".into()),
        file_ext: Some("eml".into()),
        ..Default::default()
    };
    let hit = match_expected_to_matter(&matter, &doc).expect("match");
    assert!(
        hit.is_none() || hit.as_ref().map(|(_, k)| k) != Some(&MatchKey::MessageId),
        "empty/whitespace MID must not produce MatchKey::MessageId; got {hit:?}"
    );
    assert!(
        hit.is_none(),
        "expected no match without other keys; got {hit:?}"
    );
}

/// Whitespace MID + shared empty MID scenario via full opposing run: no MessageId hits.
#[test]
fn empty_message_id_opposing_not_matched_on_mid() {
    let (_tmp, matter) = temp_matter("empty-mid-opp");
    let sha_matter = put_native(&matter, b"matter-empty-mid-bytes");
    insert_item(
        &matter,
        ItemInput {
            path: Some("m.eml".into()),
            native_sha256: Some(sha_matter),
            message_id: Some("".into()),
            file_category: Some("email".into()),
            mime_type: Some("message/rfc822".into()),
            ..Default::default()
        },
    );

    let (td, base) = utf8_tempdir();
    let path = base.join("empty-mid.csv");
    let csv = "CONTROL_NUMBER,SHA256,ITEM_ID,CUSTODIAN,FILE_NAME,FILE_EXT,FILE_CATEGORY,MIME_TYPE,MESSAGE_ID\n\
               E0,9999999999999999999999999999999999999999999999999999999999999999,,Alice,m.eml,eml,email,message/rfc822,   \n";
    fs::write(path.as_std_path(), csv).unwrap();

    let mut raw = std::collections::HashMap::new();
    raw.insert("CONTROL_NUMBER".into(), "control_number".into());
    raw.insert("SHA256".into(), "sha256".into());
    raw.insert("ITEM_ID".into(), "item_id".into());
    raw.insert("CUSTODIAN".into(), "custodian".into());
    raw.insert("FILE_NAME".into(), "file_name".into());
    raw.insert("FILE_EXT".into(), "file_ext".into());
    raw.insert("FILE_CATEGORY".into(), "file_category".into());
    raw.insert("MIME_TYPE".into(), "mime_type".into());
    raw.insert("MESSAGE_ID".into(), "message_id".into());
    let map = DatColumnMap::from_string_map(&raw).unwrap();

    let import_id =
        import_opposing_dat(&matter, path.as_std_path(), Some(&map), DatCaps::default()).unwrap();
    let report = run_opposing_gap(
        &matter,
        &OpposingGapParams {
            import_id,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(
        report.matched_count, 0,
        "empty/whitespace Message-ID must not join expected to matter"
    );
    assert_eq!(report.expected_not_in_matter_count, 1);
    drop(td);
}

/// 9. Email without MID, different hashes → unmatched
#[test]
fn email_no_mid_different_hash_unmatched() {
    let (_tmp, matter) = temp_matter("email-no-mid");
    let sha_matter = put_native(&matter, b"matter-eml-a");
    insert_item(
        &matter,
        ItemInput {
            path: Some("a.eml".into()),
            native_sha256: Some(sha_matter),
            file_category: Some("email".into()),
            ..Default::default()
        },
    );

    let (td, base) = utf8_tempdir();
    let path = base.join("email2.csv");
    let csv = "CONTROL_NUMBER,SHA256,ITEM_ID,CUSTODIAN,FILE_NAME,FILE_EXT,FILE_CATEGORY\n\
               E2,2222222222222222222222222222222222222222222222222222222222222222,,Alice,b.eml,eml,email\n";
    fs::write(path.as_std_path(), csv).unwrap();
    let import_id =
        import_opposing_dat(&matter, path.as_std_path(), None, DatCaps::default()).unwrap();
    let report = run_opposing_gap(
        &matter,
        &OpposingGapParams {
            import_id,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(report.matched_count, 0);
    assert_eq!(report.expected_not_in_matter_count, 1);
    drop(td);
}

/// 10. Column map unknown field / missing header → Error not panic
#[test]
fn column_map_validation_errors() {
    let mut raw = std::collections::HashMap::new();
    raw.insert("FOO".into(), "not_a_real_field".into());
    let err = DatColumnMap::from_string_map(&raw).unwrap_err();
    assert!(matches!(err, GapError::InvalidColumnMap(_)));

    let map = DatColumnMap::default_produce_v1();
    let err = map.resolve_indices(&["SUBJECT".into()]).unwrap_err();
    assert!(matches!(err, GapError::InvalidDatHeader { .. }));
}

/// 11. Oversized DAT fails closed
#[test]
fn oversized_dat_fails_closed() {
    let err = check_bytes_size(1000, 10).unwrap_err();
    assert!(matches!(err, GapError::DatTooLarge { .. }));

    let tiny = DatCaps {
        max_bytes: 5,
        max_rows: 100,
    };
    let bytes = b"CONTROL_NUMBER,SHA256,ITEM_ID,CUSTODIAN,FILE_NAME\nA,B,C,D,E\n";
    let err = parse_dat_bytes(bytes, &DatColumnMap::default_produce_v1(), tiny).unwrap_err();
    assert!(matches!(err, GapError::DatTooLarge { .. }));
}

/// max_rows cap: 2-row CSV with max_rows=1 → DatTooManyRows
#[test]
fn dat_max_rows_fails_closed() {
    let caps = DatCaps {
        max_bytes: 1_000_000,
        max_rows: 1,
    };
    let bytes = b"CONTROL_NUMBER,SHA256,ITEM_ID,CUSTODIAN,FILE_NAME\n\
                  R1,aa,i1,A,a.pdf\n\
                  R2,bb,i2,B,b.pdf\n";
    let err = parse_dat_bytes(bytes, &DatColumnMap::default_produce_v1(), caps).unwrap_err();
    assert!(
        matches!(err, GapError::DatTooManyRows { cap: 1, .. }),
        "expected DatTooManyRows, got {err:?}"
    );
}

/// 12. Report CSVs exist; subjects omitted
#[test]
fn report_csvs_no_subject() {
    let (_tmp, matter) = temp_matter("report-pack");
    matter
        .import_expected_custodians_csv_bytes(b"custodian\nZed\n")
        .unwrap();
    let report = run_collection_gap(&matter, &CollectionGapParams::default(), Some("j1")).unwrap();
    let dir = PathBuf::from(&report.report_path);
    assert!(dir.join("summary.csv").exists());
    assert!(dir.join("missing_custodians.csv").exists());
    assert!(dir.join("custodian_inventory.csv").exists());
    // Subjects must not appear as a column in report files
    for name in [
        "summary.csv",
        "missing_custodians.csv",
        "expected_not_in_matter.csv",
        "matched.csv",
    ] {
        let p = dir.join(name);
        if p.exists() {
            let text = fs::read_to_string(&p).unwrap();
            let header = text.lines().next().unwrap_or("");
            assert!(
                !header.to_ascii_lowercase().contains("subject"),
                "{name} must not have subject column"
            );
        }
    }
}

/// 13. Job kind constant + workspace-facing API
#[test]
fn job_kind_and_unified_run() {
    assert_eq!(JOB_KIND_GAP, "gap");
    let (_tmp, matter) = temp_matter("unified");
    let mut params = GapParams::default();
    params.kind = KIND_COLLECTION.into();
    match run_gap(&matter, "job-u", &params, None, |_| {}).unwrap() {
        GapOutcome::Succeeded(r) => {
            assert_eq!(r.kind, KIND_COLLECTION);
            assert!(!r.report_path.is_empty());
        }
        other => panic!("expected success: {other:?}"),
    }
}

#[test]
fn fixture_roster_csv_loads() {
    let path = fixture_path("roster.csv");
    assert!(path.exists(), "missing {}", path.display());
    let (_tmp, matter) = temp_matter("fixture-roster");
    let r = matter
        .import_expected_custodians_csv_path(&path)
        .expect("import fixture");
    assert!(r.inserted >= 3);
}

#[test]
fn mapped_field_enum_stable() {
    assert_eq!(MappedField::Sha256.as_str(), "sha256");
    assert_eq!(KIND_OPPOSING, "opposing");
}
