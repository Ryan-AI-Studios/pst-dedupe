//! Resumable `ocr` job.

use std::time::Instant;

use matter_core::{
    ocr_status, ApplyOcrTextInput, AuditEventInput, Matter, OcrApplyResult, OcrCandidate,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detect::{is_image_meta, is_pdf_meta, looks_like_pdf};
use crate::engine::{OcrEngine, TesseractCliEngine};
use crate::error::{codes, Error, Result};
use crate::limits::{
    engines, MAX_NATIVE_INPUT_BYTES, MAX_OCR_TEXT_BYTES, MAX_PAGES, TRUNCATION_MARKER,
};
use crate::params::OcrParams;
use crate::render::PdfRenderer;
use crate::temp::{purge_ocr_temp_dir, OcrTempFile};

/// Job kind string for process-runner.
pub const JOB_KIND_OCR: &str = "ocr";
/// Checkpoint stage name.
pub const OCR_STAGE: &str = "ocr";

/// Summary counts after an OCR run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrSummary {
    pub completed_count: u64,
    pub ocr_count: u64,
    pub skipped_count: u64,
    pub error_count: u64,
}

/// Outcome of [`run_ocr`] / [`run_ocr_with_engine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrOutcome {
    Succeeded(OcrSummary),
    Paused(OcrSummary),
    Failed {
        message: String,
        summary: OcrSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    /// 1-based next PDF page to OCR when resuming mid-document (0 = item start).
    #[serde(default)]
    next_page: u32,
    /// Accumulated OCR text when paused mid multi-page PDF (capped).
    #[serde(default)]
    partial_text: String,
    completed_count: u64,
    ocr_count: u64,
    skipped_count: u64,
    error_count: u64,
    params: serde_json::Value,
}

/// Reject oversized native length before any full CAS load.
pub fn reject_oversized_native_len(len: u64) -> Result<()> {
    reject_oversized_native_len_with_max(len, MAX_NATIVE_INPUT_BYTES)
}

/// Same as [`reject_oversized_native_len`] with an injectable max (tests).
pub fn reject_oversized_native_len_with_max(len: u64, max: u64) -> Result<()> {
    if len > max {
        return Err(Error::limit(format!("native size {len} exceeds max {max}")));
    }
    Ok(())
}

