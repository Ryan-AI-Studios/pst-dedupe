//! Safety limits for ICS extraction (spec §3.5).

/// Max native input size (50 MiB for containers).
pub const MAX_NATIVE_INPUT_BYTES: u64 = 50 * 1024 * 1024;

/// Max VEVENTs processed from one container.
pub const MAX_VEVENTS: usize = 10_000;

/// Max extracted plain-text output per event (2 MiB).
pub const MAX_EXTRACTED_TEXT_BYTES: usize = 2 * 1024 * 1024;

/// Marker appended when text is truncated at the output cap.
pub const TRUNCATION_MARKER: &str = "\n[… truncated …]\n";

/// Method ids recorded on items.
pub mod methods {
    /// Primary stack: icalendar 0.17 + chrono-tz.
    pub const ICS_ICALENDAR_V1: &str = "ics_icalendar_v1";
}

/// ICS extract status values (`ics_extract_status`).
pub mod status {
    pub const OK: &str = "ok";
    pub const ERROR: &str = "error";
    pub const SKIPPED: &str = "skipped";
}
