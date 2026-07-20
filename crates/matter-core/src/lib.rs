//! # matter-core
//!
//! On-disk **matter** store for Dedupe Desk:
//!
//! - SQLite metadata (`matter.db`) with versioned migrations (schema **v28**)
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
//! - **Matter report** CSV pack export from `CaseOverview` + jobs (0039; PDF deferred D-0039-01)
//! - **Production export** bookkeeping (`production_sets` / `production_items`, schema v20) (0040)
//! - **Production QC** run history (`qc_runs` + selection fingerprint gate, schema v21) (0041)
//! - **Gap analysis** roster + opposing expected docs + gap_runs (schema v22) (0042)
//! - **Processing profiles** (`processing_profiles` + built-in stage presets, schema v23) (0043)
//! - **Workflows** (`workflows` + `jobs.parent_job_id` + built-in multi-step recipes, schema v24) (0044)
//! - **Entity / PII hits** (`item_entity_hits` + `entity_*` rollup columns, schema v25) (0046)
//! - **People–comms graph** (`people`, `item_participants`, `people_edges`, `people_timeline`, schema v26) (0047)
//! - **Concept clustering** (`concept_cluster_sets` / `concept_clusters` / `item_concept_membership`, schema v27) (0048)
//! - **Sentiment / tone** (`sentiment_*` item columns, schema v28) (0049)
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
pub mod cluster;
pub mod entity;
pub mod error;
pub mod filter;
pub mod gap;
pub mod item_errors;
pub mod jobs;
pub mod logical_hash;
pub mod matter;
pub mod ocr;
pub mod office;
pub mod overview;
pub mod pdf;
pub mod people;
pub mod privilege;
pub mod profile;
pub mod qc;
pub mod redaction;
pub mod report;
pub mod schema;
pub mod sentiment;
pub mod thread_headers;
pub mod workflow;

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
pub use cluster::{
    ConceptCluster, ConceptClusterCandidate, ConceptClusterSet, ConceptClusterStatus,
    ConceptClusterWrite, ConceptMembershipWrite, ReplaceConceptClusterSetInput,
    DEFAULT_CONCEPT_SET_NAME,
};
pub use entity::{
    entity_flags, flag_bit_for_entity_type, CreateEntityHitInput, EntityScanCandidate,
    ItemEntityHit, ReplaceEntityHitsInput,
};
pub use error::{Error, Result};
pub use filter::{
    compile_filter, normalize_stored_instant_for_compare, parse_bound_instant, parse_item_instant,
    register_filter_functions, stored_instant_to_epoch_ms, CompiledFilter, FilterCondition,
    FilterSpec, DESK_UTC_EPOCH_MS_FN, FILTER_SPEC_VERSION, SCOPE_ENTIRE_MATTER,
    SCOPE_REVIEW_CORPUS,
};
pub use gap::{
    normalize_custodian_name, normalize_source_label, CustodianInventoryRow, ExpectedCustodian,
    ExpectedSource, GapExpectedDoc, GapExpectedDocInput, GapImportRecord, GapRunRecord,
    ImportExpectedCustodiansResult, InsertGapImportInput, InsertGapRunInput,
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
pub use people::{
    identity_kind, participant_role, people_edge_id, people_graph_pass, people_timeline_id,
    person_id_for, DomainRollupRow, ItemParticipant, PeopleEdge, PeopleGraphStatus,
    PeoplePass1Candidate, PeopleTimelineBucket, Person, UpsertItemParticipantInput,
    UpsertPersonStubInput,
};
pub use privilege::{
    basis_label, csv_escape_field, join_addrs_json, path_basename, privilege_basis,
    privilege_log_format, privilege_status, FamilyPrivilegeConsistency, ItemPrivilege,
    PrivilegeLogExportParams, PrivilegeLogExportResult, PrivilegeProtocol,
    UpsertItemPrivilegeInput, UpsertPrivilegeProtocolInput, PRIVILEGE_LOG_COLUMNS,
};
pub use profile::{
    builtin_id, builtin_profile, builtin_profiles, expand_profile_stage, is_allowlisted_stage,
    parse_profile_body, profile_body_to_json, profile_stage_plan, strip_builtin_prefix,
    ProcessingProfile, ProcessingProfileInput, ProfileBody, StagePlan, StageSpec,
    BUILTIN_EXTRACT_ONLY, BUILTIN_REDUCE_ONLY, BUILTIN_STANDARD, BUILTIN_WITH_OCR,
    CANONICAL_STAGE_ORDER, JOB_KIND_PROFILE_RUN, PROFILE_BODY_MAX_BYTES, PROFILE_BODY_VERSION,
    RESERVED_BUILTIN_NAMES,
};
pub use qc::{qc_run_is_fresh, selection_fingerprint, InsertQcRunInput, QcRunRecord};
pub use redaction::{
    build_redacted_text, merge_redaction_intervals, redaction_reason, redaction_status,
    resolve_redaction_against_body, CreateRedactionInput, ItemRedaction, RedactedTextResult,
    ResolvedRedaction, REDACTED_TOKEN, REDACTION_CONTEXT_CHARS, REDACTION_QUOTE_MAX_BYTES,
};
pub use report::{
    default_matter_report_dir, export_matter_report, rfc3339_to_excel_utc, scrub_error_summary,
    MatterReportParams, MatterReportResult, MATTER_REPORT_FORMAT_VERSION,
};
pub use schema::SCHEMA_VERSION;
pub use sentiment::{
    sentiment_polarity, ClearItemSentimentInput, RelabelItemSentimentInput, SentimentCandidate,
    WriteItemSentimentInput,
};
pub use thread_headers::{
    normalize_conversation_index_to_hex, parse_in_reply_to, parse_references_header,
    parse_references_json, references_to_json, unfold_header_value, ConversationIndexInput,
};
pub use workflow::{
    bind_workflow, builtin_workflow, builtin_workflows, collect_placeholders, evaluate_gate_kind,
    is_allowed_workflow_job_kind, is_hard_gate_kind, parse_workflow_body,
    strip_workflow_builtin_prefix, validate_workflow, validate_workflow_detailed,
    workflow_body_to_json, workflow_builtin_id, workflow_definition_hash, BoundNode, Workflow,
    WorkflowBody, WorkflowInput, WorkflowNode, WorkflowNodeType, WorkflowPlan, WorkflowValidation,
    ALLOWED_WORKFLOW_JOB_KINDS, BUILTIN_EXTRACT_THEN_STANDARD, BUILTIN_INGEST_THEN_STANDARD,
    BUILTIN_QC_THEN_PRODUCE, BUILTIN_REDUCE_ONLY_CHAIN, BUILTIN_WITH_OCR_CHAIN, HARD_GATE_KINDS,
    JOB_KIND_WORKFLOW_RUN, RESERVED_WORKFLOW_BUILTIN_NAMES, WORKFLOW_BODY_MAX_BYTES,
    WORKFLOW_BODY_VERSION,
};
