//! Safety limits for OOXML extraction (spec §3.5).

/// Max native input size (100 MiB).
pub const MAX_NATIVE_INPUT_BYTES: u64 = 100 * 1024 * 1024;

/// Hard cap on every zip entry inflate (50 MiB) — enforced via `Read::take`.
pub const MAX_UNCOMPRESSED_ENTRY_BYTES: u64 = 50 * 1024 * 1024;

/// Max inflate ratio when compressed size is known (≈100:1).
pub const MAX_INFLATE_RATIO: u64 = 100;

/// Max zip central-directory entries.
pub const MAX_ZIP_ENTRIES: usize = 10_000;

/// Max extracted plain-text output (10 MiB).
pub const MAX_EXTRACTED_TEXT_BYTES: usize = 10 * 1024 * 1024;

/// Max sheets (XLSX) or slides (PPTX) to visit.
pub const MAX_SHEETS_OR_SLIDES: usize = 500;

/// Marker appended when text is truncated at the output cap.
pub const TRUNCATION_MARKER: &str = "\n[… truncated …]\n";

/// Method ids recorded on items.
pub mod methods {
    pub const DOCX_XML_V1: &str = "docx_xml_v1";
    pub const CALAMINE_XLSX_V1: &str = "calamine_xlsx_v1";
    pub const PPTX_XML_V1: &str = "pptx_xml_v1";
}

/// Office extract status values.
pub mod status {
    pub const OK: &str = "ok";
    pub const SKIPPED: &str = "skipped";
    pub const ERROR: &str = "error";
}
