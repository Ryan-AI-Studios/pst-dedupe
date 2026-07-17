//! Matter directory layout and high-level store API.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
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
/// Matter-local workspace for extractor spill (never OS `%TEMP%` for evidence).
pub const WORKSPACE_DIR: &str = "workspace";
/// Temporary materialization under the matter root (`workspace/temp/`).
pub const WORKSPACE_TEMP_DIR: &str = "temp";

/// Default family kind for email parent + attachment children.
pub const FAMILY_KIND_EMAIL_ATTACHMENTS: &str = "email_attachments";

/// Stable item lifecycle / processing status strings.
///
/// 0016 inventory uses `discovered` / `expanded` / `error`. Later extractors
/// may set `normalized`, `extracted`, or `partial`. APIs accept any string;
/// these constants document the recommended vocabulary.
pub mod item_status {
    pub const DISCOVERED: &str = "discovered";
    pub const EXPANDED: &str = "expanded";
    pub const ERROR: &str = "error";
    pub const NORMALIZED: &str = "normalized";
    pub const EXTRACTED: &str = "extracted";
    pub const PARTIAL: &str = "partial";
}

/// Item role within a family (or standalone).
pub mod item_role {
    pub const STANDALONE: &str = "standalone";
    pub const PARENT: &str = "parent";
    pub const ATTACHMENT: &str = "attachment";
}

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

/// An item family (e.g. email + attachments).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemFamily {
    pub id: String,
    pub matter_id: String,
    pub kind: String,
    pub created_at: String,
}

/// Normalized item row (schema v2 P0 fields).
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
    // --- schema v2 ---
    pub role: Option<String>,
    pub parent_item_id: Option<String>,
    pub mime_type: Option<String>,
    pub file_category: Option<String>,
    pub custodian: Option<String>,
    pub subject: Option<String>,
    pub title: Option<String>,
    pub from_addr: Option<String>,
    pub to_addrs_json: Option<String>,
    pub cc_addrs_json: Option<String>,
    pub bcc_addrs_json: Option<String>,
    pub author: Option<String>,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub attachment_count: Option<i64>,
    pub text_sha256: Option<String>,
    pub html_sha256: Option<String>,
    /// Algorithm version used for `logical_hash` (0 = not computed).
    pub logical_hash_version: u32,
    pub extra_json: Option<String>,
}

/// Input for inserting an item row. New P0 fields are optional (null-safe).
///
/// When `role` is `None`, insert stores [`item_role::STANDALONE`].
/// When `logical_hash_version` is `None`, insert stores `0`.
#[derive(Debug, Clone, Default)]
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
    // --- schema v2 ---
    pub role: Option<String>,
    pub parent_item_id: Option<String>,
    pub mime_type: Option<String>,
    pub file_category: Option<String>,
    pub custodian: Option<String>,
    pub subject: Option<String>,
    pub title: Option<String>,
    pub from_addr: Option<String>,
    pub to_addrs_json: Option<String>,
    pub cc_addrs_json: Option<String>,
    pub bcc_addrs_json: Option<String>,
    pub author: Option<String>,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub attachment_count: Option<i64>,
    pub text_sha256: Option<String>,
    pub html_sha256: Option<String>,
    pub logical_hash_version: Option<u32>,
    pub extra_json: Option<String>,
}

