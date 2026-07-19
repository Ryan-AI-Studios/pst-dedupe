//! # matter-core
//!
//! On-disk **matter** store for Dedupe Desk:
//!
//! - SQLite metadata (`matter.db`) with versioned migrations (schema **v19**)
//! - Content-addressable blob store (CAS) for **raw physical bytes**
//! - Append-only audit log with integrity hash chain
//! - Jobs + checkpoints for resumable work
//! - Item-level error accumulator for honest partial success
//! - **Normalized Item** model + family graph (parent email ↔ attachments)
//! - Pure **logical_hash** v1 helpers (length-prefixed preimage; BCC-aware)
//! - Matter-level **dedupe** result columns + transactional batch helpers (0021)
//! - Email **threading** header storage + result columns + batch helpers (0022)
//! - **Near-duplicate** result columns + transactional batch helpers (0023)
//! - **Cull** result columns + named presets + transactional batch helpers (0024)
//! - **Promote** review-set membership columns + transactional batch helpers (0025)
//! - **Review list** thin projections for the desk Review surface (0026)
//! - **Coding** catalog + item membership + batch apply/remove with audit (0027)
//! - **Metadata filters** + `saved_searches` + paged filtered review list (0028)
//! - **FTS bookkeeping** (`fts_*` columns) + filtered-in-ids for Tantivy compose (0029)
//! - **Notes / highlights** stand-off work-product annotations (0030)
//! - **Privilege** claims + withhold holds + privilege log CSV export (0031)
//! - **Redaction** regions + true redacted text CAS artifact (0032)
//! - **Office extract** bookkeeping (`office_*` columns) for OOXML text (0033)
//! - **PDF extract** bookkeeping (`pdf_*` columns, `pdf_needs_ocr`) for embedded text (0034)
//! - **Calendar** fields (`cal_*`, `message_class`) + ICS extract bookkeeping (0035)
//! - **OCR** bookkeeping (`ocr_*` columns) for offline Tesseract text (0036)
//! - **File category** bookkeeping (`category_*` columns) for taxonomy_v1 (0037)
//! - **Case overview** aggregations (`CaseOverview` / `load_case_overview`) for desk KPIs (0038)
//!
//! ## Layout
//!
//! ```text
//! <matter-root>/
//!   matter.db
//!   blobs/sha256/<aa>/<fullhex>   # CAS (two-hex shard)
//!   index/                        # Tantivy FTS (matter-search; segments on disk)
//!   exports/                      # reserved (production)
//!   logs/                         # optional file logs
//!   workspace/temp/               # extractor spill (cleaned on open/create)
//! ```
//!
//! ## Logical hash
//!
//! Desk `logical_hash` is a versioned content identity for matter dedupe (0021+).
//! It is **not** the CLI `dedup-engine` Tier-2 content hash. See crate README.
//! Pure helpers live in [`logical_hash`]; preimages are never stored in CAS as native.
//!
//! ## Out of scope
//!
//! Purview/PST I/O, full-matter process jobs, Tantivy engine (see `matter-search`),
//! UI, encryption, multi-tenant.

pub mod audit;
pub mod calendar;
pub mod cas;
pub mod category;
pub mod error;
pub mod filter;
pub mod item_errors;
pub mod jobs;
pub mod logical_hash;
pub mod matter;
pub mod ocr;
pub mod office;
pub mod overview;
pub mod pdf;
pub mod privilege;
pub mod redaction;
pub mod schema;
pub mod thread_headers;

