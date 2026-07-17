//! # matter-core
//!
//! On-disk **matter** store for Dedupe Desk:
//!
//! - SQLite metadata (`matter.db`) with versioned migrations (schema **v2**)
//! - Content-addressable blob store (CAS) for **raw physical bytes**
//! - Append-only audit log with integrity hash chain
//! - Jobs + checkpoints for resumable work
//! - Item-level error accumulator for honest partial success
//! - **Normalized Item** model + family graph (parent email ↔ attachments)
//! - Pure **logical_hash** v1 helpers (length-prefixed preimage; BCC-aware)
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

pub use audit::{
    canonical_audit_preimage, compute_entry_hash, verify_audit_chain, AuditEvent, AuditEventInput,
    AuditHashFields, GENESIS_PREV_HASH,
};
pub use cas::{sha256_hex, Cas};
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
    item_role, item_status, Item, ItemFamily, ItemInput, ItemUpdate, Matter, MatterInfo, Source,
    DB_FILE, EXPORTS_DIR, FAMILY_KIND_EMAIL_ATTACHMENTS, INDEX_DIR, LOGS_DIR,
};
pub use schema::SCHEMA_VERSION;
