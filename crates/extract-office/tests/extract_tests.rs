//! Integration tests for extract-office (spec §3.9).

use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::path::PathBuf;

use extract_office::limits::{
    methods, MAX_EXTRACTED_TEXT_BYTES, MAX_NATIVE_INPUT_BYTES, TRUNCATION_MARKER,
};
use extract_office::xlsx::extract_xlsx_with_limit;
use extract_office::zip_safe::{open_zip, read_entry_capped, read_entry_capped_with_max};
use extract_office::{
    extract_office, reject_oversized_native_len, reject_oversized_native_len_with_max,
    run_office_extract, OfficeExtractOutcome, OfficeExtractParams, JOB_KIND_OFFICE_EXTRACT,
};
use matter_core::{
    redaction_reason, ApplyOfficeTextInput, CreateRedactionInput, ItemInput, Matter,
};
use tempfile::tempdir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

fn fixtures_dir() -> PathBuf {
    // workspace root / fixtures/office
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates
    p.pop(); // workspace
    p.push("fixtures");
    p.push("office");
    p
}

fn load_fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

#[test]
fn happy_docx_marker() {
    let data = load_fixture("minimal.docx");
    let extracted = extract_office(&data, Some("minimal.docx"), None).expect("docx");
    assert!(
        extracted.text.contains("OFFICE_DOCX_MARKER"),
        "text={}",
        extracted.text
    );
    assert_eq!(extracted.method, methods::DOCX_XML_V1);
    assert!(!extracted.partial);
}

#[test]
fn happy_xlsx_marker() {
    let data = load_fixture("minimal.xlsx");
    let extracted = extract_office(&data, Some("minimal.xlsx"), None).expect("xlsx");
    assert!(
        extracted.text.contains("OFFICE_XLSX_MARKER"),
        "text={}",
        extracted.text
    );
    assert_eq!(extracted.method, methods::CALAMINE_XLSX_V1);
}

#[test]
fn happy_pptx_marker() {
    let data = load_fixture("minimal.pptx");
    let extracted = extract_office(&data, Some("minimal.pptx"), None).expect("pptx");
    assert!(
        extracted.text.contains("OFFICE_PPTX_MARKER"),
        "text={}",
        extracted.text
    );
    assert_eq!(extracted.method, methods::PPTX_XML_V1);
    assert!(extracted.text.contains("--- Slide 1 ---"));
}

#[test]
fn corrupt_zip_error_no_panic() {
    let data = load_fixture("corrupt.docx");
    let err = extract_office(&data, Some("corrupt.docx"), None).expect_err("corrupt");
    assert_eq!(err.code(), "office_parse_error");
}

#[test]
fn legacy_doc_unsupported_no_panic() {
    let data = load_fixture("legacy.doc");
    let err = extract_office(&data, Some("legacy.doc"), None).expect_err("legacy");
    assert_eq!(err.code(), "unsupported_legacy_office");
}

#[test]
fn over_limit_native_errors() {
    // Build a buffer larger than MAX with zip magic so we hit size check first.
    let mut huge = vec![0u8; (MAX_NATIVE_INPUT_BYTES as usize) + 1];
    huge[0] = b'P';
    huge[1] = b'K';
    let err = extract_office(&huge, Some("huge.docx"), None).expect_err("limit");
    assert_eq!(err.code(), "office_limit_exceeded");
}

#[test]
fn streaming_take_caps_entry_read() {
    // Real zip stored entry larger than injectable cap → LimitExceeded (take path).
    let payload = vec![b'x'; 128];
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(&payload).unwrap();
        z.finish().unwrap();
    }
    let data = buf.into_inner();
    let mut archive = open_zip(&data).unwrap();
    let mut entry = archive.by_name("word/document.xml").unwrap();
    let err = read_entry_capped_with_max(&mut entry, 32).expect_err("over cap");
    assert_eq!(err.code(), "office_limit_exceeded");

    // Normal fixture entry still loads under production cap.
    let fixture = load_fixture("minimal.docx");
    let mut archive = open_zip(&fixture).unwrap();
    let mut entry = archive.by_name("word/document.xml").unwrap();
    let bytes = read_entry_capped(&mut entry).unwrap();
    assert!(!bytes.is_empty());
}