/// Run OCR on `matter` for the runner-created `job_id` (production path).
///
/// Production only allows the **Tesseract CLI** engine. `"engine": "mock"` and
/// unknown engines are rejected (no fabricated OCR in production). Tests inject
/// via [`run_ocr_with_engine`]. Fails closed when `enabled=false`.
pub fn run_ocr(
    matter: &Matter,
    job_id: &str,
    params: &OcrParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<OcrOutcome> {
    if !params.enabled {
        return fail_disabled(matter, job_id, params);
    }
    if let Err(e) = params.validate() {
        return fail_preflight(matter, job_id, params, Error::InvalidParams(e));
    }

    // Startup purge of residual page bitmaps (hard-crash leftovers).
    let _purged = purge_ocr_temp_dir(matter.root())?;

    let engine_name = params.engine.to_ascii_lowercase();
    // Production boundary: mock is test-injection only (never via job JSON).
    if engine_name == "mock" || engine_name == engines::MOCK {
        return fail_preflight(
            matter,
            job_id,
            params,
            Error::InvalidParams(
                "engine=mock is not allowed on the production OCR path; use Tesseract CLI".into(),
            ),
        );
    }
    if engine_name != "tesseract" && engine_name != engines::TESSERACT_CLI {
        return fail_preflight(
            matter,
            job_id,
            params,
            Error::InvalidParams(format!(
                "unknown OCR engine '{engine_name}' (production supports only tesseract)"
            )),
        );
    }

    let engine = match TesseractCliEngine::discover(
        params.tesseract_path.as_deref(),
        params.tessdata_dir.as_deref(),
        params.psm,
    ) {
        Ok(e) => e,
        Err(e) => return fail_preflight(matter, job_id, params, e),
    };
    if let Err(e) = engine.preflight_osd() {
        return fail_preflight(matter, job_id, params, e);
    }
    // Version probe must succeed before processing (honest provenance).
    if let Err(e) = engine.version() {
        return fail_preflight(matter, job_id, params, e);
    }
    run_ocr_with_engine(matter, job_id, params, &engine, cancel, progress)
}

/// Run OCR with an injected engine (tests + production after engine resolve).
pub fn run_ocr_with_engine(
    matter: &Matter,
    job_id: &str,
    params: &OcrParams,
    engine: &dyn OcrEngine,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<OcrOutcome> {
    let started = Instant::now();

    if !params.enabled {
        return fail_disabled(matter, job_id, params);
    }
    params.validate().map_err(Error::InvalidParams)?;

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate().map_err(Error::InvalidParams)?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));

    // Require a real version string for audit provenance (no silent "unknown").
    let engine_ver = match engine.version() {
        Ok(v) => v,
        Err(e) => {
            return fail_preflight(matter, job_id, params, e);
        }
    };
    let engine_label = format!("{} {engine_ver}", engine.id());

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ocr.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "enabled": effective.enabled,
            "engine": engine.id(),
            "engine_version": engine_ver,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_inner(
        matter,
        job_id,
        &effective,
        engine,
        &engine_label,
        cancel,
        &progress,
        &params_json,
        prior,
    );

    match &result {
        Ok(OcrOutcome::Succeeded(s)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ocr.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "ocr_count": s.ocr_count,
                    "skipped_count": s.skipped_count,
                    "error_count": s.error_count,
                    "completed_count": s.completed_count,
                    "duration_ms": started.elapsed().as_millis() as u64,
                    "engine": engine.id(),
                    "enabled": true,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(OcrOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: s.clone(),
                });
            }
        }
        Ok(OcrOutcome::Paused(_)) => {}
        Ok(OcrOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ocr.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "completed_count": summary.completed_count,
                    "ocr_count": summary.ocr_count,
                    "enabled": true,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(Error::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "ocr.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": e.to_string(),
                    "enabled": true,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(Error::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn fail_disabled(matter: &Matter, job_id: &str, params: &OcrParams) -> Result<OcrOutcome> {
    let msg = "OCR disabled — enable local OCR in Settings before running";
    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ocr.fail".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "error": msg,
            "enabled": false,
            "params": serde_json::to_value(params).unwrap_or(json!({})),
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    });
    Ok(OcrOutcome::Failed {
        message: msg.into(),
        summary: OcrSummary::default(),
    })
}

/// Fail before item processing with an `ocr.fail` audit (discovery/preflight).
///
/// Returns the original error so callers keep stable `Error::code()` values.
fn fail_preflight(
    matter: &Matter,
    job_id: &str,
    params: &OcrParams,
    err: Error,
) -> Result<OcrOutcome> {
    let msg = err.short_message();
    let code = err.code();
    let _ = matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "ocr.fail".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "error": msg,
            "code": code,
            "enabled": params.enabled,
            "params": serde_json::to_value(params).unwrap_or(json!({})),
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    });
    Err(err)
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, OCR_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(Error::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(call_site: &OcrParams, prior: Option<&CheckpointCursor>) -> Result<OcrParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<OcrParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(Error::Other(format!("checkpoint params unreadable: {e}")));
                }
            }
        }
    }
    Ok(call_site.clone())
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &OcrParams,
    engine: &dyn OcrEngine,
    engine_label: &str,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<OcrOutcome> {
    let mut summary = OcrSummary::default();
    let mut cursor_index = 0u64;
    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        summary.completed_count = p.completed_count;
        summary.ocr_count = p.ocr_count;
        summary.skipped_count = p.skipped_count;
        summary.error_count = p.error_count;
    }

    // Optional PDF renderer (only needed for PDF candidates).
    let pdf_renderer = PdfRenderer::discover(params.pdf_renderer_path.as_deref()).ok();

    let batch = params.batch_size.max(1);
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            write_checkpoint(matter, job_id, cursor_index, &summary, params_json, None)?;
            progress(summary.completed_count);
            return Ok(OcrOutcome::Paused(summary));
        }

        let candidates = matter.list_ocr_candidates(cursor_index, batch as u64, params.force)?;
        if candidates.is_empty() {
            break;
        }

        for cand in candidates {
            if cancel.map(|c| c()).unwrap_or(false) {
                write_checkpoint(
                    matter,
                    job_id,
                    cursor_index,
                    &summary,
                    params_json,
                    Some(&cand.id),
                )?;
                progress(summary.completed_count);
                return Ok(OcrOutcome::Paused(summary));
            }

            match process_one(
                matter,
                &cand,
                params,
                engine,
                engine_label,
                pdf_renderer.as_ref(),
                cancel,
                &mut summary,
            )? {
                ProcessOneResult::Done => {
                    cursor_index += 1;
                    progress(summary.completed_count);
                    write_checkpoint(
                        matter,
                        job_id,
                        cursor_index,
                        &summary,
                        params_json,
                        Some(&cand.id),
                    )?;
                }
                ProcessOneResult::CancelledMidItem => {
                    // Do not advance cursor — resume retries this item.
                    write_checkpoint(
                        matter,
                        job_id,
                        cursor_index,
                        &summary,
                        params_json,
                        Some(&cand.id),
                    )?;
                    progress(summary.completed_count);
                    return Ok(OcrOutcome::Paused(summary));
                }
            }
        }
    }

    Ok(OcrOutcome::Succeeded(summary))
}

