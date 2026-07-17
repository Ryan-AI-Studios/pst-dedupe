//! Matter directory layout and high-level store API.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::audit::{self, AuditEvent, AuditEventInput};
use crate::cas::Cas;
use crate::error::{Error, Result};
use crate::item_errors::{self, ItemError, ItemErrorInput};
use crate::jobs::{self, Job, JobCheckpoint, JobState};
use crate::schema::{self, SCHEMA_VERSION};

/// Database filename under the matter root.
pub const DB_FILE: &str = "matter.db";

/// Reserved directory for Tantivy full-text index (track 0029).
pub const INDEX_DIR: &str = "index";
/// Reserved directory for production export sets (track 0040).
pub const EXPORTS_DIR: &str = "exports";
/// Optional directory for file logs.
pub const LOGS_DIR: &str = "logs";

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Metadata row for the matter itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatterInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub schema_version: u32,
    pub storage_root: String,
}

/// A source path registered with the matter (Purview package, PST, ZIP, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub matter_id: String,
    pub path: String,
    pub kind: String,
    pub status: String,
    pub cursor_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Normalized item row (logical_hash reserved for later tracks).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub matter_id: String,
    pub source_id: Option<String>,
    pub family_id: Option<String>,
    pub path: Option<String>,
    pub native_sha256: Option<String>,
    pub logical_hash: Option<String>,
    pub message_id: Option<String>,
    pub status: String,
    pub size_bytes: Option<i64>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub imported_at: String,
}

/// Input for inserting a minimal item row (foundation for later extractors).
#[derive(Debug, Clone)]
pub struct ItemInput {
    pub id: Option<String>,
    pub source_id: Option<String>,
    pub family_id: Option<String>,
    pub path: Option<String>,
    pub native_sha256: Option<String>,
    pub logical_hash: Option<String>,
    pub message_id: Option<String>,
    pub status: String,
    pub size_bytes: Option<i64>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
}

/// An open matter: directory layout + SQLite connection + CAS handle.
pub struct Matter {
    root: Utf8PathBuf,
    conn: Connection,
    cas: Cas,
    matter_id: String,
}

impl Matter {
    /// Create a new matter at `root` with the given display `name`.
    ///
    /// Creates:
    /// ```text
    /// matter.db
    /// blobs/
    /// index/
    /// exports/
    /// logs/
    /// ```
    /// and applies schema migrations.
    pub fn create(root: impl AsRef<Utf8Path>, name: &str) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        if root.as_std_path().exists() {
            let db = root.join(DB_FILE);
            if db.as_std_path().exists() {
                return Err(Error::MatterAlreadyExists(root.to_string()));
            }
        }
        fs::create_dir_all(root.as_std_path())?;
        create_layout_dirs(&root)?;

        let db_path = root.join(DB_FILE);
        let conn = Connection::open(db_path.as_std_path())?;
        schema::configure_connection(&conn)?;
        schema::migrate(&conn)?;