pub use audit::{
    canonical_audit_preimage, compute_entry_hash, verify_audit_chain, AuditEvent, AuditEventInput,
    AuditHashFields, GENESIS_PREV_HASH,
};
pub use calendar::{ics_extract_status, ApplyIcsExtractInput, IcsCandidate, IcsExtractApplyResult};
pub use cas::{sha256_hex, Cas, PUT_READER_BUF_SIZE};
pub use category::{
    category_status, classify_candidate_needs_work, ApplyClassificationInput, CategoryApplyResult,
    ClassifyCandidate,
};
pub use error::{Error, Result};
pub use filter::{
    compile_filter, normalize_stored_instant_for_compare, parse_bound_instant, parse_item_instant,
    register_filter_functions, stored_instant_to_epoch_ms, CompiledFilter, FilterCondition,
    FilterSpec, DESK_UTC_EPOCH_MS_FN, FILTER_SPEC_VERSION, SCOPE_ENTIRE_MATTER,
    SCOPE_REVIEW_CORPUS,
};
pub use item_errors::{ItemError, ItemErrorInput};
pub use jobs::{Job, JobCheckpoint, JobState};
pub use logical_hash::{
    compute_email_logical_hash, compute_non_email_logical_hash, email_logical_preimage,
    non_email_logical_preimage, normalize_address, normalize_address_list, normalize_body,
    normalize_message_id, normalize_subject_strict, normalize_time_utc_second, EmailLogicalInput,
    LogicalAttachment, NonEmailLogicalInput, LOGICAL_HASH_VERSION,
};
pub use matter::{
    collapse_whitespace, display_body_digest, highlight_status, item_cull_status, item_dedup_role,
    item_dedup_tier, item_near_dup_role, item_role, item_status, item_thread_method,
    normalize_for_quote_match, re_resolve_whitespace_normalized, resolve_highlight_against_body,
    utf8_char_slice, ApplyCodesInput, ApplyCodesResult, CodeDef, CodeDefInput,
    CreateHighlightInput, CullCandidate, CullFieldUpdate, CullPreset, CullPresetInput,
    DedupRoleCounts, DedupRoleUpdate, DedupeCandidate, FtsCandidate, FtsFieldUpdate, Item,
    ItemCodeInfo, ItemFamily, ItemHighlight, ItemInput, ItemNote, ItemUpdate, Matter, MatterInfo,
    NearDupCandidate, NearDupFieldUpdate, PromoteCandidate, PromoteFieldUpdate, ResolvedHighlight,
    ReviewListRow, ReviewSet, SavedSearch, SavedSearchInput, Source, ThreadCandidate,
    ThreadFieldUpdate, UpsertNoteInput, DB_FILE, DEFAULT_REVIEW_SET_NAME, EXPORTS_DIR,
    FAMILY_KIND_EMAIL_ATTACHMENTS, HIGHLIGHT_CONTEXT_CHARS, HIGHLIGHT_DEFAULT_COLOR,
    HIGHLIGHT_QUOTE_MAX_BYTES, INDEX_DIR, LOGS_DIR, NOTE_BODY_MAX_BYTES, WORKSPACE_DIR,
    WORKSPACE_TEMP_DIR,
};
pub use ocr::{ocr_status, ApplyOcrTextInput, OcrApplyResult, OcrCandidate};
pub use office::{
    office_extract_status, ApplyOfficeTextInput, OfficeCandidate, OfficeExtractApplyResult,
};
pub use overview::{
    load_case_overview, load_case_overview_on, CaseOverview, CullOverview, ErrorOverview,
    JobsOverview, LabelCount, OcrOverview, OverviewJobRow, OverviewOptions, OverviewTotals,
    PrivilegeOverview, ReviewOverview,
};
pub use pdf::{pdf_extract_status, ApplyPdfTextInput, PdfCandidate, PdfExtractApplyResult};
pub use privilege::{
    basis_label, csv_escape_field, join_addrs_json, path_basename, privilege_basis,
    privilege_log_format, privilege_status, FamilyPrivilegeConsistency, ItemPrivilege,
    PrivilegeLogExportParams, PrivilegeLogExportResult, PrivilegeProtocol,
    UpsertItemPrivilegeInput, UpsertPrivilegeProtocolInput, PRIVILEGE_LOG_COLUMNS,
};
pub use redaction::{
    build_redacted_text, merge_redaction_intervals, redaction_reason, redaction_status,
    resolve_redaction_against_body, CreateRedactionInput, ItemRedaction, RedactedTextResult,
    ResolvedRedaction, REDACTED_TOKEN, REDACTION_CONTEXT_CHARS, REDACTION_QUOTE_MAX_BYTES,
};
pub use schema::SCHEMA_VERSION;
pub use thread_headers::{
    normalize_conversation_index_to_hex, parse_in_reply_to, parse_references_header,
    parse_references_json, references_to_json, unfold_header_value, ConversationIndexInput,
};
