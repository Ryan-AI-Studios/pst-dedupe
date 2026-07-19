//! Integration tests for extract-pdf (spec §3.8).

use std::fs;
use std::path::PathBuf;

use extract_pdf::limits::{
    methods, MAX_EXTRACTED_TEXT_BYTES, MAX_NATIVE_INPUT_BYTES, MIN_TEXT_CHARS_TOTAL,
    TRUNCATION_MARKER,
};
use extract_pdf::{
    classify_text, count_non_ws_chars, extract_pdf, extract_pdf_catch_unwind,
    extract_pdf_with_limits, looks_like_pdf, reject_oversized_native_len_with_max, run_pdf_extract,
    PdfExtractOutcome, PdfExtractParams, TextClass, JOB_KIND_PDF_EXTRACT,
};
use matter_core::{
    pdf_extract_status, ApplyPdfTextInput, ItemInput, Matter, PdfExtractApplyResult,
};
use proptest::prelude::*;
use tempfile::tempdir;

fn fixtures_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("fixtures");
    p.push("pdf");
    p
}

fn load_fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

/// Same generator as `examples/gen_fixtures.rs` (kept local for unit tests).
fn minimal_text_pdf(text: &str) -> Vec<u8> {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let content = format!("BT\n/F1 12 Tf\n72 720 Td\n({escaped}) Tj\nET\n");
    build_one_page_pdf(&content)
}

fn build_one_page_pdf(content: &str) -> Vec<u8> {
    let content_len = content.len();
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(b"%PDF-1.4\n%");
    body.extend_from_slice(&[0xE2, 0xE3, 0xCF, 0xD3]);
    body.push(b'\n');
    let mut offsets = Vec::new();
    offsets.push(body.len());
    body.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    offsets.push(body.len());
    body.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    offsets.push(body.len());
    body.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>\nendobj\n",
    );
    offsets.push(body.len());
    let content_obj =
        format!("4 0 obj\n<< /Length {content_len} >>\nstream\n{content}endstream\nendobj\n");
    body.extend_from_slice(content_obj.as_bytes());
    offsets.push(body.len());
    body.extend_from_slice(
        b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n",
    );
    let xref_pos = body.len();
    let mut xref = String::from("xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets {
        xref.push_str(&format!("{off:010} 00000 n \n"));
    }
    body.extend_from_slice(xref.as_bytes());
    let trailer = format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n");
    body.extend_from_slice(trailer.as_bytes());
    body
}

#[test]
fn happy_pdf_marker() {
    let data = load_fixture("minimal.pdf");
    assert!(looks_like_pdf(&data));
    let extracted = extract_pdf(&data, Some("minimal.pdf"), None).expect("pdf");
    assert!(
        extracted.text.contains("PDF_TEXT_MARKER"),
        "text={}",
        extracted.text
    );
    assert_eq!(extracted.method, methods::PDF_EXTRACT_V1);
    assert_eq!(extracted.class, TextClass::Ok);
    assert!(!extracted.class.needs_ocr());
    assert!(!extracted.partial);
}

#[test]
fn detect_pdf_magic() {
    assert!(looks_like_pdf(b"%PDF-1.4\n"));
    assert!(looks_like_pdf(b"\n  %PDF-1.7"));
    assert!(!looks_like_pdf(b"PK\x03\x04"));
}

#[test]
fn corrupt_pdf_no_panic() {
    let data = load_fixture("corrupt.pdf");
    let err = extract_pdf_catch_unwind(&data, Some("corrupt.pdf"), None).expect_err("corrupt");
    assert!(
        matches!(err.code(), "pdf_parse_error" | "pdf_not_pdf"),
        "code={}",
        err.code()
    );
}

#[test]
fn empty_pdf_classification() {
    let data = load_fixture("empty.pdf");
    let extracted = extract_pdf(&data, Some("empty.pdf"), None).expect("empty pdf parse");
    assert_eq!(extracted.class, TextClass::Empty);
    assert!(extracted.class.needs_ocr());
    assert!(extracted.text.is_empty() || count_non_ws_chars(&extracted.text) == 0);
}

