//! Integration tests for ocr-plugin (spec §3.12) — no system Tesseract.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use matter_core::{ocr_status, ItemInput, Matter};
use ocr_plugin::{
    default_ocr_argv, minimal_png_bytes, purge_ocr_temp_dir, run_ocr, run_ocr_with_engine,
    MockOcrEngine, OcrEngine, OcrOutcome, OcrParams, OcrTempFile, JOB_KIND_OCR,
};

fn make_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let dir = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join(name), name).unwrap();
    (dir, matter)
}

fn enabled_mock_params() -> OcrParams {
    OcrParams {
        enabled: true,
        engine: "mock".into(),
        batch_size: 10,
        ..OcrParams::default()
    }
}

#[test]
fn mock_image_sets_text_cas_and_sha() {
    let (_tmp, matter) = make_matter("ocr-img");
    let png = minimal_png_bytes();
    let native = matter.put_bytes(&png).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("scan.png".into()),
            native_sha256: Some(native.clone()),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            file_category: Some("image".into()),
            ..Default::default()
        })
        .unwrap();

    let engine = MockOcrEngine::new("HELLO_OCR_MARKER unique text");
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    let outcome = run_ocr_with_engine(
        &matter,
        &job.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    match outcome {
        OcrOutcome::Succeeded(s) => {
            assert_eq!(s.ocr_count, 1);
            assert_eq!(s.error_count, 0);
        }
        other => panic!("unexpected {other:?}"),
    }

    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.ocr_status.as_deref(), Some(ocr_status::OK));
    assert!(after.text_sha256.is_some());
    assert_eq!(after.text_sha256, after.ocr_text_sha256);
    assert_eq!(
        after.ocr_source_native_sha256.as_deref(),
        Some(native.as_str())
    );
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("HELLO_OCR_MARKER"));
    assert_eq!(after.ocr_page_count, Some(1));
}

#[test]
fn mock_pdf_candidate_clears_pdf_needs_ocr() {
    let (_tmp, matter) = make_matter("ocr-pdf");
    // Minimal PDF magic + body so looks_like_pdf passes if sniffed.
    let mut pdf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF\n".to_vec();
    pdf.extend_from_slice(b"scanned");
    let native = matter.put_bytes(&pdf).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("scan.pdf".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("application/pdf".into()),
            file_category: Some("pdf".into()),
            ..Default::default()
        })
        .unwrap();
    // Flag needs OCR as extract-pdf would.
    matter
        .connection()
        .execute(
            "UPDATE items SET pdf_needs_ocr = 1 WHERE id = ?1",
            [&item.id],
        )
        .unwrap();

    let engine = MockOcrEngine::new("PDF_OCR_BODY");
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    let outcome = run_ocr_with_engine(
        &matter,
        &job.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    assert!(matches!(outcome, OcrOutcome::Succeeded(_)));

    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.ocr_status.as_deref(), Some(ocr_status::OK));
    assert_eq!(after.pdf_needs_ocr, 0);
    assert!(after.text_sha256.is_some());
}

#[test]
fn disabled_job_fails_no_mutation() {
    let (_tmp, matter) = make_matter("ocr-off");
    let png = minimal_png_bytes();
    let native = matter.put_bytes(&png).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.png".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            ..Default::default()
        })
        .unwrap();

    let params = OcrParams {
        enabled: false,
        engine: "mock".into(),
        ..OcrParams::default()
    };
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    let outcome = run_ocr(&matter, &job.id, &params, None, |_| {}).unwrap();
    match outcome {
        OcrOutcome::Failed { message, summary } => {
            assert!(message.to_lowercase().contains("disabled"));
            assert_eq!(summary.completed_count, 0);
        }
        other => panic!("expected Failed, got {other:?}"),
    }

    let after = matter.get_item(&item.id).unwrap();
    assert!(after.ocr_status.is_none());
    assert!(after.text_sha256.is_none());
}

#[test]
fn default_argv_includes_psm_1() {
    let args = default_ocr_argv("page.png", "eng");
    let idx = args.iter().position(|a| a == "--psm").expect("psm flag");
    assert_eq!(args[idx + 1], "1");
}

#[test]
fn drop_guard_and_purge() {
    let dir = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let path = {
        let mut t = OcrTempFile::new_in(&root, ".png").unwrap();
        t.write_all(b"x").unwrap();
        t.path_buf()
    };
    assert!(!path.exists());

    let ocr_dir = ocr_plugin::ensure_ocr_temp_dir(&root).unwrap();
    let orphan = ocr_dir.as_std_path().join("orphan.png");
    std::fs::write(&orphan, b"leak").unwrap();
    assert_eq!(purge_ocr_temp_dir(&root).unwrap(), 1);
    assert!(!orphan.exists());
}

