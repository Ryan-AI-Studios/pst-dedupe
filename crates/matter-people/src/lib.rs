//! # matter-people
//!
//! Offline **people–communications graph** (track **0047**):
//!
//! - Relational `item_participants` from From/To/Cc/Bcc headers
//! - Person identity: `identity_kind` + `normalized_key` (**smtp | display | x500 | other**)
//! - Directed edges with **separate BCC counters**; Top Pairs use `visible_count` (to+cc)
//! - Two-pass job: resumable Pass 1 → atomic Pass 2 aggregates
//! - Timeline day/week buckets
//!
//! ## Honesty
//!
//! - **Not** legal name resolution or identity proof.
//! - Non-SMTP nodes retained (display/X.500) — common display names **over-merge** into one node;
//!   preferred over silently dropping internal Exchange graphs.
//! - BCC is stored for investigation but **excluded** from default Top Pairs / `visible_count`.
//! - Self-mail (A→A) is tracked on `people.self_mail_count`, never as a pair edge.
//! - Headers are primary; `include_entity_emails` default **false** (setting `true` fails closed until body join is implemented).
//! - Not Relativity Communication Analysis parity.
//! - Timeline uses available item dates only (sent → received → created).
//!
//! ## person_id
//!
//! Full **64-character** lowercase hex of
//! `sha256(identity_kind || "\0" || normalized_key)` (see `matter_core::person_id_for`).

#![forbid(unsafe_code)]

pub mod error;
pub mod normalize;
pub mod params;
pub mod pass1;
pub mod pass2;
pub mod run;

pub use error::{PeopleError, Result};
pub use normalize::{normalize_participant, NormalizedParticipant};
pub use params::{PeopleGraphParams, GRAIN_DAY, GRAIN_WEEK, SCOPE_ALL};
pub use pass1::{expand_item_participants, parse_addr_list, SOURCE_HEADER};
pub use run::{
    people_graph_fingerprint, run_people_graph, PeopleGraphOutcome, PeopleGraphReport,
    PeopleGraphSummary, JOB_KIND_PEOPLE_GRAPH, PEOPLE_GRAPH_ENGINE_VERSION, PEOPLE_GRAPH_STAGE,
};