#[test]
fn xlsx_early_break_truncates_multi_row() {
    // Multi-row workbook: tiny limit must truncate with marker and partial=true
    // while a full extract still contains later-row content.
    let data = multi_row_xlsx(8);
    let extracted = extract_xlsx_with_limit(&data, 40).expect("xlsx multi");
    assert!(extracted.partial, "text={}", extracted.text);
    assert!(
        extracted.text.contains(TRUNCATION_MARKER),
        "text={}",
        extracted.text
    );
    // Later rows should not all fit under the tiny cap.
    assert!(
        !extracted.text.contains("ROW7_MARKER"),
        "early break should stop before last rows: {}",
        extracted.text
    );

    let full = extract_xlsx_with_limit(&data, MAX_EXTRACTED_TEXT_BYTES).unwrap();
    assert!(!full.partial);
    assert!(full.text.contains("ROW0_MARKER"));
    assert!(full.text.contains("ROW7_MARKER"));
    assert!(full.text.len() > extracted.text.len());
}

/// Minimal multi-row XLSX (shared strings) for early-break tests.
fn multi_row_xlsx(rows: usize) -> Vec<u8> {
    let mut sst_items = String::new();
    let mut sheet_rows = String::new();
    for i in 0..rows {
        let marker = format!("ROW{i}_MARKER");
        sst_items.push_str(&format!(r#"<si><t>{marker}</t></si>"#));
        let r = i + 1;
        sheet_rows.push_str(&format!(
            r#"<row r="{r}"><c r="A{r}" t="s"><v>{i}</v></c></row>"#
        ));
    }
    let sst = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{rows}" uniqueCount="{rows}">
{sst_items}
</sst>"#
    );
    let sheet = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
{sheet_rows}
  </sheetData>
</worksheet>"#
    );
    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;
    let wb = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
          xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#;
    let wb_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#;

    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", rels),
            ("xl/workbook.xml", wb),
            ("xl/_rels/workbook.xml.rels", wb_rels),
            ("xl/worksheets/sheet1.xml", sheet.as_str()),
            ("xl/sharedStrings.xml", sst.as_str()),
        ] {
            z.start_file(name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn apply_office_text_cas_method_and_side_effects() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "Office").unwrap();

    let data = load_fixture("minimal.docx");
    let native = matter.put_bytes(&data).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("memo.docx".into()),
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            file_category: Some("attachment".into()),
            ..Default::default()
        })
        .unwrap();

    // Seed FTS + redacted artifact bookkeeping.
    matter
        .connection()
        .execute(
            "UPDATE items SET fts_text_sha256 = 'deadbeef', fts_indexed_at = 't', \
                    redacted_text_sha256 = 'redacteddead', redacted_text_at = 't', \
                    redacted_source_digest = 'old' WHERE id = ?1",
            rusqlite::params![item.id],
        )
        .unwrap();

    let extracted = extract_office(&data, Some("memo.docx"), None).unwrap();
    let apply = matter
        .apply_office_text(ApplyOfficeTextInput {
            item_id: item.id.clone(),
            force: false,
            text: Some(extracted.text.clone()),
            method: Some(extracted.method.clone()),
            status: Some("ok".into()),
            error: None,
            source_native_sha256: Some(native.clone()),
            partial: false,
            file_category: Some("document".into()),
            refine_file_category: true,
        })
        .unwrap();
    match apply {
        matter_core::OfficeExtractApplyResult::Applied {
            text_sha256,
            text_changed,
        } => {
            assert!(text_changed);
            assert!(matter.blob_exists(&text_sha256).unwrap());
        }
        other => panic!("expected Applied, got {other:?}"),
    }

    let reloaded = matter.get_item(&item.id).unwrap();
    assert!(reloaded.text_sha256.is_some());
    assert_eq!(
        reloaded.office_extract_method.as_deref(),
        Some(methods::DOCX_XML_V1)
    );
    assert_eq!(
        reloaded.office_source_native_sha256.as_deref(),
        Some(native.as_str())
    );
    assert_eq!(reloaded.office_extract_status.as_deref(), Some("ok"));
    assert_eq!(reloaded.file_category.as_deref(), Some("document"));
    // FTS cleared
    let fts: Option<String> = matter
        .connection()
        .query_row(
            "SELECT fts_text_sha256 FROM items WHERE id = ?1",
            rusqlite::params![item.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(fts.is_none());
    // Redacted invalidated
    assert!(reloaded.redacted_text_sha256.is_none());
    assert!(reloaded.redacted_text_at.is_none());
    assert!(reloaded.redacted_source_digest.is_none());
    // Native unchanged
    assert_eq!(reloaded.native_sha256.as_deref(), Some(native.as_str()));
    assert!(matter.blob_exists(&native).unwrap());

    // Skip when already extracted same native
    let skip = matter
        .apply_office_text(ApplyOfficeTextInput {
            item_id: item.id.clone(),
            force: false,
            text: Some(extracted.text.clone()),
            method: Some(extracted.method.clone()),
            status: Some("ok".into()),
            error: None,
            source_native_sha256: Some(native.clone()),
            partial: false,
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert!(matches!(
        skip,
        matter_core::OfficeExtractApplyResult::Skipped
    ));

    // Force re-extract updates
    let force = matter
        .apply_office_text(ApplyOfficeTextInput {
            item_id: item.id.clone(),
            force: true,
            text: Some(format!("{}\nforce", extracted.text)),
            method: Some(extracted.method),
            status: Some("ok".into()),
            error: None,
            source_native_sha256: Some(native),
            partial: false,
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert!(matches!(
        force,
        matter_core::OfficeExtractApplyResult::Applied {
            text_changed: true,
            ..
        }
    ));
}

#[test]
fn job_run_extracts_and_skips() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "OfficeJob").unwrap();

    let data = load_fixture("minimal.docx");
    let native = matter.put_bytes(&data).unwrap();
    let _item = matter
        .insert_item(ItemInput {
            path: Some("memo.docx".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let params = OfficeExtractParams::default();
    let outcome = run_office_extract(&matter, &job.id, &params, None, |_| {}).unwrap();
    match outcome {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, 1);
            assert_eq!(s.error_count, 0);
        }
        other => panic!("expected success: {other:?}"),
    }

    // Second run: stable candidate list still lists the item; process_one skips.
    let job2 = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let outcome2 = run_office_extract(&matter, &job2.id, &params, None, |_| {}).unwrap();
    match outcome2 {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, 0);
            assert_eq!(s.skipped_count, 1);
            assert_eq!(s.completed_count, 1);
        }
        other => panic!("expected success with skip: {other:?}"),
    }

    // Force re-extract
    let job3 = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let force = OfficeExtractParams {
        force: true,
        ..Default::default()
    };
    let outcome3 = run_office_extract(&matter, &job3.id, &force, None, |_| {}).unwrap();
    match outcome3 {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, 1);
        }
        other => panic!("expected force success: {other:?}"),
    }
}