#[test]
fn idempotent_skip_when_native_unchanged() {
    let (_tmp, matter) = make_matter("ocr-idem");
    let png = minimal_png_bytes();
    let native = matter.put_bytes(&png).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.png".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            ..Default::default()
        })
        .unwrap();

    let engine = MockOcrEngine::new("ONCE");
    let params = enabled_mock_params();
    let job1 = matter.create_job(JOB_KIND_OCR).unwrap();
    run_ocr_with_engine(&matter, &job1.id, &params, &engine, None, |_| {}).unwrap();
    let mid = matter.get_item(&item.id).unwrap();
    let text1 = mid.text_sha256.clone();

    let job2 = matter.create_job(JOB_KIND_OCR).unwrap();
    let outcome = run_ocr_with_engine(&matter, &job2.id, &params, &engine, None, |_| {}).unwrap();
    match outcome {
        OcrOutcome::Succeeded(s) => {
            assert_eq!(s.skipped_count, 1);
            assert_eq!(s.ocr_count, 0);
        }
        other => panic!("{other:?}"),
    }
    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.text_sha256, text1);
}

#[test]
fn force_re_ocr() {
    let (_tmp, matter) = make_matter("ocr-force");
    let png = minimal_png_bytes();
    let native = matter.put_bytes(&png).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.png".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            ..Default::default()
        })
        .unwrap();

    let engine1 = MockOcrEngine::new("FIRST");
    let params = enabled_mock_params();
    let job1 = matter.create_job(JOB_KIND_OCR).unwrap();
    run_ocr_with_engine(&matter, &job1.id, &params, &engine1, None, |_| {}).unwrap();

    let engine2 = MockOcrEngine::new("SECOND_FORCE");
    let mut force = enabled_mock_params();
    force.force = true;
    let job2 = matter.create_job(JOB_KIND_OCR).unwrap();
    let outcome = run_ocr_with_engine(&matter, &job2.id, &force, &engine2, None, |_| {}).unwrap();
    match outcome {
        OcrOutcome::Succeeded(s) => assert_eq!(s.ocr_count, 1),
        other => panic!("{other:?}"),
    }
    let after = matter.get_item(&item.id).unwrap();
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("SECOND_FORCE"));
}

