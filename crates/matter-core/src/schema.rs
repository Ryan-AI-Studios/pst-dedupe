//! Versioned SQLite schema migrations for matter.db.
//!
//! SQL is private to this crate. Callers interact through the public
//! [`crate::Matter`] API only.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Current schema version applied by this crate.
pub const SCHEMA_VERSION: u32 = 3;

/// Ordered migrations: `(target_version, sql)`.
///
/// Each migration brings the DB from `target_version - 1` to `target_version`.
const MIGRATIONS: &[(u32, &str)] = &[(1, MIGRATION_V1), (2, MIGRATION_V2), (3, MIGRATION_V3)];

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

/// Schema v2: Normalized Item P0 fields + logical_hash / message_id indexes.
///
/// Uses nullable `ADD COLUMN` only (SQLite ALTER cannot attach FKs cleanly).
/// `parent_item_id` is plain TEXT; parent existence is enforced in the Matter API.
const MIGRATION_V2: &str = r#"
ALTER TABLE items ADD COLUMN role TEXT;
ALTER TABLE items ADD COLUMN parent_item_id TEXT;
ALTER TABLE items ADD COLUMN mime_type TEXT;
ALTER TABLE items ADD COLUMN file_category TEXT;
ALTER TABLE items ADD COLUMN custodian TEXT;
ALTER TABLE items ADD COLUMN subject TEXT;
ALTER TABLE items ADD COLUMN title TEXT;
ALTER TABLE items ADD COLUMN from_addr TEXT;
ALTER TABLE items ADD COLUMN to_addrs_json TEXT;
ALTER TABLE items ADD COLUMN cc_addrs_json TEXT;
ALTER TABLE items ADD COLUMN bcc_addrs_json TEXT;
ALTER TABLE items ADD COLUMN author TEXT;
ALTER TABLE items ADD COLUMN sent_at TEXT;
ALTER TABLE items ADD COLUMN received_at TEXT;
ALTER TABLE items ADD COLUMN attachment_count INTEGER;
ALTER TABLE items ADD COLUMN text_sha256 TEXT;
ALTER TABLE items ADD COLUMN html_sha256 TEXT;
ALTER TABLE items ADD COLUMN logical_hash_version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN extra_json TEXT;

CREATE INDEX IF NOT EXISTS idx_items_logical_hash ON items(logical_hash);
CREATE INDEX IF NOT EXISTS idx_items_message_id ON items(message_id);
"#;

/// Schema v3: matter-level dedupe result columns (track 0021).
///
/// Nullable `ADD COLUMN` only. Does not overload `status`.
const MIGRATION_V3: &str = r#"
ALTER TABLE items ADD COLUMN dedup_role TEXT;
ALTER TABLE items ADD COLUMN duplicate_of_item_id TEXT;
ALTER TABLE items ADD COLUMN dedup_tier TEXT;
ALTER TABLE items ADD COLUMN dedup_group_id TEXT;
ALTER TABLE items ADD COLUMN deduped_at TEXT;
ALTER TABLE items ADD COLUMN dedup_job_id TEXT;

CREATE INDEX IF NOT EXISTS idx_items_dedup_role ON items(dedup_role);
CREATE INDEX IF NOT EXISTS idx_items_duplicate_of ON items(duplicate_of_item_id);
CREATE INDEX IF NOT EXISTS idx_items_dedup_group ON items(dedup_group_id);
"#;

