//! # matter-core
//!
//! On-disk **matter** store for Dedupe Desk:
//!
//! - SQLite metadata (`matter.db`) with versioned migrations (schema **v8**)
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
//!
//! ## Layout
//!
//! ```text
//! <matter-root>/
//!   matter.db
//!   blobs/sha256/<aa>/<fullhex>   # CAS (two-hex shard)
//!   index/                        # reserved (Tantivy)
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
//! Purview/PST I/O, full-matter process jobs, Tantivy, UI, encryption, multi-tenant.

pub mod audit;
pub mod cas;
pub mod error;
pub mod item_errors;
pub mod jobs;
pub mod logical_hash;
pub mod matter;
pub mod schema;
pub mod thread_headers;

pub use audit::{
    canonical_audit_preimage, compute_entry_hash, verify_audit_chain, AuditEvent, AuditEventInput,
    AuditHashFields, GENESIS_PREV_HASH,
};
pub use cas::{sha256_hex, Cas, PUT_READER_BUF_SIZE};
pub use error::{Error, Result};
pub use item_errors::{ItemError, ItemErrorInput};
pub use jobs::{Job, JobCheckpoint, JobState};
pub use logical_hash::{
    compute_email_logical_hash, compute_non_email_logical_hash, email_logical_preimage,
    non_email_logical_preimage, normalize_address, normalize_address_list, normalize_body,
    normalize_message_id, normalize_subject_strict, normalize_time_utc_second, EmailLogicalInput,
    LogicalAttachment, NonEmailLogicalInput, LOGICAL_HASH_VERSION,
};
pub use matter::{
    item_cull_status, item_dedup_role, item_dedup_tier, item_near_dup_role, item_role, item_status,
    item_thread_method, ApplyCodesInput, ApplyCodesResult, CodeDef, CodeDefInput, CullCandidate,
    CullFieldUpdate, CullPreset, CullPresetInput, DedupRoleCounts, DedupRoleUpdate,
    DedupeCandidate, Item, ItemCodeInfo, ItemFamily, ItemInput, ItemUpdate, Matter, MatterInfo,
    NearDupCandidate, NearDupFieldUpdate, PromoteCandidate, PromoteFieldUpdate, ReviewListRow,
    ReviewSet, Source, ThreadCandidate, ThreadFieldUpdate, DB_FILE, DEFAULT_REVIEW_SET_NAME,
    EXPORTS_DIR, FAMILY_KIND_EMAIL_ATTACHMENTS, INDEX_DIR, LOGS_DIR, WORKSPACE_DIR,
    WORKSPACE_TEMP_DIR,
};
pub use schema::SCHEMA_VERSION;
pub use thread_headers::{
    normalize_conversation_index_to_hex, parse_in_reply_to, parse_references_header,
    parse_references_json, references_to_json, unfold_header_value, ConversationIndexInput,
};