#[test]
fn cancel_between_items_no_orphan_temps() {
    let (_tmp, matter) = make_matter("ocr-cancel");
    for i in 0..3 {
        let png = minimal_png_bytes();
        let native = matter.put_bytes(&png).unwrap();
        matter
            .insert_item(ItemInput {
                path: Some(format!("p{i}.png")),
                native_sha256: Some(native),
                status: "extracted".into(),
                mime_type: Some("image/png".into()),
                ..Default::default()
            })
            .unwrap();
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag2 = cancel_flag.clone();
    let completed_flag = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let completed_flag2 = completed_flag.clone();
    let engine = MockOcrEngine::default();
    let mut params = enabled_mock_params();
    params.batch_size = 1;
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    let outcome = run_ocr_with_engine(
        &matter,
        &job.id,
        &params,
        &engine,
        Some(&|| cancel_flag2.load(Ordering::SeqCst)),
        |completed| {
            completed_flag2.store(completed, Ordering::SeqCst);
            if completed >= 1 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        },
    )
    .unwrap();
    assert!(matches!(outcome, OcrOutcome::Paused(_)), "{outcome:?}");
    assert!(completed_flag.load(Ordering::SeqCst) >= 1);

    // No residual temps after pause (page temps dropped).
    let ocr_dir = ocr_plugin::ocr_temp_dir(matter.root());
    if ocr_dir.as_std_path().exists() {
        let left: Vec<_> = std::fs::read_dir(ocr_dir.as_std_path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            left.is_empty(),
            "orphan temps after cancel: {:?}",
            left.iter().map(|e| e.path()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn redaction_present_skips() {
    let (_tmp, matter) = make_matter("ocr-redact");
    let png = minimal_png_bytes();
    let native = matter.put_bytes(&png).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.png".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            ..Default::default()
        })
        .unwrap();
    matter
        .connection()
        .execute(
            "UPDATE items SET redaction_count = 2 WHERE id = ?1",
            [&item.id],
        )
        .unwrap();

    let engine = MockOcrEngine::new("SHOULD_NOT_APPLY");
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    let outcome = run_ocr_with_engine(
        &matter,
        &job.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();
    match outcome {
        OcrOutcome::Succeeded(s) => {
            assert_eq!(s.skipped_count, 1);
            assert_eq!(s.ocr_count, 0);
        }
        other => panic!("{other:?}"),
    }
    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.ocr_status.as_deref(), Some(ocr_status::SKIPPED));
    assert!(after
        .ocr_error
        .as_deref()
        .unwrap_or("")
        .contains("redaction"));
    assert!(after.text_sha256.is_none());
}

#[test]
fn fts_and_redacted_cleared_on_success() {
    let (_tmp, matter) = make_matter("ocr-fts");
    let png = minimal_png_bytes();
    let native = matter.put_bytes(&png).unwrap();
    let prior_text = matter.put_bytes(b"old body").unwrap();
    let redacted = matter.put_bytes(b"[REDACTED]").unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.png".into()),
            native_sha256: Some(native),
            text_sha256: Some(prior_text.clone()),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            ..Default::default()
        })
        .unwrap();
    matter
        .connection()
        .execute(
            "UPDATE items SET fts_text_sha256 = ?1, fts_indexed_at = '2020-01-01T00:00:00Z', \
             fts_error = 'x', redacted_text_sha256 = ?2, redacted_text_at = '2020-01-01T00:00:00Z', \
             redacted_source_digest = ?1 WHERE id = ?3",
            rusqlite::params![prior_text, redacted, item.id],
        )
        .unwrap();

    let engine = MockOcrEngine::new("NEW_OCR_BODY");
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    run_ocr_with_engine(
        &matter,
        &job.id,
        &enabled_mock_params(),
        &engine,
        None,
        |_| {},
    )
    .unwrap();

    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.ocr_status.as_deref(), Some(ocr_status::OK));
    // FTS / redacted bookkeeping cleared on OCR success (0032 / 0029).
    let (fts, red, red_at, red_src): (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = matter
        .connection()
        .query_row(
            "SELECT fts_text_sha256, redacted_text_sha256, redacted_text_at, redacted_source_digest \
             FROM items WHERE id = ?1",
            [&item.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert!(fts.is_none());
    assert!(red.is_none());
    assert!(red_at.is_none());
    assert!(red_src.is_none());
    assert_ne!(after.text_sha256.as_deref(), Some(prior_text.as_str()));
}

#[test]
fn multi_page_mock_page_loop() {
    let engine = MockOcrEngine::with_pages(vec![
        "PAGE_ONE".into(),
        "PAGE_TWO".into(),
        "PAGE_THREE".into(),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let mut texts = Vec::new();
    for i in 0..3 {
        let mut t = OcrTempFile::new_in(&root, ".png").unwrap();
        t.write_all(&minimal_png_bytes()).unwrap();
        let r = engine.ocr_image(t.path(), "eng").unwrap();
        texts.push(r.text);
        drop(t);
        assert_eq!(texts[i], format!("PAGE_{}", ["ONE", "TWO", "THREE"][i]));
    }
    assert_eq!(texts.len(), 3);
}

/// Non-mock engine used only to prove the PDF-renderer fail-closed path
/// (mock intentionally bypasses missing renderer for CI).
struct StubNonMockEngine;

impl OcrEngine for StubNonMockEngine {
    fn id(&self) -> &str {
        "stub_tesseract_cli"
    }
    fn version(&self) -> ocr_plugin::Result<String> {
        Ok("stub-0".into())
    }
    fn ocr_image(
        &self,
        _path: &camino::Utf8Path,
        _lang: &str,
    ) -> ocr_plugin::Result<ocr_plugin::OcrPageResult> {
        Ok(ocr_plugin::OcrPageResult {
            text: "should_not_run".into(),
            confidence: None,
        })
    }
}

#[test]
fn pdf_missing_renderer_fails_closed_leaves_needs_ocr() {
    let (_tmp, matter) = make_matter("ocr-no-render");
    let mut pdf = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF\n".to_vec();
    pdf.extend_from_slice(b"scanned");
    let native = matter.put_bytes(&pdf).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("scan.pdf".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("application/pdf".into()),
            file_category: Some("pdf".into()),
            ..Default::default()
        })
        .unwrap();
    matter
        .connection()
        .execute(
            "UPDATE items SET pdf_needs_ocr = 1 WHERE id = ?1",
            [&item.id],
        )
        .unwrap();

    // Force "no renderer" even if pdftoppm exists on the host PATH.
    let mut params = enabled_mock_params();
    params.engine = "stub".into();
    params.pdf_renderer_path = Some(
        if cfg!(windows) {
            r"C:\nonexistent\ocr-plugin-missing\pdftoppm.exe"
        } else {
            "/nonexistent/ocr-plugin-missing/pdftoppm"
        }
        .into(),
    );

    let engine = StubNonMockEngine;
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    let outcome = run_ocr_with_engine(&matter, &job.id, &params, &engine, None, |_| {}).unwrap();
    match outcome {
        OcrOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1);
            assert_eq!(s.ocr_count, 0);
        }
        other => panic!("expected succeeded with item error, got {other:?}"),
    }

    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.ocr_status.as_deref(), Some(ocr_status::ERROR));
    assert!(
        after
            .ocr_error
            .as_deref()
            .unwrap_or("")
            .contains("ocr_pdf_renderer_missing")
            || after
                .ocr_error
                .as_deref()
                .unwrap_or("")
                .to_ascii_lowercase()
                .contains("renderer"),
        "error: {:?}",
        after.ocr_error
    );
    // Must not clear the 0034 handoff flag on renderer failure.
    assert_eq!(after.pdf_needs_ocr, 1);
    assert!(after.text_sha256.is_none());
    assert!(after.ocr_text_sha256.is_none());
}

#[test]
fn production_rejects_mock_engine_param() {
    let (_tmp, matter) = make_matter("ocr-no-mock-prod");
    let png = minimal_png_bytes();
    let native = matter.put_bytes(&png).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.png".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            ..Default::default()
        })
        .unwrap();
    let params = OcrParams {
        enabled: true,
        engine: "mock".into(),
        ..OcrParams::default()
    };
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    let err = run_ocr(&matter, &job.id, &params, None, |_| {}).expect_err("mock rejected");
    assert!(
        err.to_string().to_ascii_lowercase().contains("mock"),
        "{err}"
    );
    let after = matter.get_item(&item.id).unwrap();
    assert!(after.ocr_status.is_none());
    assert!(after.text_sha256.is_none());
}

