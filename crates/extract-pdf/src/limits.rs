//! Safety limits for PDF text extraction (spec §3.5 / task pins).

/// Max native input size (100 MiB).
pub const MAX_NATIVE_INPUT_BYTES: u64 = 100 * 1024 * 1024;

/// Max pages processed (partial beyond this).
pub const MAX_PAGES: usize = 500;

/// Max extracted plain-text output (10 MiB).
pub const MAX_EXTRACTED_TEXT_BYTES: usize = 10 * 1024 * 1024;

/// Minimum non-whitespace Unicode scalar chars for status `ok` (not low_text).
pub const MIN_TEXT_CHARS_TOTAL: usize = 50;

/// Minimum average non-whitespace chars per page for status `ok`.
pub const MIN_TEXT_CHARS_PER_PAGE: usize = 20;

/// Marker appended when text is truncated at the output cap.
pub const TRUNCATION_MARKER: &str = "\n[… truncated …]\n";

/// Method ids recorded on items.
pub mod methods {
    /// Primary stack: pdf-extract 0.12 (lopdf 0.42.x transitive) text operators.
    pub const PDF_EXTRACT_V1: &str = "pdf_extract_v1";
}

/// PDF extract status values (`pdf_extract_status`).
pub mod status {
    pub const OK: &str = "ok";
    pub const LOW_TEXT: &str = "low_text";
    pub const EMPTY: &str = "empty";
    pub const ERROR: &str = "error";
    pub const SKIPPED: &str = "skipped";
}