        let now = now_rfc3339();
        let matter_id = new_id("mat");
        let storage_root = root.as_str().to_string();

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![matter_id, name, now, SCHEMA_VERSION, storage_root],
        )?;

        let cas = Cas::new(&root);
        cas.ensure_layout()?;

        let matter = Self {
            root,
            conn,
            cas,
            matter_id: matter_id.clone(),
        };

        // First audit event: matter created.
        let _ = matter.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "matter.create".into(),
            entity: format!("matter:{matter_id}"),
            params_json: serde_json::json!({ "name": name }).to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;

        Ok(matter)
    }

    /// Open an existing matter at `root`.
    ///
    /// Applies any pending migrations.
    pub fn open(root: impl AsRef<Utf8Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        if !root.as_std_path().exists() {
            return Err(Error::MatterNotFound(root.to_string()));
        }
        let db_path = root.join(DB_FILE);
        if !db_path.as_std_path().exists() {
            return Err(Error::DatabaseMissing(root.to_string()));
        }

        let conn = Connection::open(db_path.as_std_path())?;
        schema::configure_connection(&conn)?;
        schema::migrate(&conn)?;

        let matter_id: String = conn
            .query_row("SELECT id FROM matters LIMIT 1", [], |row| row.get(0))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::MatterRowMissing,
                other => Error::Sqlite(other),
            })?;

        // Ensure reserved dirs exist (idempotent).
        create_layout_dirs(&root)?;
        let cas = Cas::new(&root);
        cas.ensure_layout()?;

        Ok(Self {
            root,
            conn,
            cas,
            matter_id,
        })
    }

    /// Matter root directory.
    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    /// Matter id stored in the DB.
    pub fn id(&self) -> &str {
        &self.matter_id
    }

    /// Current schema version for this matter.
    pub fn schema_version(&self) -> Result<u32> {
        schema::read_schema_version(&self.conn)
    }

    /// Load matter metadata.
    pub fn info(&self) -> Result<MatterInfo> {
        self.conn
            .query_row(
                "SELECT id, name, created_at, schema_version, storage_root FROM matters WHERE id = ?1",
                params![self.matter_id],
                |row| {
                    Ok(MatterInfo {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        created_at: row.get(2)?,
                        schema_version: row.get(3)?,
                        storage_root: row.get(4)?,
                    })
                },
            )
            .map_err(Error::from)
    }

    /// Borrow the SQLite connection (for advanced callers / verify helpers).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Content-addressable blob store handle.
    pub fn cas(&self) -> &Cas {
        &self.cas
    }

    // --- CAS convenience ---

    /// Put raw physical bytes into CAS; returns lowercase SHA-256 hex.
    pub fn put_bytes(&self, data: &[u8]) -> Result<String> {
        self.cas.put_bytes(data)
    }

    /// Get raw bytes by SHA-256 hex digest.
    pub fn get_bytes(&self, digest_hex: &str) -> Result<Vec<u8>> {
        self.cas.get_bytes(digest_hex)
    }

    /// Whether a blob with this digest exists.
    pub fn blob_exists(&self, digest_hex: &str) -> Result<bool> {
        self.cas.exists(digest_hex)
    }

    // --- Jobs / checkpoints ---

    /// Create a new job in `pending` state. Returns the job id.
    pub fn create_job(&self, kind: &str) -> Result<Job> {
        let id = new_id("job");
        let now = now_rfc3339();
        let job = jobs::create_job(&self.conn, &id, &self.matter_id, kind, &now)?;
        let _ = self.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "job.create".into(),
            entity: format!("job:{id}"),
            params_json: serde_json::json!({ "kind": kind }).to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
        Ok(job)
    }

    /// Load a job by id.
    pub fn get_job(&self, job_id: &str) -> Result<Job> {
        jobs::get_job(&self.conn, job_id)
    }

    /// Transition a job to a new state.
    pub fn set_job_state(
        &self,
        job_id: &str,
        state: JobState,
        error_summary: Option<&str>,
    ) -> Result<Job> {
        let now = now_rfc3339();
        let job = jobs::set_job_state(&self.conn, job_id, state, &now, error_summary)?;
        let _ = self.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "job.state".into(),
            entity: format!("job:{job_id}"),
            params_json: serde_json::json!({
                "state": state.as_str(),
                "error_summary": error_summary,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
        Ok(job)
    }

    /// Write (upsert) a checkpoint for `job_id` + `stage`.
    ///
    /// `cursor_json` is opaque to matter-core.
    pub fn put_checkpoint(
        &self,
        job_id: &str,
        stage: &str,
        cursor_json: &str,
        completed_count: i64,
    ) -> Result<JobCheckpoint> {
        let now = now_rfc3339();
        jobs::put_checkpoint(
            &self.conn,
            job_id,
            stage,
            cursor_json,
            completed_count,
            &now,
        )
    }

    /// Load the latest checkpoint for `job_id` + `stage`.
    pub fn get_checkpoint(&self, job_id: &str, stage: &str) -> Result<Option<JobCheckpoint>> {
        jobs::get_checkpoint(&self.conn, job_id, stage)
    }

    // --- Sources / items (minimal foundation) ---

    /// Register a source path.
    pub fn insert_source(
        &self,
        path: &str,
        kind: &str,
        status: &str,
        cursor_json: Option<&str>,
    ) -> Result<Source> {
        let id = new_id("src");
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO sources (id, matter_id, path, kind, status, cursor_json, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                id,
                self.matter_id,
                path,
                kind,
                status,
                cursor_json,
                now
            ],
        )?;
        self.get_source(&id)
    }

    /// Load a source by id.
    pub fn get_source(&self, source_id: &str) -> Result<Source> {
        self.conn
            .query_row(
                "SELECT id, matter_id, path, kind, status, cursor_json, created_at, updated_at \
                 FROM sources WHERE id = ?1",
                params![source_id],
                |row| {
                    Ok(Source {
                        id: row.get(0)?,
                        matter_id: row.get(1)?,
                        path: row.get(2)?,
                        kind: row.get(3)?,
                        status: row.get(4)?,
                        cursor_json: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::SourceNotFound(source_id.to_string())
                }
                other => Error::Sqlite(other),
            })
    }

    /// Insert a normalized item row.
    pub fn insert_item(&self, input: ItemInput) -> Result<Item> {
        let id = input.id.clone().unwrap_or_else(|| new_id("itm"));
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO items (\
                id, matter_id, source_id, family_id, path, native_sha256, logical_hash, \
                message_id, status, size_bytes, created_at, modified_at, imported_at\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                id,
                self.matter_id,
                input.source_id,
                input.family_id,
                input.path,
                input.native_sha256,
                input.logical_hash,
                input.message_id,
                input.status,
                input.size_bytes,
                input.created_at,
                input.modified_at,
                now,
            ],
        )?;
        self.get_item(&id)
    }

    /// Load an item by id.
    pub fn get_item(&self, item_id: &str) -> Result<Item> {
        self.conn
            .query_row(
                "SELECT id, matter_id, source_id, family_id, path, native_sha256, logical_hash, \
                        message_id, status, size_bytes, created_at, modified_at, imported_at \
                 FROM items WHERE id = ?1",
                params![item_id],
                |row| {
                    Ok(Item {
                        id: row.get(0)?,
                        matter_id: row.get(1)?,
                        source_id: row.get(2)?,
                        family_id: row.get(3)?,
                        path: row.get(4)?,
                        native_sha256: row.get(5)?,
                        logical_hash: row.get(6)?,
                        message_id: row.get(7)?,
                        status: row.get(8)?,
                        size_bytes: row.get(9)?,
                        created_at: row.get(10)?,
                        modified_at: row.get(11)?,
                        imported_at: row.get(12)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::ItemNotFound(item_id.to_string()),
                other => Error::Sqlite(other),
            })
    }

    // --- Item errors ---

    /// Record an item-level error without removing parent items.
    pub fn record_item_error(&self, input: ItemErrorInput) -> Result<ItemError> {
        let now = now_rfc3339();
        item_errors::record(&self.conn, &input, &now)
    }

    /// Errors for a specific item.
    pub fn item_errors_for_item(&self, item_id: &str) -> Result<Vec<ItemError>> {
        item_errors::for_item(&self.conn, item_id)
    }

    /// Errors for a source.
    pub fn item_errors_for_source(&self, source_id: &str) -> Result<Vec<ItemError>> {
        item_errors::for_source(&self.conn, source_id)
    }

    /// Errors for a job.
    pub fn item_errors_for_job(&self, job_id: &str) -> Result<Vec<ItemError>> {
        item_errors::for_job(&self.conn, job_id)
    }

    // --- Audit ---

    /// Append an audit event (hash chain linked).
    pub fn append_audit(&self, input: AuditEventInput) -> Result<AuditEvent> {
        let ts = now_rfc3339();
        audit::append_event(&self.conn, &input, &ts)
    }

    /// Verify the full audit hash chain.
    pub fn verify_audit_chain(&self) -> Result<()> {
        audit::verify_audit_chain(&self.conn)
    }
}

fn create_layout_dirs(root: &Utf8Path) -> Result<()> {
    for dir in [INDEX_DIR, EXPORTS_DIR, LOGS_DIR] {
        fs::create_dir_all(root.join(dir).as_std_path())?;
    }
    // blobs/ created via Cas::ensure_layout
    Ok(())
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn new_id(prefix: &str) -> String {
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}_{nanos:x}_{n:x}")
}
