//! Typed errors for the matter store.

use thiserror::Error;

/// Result alias for matter-core operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by matter layout, SQLite, CAS, audit, and job APIs.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("matter root does not exist: {0}")]
    MatterNotFound(String),

    #[error("matter already exists at: {0}")]
    MatterAlreadyExists(String),

    #[error("matter.db missing under root: {0}")]
    DatabaseMissing(String),

    #[error("invalid schema version: found {found}, expected {expected}")]
    SchemaVersionMismatch { found: u32, expected: u32 },

    #[error("unknown schema version in DB: {0}")]
    UnknownSchemaVersion(u32),

    #[error("invalid SHA-256 hex digest: {0}")]
    InvalidDigest(String),

    #[error("CAS collision: blob {digest} already exists with different content")]
    CasCollision { digest: String },

    #[error("CAS blob not found: {0}")]
    BlobNotFound(String),

    #[error("job not found: {0}")]
    JobNotFound(String),

    #[error("invalid job state transition: {from} -> {to}")]
    InvalidJobTransition { from: String, to: String },

    #[error("invalid job state: {0}")]
    InvalidJobState(String),

    #[error("item not found: {0}")]
    ItemNotFound(String),

    #[error("source not found: {0}")]
    SourceNotFound(String),

    #[error("item family not found: {0}")]
    FamilyNotFound(String),

    #[error("parent item not found: {0}")]
    ParentItemNotFound(String),

    #[error("cross-matter family assignment refused: {0}")]
    CrossMatterFamily(String),

    #[error("family cohesion violation: {0}")]
    FamilyCohesion(String),

    #[error("audit chain broken at seq {seq}: {reason}")]
    AuditChainBroken { seq: i64, reason: String },

    #[error("matter row missing from database")]
    MatterRowMissing,

    #[error("matter is encrypted; passphrase required (env {0})")]
    PassphraseRequired(String),

    #[error("wrong matter passphrase")]
    WrongPassphrase,

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("crypto header missing under matter root: {0}")]
    CryptoHeaderMissing(String),

    #[error("{0}")]
    Other(String),
}
