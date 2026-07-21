//! # matter-ai
//!
//! Opt-in **AI provider abstraction** + **first-pass code suggestions** (tracks **0051** / **0052**):
//!
//! - **AI off by default** — core Desk never requires AI
//! - Trait + Mock + OpenAI-compatible (local Ollama/LM Studio and cloud same shape)
//! - Remote requires explicit `allow_remote` — no silent cloud fallback
//! - Job [`JOB_KIND_AI_SUGGEST_CODES`] writes **suggestions only** (never final `item_codes`)
//! - Keys: OS keyring (Desk) + env `PST_DEDUPE_AI_API_KEY` (headless) — not SQLite
//! - JSON mode + robust fence/prose extract; full catalog guidance in prompt; middle-drop
//! - **Citations** (0052): contiguous verbatim quotes + UTF-8 byte offset hints + verify
//! - CI: Mock only — no network in default tests
//!
//! ## Honesty
//!
//! - Suggestions may be wrong; human accept required for final codes
//! - Citations can be wrong or cherry-picked; human must read in context
//! - Unverified quotes are labeled — not hidden
//! - Accept audit stores **offset pointers only** (no quote cleartext)
//! - Cloud sends matter text when remote allowed
//! - Definitions come from **your** catalog guidance — not invented law
//! - Middle-drop may omit mid-document body

#![forbid(unsafe_code)]

pub mod error;
pub mod params;
pub mod parse;
pub mod prompt;
pub mod provider;
pub mod run;
pub mod secrets;
pub mod truncate;
pub mod verify;

pub use error::{AiError, Result};
pub use params::{AiSuggestCodesParams, SCOPE_ALL, SCOPE_IN_REVIEW};
pub use parse::{
    extract_code_suggestions, ParsedCitation, ParsedCodeSuggestion, MAX_CITATIONS_PER_SUGGESTION,
};
pub use prompt::{
    build_suggest_codes_v1, build_suggest_codes_v2, format_catalog,
    PROMPT_TEMPLATE_SUGGEST_CODES_V1, PROMPT_TEMPLATE_SUGGEST_CODES_V2,
};
pub use provider::{
    host_from_base_url, is_loopback_host, is_remote_base_url, AiProvider, AiProviderKind,
    ChatMessage, ChatRole, CompletionRequest, CompletionResponse, MockAiProvider,
    OpenAiCompatibleProvider, TokenUsage, DEFAULT_TIMEOUT_SECS,
};
pub use run::{
    resolve_provider, run_ai_suggest_codes, run_ai_suggest_codes_with_provider, AiSuggestOutcome,
    AiSuggestReport, AiSuggestSummary, AI_SUGGEST_STAGE, JOB_KIND_AI_SUGGEST_CODES,
};
pub use secrets::{
    delete_api_key, resolve_api_key, resolve_api_key_optional, store_api_key, AI_API_KEY_ENV,
    KEYRING_SERVICE, KEYRING_USER,
};
pub use truncate::{
    assemble_head_tail, middle_drop, truncate_to_char_boundary, DEFAULT_MAX_TEXT_BYTES,
    TRUNCATION_MARKER,
};
pub use verify::{
    normalize_for_verify, verify_ai_citation_against_text, verify_citation_for_storage,
    VerifyCitationResult,
};
