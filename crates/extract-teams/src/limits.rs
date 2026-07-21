//! Caps and method ids for teams extract.

/// Method / parser ids written to `teams_extract_method`.
pub mod methods {
    pub const HTML_FIXTURE_V1: &str = "html_fixture_v1";
    pub const JSON_BEST_EFFORT_V1: &str = "json_best_effort_v1";
    pub const PST_ENRICH_V1: &str = "pst_enrich_v1";
}

/// Status values (re-export stable strings from matter-core usage).
pub mod status {
    pub const OK: &str = "ok";
    pub const SKIPPED: &str = "skipped";
    pub const ERROR: &str = "error";
}

/// Default max HTML/JSON native bytes loaded from CAS (20 MiB).
pub const DEFAULT_MAX_HTML_BYTES: u64 = 20_000_000;
/// Default max messages per export file.
pub const DEFAULT_MAX_MESSAGES_PER_FILE: usize = 50_000;
/// Soft cap on plain-text review body size after sanitize + inject.
pub const MAX_EXTRACTED_TEXT_BYTES: usize = 2_000_000;
/// Marker appended when body is truncated to [`MAX_EXTRACTED_TEXT_BYTES`].
pub const TRUNCATION_MARKER: &str = "\n[truncated]";