/// Partial update for an existing item.
///
/// Nested-`Option` contract (matches README / `apply_opt2`):
/// - **Outer `None`** — leave the column unchanged.
/// - **`Some(None)`** — set the column to SQL NULL (clear the field).
/// - **`Some(Some(v))`** — set the column to `v`.
///
/// Exceptions: `status` and `logical_hash_version` are plain `Option` (required /
/// non-null columns) — `None` leaves unchanged, `Some(v)` sets `v`.
///
/// 0018 extractors typically only set fields they know (`Some(Some(...))`) and
/// leave the rest as outer `None`.
#[derive(Debug, Clone, Default)]
pub struct ItemUpdate {
    pub source_id: Option<Option<String>>,
    pub family_id: Option<Option<String>>,
    pub path: Option<Option<String>>,
    pub native_sha256: Option<Option<String>>,
    pub logical_hash: Option<Option<String>>,
    pub message_id: Option<Option<String>>,
    pub status: Option<String>,
    pub size_bytes: Option<Option<i64>>,
    pub created_at: Option<Option<String>>,
    pub modified_at: Option<Option<String>>,
    pub role: Option<Option<String>>,
    pub parent_item_id: Option<Option<String>>,
    pub mime_type: Option<Option<String>>,
    pub file_category: Option<Option<String>>,
    pub custodian: Option<Option<String>>,
    pub subject: Option<Option<String>>,
    pub title: Option<Option<String>>,
    pub from_addr: Option<Option<String>>,
    pub to_addrs_json: Option<Option<String>>,
    pub cc_addrs_json: Option<Option<String>>,
    pub bcc_addrs_json: Option<Option<String>>,
    pub author: Option<Option<String>>,
    pub sent_at: Option<Option<String>>,
    pub received_at: Option<Option<String>>,
    pub attachment_count: Option<Option<i64>>,
    pub text_sha256: Option<Option<String>>,
    pub html_sha256: Option<Option<String>>,
    pub logical_hash_version: Option<u32>,
    pub extra_json: Option<Option<String>>,
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
    /// workspace/temp/
    /// ```
    /// and applies schema migrations. Cleans any leftover `workspace/temp/`
    /// contents (idempotent empty wipe).
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
        matter.cleanup_workspace_temp()?;

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
    /// Applies any pending migrations and removes leftover files under
    /// `workspace/temp/` (crash residue from prior extract materializations).
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

