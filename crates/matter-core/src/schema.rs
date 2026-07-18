//! Versioned SQLite schema migrations for matter.db.
//!
//! SQL is private to this crate. Callers interact through the public
//! [`crate::Matter`] API only.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Current schema version applied by this crate.
pub const SCHEMA_VERSION: u32 = 11;

/// Ordered migrations: `(target_version, sql)`.
///
/// Each migration brings the DB from `target_version - 1` to `target_version`.
const MIGRATIONS: &[(u32, &str)] = &[
    (1, MIGRATION_V1),
    (2, MIGRATION_V2),
    (3, MIGRATION_V3),
    (4, MIGRATION_V4),
    (5, MIGRATION_V5),
    (6, MIGRATION_V6),
    (7, MIGRATION_V7),
    (8, MIGRATION_V8),
    (9, MIGRATION_V9),
    (10, MIGRATION_V10),
    (11, MIGRATION_V11),
];

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

/// Schema v4: email threading header storage + result columns (track 0022).
///
/// Nullable `ADD COLUMN` only. Header storage columns are not cleared by the
/// thread job; result columns (`thread_*`) are.
const MIGRATION_V4: &str = r#"
ALTER TABLE items ADD COLUMN in_reply_to TEXT;
ALTER TABLE items ADD COLUMN references_json TEXT;
ALTER TABLE items ADD COLUMN conversation_topic TEXT;
ALTER TABLE items ADD COLUMN conversation_index_hex TEXT;
ALTER TABLE items ADD COLUMN thread_id TEXT;
ALTER TABLE items ADD COLUMN thread_root_item_id TEXT;
ALTER TABLE items ADD COLUMN thread_method TEXT;
ALTER TABLE items ADD COLUMN threaded_at TEXT;
ALTER TABLE items ADD COLUMN thread_job_id TEXT;

CREATE INDEX IF NOT EXISTS idx_items_thread_id ON items(thread_id);
CREATE INDEX IF NOT EXISTS idx_items_in_reply_to ON items(in_reply_to);
"#;

/// Schema v5: near-duplicate result columns (track 0023).
///
/// Nullable `ADD COLUMN` only. Does not overload `dedup_*` or `thread_*`.
const MIGRATION_V5: &str = r#"
ALTER TABLE items ADD COLUMN near_dup_group_id TEXT;
ALTER TABLE items ADD COLUMN near_dup_role TEXT;
ALTER TABLE items ADD COLUMN near_dup_similarity REAL;
ALTER TABLE items ADD COLUMN near_dup_pivot_item_id TEXT;
ALTER TABLE items ADD COLUMN near_dup_method TEXT;
ALTER TABLE items ADD COLUMN near_duped_at TEXT;
ALTER TABLE items ADD COLUMN near_dup_job_id TEXT;

CREATE INDEX IF NOT EXISTS idx_items_near_dup_group ON items(near_dup_group_id);
CREATE INDEX IF NOT EXISTS idx_items_near_dup_role ON items(near_dup_role);
"#;

/// Schema v6: cull / data-reduction result columns + named presets (track 0024).
///
/// Nullable `ADD COLUMN` only. Does not overload `dedup_*`, `thread_*`, or `near_dup_*`.
/// Preset delete never clears item cull fields.
const MIGRATION_V6: &str = r#"
ALTER TABLE items ADD COLUMN cull_status TEXT;
ALTER TABLE items ADD COLUMN cull_reasons_json TEXT;
ALTER TABLE items ADD COLUMN cull_preset_id TEXT;
ALTER TABLE items ADD COLUMN cull_preset_name TEXT;
ALTER TABLE items ADD COLUMN culled_at TEXT;
ALTER TABLE items ADD COLUMN cull_job_id TEXT;

CREATE INDEX IF NOT EXISTS idx_items_cull_status ON items(cull_status);
CREATE INDEX IF NOT EXISTS idx_items_cull_preset ON items(cull_preset_id);