/// Apply pending migrations up to [`SCHEMA_VERSION`].
///
/// Each migration step (SQL batch + `schema_meta` version bump) runs inside a
/// single `BEGIN IMMEDIATE` transaction so a crash mid-batch cannot leave the
/// DB at the old version with partial columns (re-open would re-run and fail
/// with "duplicate column name").
pub(crate) fn migrate(conn: &Connection) -> Result<u32> {
    let current = read_schema_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(Error::UnknownSchemaVersion(current));
    }

    for &(target, sql) in MIGRATIONS {
        if current >= target {
            continue;
        }
        conn.execute("BEGIN IMMEDIATE", [])?;
        let step = (|| -> Result<()> {
            conn.execute_batch(sql)?;
            if target == 1 {
                conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])?;
            } else {
                conn.execute("UPDATE schema_meta SET version = ?1", [target])?;
            }
            Ok(())
        })();
        if let Err(e) = step {
            let _ = conn.execute("ROLLBACK", []);
            return Err(e);
        }
        conn.execute("COMMIT", [])?;
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
        assert_eq!(v, 3);
        assert_eq!(read_schema_version(&conn).expect("read"), SCHEMA_VERSION);

        // v2 columns present
        let has_role: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'role'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_role);
        let has_lhash_ver: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'logical_hash_version'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_lhash_ver);
        // v3 dedupe columns present
        let has_dedup_role: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'dedup_role'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_dedup_role);
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

    /// v1 fixture (0016-style inventory) → migrate to v2 → data intact + new columns.
    #[test]
    fn migrate_v1_inventory_to_v2_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        // Stop at schema v1 only (no v2 columns).
        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute("INSERT INTO schema_meta (version) VALUES (1)", [])
            .expect("meta v1");
        assert_eq!(read_schema_version(&conn).expect("read"), 1);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v1', 'V1 Matter', '2020-01-01T00:00:00Z', 1, '/tmp/v1')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO sources (id, matter_id, path, kind, status, cursor_json, created_at, updated_at) \
             VALUES ('src_v1', 'mat_v1', 'C:\\exports\\pkg.zip', 'purview_package', 'ready', NULL, \
                     '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z')",
            [],
        )
        .expect("source");
        // 0016-style inventory rows: path + native_sha256 + status; logical_hash/message_id null.
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at) \
             VALUES ('itm_a', 'mat_v1', 'src_v1', NULL, 'files.zip!/a.txt', \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             NULL, NULL, 'expanded', 12, NULL, NULL, '2020-01-01T00:00:01Z')",
            [],
        )
        .expect("item a");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at) \
             VALUES ('itm_b', 'mat_v1', 'src_v1', NULL, 'mail.pst', \
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', \
             NULL, NULL, 'discovered', 100, NULL, NULL, '2020-01-01T00:00:02Z')",
            [],
        )
        .expect("item b");

        // v1 has no role column yet.
        let role_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'role'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(!role_exists);

        let v = migrate(&conn).expect("migrate v1→v3");
        assert_eq!(v, 3);

        // Inventory data intact.
        let (path, status, native, lhv): (String, String, String, i64) = conn
            .query_row(
                "SELECT path, status, native_sha256, logical_hash_version FROM items WHERE id = 'itm_a'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("itm_a");
        assert_eq!(path, "files.zip!/a.txt");
        assert_eq!(status, "expanded");
        assert_eq!(
            native,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(lhv, 0, "default logical_hash_version");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 2);

        // New nullable columns readable as NULL (pre-v2 inventory; NULL role ≡
        // standalone for consumers until classified).
        let role: Option<String> = conn
            .query_row("SELECT role FROM items WHERE id = 'itm_a'", [], |row| {
                row.get(0)
            })
            .expect("role");
        assert!(role.is_none());

        // v3 dedupe columns present and NULL on pre-dedupe inventory.
        let dedup_role: Option<String> = conn
            .query_row(
                "SELECT dedup_role FROM items WHERE id = 'itm_a'",
                [],
                |row| row.get(0),
            )
            .expect("dedup_role");
        assert!(dedup_role.is_none());

        // Existing FKs still work: source_id / family_id.
        let src: String = conn
            .query_row(
                "SELECT source_id FROM items WHERE id = 'itm_b'",
                [],
                |row| row.get(0),
            )
            .expect("src");
        assert_eq!(src, "src_v1");

        // denormalized matters.schema_version synced.
        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v1'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, 3);

        // v2 + v3 indexes present.
        for idx in [
            "idx_items_logical_hash",
            "idx_items_message_id",
            "idx_items_dedup_role",
            "idx_items_duplicate_of",
            "idx_items_dedup_group",
        ] {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name = ?1",
                    [idx],
                    |row| row.get(0),
                )
                .expect("sqlite_master");
            assert!(exists, "expected index {idx}");
        }
    }

    /// v2 fixture → migrate to v3 → data intact + dedupe columns present.
    #[test]
    fn migrate_v2_to_v3_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute("INSERT INTO schema_meta (version) VALUES (2)", [])
            .expect("meta v2");
        assert_eq!(read_schema_version(&conn).expect("read"), 2);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v2', 'V2 Matter', '2020-01-01T00:00:00Z', 2, '/tmp/v2')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version) \
             VALUES ('itm_mail', 'mat_v2', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1)",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v2→v3");
        assert_eq!(v, 3);

        let (status, mid, dedup): (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT status, message_id, dedup_role FROM items WHERE id = 'itm_mail'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("itm_mail");
        assert_eq!(status, "extracted");
        assert_eq!(mid.as_deref(), Some("mid@example.com"));
        assert!(dedup.is_none());

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v2'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, 3);
    }

    /// Each migration step updates schema_meta only after the full batch commits
    /// in the same transaction (smoke: completed migrate leaves version==SCHEMA_VERSION).
    #[test]
    fn migrate_steps_are_transactional() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        // Apply v1 only, then remaining via migrate — version and columns must agree.
        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute("INSERT INTO schema_meta (version) VALUES (1)", [])
            .expect("meta v1");

        let v = migrate(&conn).expect("migrate");
        assert_eq!(v, 3);
        assert_eq!(read_schema_version(&conn).expect("read"), 3);

        let has_role: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'role'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_role);
        let has_dedup: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'dedup_role'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_dedup);
    }
}
