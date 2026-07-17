//! Versioned SQLite schema migrations for matter.db.
//!
//! SQL is private to this crate. Callers interact through the public
//! [`crate::Matter`] API only.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Current schema version applied by this crate.
pub const SCHEMA_VERSION: u32 = 1;

/// Ordered migrations: `(target_version, sql)`.
///
/// Each migration brings the DB from `target_version - 1` to `target_version`.
const MIGRATIONS: &[(u32, &str)] = &[(1, MIGRATION_V1)];

const MIGRATION_V1: &str = r#"
CREATE TABLE schema_meta (
    version INTEGER NOT NULL
);

CREATE TABLE matters (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    storage_root TEXT NOT NULL
);

CREATE TABLE sources (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    path TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    cursor_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE item_families (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    kind TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE items (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    source_id TEXT REFERENCES sources(id),
    family_id TEXT REFERENCES item_families(id),
    path TEXT,
    native_sha256 TEXT,
    logical_hash TEXT,
    message_id TEXT,
    status TEXT NOT NULL,
    size_bytes INTEGER,
    created_at TEXT,
    modified_at TEXT,
    imported_at TEXT NOT NULL
);

CREATE TABLE jobs (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    error_summary TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE job_checkpoints (
    job_id TEXT NOT NULL REFERENCES jobs(id),
    stage TEXT NOT NULL,
    cursor_json TEXT NOT NULL,
    completed_count INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (job_id, stage)
);

CREATE TABLE item_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id TEXT REFERENCES items(id),
    source_id TEXT REFERENCES sources(id),
    job_id TEXT REFERENCES jobs(id),
    stage TEXT NOT NULL,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    detail TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE audit_events (
    seq INTEGER PRIMARY KEY NOT NULL,
    ts TEXT NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    entity TEXT NOT NULL,
    params_json TEXT NOT NULL,
    tool_version TEXT NOT NULL,
    prev_hash TEXT NOT NULL,
    entry_hash TEXT NOT NULL
);

CREATE INDEX idx_items_source ON items(source_id);
CREATE INDEX idx_items_family ON items(family_id);
CREATE INDEX idx_items_native_sha ON items(native_sha256);
CREATE INDEX idx_item_errors_item ON item_errors(item_id);
CREATE INDEX idx_item_errors_source ON item_errors(source_id);
CREATE INDEX idx_item_errors_job ON item_errors(job_id);
CREATE INDEX idx_jobs_state ON jobs(state);
CREATE INDEX idx_audit_entry_hash ON audit_events(entry_hash);
"#;

/// Apply pending migrations up to [`SCHEMA_VERSION`].
pub(crate) fn migrate(conn: &Connection) -> Result<u32> {
    let current = read_schema_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(Error::UnknownSchemaVersion(current));
    }

    for &(target, sql) in MIGRATIONS {
        if current >= target {
            continue;
        }
        conn.execute_batch(sql)?;
        if target == 1 {
            conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])?;
        } else {
            conn.execute("UPDATE schema_meta SET version = ?1", [target])?;
        }
    }

    let after = read_schema_version(conn)?;
    if after != SCHEMA_VERSION {
        return Err(Error::SchemaVersionMismatch {
            found: after,
            expected: SCHEMA_VERSION,
        });
    }

    // Keep denormalized `matters.schema_version` aligned with `schema_meta`.
    // Cheap and idempotent even when no migration steps ran.
    let matters_exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='matters'",
        [],
        |row| row.get(0),
    )?;
    if matters_exists {
        conn.execute("UPDATE matters SET schema_version = ?1", [SCHEMA_VERSION])?;
    }

    Ok(after)
}

/// Read the applied schema version, or `0` if the meta table is absent.
pub(crate) fn read_schema_version(conn: &Connection) -> Result<u32> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_meta'",
        [],
        |row| row.get(0),
    )?;
    if !exists {
        return Ok(0);
    }
    let version: u32 = conn.query_row("SELECT version FROM schema_meta LIMIT 1", [], |row| {
        row.get(0)
    })?;
    Ok(version)
}

/// Configure SQLite for a single-writer desktop matter DB.
pub(crate) fn configure_connection(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migrate_fresh_db_to_current() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        let v = migrate(&conn).expect("migrate");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(read_schema_version(&conn).expect("read"), SCHEMA_VERSION);
    }

    #[test]
    fn migrate_resyncs_matters_schema_version_column() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        migrate(&conn).expect("migrate");

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_test', 'T', '2020-01-01T00:00:00Z', 0, '/tmp')",
            [],
        )
        .expect("insert matter with drifted schema_version");

        let drifted: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_test'",
                [],
                |row| row.get(0),
            )
            .expect("read drifted");
        assert_eq!(drifted, 0);

        // schema_meta already at SCHEMA_VERSION — migrate is a no-op for steps
        // but must still re-sync the denormalized column.
        let v = migrate(&conn).expect("re-migrate");
        assert_eq!(v, SCHEMA_VERSION);

        let synced: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_test'",
                [],
                |row| row.get(0),
            )
            .expect("read synced");
        assert_eq!(synced, SCHEMA_VERSION);
    }
}
