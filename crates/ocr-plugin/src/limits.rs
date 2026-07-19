//! Safety limits for OCR (spec §3.9).

/// Max native input size (100 MiB).
pub const MAX_NATIVE_INPUT_BYTES: u64 = 100 * 1024 * 1024;

/// Max pages processed (partial beyond this).
pub const MAX_PAGES: usize = 500;

/// Max OCR plain-text output (10 MiB).
pub const MAX_OCR_TEXT_BYTES: usize = 10 * 1024 * 1024;

/// Default render DPI for PDF pages.
pub const DEFAULT_DPI: u32 = 200;

/// Soft max dimension (px) for rendered page images.
pub const MAX_PAGE_DIMENSION_PX: u32 = 4000;

/// Marker appended when text is truncated at the output cap.
pub const TRUNCATION_MARKER: &str = "\n[… truncated …]\n";

/// Subdirectory under `workspace/temp/` for OCR page bitmaps.
pub const OCR_TEMP_SUBDIR: &str = "ocr";

/// `ocr_status` values.
pub mod status {
    pub const OK: &str = "ok";
    pub const ERROR: &str = "error";
    pub const SKIPPED: &str = "skipped";
    pub const DISABLED: &str = "disabled";
}

/// Engine ids.
pub mod engines {
    pub const TESSERACT_CLI: &str = "tesseract_cli";
    pub const MOCK: &str = "mock";
}
