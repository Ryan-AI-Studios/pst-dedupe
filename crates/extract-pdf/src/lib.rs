//! # extract-pdf
//!
//! Pure-Rust **PDF embedded-text extraction** for Dedupe Desk (track **0034**):
//!
//! | Role | Stack |
//! |---|---|
//! | Text operators | **pdf-extract 0.12.0** (`output_doc_page` / `PlainTextOutput`, page-by-page) |
//! | Structure / encrypt / pages | **lopdf** (transitive via pdf-extract, 0.42.x) as `pdf_extract::Document` |
//!
//! Method id: [`methods::PDF_EXTRACT_V1`] (`pdf_extract_v1`).
//!
//! ## ⚠️ BLOCKING THREAD WARNING
//!
//! [`extract_pdf`], [`run_pdf_extract`] are **CPU- and IO-bound**. Callers
//! **must** run them on a dedicated blocking worker (`process-runner` matter
//! worker). Never call on the GUI or Tokio async worker.
//!
//! ## Safety
//!
//! - Native size precheck (`MAX_NATIVE_INPUT_BYTES` = 100 MiB)
//! - Page cap (`MAX_PAGES` = 500) and text cap (`MAX_EXTRACTED_TEXT_BYTES` = 10 MiB)
//! - Encrypted PDFs fail closed (`pdf_encrypted`)
//! - `catch_unwind` at item boundary via [`extract_pdf_catch_unwind`]
//!
//! ## Empty / low-text / needs OCR
//!
//! | Condition | status | `pdf_needs_ocr` |
//! |---|---|---|
//! | Zero non-whitespace chars | `empty` | 1 |
//! | Below total/page thresholds | `low_text` | 1 (text CAS still written) |
//! | Above thresholds | `ok` | 0 |
//!
//! ## Out of scope (P0)
//!
//! Page rasterization / preview CAS, PDFium/MuPDF, OCR (**0036**), geometric burn-in.

#![forbid(unsafe_code)]

pub mod detect;
pub mod error;
pub mod extract;
pub mod limits;
pub mod params;
pub mod run;
pub mod text_buf;

pub use detect::{detect_pdf, is_pdf_eligible_meta, looks_like_pdf};
pub use error::{Error, Result};
pub use extract::{
    classify_text, count_non_ws_chars, extract_pdf, extract_pdf_catch_unwind,
    extract_pdf_with_limits, ExtractedPdfText, TextClass,
};
pub use limits::{
    methods, status, MAX_EXTRACTED_TEXT_BYTES, MAX_NATIVE_INPUT_BYTES, MAX_PAGES,
    MIN_TEXT_CHARS_PER_PAGE, MIN_TEXT_CHARS_TOTAL, TRUNCATION_MARKER,
};
pub use params::PdfExtractParams;
pub use run::{
    reject_oversized_native_len, reject_oversized_native_len_with_max, run_pdf_extract,
    PdfExtractOutcome, PdfExtractSummary, JOB_KIND_PDF_EXTRACT, PDF_EXTRACT_STAGE,
};