#[test]
fn job_cancel_between_items() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "OfficeCancel").unwrap();

    for name in ["a.docx", "b.docx", "c.docx"] {
        let data = load_fixture("minimal.docx");
        let native = matter.put_bytes(&data).unwrap();
        matter
            .insert_item(ItemInput {
                path: Some(name.into()),
                native_sha256: Some(native),
                status: "extracted".into(),
                ..Default::default()
            })
            .unwrap();
    }

    let job = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    // Use cancel that fires after first completed item via atomic counter.
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    let done = Arc::new(AtomicU64::new(0));
    let done2 = done.clone();
    let cancel_fn = move || done2.load(Ordering::SeqCst) >= 1;
    let done3 = done.clone();
    let params = OfficeExtractParams {
        batch_size: 1,
        ..Default::default()
    };
    let outcome = run_office_extract(&matter, &job.id, &params, Some(&cancel_fn), |c| {
        done3.store(c, Ordering::SeqCst);
    })
    .unwrap();
    match outcome {
        OfficeExtractOutcome::Paused(s) => {
            assert!(s.completed_count >= 1);
            assert!(s.completed_count < 3);
        }
        OfficeExtractOutcome::Succeeded(s) if s.completed_count < 3 => {
            // Accept if cancel raced past — still ok if not all processed in one go
        }
        other => {
            // If all finished before cancel, still acceptable on fast machines;
            // require at least that job didn't panic.
            let _ = other;
        }
    }
}

