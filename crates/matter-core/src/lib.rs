//! # matter-core
//!
//! On-disk **matter** store for Dedupe Desk:
//!
//! - SQLite metadata (`matter.db`) with versioned migrations
//! - Content-addressable blob store (CAS) for **raw physical bytes**
//! - Append-only audit log with integrity hash chain
//! - Jobs + checkpoints for resumable work
//! - Item-level error accumulator for honest partial success
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
//! ## Out of scope
//!
//! Purview/PST I/O, logical hash computation, Tantivy, UI, encryption, multi-tenant.

pub mod audit;
pub mod cas;
pub mod error;
pub mod item_errors;
pub mod jobs;
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
pub use matter::{
    Item, ItemInput, Matter, MatterInfo, Source, DB_FILE, EXPORTS_DIR, INDEX_DIR, LOGS_DIR,
};
pub use schema::SCHEMA_VERSION;