CREATE TABLE cull_presets (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    name TEXT NOT NULL,
    description TEXT,
    rules_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_by TEXT
);

CREATE INDEX IF NOT EXISTS idx_cull_presets_matter ON cull_presets(matter_id);
"#;

/// Schema v7: review-set membership for promote-to-review (track 0025).
///
/// Nullable item columns + `review_sets` table. Flag-only membership â€” never
/// deletes items/CAS. Partial unique index enforces at most one default set
/// per matter (`is_default = 1`).
const MIGRATION_V7: &str = r#"
CREATE TABLE review_sets (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    name TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    policy TEXT,
    policy_json TEXT,
    item_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_by TEXT
);

CREATE INDEX IF NOT EXISTS idx_review_sets_matter ON review_sets(matter_id);

-- At most one default review set per matter (non-default rows unrestricted).
CREATE UNIQUE INDEX idx_review_sets_one_default
  ON review_sets(matter_id)
  WHERE is_default = 1;

ALTER TABLE items ADD COLUMN in_review INTEGER;
ALTER TABLE items ADD COLUMN review_set_id TEXT;
ALTER TABLE items ADD COLUMN review_order INTEGER;
ALTER TABLE items ADD COLUMN promoted_at TEXT;
ALTER TABLE items ADD COLUMN promote_job_id TEXT;
ALTER TABLE items ADD COLUMN promote_policy TEXT;

CREATE INDEX IF NOT EXISTS idx_items_in_review ON items(in_review);
CREATE INDEX IF NOT EXISTS idx_items_review_set_id ON items(review_set_id);
CREATE INDEX IF NOT EXISTS idx_items_review_set_order ON items(review_set_id, review_order);
"#;