enum ProcessOneResult {
    Done,
    CancelledMidItem,
}

fn already_ocr_ok(cand: &OcrCandidate, native_sha: &str, force: bool) -> bool {
    if force {
        return false;
    }
    cand.ocr_source_native_sha256.as_deref() == Some(native_sha)
        && matches!(
            cand.ocr_status.as_deref(),
            Some(ocr_status::OK) | Some(ocr_status::SKIPPED)
        )
}

#[allow(clippy::too_many_arguments)]
fn process_one(
    matter: &Matter,
    cand: &OcrCandidate,
    params: &OcrParams,
    engine: &dyn OcrEngine,
    engine_label: &str,
    pdf_renderer: Option<&PdfRenderer>,
    cancel: Option<&dyn Fn() -> bool>,
    summary: &mut OcrSummary,
) -> Result<ProcessOneResult> {
    let Some(native_sha) = cand.native_sha256.as_deref() else {
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(ProcessOneResult::Done);
    };

    if already_ocr_ok(cand, native_sha, params.force) {
        matter.apply_ocr_text(ApplyOcrTextInput {
            item_id: cand.id.clone(),
            force: false,
            text: None,
            engine: Some(engine_label.into()),
            lang: Some(params.lang.clone()),
            status: Some(ocr_status::SKIPPED.into()),
            error: None,
            source_native_sha256: Some(native_sha.into()),
            page_count: None,
            confidence: None,
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(ProcessOneResult::Done);
    }

    // Redactions present → skip (do not OCR after redaction burn-in).
    if cand.redaction_count > 0 {
        matter.apply_ocr_text(ApplyOcrTextInput {
            item_id: cand.id.clone(),
            force: true,
            text: None,
            engine: Some(engine_label.into()),
            lang: Some(params.lang.clone()),
            status: Some(ocr_status::SKIPPED.into()),
            error: Some(codes::OCR_REDACTIONS.into()),
            source_native_sha256: Some(native_sha.into()),
            page_count: None,
            confidence: None,
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(ProcessOneResult::Done);
    }

    match matter.cas_len(native_sha) {
        Ok(len) => {
            if let Err(e) = reject_oversized_native_len(len) {
                record_error(matter, &cand.id, engine_label, &params.lang, &e)?;
                summary.error_count += 1;
                summary.completed_count += 1;
                return Ok(ProcessOneResult::Done);
            }
        }
        Err(e) => {
            record_error(
                matter,
                &cand.id,
                engine_label,
                &params.lang,
                &Error::Other(format!("CAS stat: {e}")),
            )?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(ProcessOneResult::Done);
        }
    }

    let native_bytes = match matter.get_bytes_capped(native_sha, MAX_NATIVE_INPUT_BYTES) {
        Ok(b) => b,
        Err(e) => {
            let err = {
                let msg = e.to_string();
                if msg.contains("exceeds cap") {
                    Error::limit(msg)
                } else {
                    Error::Other(format!("CAS read: {e}"))
                }
            };
            record_error(matter, &cand.id, engine_label, &params.lang, &err)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(ProcessOneResult::Done);
        }
    };

    let is_image = is_image_meta(
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
        cand.file_category.as_deref(),
    );
    let is_pdf = is_pdf_meta(
        cand.path.as_deref(),
        cand.mime_type.as_deref(),
        cand.file_category.as_deref(),
    ) || looks_like_pdf(&native_bytes);

    let ocr_result = if is_image {
        ocr_image_bytes(matter, engine, &params.lang, &native_bytes)
    } else if is_pdf {
        ocr_pdf_bytes(
            matter,
            engine,
            params,
            pdf_renderer,
            &native_bytes,
            engine_label,
            cancel,
        )
    } else {
        // Unexpected candidate shape — skip.
        matter.apply_ocr_text(ApplyOcrTextInput {
            item_id: cand.id.clone(),
            force: true,
            text: None,
            engine: Some(engine_label.into()),
            lang: Some(params.lang.clone()),
            status: Some(ocr_status::SKIPPED.into()),
            error: Some("ocr_not_image_or_pdf".into()),
            source_native_sha256: Some(native_sha.into()),
            page_count: None,
            confidence: None,
        })?;
        summary.skipped_count += 1;
        summary.completed_count += 1;
        return Ok(ProcessOneResult::Done);
    };

    match ocr_result {
        Ok((text, page_count, confidence)) => {
            if text.trim().is_empty() {
                record_error(
                    matter,
                    &cand.id,
                    engine_label,
                    &params.lang,
                    &Error::Engine(codes::OCR_EMPTY_TEXT.into()),
                )?;
                summary.error_count += 1;
                summary.completed_count += 1;
                return Ok(ProcessOneResult::Done);
            }
            let apply = matter.apply_ocr_text(ApplyOcrTextInput {
                item_id: cand.id.clone(),
                force: params.force,
                text: Some(text),
                engine: Some(engine_label.into()),
                lang: Some(params.lang.clone()),
                status: Some(ocr_status::OK.into()),
                error: None,
                source_native_sha256: Some(native_sha.into()),
                page_count: Some(page_count),
                confidence,
            })?;
            match apply {
                OcrApplyResult::Skipped => summary.skipped_count += 1,
                OcrApplyResult::Applied { .. } => summary.ocr_count += 1,
                OcrApplyResult::Error { .. } => summary.error_count += 1,
            }
            summary.completed_count += 1;
            Ok(ProcessOneResult::Done)
        }
        Err(e) if e.to_string().contains("ocr_cancelled_mid_document") => {
            // No item mutation / no partial success apply.
            Ok(ProcessOneResult::CancelledMidItem)
        }
        Err(e) => {
            // PDF renderer missing / unexpected mid-doc failure: leave pdf_needs_ocr=1.
            record_error(matter, &cand.id, engine_label, &params.lang, &e)?;
            summary.error_count += 1;
            summary.completed_count += 1;
            Ok(ProcessOneResult::Done)
        }
    }
}

fn ocr_image_bytes(
    matter: &Matter,
    engine: &dyn OcrEngine,
    lang: &str,
    bytes: &[u8],
) -> Result<(String, i64, Option<f64>)> {
    let mut temp = OcrTempFile::new_in(matter.root(), ".img")?;
    temp.write_all(bytes)?;
    let page = engine.ocr_image(temp.path(), lang)?;
    // Drop temp before return (scope end).
    drop(temp);
    Ok((truncate_ocr_text(page.text), 1, page.confidence))
}

/// Truncate at a UTF-8 char boundary (never panic on multibyte OCR output).
pub fn truncate_ocr_text(mut text: String) -> String {
    if text.len() <= MAX_OCR_TEXT_BYTES {
        return text;
    }
    let mut end = MAX_OCR_TEXT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(TRUNCATION_MARKER);
    text
}

/// True when a mid-document render error likely means past last page (EOF).
fn is_likely_pdf_eof_error(err: &Error) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "wrong page",
        "page not found",
        "page range",
        "no pages",
        "out of range",
        "invalid page",
        "nothing to do",
        "unknown page",
        "page number",
        "beyond last",
        "does not exist",
    ];
    MARKERS.iter().any(|m| msg.contains(m))
}

fn ocr_pdf_bytes(
    matter: &Matter,
    engine: &dyn OcrEngine,
    params: &OcrParams,
    pdf_renderer: Option<&PdfRenderer>,
    bytes: &[u8],
    _engine_label: &str,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<(String, i64, Option<f64>)> {
    let Some(renderer) = pdf_renderer else {
        // Mock CI path: no system pdftoppm/mutool required.
        if engine.id() == engines::MOCK {
            let mut img = OcrTempFile::new_in(matter.root(), ".png")?;
            img.write_all(&minimal_png_bytes())?;
            let r = engine.ocr_image(img.path(), &params.lang)?;
            drop(img);
            return Ok((truncate_ocr_text(r.text), 1, r.confidence));
        }
        return Err(Error::PdfRendererMissing(
            "PDF OCR requires pdftoppm or mutool (set Settings path)".into(),
        ));
    };

    // Materialize PDF once under Drop-guarded temp.
    let mut pdf_temp = OcrTempFile::new_in(matter.root(), ".pdf")?;
    pdf_temp.write_all(bytes)?;
    let pdf_path = pdf_temp.path().to_owned();

    let max_pages = params.max_pages.min(MAX_PAGES);
    let mut combined = String::new();
    let mut conf_sum = 0.0f64;
    let mut conf_n = 0u32;
    let mut pages_done = 0i64;

    for page in 1u32..=(max_pages as u32) {
        // Cancel between pages (spec §3.8) — do not apply partial as success.
        if cancel.map(|c| c()).unwrap_or(false) {
            drop(pdf_temp);
            return Err(Error::Other("ocr_cancelled_mid_document".into()));
        }

        let page_img = match renderer.render_page(matter.root(), &pdf_path, page, params.dpi) {
            Ok(t) => t,
            Err(e) => {
                if page == 1 {
                    if engine.id() == engines::MOCK {
                        let mut img = OcrTempFile::new_in(matter.root(), ".png")?;
                        img.write_all(&minimal_png_bytes())?;
                        let r = engine.ocr_image(img.path(), &params.lang)?;
                        drop(img);
                        return Ok((truncate_ocr_text(r.text), 1, r.confidence));
                    }
                    return Err(e);
                }
                // Only treat *likely EOF* as end-of-document. Unexpected
                // renderer failures fail the item so we never clear
                // pdf_needs_ocr after partial garbage success.
                if is_likely_pdf_eof_error(&e) {
                    break;
                }
                return Err(e);
            }
        };
        let r = engine.ocr_image(page_img.path(), &params.lang)?;
        drop(page_img); // Drop before next page.

        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&r.text);
        pages_done += 1;
        if let Some(c) = r.confidence {
            conf_sum += c;
            conf_n += 1;
        }
        if combined.len() > MAX_OCR_TEXT_BYTES {
            combined = truncate_ocr_text(combined);
            break;
        }
    }

    drop(pdf_temp);

    if pages_done == 0 {
        if engine.id() == engines::MOCK {
            let mut img = OcrTempFile::new_in(matter.root(), ".png")?;
            img.write_all(&minimal_png_bytes())?;
            let r = engine.ocr_image(img.path(), &params.lang)?;
            drop(img);
            return Ok((truncate_ocr_text(r.text), 1, r.confidence));
        }
        return Err(Error::PdfRendererMissing(
            "PDF OCR produced zero pages".into(),
        ));
    }

    let conf = if conf_n > 0 {
        Some(conf_sum / f64::from(conf_n))
    } else {
        None
    };
    Ok((combined, pages_done, conf))
}

/// Tiny valid 1×1 PNG (synthetic fixture bytes).
pub fn minimal_png_bytes() -> Vec<u8> {
    // 1x1 transparent PNG
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn record_error(
    matter: &Matter,
    item_id: &str,
    engine_label: &str,
    lang: &str,
    err: &Error,
) -> Result<()> {
    matter.apply_ocr_text(ApplyOcrTextInput {
        item_id: item_id.into(),
        force: true,
        text: None,
        engine: Some(engine_label.into()),
        lang: Some(lang.into()),
        status: Some(ocr_status::ERROR.into()),
        error: Some(format!("{}: {}", err.code(), err.short_message())),
        source_native_sha256: None,
        page_count: None,
        confidence: None,
    })?;
    matter
        .record_item_error(matter_core::ItemErrorInput {
            item_id: Some(item_id.into()),
            source_id: None,
            job_id: None,
            stage: OCR_STAGE.into(),
            code: err.code().into(),
            message: err.short_message(),
            detail: None,
        })
        .map_err(|e| Error::Other(format!("record_item_error failed: {e}")))?;
    Ok(())
}

fn write_checkpoint(
    matter: &Matter,
    job_id: &str,
    cursor_index: u64,
    summary: &OcrSummary,
    params_json: &serde_json::Value,
    last_item_id: Option<&str>,
) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index,
        last_item_id: last_item_id.map(|s| s.to_string()),
        // Full mid-page resume of partial text is residual; field present so
        // checkpoints carry page bookkeeping for operators / future work.
        next_page: 0,
        partial_text: String::new(),
        completed_count: summary.completed_count,
        ocr_count: summary.ocr_count,
        skipped_count: summary.skipped_count,
        error_count: summary.error_count,
        params: params_json.clone(),
    };
    let cursor_json = serde_json::to_string(&cursor).unwrap_or_else(|_| "{}".into());
    matter.put_checkpoint(
        job_id,
        OCR_STAGE,
        &cursor_json,
        summary.completed_count as i64,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_oversized_native_len_unit() {
        assert!(reject_oversized_native_len_with_max(10, 10).is_ok());
        let err = reject_oversized_native_len_with_max(11, 10).unwrap_err();
        assert_eq!(err.code(), "ocr_limit_exceeded");
    }

    #[test]
    fn already_ocr_ok_respects_force() {
        let cand = OcrCandidate {
            id: "i1".into(),
            path: Some("a.png".into()),
            mime_type: None,
            native_sha256: Some("abc".into()),
            text_sha256: Some("txt".into()),
            ocr_source_native_sha256: Some("abc".into()),
            ocr_status: Some("ok".into()),
            pdf_needs_ocr: 0,
            file_category: None,
            redaction_count: 0,
        };
        assert!(already_ocr_ok(&cand, "abc", false));
        assert!(!already_ocr_ok(&cand, "abc", true));
        assert!(!already_ocr_ok(&cand, "other", false));
    }

    #[test]
    fn minimal_png_has_magic() {
        let b = minimal_png_bytes();
        assert!(crate::detect::looks_like_png(&b));
    }
}
