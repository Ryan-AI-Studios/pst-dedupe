//! # matter-promote
//!
//! Matter-level **promote-to-review** (track **0025**):
//!
//! 1. Resolve selection **policy** (`auto` → `cull_included` if cull has run, else `unique_only`)
//! 2. Select base membership (flag-only; never deletes items/CAS)
//! 3. Optional **bidirectional family expand** (children of parents **and** parents of children)
//! 4. Assign dense `review_order` via **single SQL** compound family key (no N+1)
//! 5. Write membership + checkpoint in the same SQLite transaction
//!
//! ## Identity rules
//!
//! - Never delete items or CAS blobs.
//! - Exact dups may enter only via family expand.
//! - Do **not** expand entire threads (`thread_id`) — reserved for 0056.
//!
//! ## Transactions
//!
//! Each batch of promote field updates + `put_checkpoint` commits in **one**
//! SQLite transaction via [`matter_core::Matter::apply_promote_batch_with_checkpoint`].
//!
//! ## 0026 query contract
//!
//! ```sql
//! SELECT * FROM items
//! WHERE in_review = 1
//!   AND (review_set_id = :default_set OR :default_set IS NULL)
//! ORDER BY review_order ASC;
//! ```

#![forbid(unsafe_code)]

pub mod error;
pub mod family;
pub mod order;
pub mod params;
pub mod policy;
pub mod run;

pub use error::{PromoteError, Result};
pub use family::expand_families_bidirectional;
pub use order::{ordered_membership, ordering_uses_single_query_api, FAMILY_ORDER_SQL};
pub use params::{PromoteParams, DEFAULT_REVIEW_SET_NAME};
pub use policy::{
    is_extracted_like, policy_id_valid, policy_implies_expand, resolve_policy, select_base_ids,
    select_base_ids_from_candidates, ALL_POLICY_IDS, POLICY_ALL_EXTRACTED, POLICY_AUTO,
    POLICY_CULL_INCLUDED, POLICY_CULL_INCLUDED_PLUS_FAMILY, POLICY_UNIQUE_ONLY,
    POLICY_UNIQUE_PLUS_FAMILY,
};
pub use run::{run_promote, PromoteOutcome, PromoteSummary, JOB_KIND_PROMOTE, PROMOTE_STAGE};
