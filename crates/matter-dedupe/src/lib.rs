//! # matter-dedupe
//!
//! Matter-level **tiered deduplication** over Normalized Items (track **0021**):
//!
//! 1. **Tier 1** — normalized Message-ID (when present)
//! 2. **Tier 2** — desk `logical_hash` v1 (when MID missing)
//! 3. **Family** — mark attachment children when parent is duplicate
//!
//! ## Identity rules
//!
//! - Desk suppress keys are **MID + logical_hash only**.
//! - Never use CLI `dedup-engine` preview content-hash.
//! - Never use parent email item id as attachment `duplicate_of`.
//! - Never delete items or CAS blobs.
//!
//! ## Memory
//!
//! Canonical maps use **fixed-size `[u8; 32]` keys** (see [`keys`]). Parents are
//! streamed as thin [`matter_core::DedupeCandidate`] rows — not full `Item`
//! bodies with text.
//!
//! ## Transactions
//!
//! Each batch of role updates + `put_checkpoint` commits in **one** SQLite
//! transaction via [`matter_core::Matter::apply_dedup_batch_with_checkpoint`].

#![forbid(unsafe_code)]

pub mod error;
pub mod keys;
pub mod params;
pub mod policy;
pub mod run;

pub use error::{DedupeError, Result};
pub use keys::{logical_hash_key, message_id_key, CompactKey};
pub use params::DedupeParams;
pub use policy::FamilyPolicy;
pub use run::{run_dedupe, DedupeOutcome, DedupeSummary, DEDUPE_STAGE, JOB_KIND_DEDUPE};
