//! # extract-teams
//!
//! Offline **Teams / chat export adapters** for Dedupe Desk (track **0055**):
//!
//! | Role | Stack |
//! |---|---|
//! | HTML sanitize | **ammonia** (empty tag allowlist → plain text) |
//! | HTML layout | versioned synthetic fixture parser (`html_fixture_v1`) |
//! | JSON | best-effort documented field mapping |
//! | PST | metadata enrich (SkypeTeams / Team Chat heuristics) |
//! | conversation_id | sha256 of team + channel + **UTC day** + thread |
//!
//! Method ids: [`methods::HTML_FIXTURE_V1`], [`methods::JSON_BEST_EFFORT_V1`],
//! [`methods::PST_ENRICH_V1`].
//!
//! ## ⚠️ BLOCKING THREAD WARNING
//!
//! [`run_teams_extract`] is **CPU- and IO-bound**. Callers **must** run it on a
//! dedicated blocking worker (`process-runner` matter worker). Never call on the
//! GUI or Tokio async worker.
//!
//! ## Out of scope (P0)
//!
//! Live Graph API, conversation UI (0056), real client exports in git,
//! SharePoint physical hydrate.

#![forbid(unsafe_code)]

pub mod body;
pub mod bucket;
pub mod detect;
pub mod error;
pub mod html_parse;
pub mod json_parse;
pub mod limits;
pub mod params;
pub mod pst_enrich;
pub mod run;
pub mod sanitize;

pub use body::{
    build_review_body, format_attachment_line, format_attachment_url_line, format_reaction_line,
};
pub use bucket::{normalize_chat_type, utc_day_bucket, ConversationKeys, CONV_SEP};
pub use detect::{
    detect_format, is_html_export, is_json_export, is_pst_teams_shaped, looks_like_teams_html,
    looks_like_teams_json,
};
pub use error::{Error, Result};
pub use html_parse::{parse_teams_html, ParsedChatMessage, ParsedHtmlExport};
pub use json_parse::parse_teams_json;
pub use limits::{methods, status, DEFAULT_MAX_HTML_BYTES, DEFAULT_MAX_MESSAGES_PER_FILE};
pub use params::TeamsExtractParams;
pub use pst_enrich::{enrich_from_metadata, PstEnrichInput};
pub use run::{
    run_teams_extract, TeamsExtractOutcome, TeamsExtractSummary, JOB_KIND_TEAMS_EXTRACT,
    TEAMS_EXTRACT_STAGE,
};
pub use sanitize::{cap_text, html_to_plain_text};
