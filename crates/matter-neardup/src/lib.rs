//! # matter-neardup
//!
//! Matter-level **near-duplicate detection** over Normalized Item body text
//! (track **0023**):
//!
//! 1. **Prep** — lowercase, collapse whitespace
//! 2. **Mixed-script tokenize** — CJK character n-grams + Latin word tokens
//! 3. **Shingles** — unique set for Jaccard
//! 4. **MinHash** — Approach A seeded SplitMix64 stream (`minhash_shingle_v1`)
//! 5. **Banded LSH** — candidate pairs
//! 6. **Union-find + pivot** — re-score vs pivot; demote weak members
//!
//! ## Identity rules
//!
//! - Never delete items or CAS blobs.
//! - Never treat near-dups as exact suppress (`dedup_*` is separate).
//! - Never use Kirsch–Mitzenmacher `h1+i*h2` MinHash expansion.
//! - Never load full Item JSON bodies for candidates.
//!
//! ## Memory
//!
//! Stream CAS text → sketch → drop text. Hold only
//! `(item_id, token_count, MinHashSig)` for the eligible set. Signature spill
//! for multi-million matters is deferred.
//!
//! ## Transactions
//!
//! Each batch of near_dup field updates + `put_checkpoint` commits in **one**
//! SQLite transaction via [`matter_core::Matter::apply_near_dup_batch_with_checkpoint`].

#![forbid(unsafe_code)]

pub mod cluster;
pub mod error;
pub mod lsh;
pub mod minhash;
pub mod params;
pub mod run;
pub mod shingle;
pub mod tokenize;

pub use cluster::{near_group_id, ClusterAssignment, ItemMeta, UnionFind};
pub use error::{NearDupError, Result};
pub use minhash::{expand_shingle_hashes, minhash_signature, MinHashSig, SplitMix64};
pub use params::{NearDupParams, DEFAULT_HASH_SEED, NEAR_DUP_METHOD};
pub use run::{run_neardup, NearDupOutcome, NearDupSummary, JOB_KIND_NEARDUP, NEARDUP_STAGE};
pub use tokenize::{build_shingles, is_cjk_char, prep_text, text_to_shingles, tokenize};
