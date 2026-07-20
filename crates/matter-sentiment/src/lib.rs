//! # matter-sentiment
//!
//! Offline **sentiment / tone** signal via VADER-class lexicon + rules (track **0049**):
//!
//! - Method: frozen [`METHOD_VADER_LEXICON_V1`] (`vader_lexicon_v1`)
//! - Aggregation: **unit-extreme** (max \|compound\| unit) + footer/disclaimer strip
//! - Job: resumable [`run_sentiment`] with text + method + **threshold** fingerprint
//! - Threshold-only change → **relabel** from stored compound (no CAS re-read)
//!
//! ## Honesty
//!
//! - Lexicon scores are **heuristics**, not ground-truth emotion or intent.
//! - **Footer / length dilution:** whole-doc VADER averages hostile short content
//!   toward neutral — this crate uses **unit-extreme** + footer strip; still imperfect.
//! - **Sarcasm, irony, coded language** often mis-score.
//! - **Unscored ≠ Neutral** — NULL polarity means not scored / skipped / cleared.
//! - **Not** for privilege prediction, responsiveness, or auto-coding.
//! - English-centric (multilingual residual **0054**).
//! - Offline VADER-class only — no transformers / cloud.
//!
//! ## License tree
//!
//! Runtime dep: `vader-sentimental` **0.1.3** with `default-features = false`
//! (avoids optional `clap` CLI). Transitive: `hashbrown`, `lazy_static`, `regex`,
//! `unicase` — all MIT / Apache-2.0 permissive. See crate README for audit notes.

#![forbid(unsafe_code)]

pub mod aggregate;
pub mod error;
pub mod method;
pub mod params;
pub mod prep;
pub mod run;
pub mod score;
pub mod units;

pub use aggregate::{aggregate_units, polarity_from_compound, AggregatedSentiment, UnitScore};
pub use error::{Result, SentimentError};
pub use method::METHOD_VADER_LEXICON_V1;
pub use params::{SentimentParams, SCOPE_ALL};
pub use prep::strip_headers_and_disclaimers;
pub use run::{
    run_sentiment, SentimentOutcome, SentimentReport, SentimentSummary, JOB_KIND_SENTIMENT,
    SENTIMENT_STAGE,
};
pub use score::{score_unit, score_unit_with};
pub use units::split_units;