#[test]
fn low_text_fixture_and_thresholds() {
    let data = load_fixture("low_text.pdf");
    let extracted = extract_pdf(&data, Some("low_text.pdf"), None).expect("low");
    // BATES001 is well under MIN_TEXT_CHARS_TOTAL
    assert!(
        count_non_ws_chars(&extracted.text) < MIN_TEXT_CHARS_TOTAL,
        "chars={}",
        count_non_ws_chars(&extracted.text)
    );
    assert_eq!(extracted.class, TextClass::LowText);
    assert!(extracted.class.needs_ocr());
    assert!(!extracted.text.is_empty());

    // Pure classification unit path
    assert_eq!(classify_text("   \n", 1), TextClass::Empty);
    assert_eq!(classify_text("short", 1), TextClass::LowText);
    let plenty = "abcdefghij".repeat(10);
    assert_eq!(classify_text(&plenty, 1), TextClass::Ok);
}

#[test]
fn over_limit_native_errors() {
    let mut huge = vec![0u8; (MAX_NATIVE_INPUT_BYTES as usize) + 1];
    huge[0..5].copy_from_slice(b"%PDF-");
    let err = extract_pdf(&huge, Some("huge.pdf"), None).expect_err("limit");
    assert_eq!(err.code(), "pdf_limit_exceeded");

    let err2 = reject_oversized_native_len_with_max(11, 10).unwrap_err();
    assert_eq!(err2.code(), "pdf_limit_exceeded");
}

#[test]
fn text_cap_early_break() {
    // Inject long text into a synthetic PDF and cap extract size.
    let long = "A".repeat(200);
    let data = minimal_text_pdf(&long);
    let extracted = extract_pdf_with_limits(&data, Some("long.pdf"), None, 40, 500).expect("cap");
    assert!(extracted.partial, "text={}", extracted.text);
    assert!(
        extracted.text.contains(TRUNCATION_MARKER),
        "text={}",
        extracted.text
    );
    assert!(extracted.text.len() < long.len() + TRUNCATION_MARKER.len() + 50);

    let full =
        extract_pdf_with_limits(&data, Some("long.pdf"), None, MAX_EXTRACTED_TEXT_BYTES, 500)
            .expect("full");
    assert!(full.text.contains(&long[..50]));
}