/// Schema v8: coding catalog + item↔code membership (track 0027).
///
/// Matter-scoped code definitions and membership rows only — never deletes
/// items/CAS. Inactive definitions remain for historical membership display.
const MIGRATION_V8: &str = r#"
CREATE TABLE code_definitions (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    key TEXT NOT NULL,
    label TEXT NOT NULL,
    group_key TEXT NOT NULL,
    cardinality TEXT NOT NULL,
    color TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_code_definitions_matter_key
  ON code_definitions(matter_id, key);

CREATE INDEX idx_code_definitions_matter_group_sort
  ON code_definitions(matter_id, group_key, sort_order);

CREATE TABLE item_codes (
    item_id TEXT NOT NULL,
    code_id TEXT NOT NULL,
    set_at TEXT NOT NULL,
    set_by TEXT NOT NULL,
    PRIMARY KEY (item_id, code_id)
);

CREATE INDEX idx_item_codes_item ON item_codes(item_id);
CREATE INDEX idx_item_codes_code ON item_codes(code_id);
"#;

/// Schema v9: saved searches + review-list ORDER BY index (track 0028).
///
/// Named `FilterSpec` JSON rows (live re-run on load). Partial compound index
/// supports filtered Review list `ORDER BY review_order, imported_at, path, id`
/// under the default `in_review = 1` scope. SQLite ASC still sorts NULLs first
/// for `review_order`; list SQL uses `(review_order IS NULL), review_order`
/// to emulate NULLS LAST without relying on SQLite version features.
const MIGRATION_V9: &str = r#"
CREATE TABLE saved_searches (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    name TEXT NOT NULL,
    description TEXT,
    scope TEXT NOT NULL,
    filter_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_by TEXT
);

CREATE UNIQUE INDEX idx_saved_searches_matter_name
  ON saved_searches(matter_id, name);

CREATE INDEX IF NOT EXISTS idx_items_review_list_order
  ON items(review_set_id, review_order, imported_at, path, id)
  WHERE in_review = 1;
"#;

/// Schema v10: Tantivy FTS bookkeeping + saved search keyword (track 0029).
///
/// Nullable `fts_*` columns on items track which CAS digest was last indexed.
/// `saved_searches.keyword` stores the optional body keyword query beside
/// metadata `filter_json`. Tantivy segments live under `index/` on disk — never
/// in SQLite (no FTS5 primary).
const MIGRATION_V10: &str = r#"
ALTER TABLE items ADD COLUMN fts_text_sha256 TEXT;
ALTER TABLE items ADD COLUMN fts_indexed_at TEXT;
ALTER TABLE items ADD COLUMN fts_error TEXT;
ALTER TABLE saved_searches ADD COLUMN keyword TEXT;
"#;

/// Schema v11: stand-off notes + text highlights (track 0030).
///
/// Work-product annotations beside the document — never rewrite CAS body text.
/// Highlights store UTF-8 **char** indices + TextQuoteSelector-style fields.
/// Denormalized `note_count` / `highlight_count` on items keep list badges fast.
/// Hard-delete is OK; audit retains body / range snapshots.
const MIGRATION_V11: &str = r#"
CREATE TABLE item_notes (
    id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    matter_id TEXT NOT NULL,
    body TEXT NOT NULL,
    highlight_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_by TEXT NOT NULL,
    updated_by TEXT NOT NULL
);

CREATE INDEX idx_item_notes_item_updated
  ON item_notes(item_id, updated_at DESC);

CREATE INDEX idx_item_notes_matter ON item_notes(matter_id);

CREATE INDEX idx_item_notes_highlight ON item_notes(highlight_id);

CREATE TABLE item_highlights (
    id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    matter_id TEXT NOT NULL,
    start_utf8 INTEGER NOT NULL,
    end_utf8 INTEGER NOT NULL,
    exact_quote TEXT NOT NULL,
    prefix TEXT,
    suffix TEXT,
    body_digest TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#FFF59D',
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_by TEXT NOT NULL
);

CREATE INDEX idx_item_highlights_item ON item_highlights(item_id);

CREATE INDEX idx_item_highlights_matter_status
  ON item_highlights(matter_id, status);

ALTER TABLE items ADD COLUMN note_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN highlight_count INTEGER NOT NULL DEFAULT 0;
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
///
/// Also registers filter UDFs (e.g. `desk_utc_epoch_ms`) required by compiled
/// metadata date predicates (track 0028).
pub(crate) fn configure_connection(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;",
    )?;
    crate::filter::register_filter_functions(conn)?;
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
        assert_eq!(v, 11);
        assert_eq!(read_schema_version(&conn).expect("read"), SCHEMA_VERSION);

        // v10 FTS bookkeeping columns present
        let has_fts: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'fts_text_sha256'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_fts, "expected fts_text_sha256 on items");
        // v11 notes / highlights
        let has_notes: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_notes'",
                [],
                |row| row.get(0),
            )
            .expect("item_notes");
        assert!(has_notes);
        let has_kw: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('saved_searches') WHERE name = 'keyword'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_kw, "expected keyword on saved_searches");

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
        // v4 thread columns present
        let has_thread_id: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'thread_id'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_thread_id);
        let has_in_reply_to: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'in_reply_to'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_in_reply_to);
        // v5 near-dup columns present
        let has_near_dup_role: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'near_dup_role'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_near_dup_role);
        let has_near_dup_sim: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'near_dup_similarity'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_near_dup_sim);
        // v6 cull columns present
        let has_cull_status: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'cull_status'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_cull_status);
        let has_cull_presets: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='cull_presets'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_cull_presets);
        // v7 review-set columns + table
        let has_in_review: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'in_review'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_in_review);
        let has_review_sets: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='review_sets'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_review_sets);
        let has_one_default_idx: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_review_sets_one_default'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_one_default_idx);
        // v8 coding tables
        let has_code_defs: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='code_definitions'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_code_defs);
        let has_item_codes: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_codes'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_item_codes);
        // v9 saved_searches + review list order index
        let has_saved: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='saved_searches'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_saved);
        let has_review_list_idx: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_items_review_list_order'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_review_list_idx);
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

        // schema_meta already at SCHEMA_VERSION â€” migrate is a no-op for steps
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

    /// v1 fixture (0016-style inventory) â†’ migrate to v2 â†’ data intact + new columns.
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

        let v = migrate(&conn).expect("migrate v1â†’v6");
        assert_eq!(v, SCHEMA_VERSION);

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

        // New nullable columns readable as NULL (pre-v2 inventory; NULL role â‰¡
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

        // v5 near-dup columns present and NULL.
        let near_dup_role: Option<String> = conn
            .query_row(
                "SELECT near_dup_role FROM items WHERE id = 'itm_a'",
                [],
                |row| row.get(0),
            )
            .expect("near_dup_role");
        assert!(near_dup_role.is_none());

        // v6 cull columns present and NULL.
        let cull_status: Option<String> = conn
            .query_row(
                "SELECT cull_status FROM items WHERE id = 'itm_a'",
                [],
                |row| row.get(0),
            )
            .expect("cull_status");
        assert!(cull_status.is_none());

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
        assert_eq!(ms, SCHEMA_VERSION);

        // v2 + v3 + v4 + v5 + v6 indexes present.
        for idx in [
            "idx_items_logical_hash",
            "idx_items_message_id",
            "idx_items_dedup_role",
            "idx_items_duplicate_of",
            "idx_items_dedup_group",
            "idx_items_thread_id",
            "idx_items_in_reply_to",
            "idx_items_near_dup_group",
            "idx_items_near_dup_role",
            "idx_items_cull_status",
            "idx_items_cull_preset",
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

    /// v2 fixture â†’ migrate to current â†’ data intact + dedupe/thread columns.
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

        let v = migrate(&conn).expect("migrate v2â†’v6");
        assert_eq!(v, SCHEMA_VERSION);

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
        assert_eq!(ms, SCHEMA_VERSION);
    }

    /// v3 fixture â†’ migrate to current â†’ data intact + thread columns present.
    #[test]
    fn migrate_v3_to_v4_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute_batch(MIGRATION_V3).expect("v3");
        conn.execute("INSERT INTO schema_meta (version) VALUES (3)", [])
            .expect("meta v3");
        assert_eq!(read_schema_version(&conn).expect("read"), 3);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v3', 'V3 Matter', '2020-01-01T00:00:00Z', 3, '/tmp/v3')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, dedup_role) \
             VALUES ('itm_mail', 'mat_v3', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, 'unique')",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v3â†’v6");
        assert_eq!(v, SCHEMA_VERSION);

        let (status, mid, dedup, thread_id, in_reply): (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT status, message_id, dedup_role, thread_id, in_reply_to \
                 FROM items WHERE id = 'itm_mail'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("itm_mail");
        assert_eq!(status, "extracted");
        assert_eq!(mid.as_deref(), Some("mid@example.com"));
        assert_eq!(dedup.as_deref(), Some("unique"));
        assert!(thread_id.is_none());
        assert!(in_reply.is_none());

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v3'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);

        for idx in ["idx_items_thread_id", "idx_items_in_reply_to"] {
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

    /// v4 fixture â†’ migrate to v5 â†’ data intact + near-dup columns present.
    #[test]
    fn migrate_v4_to_v5_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute_batch(MIGRATION_V3).expect("v3");
        conn.execute_batch(MIGRATION_V4).expect("v4");
        conn.execute("INSERT INTO schema_meta (version) VALUES (4)", [])
            .expect("meta v4");
        assert_eq!(read_schema_version(&conn).expect("read"), 4);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v4', 'V4 Matter', '2020-01-01T00:00:00Z', 4, '/tmp/v4')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, dedup_role, thread_id, thread_method) \
             VALUES ('itm_mail', 'mat_v4', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, 'unique', 'tid-1', 'headers')",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v4â†’v6");
        assert_eq!(v, SCHEMA_VERSION);

        let status: String = conn
            .query_row(
                "SELECT status FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("status");
        let mid: Option<String> = conn
            .query_row(
                "SELECT message_id FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("mid");
        let dedup: Option<String> = conn
            .query_row(
                "SELECT dedup_role FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("dedup");
        let thread_id: Option<String> = conn
            .query_row(
                "SELECT thread_id FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("thread_id");
        let near_role: Option<String> = conn
            .query_row(
                "SELECT near_dup_role FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("near_role");
        let near_sim: Option<f64> = conn
            .query_row(
                "SELECT near_dup_similarity FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("near_sim");
        assert_eq!(status, "extracted");
        assert_eq!(mid.as_deref(), Some("mid@example.com"));
        assert_eq!(dedup.as_deref(), Some("unique"));
        assert_eq!(thread_id.as_deref(), Some("tid-1"));
        assert!(near_role.is_none());
        assert!(near_sim.is_none());

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v4'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);

        for idx in ["idx_items_near_dup_group", "idx_items_near_dup_role"] {
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

    /// v5 fixture â†’ migrate to v6 â†’ data intact + cull columns + cull_presets table.
    #[test]
    fn migrate_v5_to_v6_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute_batch(MIGRATION_V3).expect("v3");
        conn.execute_batch(MIGRATION_V4).expect("v4");
        conn.execute_batch(MIGRATION_V5).expect("v5");
        conn.execute("INSERT INTO schema_meta (version) VALUES (5)", [])
            .expect("meta v5");
        assert_eq!(read_schema_version(&conn).expect("read"), 5);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v5', 'V5 Matter', '2020-01-01T00:00:00Z', 5, '/tmp/v5')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, dedup_role, thread_id, near_dup_role) \
             VALUES ('itm_mail', 'mat_v5', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, 'unique', 'tid-1', 'unique')",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v5â†’current");
        assert_eq!(v, SCHEMA_VERSION);

        let near_role: Option<String> = conn
            .query_row(
                "SELECT near_dup_role FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("near_role");
        assert_eq!(near_role.as_deref(), Some("unique"));

        let cull_status: Option<String> = conn
            .query_row(
                "SELECT cull_status FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("cull_status");
        assert!(cull_status.is_none());

        let has_presets: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='cull_presets'",
                [],
                |row| row.get(0),
            )
            .expect("cull_presets table");
        assert!(has_presets);

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v5'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);

        for idx in ["idx_items_cull_status", "idx_items_cull_preset"] {
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

    /// v6 fixture â†’ migrate to v7 â†’ data intact + review columns + review_sets.
    #[test]
    fn migrate_v6_to_v7_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute_batch(MIGRATION_V3).expect("v3");
        conn.execute_batch(MIGRATION_V4).expect("v4");
        conn.execute_batch(MIGRATION_V5).expect("v5");
        conn.execute_batch(MIGRATION_V6).expect("v6");
        conn.execute("INSERT INTO schema_meta (version) VALUES (6)", [])
            .expect("meta v6");
        assert_eq!(read_schema_version(&conn).expect("read"), 6);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v6', 'V6 Matter', '2020-01-01T00:00:00Z', 6, '/tmp/v6')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, dedup_role, thread_id, near_dup_role, \
             cull_status, cull_preset_name) \
             VALUES ('itm_mail', 'mat_v6', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, 'unique', 'tid-1', 'unique', 'included', 'unique_only')",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v6 to current");
        assert_eq!(v, SCHEMA_VERSION);

        let cull_status: Option<String> = conn
            .query_row(
                "SELECT cull_status FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("cull_status");
        assert_eq!(cull_status.as_deref(), Some("included"));

        let in_review: Option<i64> = conn
            .query_row(
                "SELECT in_review FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("in_review");
        assert!(in_review.is_none());

        let has_review_sets: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='review_sets'",
                [],
                |row| row.get(0),
            )
            .expect("review_sets table");
        assert!(has_review_sets);

        // Partial unique index rejects two defaults for the same matter.
        conn.execute(
            "INSERT INTO review_sets (id, matter_id, name, is_default, policy, policy_json, \
             item_count, created_at, updated_at, created_by) \
             VALUES ('rs1', 'mat_v6', 'Review Corpus', 1, NULL, NULL, 0, \
             '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
            [],
        )
        .expect("first default");
        let err = conn
            .execute(
                "INSERT INTO review_sets (id, matter_id, name, is_default, policy, policy_json, \
                 item_count, created_at, updated_at, created_by) \
                 VALUES ('rs2', 'mat_v6', 'Other', 1, NULL, NULL, 0, \
                 '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
                [],
            )
            .expect_err("second default must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("UNIQUE") || msg.contains("unique"),
            "expected unique violation, got: {msg}"
        );

        // Multiple non-default sets are allowed.
        conn.execute(
            "INSERT INTO review_sets (id, matter_id, name, is_default, policy, policy_json, \
             item_count, created_at, updated_at, created_by) \
             VALUES ('rs3', 'mat_v6', 'Secondary', 0, NULL, NULL, 0, \
             '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
            [],
        )
        .expect("non-default ok");

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v6'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);

        for idx in [
            "idx_review_sets_one_default",
            "idx_items_in_review",
            "idx_items_review_set_id",
            "idx_items_review_set_order",
            "idx_code_definitions_matter_key",
            "idx_item_codes_item",
            "idx_saved_searches_matter_name",
            "idx_items_review_list_order",
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

    /// v7 fixture → migrate to current → data intact + coding tables.
    #[test]
    fn migrate_v7_to_v8_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute_batch(MIGRATION_V3).expect("v3");
        conn.execute_batch(MIGRATION_V4).expect("v4");
        conn.execute_batch(MIGRATION_V5).expect("v5");
        conn.execute_batch(MIGRATION_V6).expect("v6");
        conn.execute_batch(MIGRATION_V7).expect("v7");
        conn.execute("INSERT INTO schema_meta (version) VALUES (7)", [])
            .expect("meta v7");
        assert_eq!(read_schema_version(&conn).expect("read"), 7);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v7', 'V7 Matter', '2020-01-01T00:00:00Z', 7, '/tmp/v7')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO review_sets (id, matter_id, name, is_default, policy, policy_json, \
             item_count, created_at, updated_at, created_by) \
             VALUES ('rs1', 'mat_v7', 'Review Corpus', 1, NULL, NULL, 1, \
             '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
            [],
        )
        .expect("review set");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, in_review, review_set_id, review_order) \
             VALUES ('itm_mail', 'mat_v7', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, 1, 'rs1', 1)",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v7 to current");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, 11);

        let in_review: Option<i64> = conn
            .query_row(
                "SELECT in_review FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("in_review");
        assert_eq!(in_review, Some(1));

        let has_code_defs: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='code_definitions'",
                [],
                |row| row.get(0),
            )
            .expect("code_definitions");
        assert!(has_code_defs);
        let has_item_codes: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_codes'",
                [],
                |row| row.get(0),
            )
            .expect("item_codes");
        assert!(has_item_codes);

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v7'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);

        // Unique (matter_id, key) enforced.
        conn.execute(
            "INSERT INTO code_definitions (id, matter_id, key, label, group_key, cardinality, \
             color, sort_order, is_active, created_at) \
             VALUES ('cd1', 'mat_v7', 'responsive', 'Responsive', 'responsiveness', 'single', \
             NULL, 1, 1, '2020-01-01T00:00:00Z')",
            [],
        )
        .expect("first code");
        let err = conn
            .execute(
                "INSERT INTO code_definitions (id, matter_id, key, label, group_key, cardinality, \
                 color, sort_order, is_active, created_at) \
                 VALUES ('cd2', 'mat_v7', 'responsive', 'Dup', 'responsiveness', 'single', \
                 NULL, 2, 1, '2020-01-01T00:00:00Z')",
                [],
            )
            .expect_err("duplicate key must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("UNIQUE") || msg.contains("unique"),
            "expected unique violation, got: {msg}"
        );
    }

    /// v8 fixture → migrate to v9 → data intact + saved_searches + list index.
    #[test]
    fn migrate_v8_to_v9_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute_batch(MIGRATION_V3).expect("v3");
        conn.execute_batch(MIGRATION_V4).expect("v4");
        conn.execute_batch(MIGRATION_V5).expect("v5");
        conn.execute_batch(MIGRATION_V6).expect("v6");
        conn.execute_batch(MIGRATION_V7).expect("v7");
        conn.execute_batch(MIGRATION_V8).expect("v8");
        conn.execute("INSERT INTO schema_meta (version) VALUES (8)", [])
            .expect("meta v8");
        assert_eq!(read_schema_version(&conn).expect("read"), 8);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v8', 'V8 Matter', '2020-01-01T00:00:00Z', 8, '/tmp/v8')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, in_review, review_set_id, review_order) \
             VALUES ('itm_mail', 'mat_v8', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, 1, 'rs1', 1)",
            [],
        )
        .expect("item");
        conn.execute(
            "INSERT INTO code_definitions (id, matter_id, key, label, group_key, cardinality, \
             color, sort_order, is_active, created_at) \
             VALUES ('cd1', 'mat_v8', 'responsive', 'Responsive', 'responsiveness', 'single', \
             NULL, 1, 1, '2020-01-01T00:00:00Z')",
            [],
        )
        .expect("code");

        let v = migrate(&conn).expect("migrate v8 to current");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, 11);

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_mail'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("inbox/a.eml"));

        let key: String = conn
            .query_row(
                "SELECT key FROM code_definitions WHERE id = 'cd1'",
                [],
                |row| row.get(0),
            )
            .expect("code key");
        assert_eq!(key, "responsive");

        let has_saved: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='saved_searches'",
                [],
                |row| row.get(0),
            )
            .expect("saved_searches");
        assert!(has_saved);

        let has_idx: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_items_review_list_order'",
                [],
                |row| row.get(0),
            )
            .expect("idx");
        assert!(has_idx, "expected idx_items_review_list_order");

        let has_fts: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'fts_text_sha256'",
                [],
                |row| row.get(0),
            )
            .expect("fts col");
        assert!(has_fts);

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v8'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);

        // Unique (matter_id, name) on saved_searches.
        conn.execute(
            "INSERT INTO saved_searches (id, matter_id, name, description, scope, filter_json, \
             created_at, updated_at, created_by) \
             VALUES ('ss1', 'mat_v8', 'Uncoded', NULL, 'review_corpus', '{}', \
             '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
            [],
        )
        .expect("first saved search");
        let err = conn
            .execute(
                "INSERT INTO saved_searches (id, matter_id, name, description, scope, filter_json, \
                 created_at, updated_at, created_by) \
                 VALUES ('ss2', 'mat_v8', 'Uncoded', NULL, 'review_corpus', '{}', \
                 '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
                [],
            )
            .expect_err("duplicate name must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("UNIQUE") || msg.contains("unique"),
            "expected unique violation, got: {msg}"
        );
    }

    /// v9 fixture → migrate to current → data intact + fts_* + keyword columns.
    #[test]
    fn migrate_v9_to_v10_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute_batch(MIGRATION_V3).expect("v3");
        conn.execute_batch(MIGRATION_V4).expect("v4");
        conn.execute_batch(MIGRATION_V5).expect("v5");
        conn.execute_batch(MIGRATION_V6).expect("v6");
        conn.execute_batch(MIGRATION_V7).expect("v7");
        conn.execute_batch(MIGRATION_V8).expect("v8");
        conn.execute_batch(MIGRATION_V9).expect("v9");
        conn.execute("INSERT INTO schema_meta (version) VALUES (9)", [])
            .expect("meta v9");
        assert_eq!(read_schema_version(&conn).expect("read"), 9);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v9', 'V9 Matter', '2020-01-01T00:00:00Z', 9, '/tmp/v9')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review) \
             VALUES ('itm_mail', 'mat_v9', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, \
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 1)",
            [],
        )
        .expect("item");
        conn.execute(
            "INSERT INTO saved_searches (id, matter_id, name, description, scope, filter_json, \
             created_at, updated_at, created_by) \
             VALUES ('ss1', 'mat_v9', 'Alice', NULL, 'review_corpus', '{}', \
             '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
            [],
        )
        .expect("saved search");

        let v = migrate(&conn).expect("migrate v9 to current");
        assert_eq!(v, SCHEMA_VERSION);

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_mail'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("inbox/a.eml"));

        let fts: Option<String> = conn
            .query_row(
                "SELECT fts_text_sha256 FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("fts");
        assert!(fts.is_none(), "new fts column starts NULL");

        let keyword: Option<String> = conn
            .query_row(
                "SELECT keyword FROM saved_searches WHERE id = 'ss1'",
                [],
                |row| row.get(0),
            )
            .expect("keyword");
        assert!(keyword.is_none());

        let name: String = conn
            .query_row(
                "SELECT name FROM saved_searches WHERE id = 'ss1'",
                [],
                |row| row.get(0),
            )
            .expect("name");
        assert_eq!(name, "Alice");

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v9'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    /// v10 fixture → migrate to v11 → data intact + notes/highlights tables + counts.
    #[test]
    fn migrate_v10_to_v11_preserves_rows() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute_batch(MIGRATION_V2).expect("v2");
        conn.execute_batch(MIGRATION_V3).expect("v3");
        conn.execute_batch(MIGRATION_V4).expect("v4");
        conn.execute_batch(MIGRATION_V5).expect("v5");
        conn.execute_batch(MIGRATION_V6).expect("v6");
        conn.execute_batch(MIGRATION_V7).expect("v7");
        conn.execute_batch(MIGRATION_V8).expect("v8");
        conn.execute_batch(MIGRATION_V9).expect("v9");
        conn.execute_batch(MIGRATION_V10).expect("v10");
        conn.execute("INSERT INTO schema_meta (version) VALUES (10)", [])
            .expect("meta v10");
        assert_eq!(read_schema_version(&conn).expect("read"), 10);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v10', 'V10 Matter', '2020-01-01T00:00:00Z', 10, '/tmp/v10')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review, \
             fts_text_sha256) \
             VALUES ('itm_mail', 'mat_v10', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, \
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 1, \
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb')",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v10 to v11");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, 11);

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_mail'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("inbox/a.eml"));

        let note_count: i64 = conn
            .query_row(
                "SELECT note_count FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("note_count");
        assert_eq!(note_count, 0);

        let hl_count: i64 = conn
            .query_row(
                "SELECT highlight_count FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("highlight_count");
        assert_eq!(hl_count, 0);

        let has_notes: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_notes'",
                [],
                |row| row.get(0),
            )
            .expect("item_notes");
        assert!(has_notes);

        let has_hl: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_highlights'",
                [],
                |row| row.get(0),
            )
            .expect("item_highlights");
        assert!(has_hl);

        let fts: Option<String> = conn
            .query_row(
                "SELECT fts_text_sha256 FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("fts");
        assert_eq!(
            fts.as_deref(),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v10'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    /// Each migration step updates schema_meta only after the full batch commits
    /// in the same transaction (smoke: completed migrate leaves version==SCHEMA_VERSION).
    #[test]
    fn migrate_steps_are_transactional() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");

        // Apply v1 only, then remaining via migrate â€” version and columns must agree.
        conn.execute_batch(MIGRATION_V1).expect("v1");
        conn.execute("INSERT INTO schema_meta (version) VALUES (1)", [])
            .expect("meta v1");

        let v = migrate(&conn).expect("migrate");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(read_schema_version(&conn).expect("read"), SCHEMA_VERSION);

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
        let has_thread: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'thread_id'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_thread);
        let has_near: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'near_dup_role'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_near);
        let has_cull: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'cull_status'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_cull);
    }
}
