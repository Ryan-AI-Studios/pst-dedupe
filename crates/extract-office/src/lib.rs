//! # extract-office
//!
//! Pure-Rust **OOXML text extraction** for Dedupe Desk (track **0033**):
//!
//! | Format | Method | Approach |
//! |---|---|---|
//! | DOCX | `docx_xml_v1` | zip + quick-xml (`word/document.xml` `w:t`) |
//! | XLSX | `calamine_xlsx_v1` | calamine with **running text length** + early break |
//! | PPTX | `pptx_xml_v1` | zip + quick-xml (`ppt/slides/slide*.xml` `a:t`) |
//!
//! ## ⚠️ BLOCKING THREAD WARNING
//!
//! [`extract_office`], [`run_office_extract`] are **CPU- and IO-bound**. Callers
//! **must** run them on a dedicated blocking worker (`process-runner` matter
//! worker). Never call on the GUI or Tokio async worker.
//!
//! ## Safety (normative)
//!
//! - Every zip entry is read with **`Read::take(MAX_UNCOMPRESSED_ENTRY_BYTES)`**
//!   — never unbounded `read_to_end` on an entry stream.
//! - Native input size, inflate ratio, entry count, text cap, sheet/slide caps.
//! - Path-safe entry names (reject `..`, absolute).
//! - `catch_unwind` at item boundary via [`extract_office_catch_unwind`].
//!
//! ## Job
//!
//! Kind [`JOB_KIND_OFFICE_EXTRACT`] (`office_extract`) fills `text_sha256` from
//! CAS natives and records `office_*` bookkeeping (schema v14).
//!
//! ## Out of scope
//!
//! PDF, LibreOffice, legacy OLE recovery, password recovery, macro execution,
//! native Office redaction, WYSIWYG preview.

#![forbid(unsafe_code)]

pub mod detect;
pub mod docx;
pub mod error;
pub mod extract;
pub mod limits;
pub mod params;
pub mod pptx;
pub mod run;
pub mod text_buf;
pub mod xlsx;
pub mod zip_safe;

pub use detect::{detect_format, is_office_eligible_meta, OfficeFormat};
pub use error::{Error, Result};
pub use extract::{extract_format, extract_office, extract_office_catch_unwind};
pub use limits::{
    methods, status, MAX_EXTRACTED_TEXT_BYTES, MAX_INFLATE_RATIO, MAX_NATIVE_INPUT_BYTES,
    MAX_SHEETS_OR_SLIDES, MAX_UNCOMPRESSED_ENTRY_BYTES, MAX_ZIP_ENTRIES, TRUNCATION_MARKER,
};
pub use params::OfficeExtractParams;
pub use run::{
    run_office_extract, OfficeExtractOutcome, OfficeExtractSummary, JOB_KIND_OFFICE_EXTRACT,
    OFFICE_EXTRACT_STAGE,
};

/// Successful extract payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedText {
    pub text: String,
    pub method: String,
    /// True when output hit the text cap or sheet/slide cap.
    pub partial: bool,
    pub format: OfficeFormat,
}
