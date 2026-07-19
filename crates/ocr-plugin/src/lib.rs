//! # ocr-plugin
//!
//! Opt-in **local OCR** for Dedupe Desk (track **0036**):
//!
//! | Role | Stack |
//! |---|---|
//! | Primary engine | **Tesseract CLI** sidecar (`--psm 1` OSD default) |
//! | Tests / CI | [`MockOcrEngine`] (no system Tesseract) |
//! | PDF pages | Optional **pdftoppm** / **mutool** page-at-a-time render |
//!
//! ## ⚠️ BLOCKING THREAD WARNING
//!
//! [`run_ocr`], [`run_ocr_with_engine`] are **CPU- and IO-bound**. Callers
//! **must** run them on a dedicated blocking worker (`process-runner` matter
//! worker). Never call on the GUI or Tokio async worker.
//!
//! ## Enable gate
//!
//! Default **OFF**. Job fails closed when `params.enabled` is false — no item
//! mutation. Desk passes enable flag + tool paths in job params JSON.
//!
//! ## Safety
//!
//! - Page-at-a-time; **Drop-guarded** temps under `workspace/temp/ocr/`
//! - Startup **purge** of residual OCR temps
//! - Native size / page / text caps
//! - Skip when `redaction_count > 0`
//! - No cloud OCR; proxy env scrubbed on child processes

#![forbid(unsafe_code)]

pub mod detect;
pub mod engine;
pub mod error;
pub mod limits;
pub mod params;
pub mod render;
pub mod run;
pub mod temp;

pub use detect::{is_image_meta, is_pdf_meta, looks_like_pdf, looks_like_png, IMAGE_EXTS};
pub use engine::{
    default_ocr_argv, MockOcrEngine, OcrEngine, OcrPageResult, TesseractCliEngine, DEFAULT_PSM,
};
pub use error::{Error, Result};
pub use limits::{
    engines, status, DEFAULT_DPI, MAX_NATIVE_INPUT_BYTES, MAX_OCR_TEXT_BYTES, MAX_PAGES,
    OCR_TEMP_SUBDIR, TRUNCATION_MARKER,
};
pub use params::OcrParams;
pub use render::{PdfRenderer, PdfRendererKind};
pub use run::{
    minimal_png_bytes, reject_oversized_native_len, reject_oversized_native_len_with_max, run_ocr,
    run_ocr_with_engine, truncate_ocr_text, OcrOutcome, OcrSummary, JOB_KIND_OCR, OCR_STAGE,
};
pub use temp::{ensure_ocr_temp_dir, ocr_temp_dir, purge_ocr_temp_dir, OcrTempFile};