#[test]
fn truncate_ocr_text_utf8_safe() {
    // Multibyte chars near the cap must not panic.
    let marker = ocr_plugin::TRUNCATION_MARKER;
    // Build a string just over a small simulated cap by using public helper
    // against a string of multi-byte characters larger than the real cap is
    // impractical; unit-test the helper with a local-sized string by verifying
    // boundary safety on a constructed oversize buffer of 2-byte chars.
    // We exercise the public API with a large-ish payload of "é" (2 bytes).
    let unit = "é"; // 2 bytes UTF-8
    let n = (ocr_plugin::MAX_OCR_TEXT_BYTES / unit.len()) + 8;
    let big = unit.repeat(n);
    let out = ocr_plugin::truncate_ocr_text(big);
    assert!(out.len() <= ocr_plugin::MAX_OCR_TEXT_BYTES + marker.len());
    assert!(out.ends_with(marker) || out.len() <= ocr_plugin::MAX_OCR_TEXT_BYTES);
    // Valid UTF-8 (would have panicked on bad truncate).
    assert!(std::str::from_utf8(out.as_bytes()).is_ok());
}

#[test]
fn enabled_missing_tesseract_path_fails_honestly() {
    let (_tmp, matter) = make_matter("ocr-no-tess");
    let png = minimal_png_bytes();
    let native = matter.put_bytes(&png).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("a.png".into()),
            native_sha256: Some(native),
            status: "extracted".into(),
            mime_type: Some("image/png".into()),
            ..Default::default()
        })
        .unwrap();

    let params = OcrParams {
        enabled: true,
        engine: "tesseract".into(),
        tesseract_path: Some(
            if cfg!(windows) {
                r"C:\nonexistent\ocr-plugin-tesseract-missing\tesseract.exe"
            } else {
                "/nonexistent/ocr-plugin-tesseract-missing/tesseract"
            }
            .into(),
        ),
        ..OcrParams::default()
    };
    let job = matter.create_job(JOB_KIND_OCR).unwrap();
    let err = run_ocr(&matter, &job.id, &params, None, |_| {}).expect_err("must fail");
    assert_eq!(err.code(), "ocr_engine_not_found");

    let after = matter.get_item(&item.id).unwrap();
    assert!(
        after.ocr_status.is_none(),
        "no item mutation on engine resolve fail"
    );
    assert!(after.text_sha256.is_none());
}