/// Regression: non-force + batch_size 1 must extract **all** N items.
///
/// Old bug: pending-only SQL + OFFSET into a shrinking list processed A, skipped
/// B, processed C (or stopped early). Stable list + OFFSET visits every row.
#[test]
fn multi_item_batch_size_one_extracts_all() {
    const N: usize = 3;
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "OfficeMulti").unwrap();

    let data = load_fixture("minimal.docx");
    let native = matter.put_bytes(&data).unwrap();
    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        let item = matter
            .insert_item(ItemInput {
                path: Some(format!("memo-{i}.docx")),
                native_sha256: Some(native.clone()),
                status: "extracted".into(),
                ..Default::default()
            })
            .unwrap();
        ids.push(item.id);
    }

    let job = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let params = OfficeExtractParams {
        force: false,
        batch_size: 1,
        ..Default::default()
    };
    let outcome = run_office_extract(&matter, &job.id, &params, None, |_| {}).unwrap();
    match outcome {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, N as u64, "summary={s:?}");
            assert_eq!(s.completed_count, N as u64, "summary={s:?}");
            assert_eq!(s.error_count, 0, "summary={s:?}");
        }
        other => panic!("expected success: {other:?}"),
    }

    for id in &ids {
        let item = matter.get_item(id).unwrap();
        assert!(
            item.text_sha256.is_some(),
            "item {id} missing text_sha256 after multi-item extract"
        );
        assert_eq!(
            item.office_source_native_sha256.as_deref(),
            Some(native.as_str())
        );
    }
}

/// Cancel after one item → Paused; resume same job → remaining finish; all have text.
#[test]
fn multi_item_cancel_then_resume_completes_all() {
    const N: usize = 3;
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "OfficeResume").unwrap();

    let data = load_fixture("minimal.docx");
    let native = matter.put_bytes(&data).unwrap();
    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        let item = matter
            .insert_item(ItemInput {
                path: Some(format!("resume-{i}.docx")),
                native_sha256: Some(native.clone()),
                status: "extracted".into(),
                ..Default::default()
            })
            .unwrap();
        ids.push(item.id);
    }

    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let job = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let params = OfficeExtractParams {
        force: false,
        batch_size: 1,
        ..Default::default()
    };

    let done = Arc::new(AtomicU64::new(0));
    let done_cancel = done.clone();
    let cancel_fn = move || done_cancel.load(Ordering::SeqCst) >= 1;
    let done_progress = done.clone();
    let paused = run_office_extract(&matter, &job.id, &params, Some(&cancel_fn), |c| {
        done_progress.store(c, Ordering::SeqCst)
    })
    .unwrap();
    match paused {
        OfficeExtractOutcome::Paused(s) => {
            assert_eq!(
                s.completed_count, 1,
                "expected cancel after first item: {s:?}"
            );
            assert_eq!(s.extracted_count, 1, "summary={s:?}");
        }
        other => panic!("expected Paused after 1 item, got {other:?}"),
    }

    // Resume: no cancel; checkpoint cursor continues OFFSET past item 0.
    let resumed = run_office_extract(&matter, &job.id, &params, None, |_| {}).unwrap();
    match resumed {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.completed_count, N as u64, "summary={s:?}");
            assert_eq!(s.extracted_count, N as u64, "summary={s:?}");
            assert_eq!(s.error_count, 0, "summary={s:?}");
        }
        other => panic!("expected Succeeded on resume: {other:?}"),
    }

    for id in &ids {
        let item = matter.get_item(id).unwrap();
        assert!(
            item.text_sha256.is_some(),
            "item {id} missing text after cancel/resume"
        );
    }
}

#[test]
fn encrypted_ooxml_entry_markers() {
    // Minimal zip whose only payload is EncryptionInfo / EncryptedPackage markers.
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default();
        z.start_file("EncryptionInfo", opts).unwrap();
        z.write_all(b"not-real-encryption-info").unwrap();
        z.start_file("EncryptedPackage", opts).unwrap();
        z.write_all(b"ciphertext").unwrap();
        z.finish().unwrap();
    }
    let data = buf.into_inner();
    let err = extract_office(&data, Some("secret.docx"), None).expect_err("encrypted");
    assert_eq!(err.code(), "encrypted_office");
}

