//! # extract-calendar
//!
//! Pure-Rust **iCalendar (ICS) extraction** for Dedupe Desk (track **0035**):
//!
//! | Role | Stack |
//! |---|---|
//! | ICS parse | **icalendar 0.17.x** (`parser` feature) |
//! | TZID → offset | **chrono-tz** / IANA (via icalendar `chrono-tz` feature) |
//!
//! Method id: [`methods::ICS_ICALENDAR_V1`] (`ics_icalendar_v1`).
//!
//! ## ⚠️ BLOCKING THREAD WARNING
//!
//! [`parse_ics`], [`run_ics_extract`] are **CPU- and IO-bound**. Callers
//! **must** run them on a dedicated blocking worker (`process-runner` matter
//! worker). Never call on the GUI or Tokio async worker.
//!
//! ## Container model (multi-event ICS)
//!
//! A multi-event `.ics` is a **container** (`file_category=archive` parent).
//! Each VEVENT becomes a child with an **isolated single-event** native in CAS.
//! Child `native_sha256` is **never** the mega-file digest (0040 produce safety).
//!
//! ## Out of scope (P0)
//!
//! RRULE expansion, full VTIMEZONE rewrite, PidLid (PST path), free/busy UI.

#![forbid(unsafe_code)]

pub mod detect;
pub mod error;
pub mod extract;
pub mod limits;
pub mod params;
pub mod run;
pub mod text;

pub use detect::{detect_ics, is_ics_eligible_meta, looks_like_ics};
pub use error::{Error, Result};
pub use extract::{
    build_single_event_ics, count_vevents_in_ics, extract_ics_catch_unwind, parse_ics,
    parse_ics_with_limits, CalendarEventFields, ParsedIcs, ParsedVEvent,
};
pub use limits::{
    methods, status, MAX_EXTRACTED_TEXT_BYTES, MAX_NATIVE_INPUT_BYTES, MAX_VEVENTS,
    TRUNCATION_MARKER,
};
pub use params::IcsExtractParams;
pub use run::{
    reject_oversized_native_len, reject_oversized_native_len_with_max, run_ics_extract,
    IcsExtractOutcome, IcsExtractSummary, ICS_EXTRACT_STAGE, JOB_KIND_ICS_EXTRACT,
};
pub use text::synthesize_calendar_review_text;
