//! # matter-produce
//!
//! Matter-level **production export** (track **0040**):
//!
//! 1. Select items (`in_review` corpus or explicit ids)
//! 2. **Withhold fail-closed** — never write natives/text/DAT for withheld
//! 3. Assign stable control numbers (`{PREFIX}{seq}`)
//! 4. Stream-copy CAS natives (or export-only EML) + redacted/extracted text
//! 5. Write Concordance **DAT** (UTF-8 BOM, þ/¶, ® newlines, UTC dates) + CSV twin
//!
//! ## Contracts
//!
//! - Withheld items never appear in the volume (skip or `fail_if_withheld` abort)
//! - `redaction_count > 0` → text **only** from `redacted_text_sha256` (never original)
//! - Privilege descriptions and notes are **never** load-file fields
//! - EML is packaging only — not `native_sha256` identity
//! - `expand_family=false` default; broken-family QC owned by **0041**
//! - `require_qc_pass=true` default — refuses produce without a fresh passed QC run
//!
//! ## Job
//!
//! Kind [`JOB_KIND_PRODUCE`] (`"produce"`; alias `"production_export"`).
//! Resumable via checkpoint stage [`PRODUCE_STAGE`]. Option C: no `create_job`.

#![forbid(unsafe_code)]

pub mod dat;
pub mod error;
pub mod layout;
pub mod params;
pub mod resolve;
pub mod run;

pub use dat::{
    encode_dat_field, format_utc_datetime, write_load_csv, write_load_dat, LoadRow, DAT_FIELDS,
    DAT_NEWLINE, DAT_QUALIFIER, DAT_SEPARATOR, UTF8_BOM,
};
pub use error::{ProduceError, Result};
pub use layout::{
    production_stamp, resolve_output_root, volume_has_production_content, VolumeLayout, DATA_DIR,
    NATIVES_DIR, PRODUCTIONS_DIR, TEXT_DIR,
};
pub use params::{ProduceParams, DEFAULT_BATES_PREFIX, SCOPE_ITEM_IDS, SCOPE_REVIEW_CORPUS};
pub use run::{
    run_produce, ProduceOutcome, ProduceSummary, JOB_KIND_PRODUCE, JOB_KIND_PRODUCTION_EXPORT,
    PRODUCE_STAGE,
};