        let matter = Self {
            root,
            conn,
            cas,
            matter_id,
        };
        matter.cleanup_workspace_temp()?;
        Ok(matter)
    }

    /// Path to `workspace/temp/` under this matter root.
    pub fn workspace_temp_dir(&self) -> Utf8PathBuf {
        self.root.join(WORKSPACE_DIR).join(WORKSPACE_TEMP_DIR)
    }

    /// Recursively delete **contents** of `workspace/temp/` (keeps the directory).
    ///
    /// Used on create/open so crash residue (e.g. CAS-materialized PSTs) cannot
    /// accumulate under the matter. Best-effort: I/O errors surface as
    /// [`Error::Io`].
    pub fn cleanup_workspace_temp(&self) -> Result<()> {
        let temp = self.workspace_temp_dir();
        fs::create_dir_all(temp.as_std_path())?;
        for entry in fs::read_dir(temp.as_std_path())? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
        Ok(())
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

    /// Stream raw physical bytes into CAS (bounded buffer; no full `Vec` required).
    pub fn put_reader<R: std::io::Read>(&self, reader: &mut R) -> Result<String> {
        self.cas.put_reader(reader)
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

    // --- Sources / items ---

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
                map_source_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::SourceNotFound(source_id.to_string())
                }
                other => Error::Sqlite(other),
            })
    }

    /// Update source `status` and `cursor_json` (resume metadata mirror).
    ///
    /// `cursor_json: None` stores SQL NULL. Callers that only change status
    /// should re-pass the existing cursor from [`Self::get_source`].
    pub fn update_source(
        &self,
        source_id: &str,
        status: &str,
        cursor_json: Option<&str>,
    ) -> Result<Source> {
        // Ensure source exists first.
        let _ = self.get_source(source_id)?;
        let now = now_rfc3339();
        self.conn.execute(
            "UPDATE sources SET status = ?1, cursor_json = ?2, updated_at = ?3 WHERE id = ?4",
            params![status, cursor_json, now, source_id],
        )?;
        self.get_source(source_id)
    }

    /// Insert a normalized item row (schema v2 fields null-safe).
    pub fn insert_item(&self, input: ItemInput) -> Result<Item> {
        let id = input.id.clone().unwrap_or_else(|| new_id("itm"));
        let now = now_rfc3339();
        let role = input
            .role
            .clone()
            .unwrap_or_else(|| item_role::STANDALONE.to_string());
        let logical_hash_version = input.logical_hash_version.unwrap_or(0);

        // App-level parent existence, same-matter, and family_id cohesion when set.
        let mut family_id = input.family_id;
        if let Some(ref parent_id) = input.parent_item_id {
            let parent = self
                .get_item(parent_id)
                .map_err(|_| Error::ParentItemNotFound(parent_id.clone()))?;
            if parent.matter_id != self.matter_id {
                return Err(Error::CrossMatterFamily(format!(
                    "parent {parent_id} belongs to matter {}",
                    parent.matter_id
                )));
            }
            family_id = resolve_family_with_parent(&parent, family_id)?;
        }
        if let Some(ref fid) = family_id {
            let fam = self.get_family(fid)?;
            if fam.matter_id != self.matter_id {
                return Err(Error::CrossMatterFamily(format!(
                    "family {fid} belongs to matter {}",
                    fam.matter_id
                )));
            }
        }

        self.conn.execute(
            "INSERT INTO items (\
                id, matter_id, source_id, family_id, path, native_sha256, logical_hash, \
                message_id, status, size_bytes, created_at, modified_at, imported_at, \
                role, parent_item_id, mime_type, file_category, custodian, subject, title, \
                from_addr, to_addrs_json, cc_addrs_json, bcc_addrs_json, author, \
                sent_at, received_at, attachment_count, text_sha256, html_sha256, \
                logical_hash_version, extra_json\
             ) VALUES (\
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, \
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, \
                ?26, ?27, ?28, ?29, ?30, ?31, ?32\
             )",
            params![
                id,
                self.matter_id,
                input.source_id,
                family_id,
                input.path,
                input.native_sha256,
                input.logical_hash,
                input.message_id,
                input.status,
                input.size_bytes,
                input.created_at,
                input.modified_at,
                now,
                role,
                input.parent_item_id,
                input.mime_type,
                input.file_category,
                input.custodian,
                input.subject,
                input.title,
                input.from_addr,
                input.to_addrs_json,
                input.cc_addrs_json,
                input.bcc_addrs_json,
                input.author,
                input.sent_at,
                input.received_at,
                input.attachment_count,
                input.text_sha256,
                input.html_sha256,
                logical_hash_version,
                input.extra_json,
            ],
        )?;

        if let Some(ref parent_id) = input.parent_item_id {
            self.recompute_attachment_count(parent_id)?;
        }

        self.get_item(&id)
    }

    /// Partially update an item. See [`ItemUpdate`] for Option semantics.
    ///
    /// Silent on audit by default (high-volume extractors may batch audit themselves).
    pub fn update_item(&self, item_id: &str, update: ItemUpdate) -> Result<Item> {
        let current = self.get_item(item_id)?;
        let old_parent = current.parent_item_id.clone();

        let source_id = apply_opt2(update.source_id, current.source_id);
        let family_id = apply_opt2(update.family_id, current.family_id);
        let path = apply_opt2(update.path, current.path);
        let native_sha256 = apply_opt2(update.native_sha256, current.native_sha256);
        let logical_hash = apply_opt2(update.logical_hash, current.logical_hash);
        let message_id = apply_opt2(update.message_id, current.message_id);
        let status = update.status.unwrap_or(current.status);
        let size_bytes = apply_opt2(update.size_bytes, current.size_bytes);
        let created_at = apply_opt2(update.created_at, current.created_at);
        let modified_at = apply_opt2(update.modified_at, current.modified_at);
        let role = apply_opt2(update.role, current.role);
        let parent_item_id = apply_opt2(update.parent_item_id, current.parent_item_id);
        let mime_type = apply_opt2(update.mime_type, current.mime_type);
        let file_category = apply_opt2(update.file_category, current.file_category);
        let custodian = apply_opt2(update.custodian, current.custodian);
        let subject = apply_opt2(update.subject, current.subject);
        let title = apply_opt2(update.title, current.title);
        let from_addr = apply_opt2(update.from_addr, current.from_addr);
        let to_addrs_json = apply_opt2(update.to_addrs_json, current.to_addrs_json);
        let cc_addrs_json = apply_opt2(update.cc_addrs_json, current.cc_addrs_json);
        let bcc_addrs_json = apply_opt2(update.bcc_addrs_json, current.bcc_addrs_json);
        let author = apply_opt2(update.author, current.author);
        let sent_at = apply_opt2(update.sent_at, current.sent_at);
        let received_at = apply_opt2(update.received_at, current.received_at);
        let attachment_count = apply_opt2(update.attachment_count, current.attachment_count);
        let text_sha256 = apply_opt2(update.text_sha256, current.text_sha256);
        let html_sha256 = apply_opt2(update.html_sha256, current.html_sha256);
        let logical_hash_version = update
            .logical_hash_version
            .unwrap_or(current.logical_hash_version);
        let extra_json = apply_opt2(update.extra_json, current.extra_json);

        let mut family_id = family_id;
        if let Some(ref parent_id) = parent_item_id {
            if parent_id != item_id {
                let parent = self
                    .get_item(parent_id)
                    .map_err(|_| Error::ParentItemNotFound(parent_id.clone()))?;
                if parent.matter_id != self.matter_id {
                    return Err(Error::CrossMatterFamily(format!(
                        "parent {parent_id} belongs to matter {}",
                        parent.matter_id
                    )));
                }
                family_id = resolve_family_with_parent(&parent, family_id)?;
            }
        }
        if let Some(ref fid) = family_id {
            let fam = self.get_family(fid)?;
            if fam.matter_id != self.matter_id {
                return Err(Error::CrossMatterFamily(format!(
                    "family {fid} belongs to matter {}",
                    fam.matter_id
                )));
            }
        }

        self.conn.execute(
            "UPDATE items SET \
                source_id = ?1, family_id = ?2, path = ?3, native_sha256 = ?4, \
                logical_hash = ?5, message_id = ?6, status = ?7, size_bytes = ?8, \
                created_at = ?9, modified_at = ?10, role = ?11, parent_item_id = ?12, \
                mime_type = ?13, file_category = ?14, custodian = ?15, subject = ?16, \
                title = ?17, from_addr = ?18, to_addrs_json = ?19, cc_addrs_json = ?20, \
                bcc_addrs_json = ?21, author = ?22, sent_at = ?23, received_at = ?24, \
                attachment_count = ?25, text_sha256 = ?26, html_sha256 = ?27, \
                logical_hash_version = ?28, extra_json = ?29 \
             WHERE id = ?30",
            params![
                source_id,
                family_id,
                path,
                native_sha256,
                logical_hash,
                message_id,
                status,
                size_bytes,
                created_at,
                modified_at,
                role,
                parent_item_id,
                mime_type,
                file_category,
                custodian,
                subject,
                title,
                from_addr,
                to_addrs_json,
                cc_addrs_json,
                bcc_addrs_json,
                author,
                sent_at,
                received_at,
                attachment_count,
                text_sha256,
                html_sha256,
                logical_hash_version,
                extra_json,
                item_id,
            ],
        )?;

        if old_parent != parent_item_id {
            if let Some(ref old) = old_parent {
                self.recompute_attachment_count(old)?;
            }
            if let Some(ref new_parent) = parent_item_id {
                if new_parent != item_id {
                    self.recompute_attachment_count(new_parent)?;
                }
            }
        }

        self.get_item(item_id)
    }

    /// Load an item by id.
    pub fn get_item(&self, item_id: &str) -> Result<Item> {
        self.conn
            .query_row(
                &item_select_sql("WHERE id = ?1"),
                params![item_id],
                map_item_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Error::ItemNotFound(item_id.to_string()),
                other => Error::Sqlite(other),
            })
    }

    /// Lookup the first inventory item for `(source_id, path)`.
    ///
    /// Used by ingest resume to skip already-CAS'd logical paths. No unique
    /// index yet; callers must still avoid double-inserts.
    pub fn item_by_source_path(&self, source_id: &str, path: &str) -> Result<Option<Item>> {
        self.conn
            .query_row(
                &item_select_sql(
                    "WHERE source_id = ?1 AND path = ?2 ORDER BY imported_at ASC LIMIT 1",
                ),
                params![source_id, path],
                map_item_row,
            )
            .optional()
            .map_err(Error::from)
    }

    /// List all items registered under a source (inventory / resume helpers).
    pub fn list_items_for_source(&self, source_id: &str) -> Result<Vec<Item>> {
        let mut stmt = self.conn.prepare(&item_select_sql(
            "WHERE source_id = ?1 ORDER BY imported_at ASC, id ASC",
        ))?;
        let rows = stmt.query_map(params![source_id], map_item_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Items sharing a logical hash (prep for 0021 dedupe).
    pub fn items_by_logical_hash(&self, logical_hash: &str) -> Result<Vec<Item>> {
        let mut stmt = self.conn.prepare(&item_select_sql(
            "WHERE logical_hash = ?1 ORDER BY imported_at ASC, id ASC",
        ))?;
        let rows = stmt.query_map(params![logical_hash], map_item_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Items sharing a message_id (prep for 0021 dedupe).
    pub fn items_by_message_id(&self, message_id: &str) -> Result<Vec<Item>> {
        let mut stmt = self.conn.prepare(&item_select_sql(
            "WHERE message_id = ?1 ORDER BY imported_at ASC, id ASC",
        ))?;
        let rows = stmt.query_map(params![message_id], map_item_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // --- Family graph ---

    /// Create an item family. Empty `kind` defaults to [`FAMILY_KIND_EMAIL_ATTACHMENTS`].
    pub fn insert_family(&self, kind: &str) -> Result<ItemFamily> {
        let kind = if kind.is_empty() {
            FAMILY_KIND_EMAIL_ATTACHMENTS
        } else {
            kind
        };
        let id = new_id("fam");
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO item_families (id, matter_id, kind, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, self.matter_id, kind, now],
        )?;

        let _ = self.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "family.create".into(),
            entity: format!("family:{id}"),
            params_json: serde_json::json!({ "kind": kind }).to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;

        self.get_family(&id)
    }

    /// Load a family by id.
    pub fn get_family(&self, family_id: &str) -> Result<ItemFamily> {
        self.conn
            .query_row(
                "SELECT id, matter_id, kind, created_at FROM item_families WHERE id = ?1",
                params![family_id],
                map_family_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::FamilyNotFound(family_id.to_string())
                }
                other => Error::Sqlite(other),
            })
    }

    /// List all items that belong to a family.
    pub fn list_family_members(&self, family_id: &str) -> Result<Vec<Item>> {
        // Ensure family exists.
        let _ = self.get_family(family_id)?;
        let mut stmt = self.conn.prepare(&item_select_sql(
            "WHERE family_id = ?1 ORDER BY imported_at ASC, id ASC",
        ))?;
        let rows = stmt.query_map(params![family_id], map_item_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Set `family_id`, `role`, and `parent_item_id` together.
    ///
    /// When `parent_item_id` is set, the parent must exist, share this matter,
    /// and share (or supply) the child's `family_id`. If `family_id` is omitted
    /// but the parent belongs to a family, the child inherits that family.
    /// On parent link / reparent / clear, both the previous and new parents have
    /// their denormalized `attachment_count` recomputed from children.
    pub fn set_item_family_role(
        &self,
        item_id: &str,
        family_id: Option<&str>,
        role: &str,
        parent_item_id: Option<&str>,
    ) -> Result<Item> {
        let current = self.get_item(item_id)?;
        let old_parent = current.parent_item_id.clone();

        let mut resolved_family = family_id.map(|s| s.to_string());

        if let Some(pid) = parent_item_id {
            let parent = self
                .get_item(pid)
                .map_err(|_| Error::ParentItemNotFound(pid.to_string()))?;
            if parent.matter_id != self.matter_id {
                return Err(Error::CrossMatterFamily(format!(
                    "parent {pid} belongs to matter {}",
                    parent.matter_id
                )));
            }
            resolved_family = resolve_family_with_parent(&parent, resolved_family)?;
        }

        if let Some(ref fid) = resolved_family {
            let fam = self.get_family(fid)?;
            if fam.matter_id != self.matter_id {
                return Err(Error::CrossMatterFamily(format!(
                    "family {fid} belongs to matter {}",
                    fam.matter_id
                )));
            }
        }

        self.conn.execute(
            "UPDATE items SET family_id = ?1, role = ?2, parent_item_id = ?3 WHERE id = ?4",
            params![resolved_family, role, parent_item_id, item_id],
        )?;

        let old_as_str = old_parent.as_deref();
        if old_as_str != parent_item_id {
            if let Some(old) = old_as_str {
                self.recompute_attachment_count(old)?;
            }
            if let Some(pid) = parent_item_id {
                self.recompute_attachment_count(pid)?;
            }
        } else if let Some(pid) = parent_item_id {
            // Same parent reaffirmed (e.g. role-only update with parent still set):
            // still recompute so callers can repair a stale count.
            self.recompute_attachment_count(pid)?;
        }

        self.get_item(item_id)
    }

    /// List direct attachment children of a parent item.
    pub fn list_attachments(&self, parent_item_id: &str) -> Result<Vec<Item>> {
        let mut stmt = self.conn.prepare(&item_select_sql(
            "WHERE parent_item_id = ?1 ORDER BY imported_at ASC, id ASC",
        ))?;
        let rows = stmt.query_map(params![parent_item_id], map_item_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Resolve the parent item for a child (via `parent_item_id`).
    pub fn get_parent(&self, child_id: &str) -> Result<Option<Item>> {
        let child = self.get_item(child_id)?;
        match child.parent_item_id {
            Some(ref pid) => self.get_item(pid).map(Some),
            None => Ok(None),
        }
    }

    fn recompute_attachment_count(&self, parent_id: &str) -> Result<()> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM items WHERE parent_item_id = ?1",
            params![parent_id],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "UPDATE items SET attachment_count = ?1 WHERE id = ?2",
            params![count, parent_id],
        )?;
        Ok(())
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
    for dir in [INDEX_DIR, EXPORTS_DIR, LOGS_DIR, WORKSPACE_DIR] {
        fs::create_dir_all(root.join(dir).as_std_path())?;
    }
    fs::create_dir_all(
        root.join(WORKSPACE_DIR)
            .join(WORKSPACE_TEMP_DIR)
            .as_std_path(),
    )?;
    // blobs/ created via Cas::ensure_layout
    Ok(())
}

/// Parent and child must share `family_id` (spec §3.3).
///
/// When the child has no `family_id` but the parent does, inherit the parent's.
/// Mismatches or a parent link with no family on either side are rejected.
fn resolve_family_with_parent(
    parent: &Item,
    child_family_id: Option<String>,
) -> Result<Option<String>> {
    match (child_family_id, parent.family_id.as_deref()) {
        (Some(fid), Some(p_fid)) if fid == p_fid => Ok(Some(fid)),
        (Some(fid), Some(p_fid)) => Err(Error::FamilyCohesion(format!(
            "parent item must share family_id with child (parent family {p_fid}, child family {fid})"
        ))),
        (Some(fid), None) => Err(Error::FamilyCohesion(format!(
            "parent item must share family_id with child (parent has no family, child family {fid})"
        ))),
        (None, Some(p_fid)) => Ok(Some(p_fid.to_string())),
        (None, None) => Err(Error::FamilyCohesion(
            "parent item must share family_id with child \
             (neither has family_id; set family_id when linking parent)"
                .into(),
        )),
    }
}

fn map_source_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Source> {
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
}

fn map_family_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemFamily> {
    Ok(ItemFamily {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        kind: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        created_at: row.get(3)?,
    })
}

const ITEM_COLUMNS: &str =
    "id, matter_id, source_id, family_id, path, native_sha256, logical_hash, \
    message_id, status, size_bytes, created_at, modified_at, imported_at, \
    role, parent_item_id, mime_type, file_category, custodian, subject, title, \
    from_addr, to_addrs_json, cc_addrs_json, bcc_addrs_json, author, \
    sent_at, received_at, attachment_count, text_sha256, html_sha256, \
    logical_hash_version, extra_json";

fn item_select_sql(suffix: &str) -> String {
    format!("SELECT {ITEM_COLUMNS} FROM items {suffix}")
}

fn map_item_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
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
        role: row.get(13)?,
        parent_item_id: row.get(14)?,
        mime_type: row.get(15)?,
        file_category: row.get(16)?,
        custodian: row.get(17)?,
        subject: row.get(18)?,
        title: row.get(19)?,
        from_addr: row.get(20)?,
        to_addrs_json: row.get(21)?,
        cc_addrs_json: row.get(22)?,
        bcc_addrs_json: row.get(23)?,
        author: row.get(24)?,
        sent_at: row.get(25)?,
        received_at: row.get(26)?,
        attachment_count: row.get(27)?,
        text_sha256: row.get(28)?,
        html_sha256: row.get(29)?,
        logical_hash_version: row.get::<_, i64>(30)? as u32,
        extra_json: row.get(31)?,
    })
}

/// Apply nested Option update: outer None = leave, Some(v) = set to v (including None = SQL NULL).
fn apply_opt2<T>(update: Option<Option<T>>, current: Option<T>) -> Option<T> {
    match update {
        None => current,
        Some(v) => v,
    }
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
