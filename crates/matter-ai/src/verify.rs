//! Citation grounding verify (track 0052).
//!
//! Implementation lives in [`matter_core::ai_verify`] so Desk and accept-path
//! can re-verify without a job-crate dependency. This module re-exports the
//! pure API for existing `matter_ai::verify_*` call sites.

pub use matter_core::{
    normalize_for_verify, verify_ai_citation_against_text, verify_citation_for_storage,
    VerifyCitationResult,
};