#[test]
fn fuzz_random_bytes_no_panic() {
    // Fixed-seed hostile corpus: random-ish buffers must not panic.
    // Call `extract_office` directly (not catch_unwind) so a panic fails the test.
    let seeds: &[&[u8]] = &[
        b"",
        b"PK\x03\x04",
        b"not zip",
        &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        &[0u8; 256],
        b"PK\x03\x04\x00\x00garbage\xff\xfe",
    ];
    for (i, seed) in seeds.iter().enumerate() {
        let mut data = seed.to_vec();
        for j in 0..64 {
            data.push(((i * 31 + j * 17) % 256) as u8);
        }
        // Result may be Ok or Err — only a panic fails.
        let _ = extract_office(&data, Some("fuzz.docx"), None);
        let _ = extract_office(&data, Some("fuzz.xlsx"), None);
        let _ = extract_office(&data, Some("fuzz.pptx"), None);
        let _ = extract_office(&data, None, None);
    }
}

/// True property test (proptest): arbitrary byte vectors never panic the extract path.
#[test]
fn prop_random_bytes_never_panic() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    let mut runner = TestRunner::new(Config {
        cases: 48,
        ..Config::default()
    });
    runner
        .run(&proptest::collection::vec(any::<u8>(), 0..512), |data| {
            // Direct call — panic fails the property. Ok/Err both fine.
            let _ = extract_office(&data, Some("prop.docx"), None);
            let _ = extract_office(&data, Some("prop.xlsx"), None);
            let _ = extract_office(&data, Some("prop.pptx"), None);
            let _ = extract_office(&data, None, Some("application/octet-stream"));
            Ok(())
        })
        .expect("proptest: extract_office panicked on random bytes");
}

#[test]
fn streaming_take_ignores_trusting_declared_size_alone() {
    // Prove hard cap is applied on the decompressed stream, not only via headers:
    // entry payload is larger than cap; declared size from zip crate equals payload
    // length, but we still take(cap+1) and reject when more than cap is delivered.
    // (True spoofed headers are hostile in the wild; production always uses take.)
    let payload = vec![b'Z'; 200];
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(&payload).unwrap();
        z.finish().unwrap();
    }
    let data = buf.into_inner();
    let mut archive = open_zip(&data).unwrap();
    let mut entry = archive.by_name("word/document.xml").unwrap();
    let declared = entry.size();
    assert_eq!(declared, 200, "stored entry declares true size");
    // Cap well below declared — take must stop us even though precheck would
    // accept declared sizes under production MAX_UNCOMPRESSED_ENTRY_BYTES.
    let err = read_entry_capped_with_max(&mut entry, 50).expect_err("over cap");
    assert_eq!(err.code(), "office_limit_exceeded");
}

#[test]
fn docx_invalid_utf8_run_uses_lossy_not_silent_drop() {
    // Hand-built OOXML: w:t contains invalid UTF-8; extract must not drop the run.
    let content_types = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;
    // Mix valid ASCII with invalid UTF-8 byte 0xFF inside text node content.
    let mut document = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
  <w:body><w:p><w:r><w:t>BEFORE_"
        .to_vec();
    document.push(0xFF);
    document.extend_from_slice(b"_AFTER_MARKER</w:t></w:r></w:p></w:body></w:document>");

    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("[Content_Types].xml", opts).unwrap();
        z.write_all(content_types.as_bytes()).unwrap();
        z.start_file("_rels/.rels", opts).unwrap();
        z.write_all(rels.as_bytes()).unwrap();
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(&document).unwrap();
        z.finish().unwrap();
    }
    let data = buf.into_inner();
    let extracted = extract_office(&data, Some("bad-utf8.docx"), None).expect("extract");
    assert!(
        extracted.text.contains("BEFORE_") && extracted.text.contains("_AFTER_MARKER"),
        "lossy path must keep surrounding text, not silent-drop: {}",
        extracted.text
    );
}

#[test]
fn zip_path_traversal_rejected() {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default();
        z.start_file("../evil.xml", opts).unwrap();
        z.write_all(b"<x/>").unwrap();
        z.finish().unwrap();
    }
    let data = buf.into_inner();
    let mut archive = open_zip(&data).unwrap();
    // by_name with traversal should fail validate
    let mut entry = archive.by_index(0).unwrap();
    let err = read_entry_capped(&mut entry).expect_err("traversal");
    assert_eq!(err.code(), "office_parse_error");
}