#[test]
fn apply_pdf_text_idempotent_and_side_effects() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "Pdf").unwrap();

    let data = load_fixture("minimal.pdf");
    let native = matter.put_bytes(&data).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("memo.pdf".into()),
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            file_category: Some("attachment".into()),
            ..Default::default()
        })
        .unwrap();

    matter
        .connection()
        .execute(
            "UPDATE items SET fts_text_sha256 = 'deadbeef', fts_indexed_at = 't', \
                    redacted_text_sha256 = 'redacteddead', redacted_text_at = 't', \
                    redacted_source_digest = 'old' WHERE id = ?1",
            rusqlite::params![item.id],
        )
        .unwrap();

    let extracted = extract_pdf(&data, Some("memo.pdf"), None).unwrap();
    let apply = matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: false,
            text: Some(extracted.text.clone()),
            method: Some(extracted.method.clone()),
            status: Some(pdf_extract_status::OK.into()),
            error: None,
            source_native_sha256: Some(native.clone()),
            partial: false,
            page_count: Some(extracted.page_count as i64),
            needs_ocr: Some(0),
            file_category: Some("pdf".into()),
            refine_file_category: true,
        })
        .unwrap();
    assert!(matches!(
        apply,
        PdfExtractApplyResult::Applied {
            text_changed: true,
            ..
        }
    ));

    let reloaded = matter.get_item(&item.id).unwrap();
    assert!(reloaded.text_sha256.is_some());
    assert_eq!(reloaded.pdf_extract_status.as_deref(), Some("ok"));
    assert_eq!(reloaded.pdf_needs_ocr, 0);
    assert_eq!(
        reloaded.pdf_source_native_sha256.as_deref(),
        Some(native.as_str())
    );
    assert_eq!(reloaded.file_category.as_deref(), Some("pdf"));
    assert!(reloaded.redacted_text_sha256.is_none());
    assert!(reloaded.redacted_text_at.is_none());
    assert!(reloaded.redacted_source_digest.is_none());
    let fts: Option<String> = matter
        .connection()
        .query_row(
            "SELECT fts_text_sha256 FROM items WHERE id = ?1",
            rusqlite::params![item.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(fts.is_none());

    // Idempotent skip
    let skip = matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: false,
            text: Some(extracted.text.clone()),
            method: Some(extracted.method.clone()),
            status: Some(pdf_extract_status::OK.into()),
            error: None,
            source_native_sha256: Some(native.clone()),
            partial: false,
            page_count: None,
            needs_ocr: Some(0),
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert!(matches!(skip, PdfExtractApplyResult::Skipped));

    // Force re-extract updates text
    let force = matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: true,
            text: Some(format!("{}\nforce", extracted.text)),
            method: Some(extracted.method),
            status: Some(pdf_extract_status::OK.into()),
            error: None,
            source_native_sha256: Some(native),
            partial: false,
            page_count: None,
            needs_ocr: Some(0),
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert!(matches!(
        force,
        PdfExtractApplyResult::Applied {
            text_changed: true,
            ..
        }
    ));
}

#[test]
fn apply_empty_and_low_text_needs_ocr() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "PdfEmpty").unwrap();
    let native = matter.put_bytes(b"%PDF-1.4 empty-ish").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("scan.pdf".into()),
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    let empty = matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: false,
            text: None,
            method: Some(methods::PDF_EXTRACT_V1.into()),
            status: Some(pdf_extract_status::EMPTY.into()),
            error: Some("pdf_empty_text".into()),
            source_native_sha256: Some(native.clone()),
            partial: false,
            page_count: Some(1),
            needs_ocr: Some(1),
            file_category: Some("pdf".into()),
            refine_file_category: true,
        })
        .unwrap();
    assert!(matches!(empty, PdfExtractApplyResult::Empty { .. }));
    let r = matter.get_item(&item.id).unwrap();
    assert!(r.text_sha256.is_none());
    assert_eq!(r.pdf_extract_status.as_deref(), Some("empty"));
    assert_eq!(r.pdf_needs_ocr, 1);
    assert_eq!(r.pdf_source_native_sha256.as_deref(), Some(native.as_str()));

    // Low-text still writes CAS
    let low = matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: true,
            text: Some("BATES001".into()),
            method: Some(methods::PDF_EXTRACT_V1.into()),
            status: Some(pdf_extract_status::LOW_TEXT.into()),
            error: None,
            source_native_sha256: Some(native.clone()),
            partial: false,
            page_count: Some(2),
            needs_ocr: Some(1),
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert!(matches!(low, PdfExtractApplyResult::LowText { .. }));
    let r2 = matter.get_item(&item.id).unwrap();
    assert!(r2.text_sha256.is_some());
    assert_eq!(r2.pdf_extract_status.as_deref(), Some("low_text"));
    assert_eq!(r2.pdf_needs_ocr, 1);
}

/// Empty success clears prior text + redacted_* + FTS (force path).
#[test]
fn apply_empty_invalidates_redacted_and_fts() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "PdfEmptyRedacted").unwrap();
    let native = matter.put_bytes(b"%PDF-1.4 prior-body").unwrap();
    let prior_text = matter.put_bytes(b"prior extracted body").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("was-text.pdf".into()),
            native_sha256: Some(native.clone()),
            text_sha256: Some(prior_text),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    matter
        .connection()
        .execute(
            "UPDATE items SET fts_text_sha256 = 'deadbeef', fts_indexed_at = 't', \
                    redacted_text_sha256 = 'redacteddead', redacted_text_at = 't', \
                    redacted_source_digest = 'old' WHERE id = ?1",
            rusqlite::params![item.id],
        )
        .unwrap();

    let empty = matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: true,
            text: None,
            method: Some(methods::PDF_EXTRACT_V1.into()),
            status: Some(pdf_extract_status::EMPTY.into()),
            error: Some("pdf_empty_text".into()),
            source_native_sha256: Some(native.clone()),
            partial: false,
            page_count: Some(1),
            needs_ocr: Some(1),
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert!(matches!(empty, PdfExtractApplyResult::Empty { .. }));

    let r = matter.get_item(&item.id).unwrap();
    assert!(r.text_sha256.is_none());
    assert_eq!(r.pdf_extract_status.as_deref(), Some("empty"));
    assert_eq!(r.pdf_needs_ocr, 1);
    assert_eq!(r.pdf_source_native_sha256.as_deref(), Some(native.as_str()));
    assert!(r.redacted_text_sha256.is_none());
    assert!(r.redacted_text_at.is_none());
    assert!(r.redacted_source_digest.is_none());
    let fts: Option<String> = matter
        .connection()
        .query_row(
            "SELECT fts_text_sha256 FROM items WHERE id = ?1",
            rusqlite::params![item.id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(fts.is_none());
}

/// Error bookkeeping must not wipe prior empty/low_text OCR candidacy.
#[test]
fn apply_error_preserves_needs_ocr() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "PdfNeedsOcrSurvive").unwrap();
    let native = matter.put_bytes(b"%PDF-1.4 scan").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("scan.pdf".into()),
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: false,
            text: None,
            method: Some(methods::PDF_EXTRACT_V1.into()),
            status: Some(pdf_extract_status::EMPTY.into()),
            error: Some("pdf_empty_text".into()),
            source_native_sha256: Some(native.clone()),
            partial: false,
            page_count: Some(1),
            needs_ocr: Some(1),
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert_eq!(matter.get_item(&item.id).unwrap().pdf_needs_ocr, 1);

    let err = matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: true,
            text: None,
            method: None,
            status: Some(pdf_extract_status::ERROR.into()),
            error: Some("pdf_parse_error: boom".into()),
            source_native_sha256: Some(native),
            partial: false,
            page_count: None,
            needs_ocr: Some(0), // even if caller wrongly passes 0, column must stay
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert!(matches!(err, PdfExtractApplyResult::Error { .. }));

    let r = matter.get_item(&item.id).unwrap();
    assert_eq!(r.pdf_extract_status.as_deref(), Some("error"));
    assert_eq!(
        r.pdf_needs_ocr, 1,
        "error must not wipe prior empty OCR candidacy"
    );
}

