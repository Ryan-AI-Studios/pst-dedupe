//! Platform.db schema (independent of matter.db SCHEMA_VERSION).

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Current platform control-plane schema version.
pub const PLATFORM_SCHEMA_VERSION: u32 = 1;

const MIGRATION_V1: &str = r#"
CREATE TABLE schema_meta (
    version INTEGER NOT NULL
);

CREATE TABLE tenants (
    id TEXT PRIMARY KEY NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    jit_provision INTEGER NOT NULL DEFAULT 0,
    oidc_required INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE tenant_idp_configs (
    tenant_id TEXT PRIMARY KEY NOT NULL REFERENCES tenants(id),
    issuer_url TEXT NOT NULL,
    client_id TEXT NOT NULL,
    secret_env TEXT,
    secret_ciphertext BLOB,
    secret_nonce BLOB,
    audiences_json TEXT NOT NULL DEFAULT '[]',
    role_claim_map_json TEXT NOT NULL DEFAULT '{}',
    allowed_email_domains_json TEXT NOT NULL DEFAULT '[]',
    required_groups_json TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL
);

CREATE TABLE platform_matters (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    matter_id TEXT NOT NULL,
    storage_root TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    registered_at TEXT NOT NULL,
    UNIQUE (tenant_id, matter_id),
    UNIQUE (storage_root)
);

CREATE INDEX idx_platform_matters_tenant ON platform_matters(tenant_id);

CREATE TABLE oidc_pending (
    state TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    code_verifier TEXT NOT NULL,
    nonce TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_oidc_pending_expires ON oidc_pending(expires_at);
"#;

/// Apply platform migrations up to [`PLATFORM_SCHEMA_VERSION`].
pub fn migrate(conn: &Connection) -> Result<u32> {
    let current = read_schema_version(conn)?;
    if current > PLATFORM_SCHEMA_VERSION {
        return Err(Error::SchemaVersionMismatch {
            found: current,
            expected: PLATFORM_SCHEMA_VERSION,
        });
    }
    if current < 1 {
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        let step = (|| -> Result<()> {
            conn.execute_batch(MIGRATION_V1)?;
            conn.execute(
                "INSERT INTO schema_meta (version) VALUES (?1)",
                [PLATFORM_SCHEMA_VERSION],
            )?;
            Ok(())
        })();
        match step {
            Ok(()) => {
                conn.execute_batch("COMMIT;")?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e);
            }
        }
    }
    let after = read_schema_version(conn)?;
    if after != PLATFORM_SCHEMA_VERSION {
        return Err(Error::SchemaVersionMismatch {
            found: after,
            expected: PLATFORM_SCHEMA_VERSION,
        });
    }
    Ok(after)
}

pub fn read_schema_version(conn: &Connection) -> Result<u32> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_meta'",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(0);
    }
    let v: u32 = conn.query_row("SELECT version FROM schema_meta LIMIT 1", [], |row| {
        row.get(0)
    })?;
    Ok(v)
}