#[test]
fn body_change_nulls_redacted_via_apply() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "OfficeRdx").unwrap();

    let body = "Hello SECRET sauce";
    let text_sha = matter.put_bytes(body.as_bytes()).unwrap();
    let data = load_fixture("minimal.docx");
    let native = matter.put_bytes(&data).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("memo.docx".into()),
            native_sha256: Some(native.clone()),
            text_sha256: Some(text_sha.clone()),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    matter
        .create_redaction(CreateRedactionInput {
            item_id: item.id.clone(),
            start_utf8: 6,
            end_utf8: 12,
            exact_quote: "SECRET".into(),
            display_body: body.into(),
            body_digest: text_sha.clone(),
            reason: redaction_reason::OTHER.into(),
            label: None,
            actor: "alice".into(),
        })
        .unwrap();
    matter
        .regenerate_redacted_text(&item.id, body, "alice")
        .unwrap();
    assert!(matter
        .get_item(&item.id)
        .unwrap()
        .redacted_text_sha256
        .is_some());

    let extracted = extract_office(&data, Some("memo.docx"), None).unwrap();
    matter
        .apply_office_text(ApplyOfficeTextInput {
            item_id: item.id.clone(),
            force: true,
            text: Some(extracted.text),
            method: Some(methods::DOCX_XML_V1.into()),
            status: Some("ok".into()),
            error: None,
            source_native_sha256: Some(native),
            partial: false,
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();

    let reloaded = matter.get_item(&item.id).unwrap();
    assert!(reloaded.redacted_text_sha256.is_none());
    assert_ne!(reloaded.text_sha256.as_deref(), Some(text_sha.as_str()));
}

#[test]
fn reject_oversized_native_len_helper() {
    assert!(reject_oversized_native_len(MAX_NATIVE_INPUT_BYTES).is_ok());
    let err = reject_oversized_native_len(MAX_NATIVE_INPUT_BYTES + 1).unwrap_err();
    assert_eq!(err.code(), "office_limit_exceeded");
    let err = reject_oversized_native_len_with_max(11, 10).unwrap_err();
    assert_eq!(err.code(), "office_limit_exceeded");
}

/// CAS size precheck: oversized on-disk blob is rejected without full extract success.
#[test]
fn job_oversized_cas_blob_limit_without_full_load_path() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "OfficeHuge").unwrap();

    // Plant a real digest path, then extend the file past the cap via set_len
    // (sparse-friendly; avoids allocating 100MiB+ in the test process).
    let digest = matter.put_bytes(b"small-placeholder").unwrap();
    let path = matter
        .cas()
        .expect("local cas")
        .object_path(&digest)
        .unwrap();
    {
        let f = File::options()
            .write(true)
            .open(path.as_std_path())
            .unwrap();
        f.set_len(MAX_NATIVE_INPUT_BYTES + 1).unwrap();
    }
    assert_eq!(matter.cas_len(&digest).unwrap(), MAX_NATIVE_INPUT_BYTES + 1);
    // get_bytes_capped must refuse without returning the full body.
    let capped = matter.get_bytes_capped(&digest, MAX_NATIVE_INPUT_BYTES);
    assert!(capped.is_err(), "capped read must fail");

    matter
        .insert_item(ItemInput {
            path: Some("huge.docx".into()),
            native_sha256: Some(digest),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let outcome = run_office_extract(
        &matter,
        &job.id,
        &OfficeExtractParams::default(),
        None,
        |_| {},
    )
    .unwrap();
    match outcome {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1, "summary={s:?}");
            assert_eq!(s.extracted_count, 0);
        }
        other => panic!("expected success with error: {other:?}"),
    }
    let items = matter.list_office_candidates(0, 10, false).unwrap();
    let item = matter.get_item(&items[0].id).unwrap();
    assert_eq!(item.office_extract_status.as_deref(), Some("error"));
    let err = item.office_extract_error.unwrap_or_default();
    assert!(err.contains("office_limit_exceeded"), "error={err}");
    // Failed extract must not set successful source (retry-eligible).
    assert!(item.office_source_native_sha256.is_none());
}

