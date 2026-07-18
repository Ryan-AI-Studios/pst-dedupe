//! # matter-cull
//!
//! Matter-level **flag-only data reduction** (track **0024**):
//!
//! 1. Load a **named preset** (built-in or DB) or inline `CullRules`
//! 2. Evaluate composable rules per item (collect **all** matching reasons)
//! 3. Apply **family integrity** (default: included parent ⇒ all direct children included)
//! 4. Write `cull_*` fields + checkpoint in the same SQLite transaction
//!
//! ## Identity rules
//!
//! - Never delete items or CAS blobs.
//! - Near-dup members are **not** culled by default.
//! - Date bounds require RFC3339 **with offset/Z**; start inclusive / end exclusive.
//! - DeNIST (optional) matches **SHA-256 only**; MD5/SHA-1 lists fail closed.
//!
//! ## Transactions
//!
//! Each batch of cull field updates + `put_checkpoint` commits in **one**
//! SQLite transaction via [`matter_core::Matter::apply_cull_batch_with_checkpoint`].

#![forbid(unsafe_code)]

pub mod denist;
pub mod error;
pub mod eval;
pub mod family;
pub mod params;
pub mod presets;
pub mod rules;
pub mod run;

pub use denist::{load_sha256_list, matches_denist, parse_sha256_list, DenistList};
pub use error::{CullError, Result};
pub use eval::{evaluate_item, reasons_to_json, ItemCullDecision};
pub use family::apply_family_policy;
pub use params::CullParams;
pub use presets::{
    builtin_rules, builtin_rules_json, date_window, noise_light, unique_only, unique_plus_family,
    BUILTIN_PRESET_NAMES, PRESET_DATE_WINDOW, PRESET_NOISE_LIGHT, PRESET_UNIQUE_ONLY,
    PRESET_UNIQUE_PLUS_FAMILY,
};
pub use rules::{
    parse_bound_instant, reason, CullRules, DateField, DateRule, EmptyRule, FamilyPolicy, ListMode,
    MissingDatePolicy, PathContainsRule, StringListRule,
};
pub use run::{run_cull, CullOutcome, CullSummary, CULL_STAGE, JOB_KIND_CULL};
