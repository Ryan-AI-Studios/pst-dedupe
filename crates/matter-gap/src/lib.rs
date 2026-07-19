//! # matter-gap
//!
//! **Gap analysis** engine (track **0042**):
//!
//! 1. **Expected roster** — import/list expected custodians; flag missing (warn)
//! 2. **Date coverage** — optional window empty (error) + week/month holes (warn)
//! 3. **Opposing DAT** — parse 0040 Concordance DAT / CSV into expected docs
//! 4. **Set compare** — Message-ID / item / logical for email → SHA-256 → control
//! 5. **Report pack** under `exports/gap/`
//!
//! ## Job
//!
//! Kind [`JOB_KIND_GAP`] (`"gap"`). Option C: no `create_job` inside the engine.
//!
//! ## Privacy
//!
//! Subjects are not stored on `gap_expected_docs` and are omitted from reports by default.

#![forbid(unsafe_code)]

pub mod column_map;
pub mod compare;
pub mod dat_parse;
pub mod date_coverage;
pub mod error;
pub mod params;
pub mod report;
pub mod roster;
pub mod run;

pub use column_map::{DatColumnMap, MappedField};
pub use compare::{
    compare_import, is_email_like, match_expected_to_matter,
    match_expected_to_matter_with_mid_index, CompareHit, CompareResult, MatchKey,
};
pub use dat_parse::{
    check_bytes_size, check_file_size, decode_dat_field, enforce_caps, parse_dat_bytes,
    parse_dat_file, DatCaps, DatFormat, ParsedDat,
};
pub use date_coverage::{
    allowed_buckets, analyze_date_coverage, DateBucketRow, DateFinding, GapSeverity,
    FINDING_DATE_BUCKET_HOLE, FINDING_DATE_WINDOW_EMPTY,
};
pub use error::{GapError, Result};
pub use params::{
    CollectionGapParams, GapParams, OpposingGapParams, BUCKET_MONTH, BUCKET_WEEK,
    DEFAULT_MAX_DAT_BYTES, DEFAULT_MAX_DAT_ROWS, KIND_BOTH, KIND_COLLECTION, KIND_OPPOSING,
    SCOPE_INVENTORY, SCOPE_IN_REVIEW, SCOPE_PRODUCTION_SET,
};
pub use report::{count_severities, default_gap_report_dir, write_gap_report, GapReportMeta};
pub use roster::{
    analyze_collection_roster, run_roster_analysis, CollectionGapAnalysis, RosterFinding,
    FINDING_MISSING_CUSTODIAN, FINDING_UNEXPECTED_CUSTODIAN,
};
pub use run::{
    import_opposing_dat, import_roster_csv, run_collection_gap, run_gap, run_opposing_gap,
    GapOutcome, GapReport, GapSummary, GAP_STAGE, JOB_KIND_GAP,
};