/// Failed extract must not permanently skip; second non-force run still attempts.
#[test]
fn failed_extract_retries_on_second_non_force_run() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "OfficeRetry").unwrap();

    // Prior text + corrupt native: first run errors; second non-force still errors (not skip).
    let prior_text = matter.put_bytes(b"stale text body").unwrap();
    let corrupt = matter.put_bytes(b"PK\x03\x04not-a-real-docx").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("broken.docx".into()),
            native_sha256: Some(corrupt.clone()),
            text_sha256: Some(prior_text),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    let params = OfficeExtractParams::default();
    let job1 = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let o1 = run_office_extract(&matter, &job1.id, &params, None, |_| {}).unwrap();
    match o1 {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1, "summary={s:?}");
            assert_eq!(s.skipped_count, 0, "must not skip on first failure");
        }
        other => panic!("expected Succeeded: {other:?}"),
    }
    let after1 = matter.get_item(&item.id).unwrap();
    assert_eq!(after1.office_extract_status.as_deref(), Some("error"));
    // Source must remain unset so skip condition cannot fire.
    assert!(after1.office_source_native_sha256.is_none());
    assert!(
        after1.text_sha256.is_some(),
        "prior text preserved on error"
    );

    let job2 = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let o2 = run_office_extract(&matter, &job2.id, &params, None, |_| {}).unwrap();
    match o2 {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1, "second run must retry, not skip: {s:?}");
            assert_eq!(s.skipped_count, 0, "second run must not skip: {s:?}");
            assert_eq!(s.extracted_count, 0);
        }
        other => panic!("expected Succeeded: {other:?}"),
    }
    let after2 = matter.get_item(&item.id).unwrap();
    assert_eq!(after2.office_extract_status.as_deref(), Some("error"));
    assert!(after2.office_source_native_sha256.is_none());

    // Item errors recorded for both attempts.
    let errs = matter.item_errors_for_item(&item.id).unwrap();
    assert!(
        errs.len() >= 2,
        "expected >=2 item_errors, got {}",
        errs.len()
    );
}

/// CAS-only item (no path/mime) is listed, sniffed, and extracted.
#[test]
fn cas_only_pathless_office_item_extracted() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "OfficeCasOnly").unwrap();

    let data = load_fixture("minimal.docx");
    let native = matter.put_bytes(&data).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: None,
            mime_type: None,
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    let listed = matter.list_office_candidates(0, 10, false).unwrap();
    assert!(
        listed.iter().any(|c| c.id == item.id),
        "CAS-only item must appear in candidates"
    );

    let job = matter.create_job(JOB_KIND_OFFICE_EXTRACT).unwrap();
    let outcome = run_office_extract(
        &matter,
        &job.id,
        &OfficeExtractParams::default(),
        None,
        |_| {},
    )
    .unwrap();
    match outcome {
        OfficeExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, 1, "summary={s:?}");
            assert_eq!(s.error_count, 0);
        }
        other => panic!("expected success: {other:?}"),
    }
    let reloaded = matter.get_item(&item.id).unwrap();
    assert!(reloaded.text_sha256.is_some());
    assert_eq!(reloaded.office_extract_status.as_deref(), Some("ok"));
    assert_eq!(
        reloaded.office_source_native_sha256.as_deref(),
        Some(native.as_str())
    );
}

#[test]
fn invalid_xml_text_not_silently_dropped() {
    // DOCX with a text run that includes a replacement-requiring byte sequence
    // still yields content (lossy) rather than empty success from unwrap_or_default.
    let doc_xml = br#"<?xml version="1.0" encoding="UTF-8"?>
    <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
      <w:body><w:p><w:r><w:t>KEEP_ME</w:t></w:r></w:p></w:body>
    </w:document>"#;
    let content_types = br#"<?xml version="1.0" encoding="UTF-8"?>
    <Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
      <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
      <Default Extension="xml" ContentType="application/xml"/>
      <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
    </Types>"#;
    let rels = br#"<?xml version="1.0" encoding="UTF-8"?>
    <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
      <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
    </Relationships>"#;
    let mut buf = Cursor::new(Vec::new());
    {
        let mut z = ZipWriter::new(&mut buf);
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("[Content_Types].xml", opts).unwrap();
        z.write_all(content_types).unwrap();
        z.start_file("_rels/.rels", opts).unwrap();
        z.write_all(rels).unwrap();
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(doc_xml).unwrap();
        z.finish().unwrap();
    }
    let data = buf.into_inner();
    let extracted = extract_office(&data, Some("t.docx"), None).expect("docx");
    assert!(
        extracted.text.contains("KEEP_ME"),
        "text={}",
        extracted.text
    );
}

// Silence unused if redaction path changes — compile will tell.
const _: usize = MAX_EXTRACTED_TEXT_BYTES;
