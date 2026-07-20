//! # matter-entity
//!
//! Offline **entity / PII detection** via versioned regex packs (track **0046**):
//!
//! - Built-in packs: `email`, `phone_us`, `ssn_us`, `credit_card`, `currency_usd`
//! - Post-validation: **Luhn** (cards), light SSN invalid rules
//! - Storage: **mask + match_hash only** — never cleartext PAN/SSN
//! - Job: resumable [`run_entity_scan`] with digest-aware skip/rescan
//!
//! ## Honesty
//!
//! Regex + Luhn ≠ guaranteed PII presence. Tracking numbers and random digits may
//! still match before validation. Not a substitute for privilege / production QC.
//! Not PHI-specialized.
//!
//! ## Engine safety
//!
//! Uses the Rust **`regex` crate only** (finite automata — linear-time matching in
//! practice; no catastrophic backtracking / ReDoS). Do not replace with PCRE or
//! backtracking engines for pack matching.
//!
//! ## Privacy
//!
//! Email domain remains **fully visible** in `masked_value` (investigation filter).
//! Cards/SSNs store last-4 style masks. Stable `match_hash` uses normalized forms.
//!
//! ## Idempotency
//!
//! `reset: false` skips only when `entity_scanned_text_sha256` equals the
//! current **full-success** scan fingerprint (packs+versions, body digest,
//! `trunc=full`, subject/from content hashes). Incomplete CAS loads
//! (`body=err:…`) and truncated scans (`trunc=N`) never match → retry.

#![forbid(unsafe_code)]

pub mod error;
pub mod luhn;
pub mod mask;
pub mod packs;
pub mod params;
pub mod run;
pub mod scan;

pub use error::{EntityError, Result};
pub use luhn::luhn_valid;
pub use mask::{
    mask_card, mask_email, mask_phone, mask_ssn, match_hash, normalize_email, subject_scan_marker,
};
pub use packs::{default_pack_ids, is_known_pack, pack_version};
pub use params::{EntityScanParams, SCOPE_ALL};
pub use run::{
    build_scan_fingerprint, full_success_fingerprint, run_entity_scan, BodyScanOutcome,
    EntityScanOutcome, EntityScanReport, EntityScanSummary, ENTITY_SCAN_STAGE, ESCAN_FP_VERSION,
    JOB_KIND_ENTITY_SCAN,
};
pub use scan::{flags_from_hits, safe_byte_slice, scan_text, RawHit};