/// Pure error apply does not claim successful source (retry-eligible).
#[test]
fn apply_error_does_not_set_source_native() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "PdfErrSource").unwrap();
    let native = matter.put_bytes(b"%PDF-1.4 broken").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("broken.pdf".into()),
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    let err = matter
        .apply_pdf_text(ApplyPdfTextInput {
            item_id: item.id.clone(),
            force: true,
            text: None,
            method: None,
            status: Some(pdf_extract_status::ERROR.into()),
            error: Some("pdf_parse_error: corrupt".into()),
            source_native_sha256: Some(native.clone()),
            partial: false,
            page_count: None,
            needs_ocr: None,
            file_category: None,
            refine_file_category: false,
        })
        .unwrap();
    assert!(matches!(err, PdfExtractApplyResult::Error { .. }));

    let r = matter.get_item(&item.id).unwrap();
    assert_eq!(r.pdf_extract_status.as_deref(), Some("error"));
    assert!(
        r.pdf_source_native_sha256.is_none(),
        "error must not set pdf_source_native_sha256"
    );
}

/// Job path: corrupt native → error status, source NULL; second non-force still retries.
#[test]
fn job_corrupt_error_no_source_retries() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "PdfCorruptJob").unwrap();

    let corrupt = load_fixture("corrupt.pdf");
    let native = matter.put_bytes(&corrupt).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("corrupt.pdf".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    let params = PdfExtractParams::default();
    let job1 = matter.create_job(JOB_KIND_PDF_EXTRACT).unwrap();
    let o1 = run_pdf_extract(&matter, &job1.id, &params, None, |_| {}).unwrap();
    match o1 {
        PdfExtractOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1, "summary={s:?}");
            assert_eq!(s.skipped_count, 0, "must not skip on first failure");
        }
        other => panic!("expected Succeeded: {other:?}"),
    }
    let after1 = matter.get_item(&item.id).unwrap();
    assert_eq!(after1.pdf_extract_status.as_deref(), Some("error"));
    assert!(after1.pdf_source_native_sha256.is_none());

    let job2 = matter.create_job(JOB_KIND_PDF_EXTRACT).unwrap();
    let o2 = run_pdf_extract(&matter, &job2.id, &params, None, |_| {}).unwrap();
    match o2 {
        PdfExtractOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1, "second run must retry, not skip: {s:?}");
            assert_eq!(s.skipped_count, 0, "second run must not skip: {s:?}");
        }
        other => panic!("expected Succeeded: {other:?}"),
    }
    let after2 = matter.get_item(&item.id).unwrap();
    assert_eq!(after2.pdf_extract_status.as_deref(), Some("error"));
    assert!(after2.pdf_source_native_sha256.is_none());
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
    let matter = Matter::create(&root, "PdfMulti").unwrap();

    let data = load_fixture("minimal.pdf");
    let native = matter.put_bytes(&data).unwrap();
    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        let item = matter
            .insert_item(ItemInput {
                path: Some(format!("memo-{i}.pdf")),
                native_sha256: Some(native.clone()),
                status: "extracted".into(),
                ..Default::default()
            })
            .unwrap();
        ids.push(item.id);
    }

    let job = matter.create_job(JOB_KIND_PDF_EXTRACT).unwrap();
    let params = PdfExtractParams {
        force: false,
        batch_size: 1,
    };
    let outcome = run_pdf_extract(&matter, &job.id, &params, None, |_| {}).unwrap();
    match outcome {
        PdfExtractOutcome::Succeeded(s) => {
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
            item.pdf_source_native_sha256.as_deref(),
            Some(native.as_str())
        );
    }
}

