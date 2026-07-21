//! Typed errors for the platform control plane.

use thiserror::Error;

/// Result alias for platform operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Platform registry / IdP / sandbox errors.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("platform database not found: {0}")]
    PlatformNotFound(String),

    #[error("platform already exists at: {0}")]
    PlatformAlreadyExists(String),

    #[error("invalid platform schema version: found {found}, expected {expected}")]
    SchemaVersionMismatch { found: u32, expected: u32 },

    #[error("tenant not found: {0}")]
    TenantNotFound(String),

    #[error("tenant slug already exists: {0}")]
    TenantSlugExists(String),

    #[error("matter registration not found")]
    MatterNotRegistered,

    #[error("path outside PLATFORM_STORAGE_ROOT sandbox: {0}")]
    PathNotSandboxed(String),

    #[error("platform master key required to store or decrypt IdP client secrets")]
    PmkRequired,

    #[error("invalid platform master key: {0}")]
    InvalidPmk(String),

    #[error("IdP client secret not available (missing env or ciphertext)")]
    SecretUnavailable,

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("JIT misconfigured: enable requires allowed_email_domains and/or required_groups")]
    JitOpenForbidden,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("{0}")]
    Other(String),
}