/// Cancel after one item → Paused; resume same job → remaining finish.
#[test]
fn multi_item_cancel_then_resume_completes_all() {
    const N: usize = 3;
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "PdfResume").unwrap();

    let data = load_fixture("minimal.pdf");
    let native = matter.put_bytes(&data).unwrap();
    let mut ids = Vec::with_capacity(N);
    for i in 0..N {
        let item = matter
            .insert_item(ItemInput {
                path: Some(format!("resume-{i}.pdf")),
                native_sha256: Some(native.clone()),
                status: "extracted".into(),
                ..Default::default()
            })
            .unwrap();
        ids.push(item.id);
    }

    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let job = matter.create_job(JOB_KIND_PDF_EXTRACT).unwrap();
    let params = PdfExtractParams {
        force: false,
        batch_size: 1,
    };

    let done = Arc::new(AtomicU64::new(0));
    let done_cancel = done.clone();
    let cancel_fn = move || done_cancel.load(Ordering::SeqCst) >= 1;
    let done_progress = done.clone();
    let paused = run_pdf_extract(&matter, &job.id, &params, Some(&cancel_fn), |c| {
        done_progress.store(c, Ordering::SeqCst)
    })
    .unwrap();
    match paused {
        PdfExtractOutcome::Paused(s) => {
            assert_eq!(
                s.completed_count, 1,
                "expected cancel after first item: {s:?}"
            );
            assert_eq!(s.extracted_count, 1, "summary={s:?}");
        }
        other => panic!("expected Paused after 1 item, got {other:?}"),
    }

    let resumed = run_pdf_extract(&matter, &job.id, &params, None, |_| {}).unwrap();
    match resumed {
        PdfExtractOutcome::Succeeded(s) => {
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
fn job_run_extracts_and_skips() {
    let dir = tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().join("m")).unwrap();
    let matter = Matter::create(&root, "PdfJob").unwrap();

    let data = load_fixture("minimal.pdf");
    let native = matter.put_bytes(&data).unwrap();
    let _item = matter
        .insert_item(ItemInput {
            path: Some("memo.pdf".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_PDF_EXTRACT).unwrap();
    let params = PdfExtractParams::default();
    let outcome = run_pdf_extract(&matter, &job.id, &params, None, |_| {}).unwrap();
    match outcome {
        PdfExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, 1);
            assert_eq!(s.error_count, 0);
        }
        other => panic!("expected success: {other:?}"),
    }

    let job2 = matter.create_job(JOB_KIND_PDF_EXTRACT).unwrap();
    let outcome2 = run_pdf_extract(&matter, &job2.id, &params, None, |_| {}).unwrap();
    match outcome2 {
        PdfExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, 0);
            assert_eq!(s.skipped_count, 1);
            assert_eq!(s.completed_count, 1);
        }
        other => panic!("expected skip: {other:?}"),
    }

    // Force re-extract job path
    let job3 = matter.create_job(JOB_KIND_PDF_EXTRACT).unwrap();
    let force = PdfExtractParams {
        force: true,
        ..Default::default()
    };
    let outcome3 = run_pdf_extract(&matter, &job3.id, &force, None, |_| {}).unwrap();
    match outcome3 {
        PdfExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, 1, "force must re-extract: {s:?}");
            assert_eq!(s.skipped_count, 0, "force must not skip: {s:?}");
            assert_eq!(s.error_count, 0, "summary={s:?}");
        }
        other => panic!("expected force success: {other:?}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn proptest_pdf_bytes_no_panic(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        // Seed: random bytes never panic the catch_unwind path.
        let _ = extract_pdf_catch_unwind(&bytes, Some("fuzz.pdf"), None);
    }
}
