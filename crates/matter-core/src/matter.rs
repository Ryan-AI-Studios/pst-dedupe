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

/// Matter-level dedupe role on items (schema v3 / track 0021).
pub mod item_dedup_role {
    pub const UNIQUE: &str = "unique";
    pub const DUPLICATE: &str = "duplicate";
    pub const SKIPPED: &str = "skipped";
}

/// Tier that assigned the dedupe role (schema v3 / track 0021).
pub mod item_dedup_tier {
    pub const MESSAGE_ID: &str = "message_id";
    pub const LOGICAL_HASH: &str = "logical_hash";
    pub const FAMILY: &str = "family";
    pub const NONE: &str = "none";
}

/// How a `thread_id` was assigned (schema v4 / track 0022).
pub mod item_thread_method {
    pub const HEADERS: &str = "headers";
    pub const SUBJECT: &str = "subject";
    pub const CONVERSATION_INDEX: &str = "conversation_index";
    pub const SINGLETON: &str = "singleton";
    pub const NONE: &str = "none";
}

/// Near-duplicate role on items (schema v5 / track 0023).
pub mod item_near_dup_role {
    pub const PIVOT: &str = "pivot";
    pub const MEMBER: &str = "member";
    pub const UNIQUE: &str = "unique";
    pub const SKIPPED: &str = "skipped";
}

/// Cull / data-reduction disposition on items (schema v6 / track 0024).
///
/// Flag-only: never deletes items or CAS blobs.
pub mod item_cull_status {
    pub const INCLUDED: &str = "included";
    pub const CULLED: &str = "culled";
}

/// Default review-set display name (schema v7 / track 0025).
pub const DEFAULT_REVIEW_SET_NAME: &str = "Review Corpus";

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

/// Normalized item row (schema v2–v7: P0 + dedupe + thread + near-dup + cull + promote).
///
/// `PartialEq` only (not `Eq`) because `near_dup_similarity` is `f64`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    // --- schema v3 (dedupe) ---
    pub dedup_role: Option<String>,
    pub duplicate_of_item_id: Option<String>,
    pub dedup_tier: Option<String>,
    pub dedup_group_id: Option<String>,
    pub deduped_at: Option<String>,
    pub dedup_job_id: Option<String>,
    // --- schema v4 (threading) ---
    pub in_reply_to: Option<String>,
    pub references_json: Option<String>,
    pub conversation_topic: Option<String>,
    pub conversation_index_hex: Option<String>,
    pub thread_id: Option<String>,
    pub thread_root_item_id: Option<String>,
    pub thread_method: Option<String>,
    pub threaded_at: Option<String>,
    pub thread_job_id: Option<String>,
    // --- schema v5 (near-dup) ---
    pub near_dup_group_id: Option<String>,
    pub near_dup_role: Option<String>,
    pub near_dup_similarity: Option<f64>,
    pub near_dup_pivot_item_id: Option<String>,
    pub near_dup_method: Option<String>,
    pub near_duped_at: Option<String>,
    pub near_dup_job_id: Option<String>,
    // --- schema v6 (cull) ---
    pub cull_status: Option<String>,
    pub cull_reasons_json: Option<String>,
    pub cull_preset_id: Option<String>,
    pub cull_preset_name: Option<String>,
    pub culled_at: Option<String>,
    pub cull_job_id: Option<String>,
    // --- schema v7 (promote / review set) ---
    /// 0/1 membership flag; NULL = never promoted.
    pub in_review: Option<i64>,
    pub review_set_id: Option<String>,
    pub review_order: Option<i64>,
    pub promoted_at: Option<String>,
    pub promote_job_id: Option<String>,
    pub promote_policy: Option<String>,
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
    // --- schema v3 (dedupe) ---
    pub dedup_role: Option<String>,
    pub duplicate_of_item_id: Option<String>,
    pub dedup_tier: Option<String>,
    pub dedup_group_id: Option<String>,
    pub deduped_at: Option<String>,
    pub dedup_job_id: Option<String>,
    // --- schema v4 (threading) ---
    pub in_reply_to: Option<String>,
    pub references_json: Option<String>,
    pub conversation_topic: Option<String>,
    pub conversation_index_hex: Option<String>,
    pub thread_id: Option<String>,
    pub thread_root_item_id: Option<String>,
    pub thread_method: Option<String>,
    pub threaded_at: Option<String>,
    pub thread_job_id: Option<String>,
    // --- schema v5 (near-dup) ---
    pub near_dup_group_id: Option<String>,
    pub near_dup_role: Option<String>,
    pub near_dup_similarity: Option<f64>,
    pub near_dup_pivot_item_id: Option<String>,
    pub near_dup_method: Option<String>,
    pub near_duped_at: Option<String>,
    pub near_dup_job_id: Option<String>,
    // --- schema v6 (cull) — usually left null on insert; set by cull job ---
    pub cull_status: Option<String>,
    pub cull_reasons_json: Option<String>,
    pub cull_preset_id: Option<String>,
    pub cull_preset_name: Option<String>,
    pub culled_at: Option<String>,
    pub cull_job_id: Option<String>,
    // --- schema v7 (promote) — usually left null on insert; set by promote job ---
    pub in_review: Option<i64>,
    pub review_set_id: Option<String>,
    pub review_order: Option<i64>,
    pub promoted_at: Option<String>,
    pub promote_job_id: Option<String>,
    pub promote_policy: Option<String>,
}

/// Thin row for matter-level email parent dedupe (no body text).
///
/// Identity columns only — safe for large-matter streaming / paging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupeCandidate {
    pub id: String,
    pub message_id: Option<String>,
    pub logical_hash: Option<String>,
    pub path: Option<String>,
    pub imported_at: String,
    pub role: Option<String>,
    pub file_category: Option<String>,
    pub status: String,
    pub dedup_role: Option<String>,
}

/// Thin row for matter-level email threading (no body text).
///
/// Header + order keys only — safe for large-matter streaming / paging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadCandidate {
    pub id: String,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references_json: Option<String>,
    pub subject: Option<String>,
    pub conversation_index_hex: Option<String>,
    pub path: Option<String>,
    pub imported_at: String,
    pub role: Option<String>,
    pub file_category: Option<String>,
    pub status: String,
    pub thread_id: Option<String>,
    pub parent_item_id: Option<String>,
}

/// Counts of items by `dedup_role` (NULL counted as `null_role`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupRoleCounts {
    pub unique: u64,
    pub duplicate: u64,
    pub skipped: u64,
    pub null_role: u64,
}

/// One item's dedupe field assignment for transactional batch write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupRoleUpdate {
    pub item_id: String,
    pub dedup_role: Option<String>,
    pub duplicate_of_item_id: Option<String>,
    pub dedup_tier: Option<String>,
    pub dedup_group_id: Option<String>,
    pub deduped_at: Option<String>,
    pub dedup_job_id: Option<String>,
    /// When set, replaces `extra_json` for the item (e.g. family_attach_unmatched).
    pub extra_json: Option<Option<String>>,
}

/// One item's thread field assignment for transactional batch write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadFieldUpdate {
    pub item_id: String,
    pub thread_id: Option<String>,
    pub thread_root_item_id: Option<String>,
    pub thread_method: Option<String>,
    pub threaded_at: Option<String>,
    pub thread_job_id: Option<String>,
}

/// Thin row for near-duplicate sketching (no body text).
///
/// Identity + text digest + order keys only — safe for large-matter paging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NearDupCandidate {
    pub id: String,
    pub text_sha256: Option<String>,
    pub dedup_role: Option<String>,
    pub path: Option<String>,
    pub imported_at: String,
    pub role: Option<String>,
    pub parent_item_id: Option<String>,
    pub status: String,
}

/// One item's near-dup field assignment for transactional batch write.
#[derive(Debug, Clone, PartialEq)]
pub struct NearDupFieldUpdate {
    pub item_id: String,
    pub near_dup_group_id: Option<String>,
    pub near_dup_role: Option<String>,
    pub near_dup_similarity: Option<f64>,
    pub near_dup_pivot_item_id: Option<String>,
    pub near_dup_method: Option<String>,
    pub near_duped_at: Option<String>,
    pub near_dup_job_id: Option<String>,
}

/// Thin row for cull evaluation (no body text).
///
/// Filter + family columns only — safe for large-matter streaming / paging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CullCandidate {
    pub id: String,
    pub parent_item_id: Option<String>,
    pub family_id: Option<String>,
    pub dedup_role: Option<String>,
    pub near_dup_role: Option<String>,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub path: Option<String>,
    pub custodian: Option<String>,
    pub file_category: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub status: String,
    pub native_sha256: Option<String>,
    pub text_sha256: Option<String>,
    pub role: Option<String>,
    pub imported_at: String,
}

/// One item's cull field assignment for transactional batch write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CullFieldUpdate {
    pub item_id: String,
    pub cull_status: Option<String>,
    pub cull_reasons_json: Option<String>,
    pub cull_preset_id: Option<String>,
    pub cull_preset_name: Option<String>,
    pub culled_at: Option<String>,
    pub cull_job_id: Option<String>,
}

/// Thin row for promote selection / family-aware ordering (no body text).
///
/// Columns needed for policy filters and the single-pass compound `ORDER BY`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromoteCandidate {
    pub id: String,
    pub parent_item_id: Option<String>,
    pub path: Option<String>,
    pub status: String,
    pub dedup_role: Option<String>,
    pub cull_status: Option<String>,
    pub role: Option<String>,
}

/// One item's promote / review-set membership assignment for batch write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromoteFieldUpdate {
    pub item_id: String,
    /// 0 = not in review, 1 = in review.
    pub in_review: Option<i64>,
    pub review_set_id: Option<String>,
    pub review_order: Option<i64>,
    pub promoted_at: Option<String>,
    pub promote_job_id: Option<String>,
    pub promote_policy: Option<String>,
}

/// Named review set (schema v7 / track 0025).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSet {
    pub id: String,
    pub matter_id: String,
    pub name: String,
    pub is_default: bool,
    pub policy: Option<String>,
    pub policy_json: Option<String>,
    pub item_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
}

/// Named cull preset stored per matter (schema v6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CullPreset {
    pub id: String,
    pub matter_id: String,
    pub name: String,
    pub description: Option<String>,
    pub rules_json: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
}

/// Input for inserting or updating a cull preset.
#[derive(Debug, Clone)]
pub struct CullPresetInput {
    /// When `Some`, update that id; when `None`, insert a new row.
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub rules_json: String,
    pub created_by: Option<String>,
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
    // --- schema v3 (dedupe) ---
    pub dedup_role: Option<Option<String>>,
    pub duplicate_of_item_id: Option<Option<String>>,
    pub dedup_tier: Option<Option<String>>,
    pub dedup_group_id: Option<Option<String>>,
    pub deduped_at: Option<Option<String>>,
    pub dedup_job_id: Option<Option<String>>,
    // --- schema v4 (threading) ---
    pub in_reply_to: Option<Option<String>>,
    pub references_json: Option<Option<String>>,
    pub conversation_topic: Option<Option<String>>,
    pub conversation_index_hex: Option<Option<String>>,
    pub thread_id: Option<Option<String>>,
    pub thread_root_item_id: Option<Option<String>>,
    pub thread_method: Option<Option<String>>,
    pub threaded_at: Option<Option<String>>,
    pub thread_job_id: Option<Option<String>>,
    // --- schema v5 (near-dup) ---
    pub near_dup_group_id: Option<Option<String>>,
    pub near_dup_role: Option<Option<String>>,
    pub near_dup_similarity: Option<Option<f64>>,
    pub near_dup_pivot_item_id: Option<Option<String>>,
    pub near_dup_method: Option<Option<String>>,
    pub near_duped_at: Option<Option<String>>,
    pub near_dup_job_id: Option<Option<String>>,
    // --- schema v6 (cull) ---
    pub cull_status: Option<Option<String>>,
    pub cull_reasons_json: Option<Option<String>>,
    pub cull_preset_id: Option<Option<String>>,
    pub cull_preset_name: Option<Option<String>>,
    pub culled_at: Option<Option<String>>,
    pub cull_job_id: Option<Option<String>>,
    // --- schema v7 (promote) ---
    pub in_review: Option<Option<i64>>,
    pub review_set_id: Option<Option<String>>,
    pub review_order: Option<Option<i64>>,
    pub promoted_at: Option<Option<String>>,
    pub promote_job_id: Option<Option<String>>,
    pub promote_policy: Option<Option<String>>,
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
    ///
    /// **Do not** call this from a concurrent progress/status poller while
    /// another handle is extracting: temp cleanup would race CAS materialization.
    /// Use [`Matter::open_for_read`] for concurrent readers.
    pub fn open(root: impl AsRef<Utf8Path>) -> Result<Self> {
        Self::open_inner(root, true)
    }

    /// Open an existing matter **without** cleaning `workspace/temp/`.
    ///
    /// Intended for concurrent read-only use (progress pollers, status queries)
    /// while a primary worker holds an open matter that may materialize PSTs
    /// under `workspace/temp/`. Still opens a separate SQLite connection (WAL).
    pub fn open_for_read(root: impl AsRef<Utf8Path>) -> Result<Self> {
        Self::open_inner(root, false)
    }

    fn open_inner(root: impl AsRef<Utf8Path>, cleanup_temp: bool) -> Result<Self> {
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
        if cleanup_temp {
            matter.cleanup_workspace_temp()?;
        }
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

    /// List all jobs for this matter (newest first).
    pub fn list_jobs(&self) -> Result<Vec<Job>> {
        jobs::list_jobs(&self.conn, &self.matter_id)
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

    /// List all sources for this matter (oldest first).
    pub fn list_sources(&self) -> Result<Vec<Source>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, matter_id, path, kind, status, cursor_json, created_at, updated_at \
             FROM sources WHERE matter_id = ?1 ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![self.matter_id], map_source_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Count items in the matter (all statuses).
    pub fn count_items(&self) -> Result<u64> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM items WHERE matter_id = ?1",
            params![self.matter_id],
            |row| row.get(0),
        )?;
        Ok(n as u64)
    }

    /// List items with the given `file_category` (e.g. `"pst"` inventory rows).
    pub fn list_items_by_file_category(&self, file_category: &str) -> Result<Vec<Item>> {
        let sql = item_select_sql(
            "WHERE matter_id = ?1 AND file_category = ?2 ORDER BY imported_at ASC, id ASC",
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![self.matter_id, file_category], map_item_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
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
                logical_hash_version, extra_json, \
                dedup_role, duplicate_of_item_id, dedup_tier, dedup_group_id, \
                deduped_at, dedup_job_id, \
                in_reply_to, references_json, conversation_topic, conversation_index_hex, \
                thread_id, thread_root_item_id, thread_method, threaded_at, thread_job_id, \
                near_dup_group_id, near_dup_role, near_dup_similarity, near_dup_pivot_item_id, \
                near_dup_method, near_duped_at, near_dup_job_id, \
                cull_status, cull_reasons_json, cull_preset_id, cull_preset_name, \
                culled_at, cull_job_id, \
                in_review, review_set_id, review_order, promoted_at, promote_job_id, promote_policy\
             ) VALUES (\
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, \
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, \
                ?26, ?27, ?28, ?29, ?30, ?31, ?32, \
                ?33, ?34, ?35, ?36, ?37, ?38, \
                ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46, ?47, \
                ?48, ?49, ?50, ?51, ?52, ?53, ?54, \
                ?55, ?56, ?57, ?58, ?59, ?60, \
                ?61, ?62, ?63, ?64, ?65, ?66\
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
                input.dedup_role,
                input.duplicate_of_item_id,
                input.dedup_tier,
                input.dedup_group_id,
                input.deduped_at,
                input.dedup_job_id,
                input.in_reply_to,
                input.references_json,
                input.conversation_topic,
                input.conversation_index_hex,
                input.thread_id,
                input.thread_root_item_id,
                input.thread_method,
                input.threaded_at,
                input.thread_job_id,
                input.near_dup_group_id,
                input.near_dup_role,
                input.near_dup_similarity,
                input.near_dup_pivot_item_id,
                input.near_dup_method,
                input.near_duped_at,
                input.near_dup_job_id,
                input.cull_status,
                input.cull_reasons_json,
                input.cull_preset_id,
                input.cull_preset_name,
                input.culled_at,
                input.cull_job_id,
                input.in_review,
                input.review_set_id,
                input.review_order,
                input.promoted_at,
                input.promote_job_id,
                input.promote_policy,
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
        let dedup_role = apply_opt2(update.dedup_role, current.dedup_role);
        let duplicate_of_item_id =
            apply_opt2(update.duplicate_of_item_id, current.duplicate_of_item_id);
        let dedup_tier = apply_opt2(update.dedup_tier, current.dedup_tier);
        let dedup_group_id = apply_opt2(update.dedup_group_id, current.dedup_group_id);
        let deduped_at = apply_opt2(update.deduped_at, current.deduped_at);
        let dedup_job_id = apply_opt2(update.dedup_job_id, current.dedup_job_id);
        let in_reply_to = apply_opt2(update.in_reply_to, current.in_reply_to);
        let references_json = apply_opt2(update.references_json, current.references_json);
        let conversation_topic = apply_opt2(update.conversation_topic, current.conversation_topic);
        let conversation_index_hex = apply_opt2(
            update.conversation_index_hex,
            current.conversation_index_hex,
        );
        let thread_id = apply_opt2(update.thread_id, current.thread_id);
        let thread_root_item_id =
            apply_opt2(update.thread_root_item_id, current.thread_root_item_id);
        let thread_method = apply_opt2(update.thread_method, current.thread_method);
        let threaded_at = apply_opt2(update.threaded_at, current.threaded_at);
        let thread_job_id = apply_opt2(update.thread_job_id, current.thread_job_id);
        let near_dup_group_id = apply_opt2(update.near_dup_group_id, current.near_dup_group_id);
        let near_dup_role = apply_opt2(update.near_dup_role, current.near_dup_role);
        let near_dup_similarity =
            apply_opt2(update.near_dup_similarity, current.near_dup_similarity);
        let near_dup_pivot_item_id = apply_opt2(
            update.near_dup_pivot_item_id,
            current.near_dup_pivot_item_id,
        );
        let near_dup_method = apply_opt2(update.near_dup_method, current.near_dup_method);
        let near_duped_at = apply_opt2(update.near_duped_at, current.near_duped_at);
        let near_dup_job_id = apply_opt2(update.near_dup_job_id, current.near_dup_job_id);
        let cull_status = apply_opt2(update.cull_status, current.cull_status);
        let cull_reasons_json = apply_opt2(update.cull_reasons_json, current.cull_reasons_json);
        let cull_preset_id = apply_opt2(update.cull_preset_id, current.cull_preset_id);
        let cull_preset_name = apply_opt2(update.cull_preset_name, current.cull_preset_name);
        let culled_at = apply_opt2(update.culled_at, current.culled_at);
        let cull_job_id = apply_opt2(update.cull_job_id, current.cull_job_id);
        let in_review = apply_opt2(update.in_review, current.in_review);
        let review_set_id = apply_opt2(update.review_set_id, current.review_set_id);
        let review_order = apply_opt2(update.review_order, current.review_order);
        let promoted_at = apply_opt2(update.promoted_at, current.promoted_at);
        let promote_job_id = apply_opt2(update.promote_job_id, current.promote_job_id);
        let promote_policy = apply_opt2(update.promote_policy, current.promote_policy);

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
                logical_hash_version = ?28, extra_json = ?29, \
                dedup_role = ?30, duplicate_of_item_id = ?31, dedup_tier = ?32, \
                dedup_group_id = ?33, deduped_at = ?34, dedup_job_id = ?35, \
                in_reply_to = ?36, references_json = ?37, conversation_topic = ?38, \
                conversation_index_hex = ?39, thread_id = ?40, thread_root_item_id = ?41, \
                thread_method = ?42, threaded_at = ?43, thread_job_id = ?44, \
                near_dup_group_id = ?45, near_dup_role = ?46, near_dup_similarity = ?47, \
                near_dup_pivot_item_id = ?48, near_dup_method = ?49, near_duped_at = ?50, \
                near_dup_job_id = ?51, \
                cull_status = ?52, cull_reasons_json = ?53, cull_preset_id = ?54, \
                cull_preset_name = ?55, culled_at = ?56, cull_job_id = ?57, \
                in_review = ?58, review_set_id = ?59, review_order = ?60, \
                promoted_at = ?61, promote_job_id = ?62, promote_policy = ?63 \
             WHERE id = ?64",
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
                dedup_role,
                duplicate_of_item_id,
                dedup_tier,
                dedup_group_id,
                deduped_at,
                dedup_job_id,
                in_reply_to,
                references_json,
                conversation_topic,
                conversation_index_hex,
                thread_id,
                thread_root_item_id,
                thread_method,
                threaded_at,
                thread_job_id,
                near_dup_group_id,
                near_dup_role,
                near_dup_similarity,
                near_dup_pivot_item_id,
                near_dup_method,
                near_duped_at,
                near_dup_job_id,
                cull_status,
                cull_reasons_json,
                cull_preset_id,
                cull_preset_name,
                culled_at,
                cull_job_id,
                in_review,
                review_set_id,
                review_order,
                promoted_at,
                promote_job_id,
                promote_policy,
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

    // --- Dedupe (schema v3) ---

    /// Eligible email parents for matter-level dedupe, ordered by first-seen wins:
    /// `imported_at ASC, path ASC, id ASC`.
    ///
    /// Thin rows only (no body text). Prefer
    /// [`Self::list_email_parents_for_dedupe_range`] for large matters.
    pub fn list_email_parents_for_dedupe(&self) -> Result<Vec<DedupeCandidate>> {
        self.list_email_parents_for_dedupe_range(0, u64::MAX)
    }

    /// Paged eligible parents (same order/filter as
    /// [`Self::list_email_parents_for_dedupe`]).
    pub fn list_email_parents_for_dedupe_range(
        &self,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<DedupeCandidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, message_id, logical_hash, path, imported_at, role, \
                    file_category, status, dedup_role \
             FROM items \
             WHERE matter_id = ?1 \
               AND status IN ('extracted', 'partial', 'normalized') \
               AND ( \
                     role = 'parent' \
                     OR (file_category = 'email' AND IFNULL(role, '') != 'attachment') \
                   ) \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?2 OFFSET ?3",
        )?;
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let rows = stmt.query_map(
            params![self.matter_id, limit_i, offset as i64],
            map_dedupe_candidate_row,
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Count of eligible email parents for dedupe.
    pub fn count_email_parents_for_dedupe(&self) -> Result<u64> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM items \
             WHERE matter_id = ?1 \
               AND status IN ('extracted', 'partial', 'normalized') \
               AND ( \
                     role = 'parent' \
                     OR (file_category = 'email' AND IFNULL(role, '') != 'attachment') \
                   )",
            params![self.matter_id],
            |row| row.get(0),
        )?;
        Ok(n as u64)
    }

    /// Aggregate counts by `dedup_role` for this matter.
    pub fn count_by_dedup_role(&self) -> Result<DedupRoleCounts> {
        let mut stmt = self.conn.prepare(
            "SELECT dedup_role, COUNT(*) FROM items WHERE matter_id = ?1 GROUP BY dedup_role",
        )?;
        let rows = stmt.query_map(params![self.matter_id], |row| {
            let role: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((role, count as u64))
        })?;
        let mut out = DedupRoleCounts::default();
        for row in rows {
            let (role, count) = row?;
            match role.as_deref() {
                Some(item_dedup_role::UNIQUE) => out.unique += count,
                Some(item_dedup_role::DUPLICATE) => out.duplicate += count,
                Some(item_dedup_role::SKIPPED) => out.skipped += count,
                _ => out.null_role += count,
            }
        }
        Ok(out)
    }

    /// Clear dedupe columns for eligible parents (and optionally their direct
    /// attachments). Single SQLite transaction.
    ///
    /// Returns the number of rows updated.
    pub fn clear_dedupe_fields(&self, include_attachments: bool) -> Result<u64> {
        let matter_id = self.matter_id.clone();
        self.with_transaction(|conn| {
            let n_parents = conn.execute(
                "UPDATE items SET \
                    dedup_role = NULL, duplicate_of_item_id = NULL, dedup_tier = NULL, \
                    dedup_group_id = NULL, deduped_at = NULL, dedup_job_id = NULL \
                 WHERE matter_id = ?1 \
                   AND status IN ('extracted', 'partial', 'normalized') \
                   AND ( \
                         role = 'parent' \
                         OR (file_category = 'email' AND IFNULL(role, '') != 'attachment') \
                       )",
                params![matter_id],
            )?;
            let mut total = n_parents as u64;
            if include_attachments {
                // Only clear attaches under eligible email parents — never wipe
                // dedupe fields on unrelated parented items (non-email trees).
                let n_att = conn.execute(
                    "UPDATE items SET \
                        dedup_role = NULL, duplicate_of_item_id = NULL, dedup_tier = NULL, \
                        dedup_group_id = NULL, deduped_at = NULL, dedup_job_id = NULL \
                     WHERE matter_id = ?1 \
                       AND (role = 'attachment' OR parent_item_id IS NOT NULL) \
                       AND parent_item_id IN ( \
                         SELECT id FROM items WHERE matter_id = ?1 \
                           AND status IN ('extracted', 'partial', 'normalized') \
                           AND ( \
                                 role = 'parent' \
                                 OR (file_category = 'email' AND IFNULL(role, '') != 'attachment') \
                               ) \
                       )",
                    params![matter_id],
                )?;
                total += n_att as u64;
            }
            Ok(total)
        })
    }

    /// Run `f` inside a single `BEGIN IMMEDIATE` … `COMMIT` transaction on the
    /// matter connection. Rolls back on error.
    ///
    /// Use for batch role writes + checkpoint that must commit together (DoD-5).
    pub fn with_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        self.conn.execute("BEGIN IMMEDIATE", [])?;
        match f(&self.conn) {
            Ok(v) => {
                self.conn.execute("COMMIT", [])?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.conn.execute("ROLLBACK", []);
                Err(e)
            }
        }
    }

    /// Apply N dedupe role updates and upsert the job checkpoint in **one**
    /// SQLite transaction (DoD-5).
    pub fn apply_dedup_batch_with_checkpoint(
        &self,
        job_id: &str,
        stage: &str,
        updates: &[DedupRoleUpdate],
        cursor_json: &str,
        completed_count: i64,
    ) -> Result<()> {
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            for u in updates {
                if let Some(ref extra) = u.extra_json {
                    conn.execute(
                        "UPDATE items SET \
                            dedup_role = ?1, duplicate_of_item_id = ?2, dedup_tier = ?3, \
                            dedup_group_id = ?4, deduped_at = ?5, dedup_job_id = ?6, \
                            extra_json = ?7 \
                         WHERE id = ?8",
                        params![
                            u.dedup_role,
                            u.duplicate_of_item_id,
                            u.dedup_tier,
                            u.dedup_group_id,
                            u.deduped_at,
                            u.dedup_job_id,
                            extra,
                            u.item_id,
                        ],
                    )?;
                } else {
                    conn.execute(
                        "UPDATE items SET \
                            dedup_role = ?1, duplicate_of_item_id = ?2, dedup_tier = ?3, \
                            dedup_group_id = ?4, deduped_at = ?5, dedup_job_id = ?6 \
                         WHERE id = ?7",
                        params![
                            u.dedup_role,
                            u.duplicate_of_item_id,
                            u.dedup_tier,
                            u.dedup_group_id,
                            u.deduped_at,
                            u.dedup_job_id,
                            u.item_id,
                        ],
                    )?;
                }
            }
            jobs::put_checkpoint(conn, job_id, stage, cursor_json, completed_count, &now)?;
            Ok(())
        })
    }

    // --- Threading (schema v4) ---

    /// Eligible email parents for matter-level threading, ordered by stable order:
    /// `imported_at ASC, path ASC, id ASC`.
    pub fn list_email_parents_for_thread(&self) -> Result<Vec<ThreadCandidate>> {
        self.list_email_parents_for_thread_range(0, u64::MAX)
    }

    /// Paged eligible parents (same order/filter as
    /// [`Self::list_email_parents_for_thread`]).
    pub fn list_email_parents_for_thread_range(
        &self,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<ThreadCandidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, message_id, in_reply_to, references_json, subject, \
                    conversation_index_hex, path, imported_at, role, file_category, \
                    status, thread_id, parent_item_id \
             FROM items \
             WHERE matter_id = ?1 \
               AND status IN ('extracted', 'partial', 'normalized') \
               AND ( \
                     role = 'parent' \
                     OR (file_category = 'email' AND IFNULL(role, '') != 'attachment') \
                   ) \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?2 OFFSET ?3",
        )?;
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let rows = stmt.query_map(
            params![self.matter_id, limit_i, offset as i64],
            map_thread_candidate_row,
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Count of eligible email parents for threading.
    pub fn count_email_parents_for_thread(&self) -> Result<u64> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM items \
             WHERE matter_id = ?1 \
               AND status IN ('extracted', 'partial', 'normalized') \
               AND ( \
                     role = 'parent' \
                     OR (file_category = 'email' AND IFNULL(role, '') != 'attachment') \
                   )",
            params![self.matter_id],
            |row| row.get(0),
        )?;
        Ok(n as u64)
    }

    /// Clear thread *result* columns for eligible parents (and optionally their
    /// direct attachments). Does **not** clear header storage columns
    /// (`in_reply_to`, `references_json`, `conversation_topic`,
    /// `conversation_index_hex`).
    ///
    /// Returns the number of rows updated.
    pub fn clear_thread_fields(&self, include_attachments: bool) -> Result<u64> {
        let matter_id = self.matter_id.clone();
        self.with_transaction(|conn| {
            let n_parents = conn.execute(
                "UPDATE items SET \
                    thread_id = NULL, thread_root_item_id = NULL, thread_method = NULL, \
                    threaded_at = NULL, thread_job_id = NULL \
                 WHERE matter_id = ?1 \
                   AND status IN ('extracted', 'partial', 'normalized') \
                   AND ( \
                         role = 'parent' \
                         OR (file_category = 'email' AND IFNULL(role, '') != 'attachment') \
                       )",
                params![matter_id],
            )?;
            let mut total = n_parents as u64;
            if include_attachments {
                let n_att = conn.execute(
                    "UPDATE items SET \
                        thread_id = NULL, thread_root_item_id = NULL, thread_method = NULL, \
                        threaded_at = NULL, thread_job_id = NULL \
                     WHERE matter_id = ?1 \
                       AND (role = 'attachment' OR parent_item_id IS NOT NULL) \
                       AND parent_item_id IN ( \
                         SELECT id FROM items WHERE matter_id = ?1 \
                           AND status IN ('extracted', 'partial', 'normalized') \
                           AND ( \
                                 role = 'parent' \
                                 OR (file_category = 'email' AND IFNULL(role, '') != 'attachment') \
                               ) \
                       )",
                    params![matter_id],
                )?;
                total += n_att as u64;
            }
            Ok(total)
        })
    }

    /// Apply N thread field updates and upsert the job checkpoint in **one**
    /// SQLite transaction (same pattern as dedupe / DoD-5).
    pub fn apply_thread_batch_with_checkpoint(
        &self,
        job_id: &str,
        stage: &str,
        updates: &[ThreadFieldUpdate],
        cursor_json: &str,
        completed_count: i64,
    ) -> Result<()> {
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            for u in updates {
                conn.execute(
                    "UPDATE items SET \
                        thread_id = ?1, thread_root_item_id = ?2, thread_method = ?3, \
                        threaded_at = ?4, thread_job_id = ?5 \
                     WHERE id = ?6",
                    params![
                        u.thread_id,
                        u.thread_root_item_id,
                        u.thread_method,
                        u.threaded_at,
                        u.thread_job_id,
                        u.item_id,
                    ],
                )?;
            }
            jobs::put_checkpoint(conn, job_id, stage, cursor_json, completed_count, &now)?;
            Ok(())
        })
    }

    // --- Near-duplicate detection (schema v5) ---

    /// Eligible items for near-dup sketching, ordered by stable order:
    /// `imported_at ASC, path ASC, id ASC`.
    ///
    /// Status filter: `extracted` / `partial` / `normalized`. When
    /// `include_attachments` is false, attachment-role rows are excluded.
    pub fn list_neardup_candidates(
        &self,
        include_attachments: bool,
    ) -> Result<Vec<NearDupCandidate>> {
        self.list_neardup_candidates_range(include_attachments, 0, u64::MAX)
    }

    /// Paged eligible near-dup candidates (same order/filter as
    /// [`Self::list_neardup_candidates`]).
    pub fn list_neardup_candidates_range(
        &self,
        include_attachments: bool,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<NearDupCandidate>> {
        let attach_clause = if include_attachments {
            ""
        } else {
            " AND IFNULL(role, '') != 'attachment' "
        };
        let sql = format!(
            "SELECT id, text_sha256, dedup_role, path, imported_at, role, parent_item_id, status \
             FROM items \
             WHERE matter_id = ?1 \
               AND status IN ('extracted', 'partial', 'normalized') \
               {attach_clause} \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let rows = stmt.query_map(
            params![self.matter_id, limit_i, offset as i64],
            map_neardup_candidate_row,
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Count of eligible near-dup candidates.
    pub fn count_neardup_candidates(&self, include_attachments: bool) -> Result<u64> {
        let attach_clause = if include_attachments {
            ""
        } else {
            " AND IFNULL(role, '') != 'attachment' "
        };
        let sql = format!(
            "SELECT COUNT(*) FROM items \
             WHERE matter_id = ?1 \
               AND status IN ('extracted', 'partial', 'normalized') \
               {attach_clause}"
        );
        let n: i64 = self
            .conn
            .query_row(&sql, params![self.matter_id], |row| row.get(0))?;
        Ok(n as u64)
    }

    /// Clear near-dup *result* columns for status-eligible items.
    ///
    /// Returns the number of rows updated.
    pub fn clear_near_dup_fields(&self) -> Result<u64> {
        let matter_id = self.matter_id.clone();
        self.with_transaction(|conn| {
            let n = conn.execute(
                "UPDATE items SET \
                    near_dup_group_id = NULL, near_dup_role = NULL, near_dup_similarity = NULL, \
                    near_dup_pivot_item_id = NULL, near_dup_method = NULL, near_duped_at = NULL, \
                    near_dup_job_id = NULL \
                 WHERE matter_id = ?1 \
                   AND status IN ('extracted', 'partial', 'normalized')",
                params![matter_id],
            )?;
            Ok(n as u64)
        })
    }

    /// Apply N near-dup field updates and upsert the job checkpoint in **one**
    /// SQLite transaction (same pattern as dedupe / thread).
    pub fn apply_near_dup_batch_with_checkpoint(
        &self,
        job_id: &str,
        stage: &str,
        updates: &[NearDupFieldUpdate],
        cursor_json: &str,
        completed_count: i64,
    ) -> Result<()> {
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            for u in updates {
                conn.execute(
                    "UPDATE items SET \
                        near_dup_group_id = ?1, near_dup_role = ?2, near_dup_similarity = ?3, \
                        near_dup_pivot_item_id = ?4, near_dup_method = ?5, near_duped_at = ?6, \
                        near_dup_job_id = ?7 \
                     WHERE id = ?8",
                    params![
                        u.near_dup_group_id,
                        u.near_dup_role,
                        u.near_dup_similarity,
                        u.near_dup_pivot_item_id,
                        u.near_dup_method,
                        u.near_duped_at,
                        u.near_dup_job_id,
                        u.item_id,
                    ],
                )?;
            }
            jobs::put_checkpoint(conn, job_id, stage, cursor_json, completed_count, &now)?;
            Ok(())
        })
    }

    // --- Cull / data reduction (schema v6) ---

    /// Eligible items for cull evaluation, ordered by stable order:
    /// `imported_at ASC, path ASC, id ASC`.
    ///
    /// When `process_attachments` is false, attachment-role rows are excluded.
    /// Status is not pre-filtered here — rules may gate on `statuses.include`.
    pub fn list_cull_candidates(&self, process_attachments: bool) -> Result<Vec<CullCandidate>> {
        self.list_cull_candidates_range(process_attachments, 0, u64::MAX)
    }

    /// Paged cull candidates (same order/filter as [`Self::list_cull_candidates`]).
    pub fn list_cull_candidates_range(
        &self,
        process_attachments: bool,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<CullCandidate>> {
        let attach_clause = if process_attachments {
            ""
        } else {
            " AND IFNULL(role, '') != 'attachment' "
        };
        let sql = format!(
            "SELECT id, parent_item_id, family_id, dedup_role, near_dup_role, \
                    sent_at, received_at, created_at, modified_at, path, custodian, \
                    file_category, mime_type, size_bytes, status, native_sha256, \
                    text_sha256, role, imported_at \
             FROM items \
             WHERE matter_id = ?1 \
               {attach_clause} \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let rows = stmt.query_map(
            params![self.matter_id, limit_i, offset as i64],
            map_cull_candidate_row,
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Count of cull candidates under the same filter as list.
    pub fn count_cull_candidates(&self, process_attachments: bool) -> Result<u64> {
        let attach_clause = if process_attachments {
            ""
        } else {
            " AND IFNULL(role, '') != 'attachment' "
        };
        let sql = format!("SELECT COUNT(*) FROM items WHERE matter_id = ?1 {attach_clause}");
        let n: i64 = self
            .conn
            .query_row(&sql, params![self.matter_id], |row| row.get(0))?;
        Ok(n as u64)
    }

    /// Clear cull *result* columns for the **eligible** set only — same filter
    /// as [`Self::list_cull_candidates`] (optional attachment exclusion).
    ///
    /// When `process_attachments` is false, attachment-role rows keep prior
    /// cull fields (they are not re-evaluated by the job).
    ///
    /// Returns the number of rows updated.
    pub fn clear_cull_fields(&self, process_attachments: bool) -> Result<u64> {
        let matter_id = self.matter_id.clone();
        let attach_clause = if process_attachments {
            ""
        } else {
            " AND IFNULL(role, '') != 'attachment' "
        };
        self.with_transaction(|conn| {
            let sql = format!(
                "UPDATE items SET \
                    cull_status = NULL, cull_reasons_json = NULL, cull_preset_id = NULL, \
                    cull_preset_name = NULL, culled_at = NULL, cull_job_id = NULL \
                 WHERE matter_id = ?1 \
                   {attach_clause}"
            );
            let n = conn.execute(&sql, params![matter_id])?;
            Ok(n as u64)
        })
    }

    /// Apply N cull field updates and upsert the job checkpoint in **one**
    /// SQLite transaction (same pattern as near-dup / dedupe).
    pub fn apply_cull_batch_with_checkpoint(
        &self,
        job_id: &str,
        stage: &str,
        updates: &[CullFieldUpdate],
        cursor_json: &str,
        completed_count: i64,
    ) -> Result<()> {
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            for u in updates {
                conn.execute(
                    "UPDATE items SET \
                        cull_status = ?1, cull_reasons_json = ?2, cull_preset_id = ?3, \
                        cull_preset_name = ?4, culled_at = ?5, cull_job_id = ?6 \
                     WHERE id = ?7",
                    params![
                        u.cull_status,
                        u.cull_reasons_json,
                        u.cull_preset_id,
                        u.cull_preset_name,
                        u.culled_at,
                        u.cull_job_id,
                        u.item_id,
                    ],
                )?;
            }
            jobs::put_checkpoint(conn, job_id, stage, cursor_json, completed_count, &now)?;
            Ok(())
        })
    }

    /// List all cull presets for this matter, ordered by name.
    pub fn list_cull_presets(&self) -> Result<Vec<CullPreset>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, matter_id, name, description, rules_json, created_at, updated_at, created_by \
             FROM cull_presets WHERE matter_id = ?1 ORDER BY name ASC",
        )?;
        let rows = stmt.query_map(params![self.matter_id], map_cull_preset_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Load a cull preset by id.
    pub fn get_cull_preset(&self, preset_id: &str) -> Result<CullPreset> {
        self.conn
            .query_row(
                "SELECT id, matter_id, name, description, rules_json, created_at, updated_at, created_by \
                 FROM cull_presets WHERE id = ?1",
                params![preset_id],
                map_cull_preset_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("cull preset not found: {preset_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// Insert or update a cull preset. Name uniqueness is app-enforced per matter.
    pub fn upsert_cull_preset(&self, input: CullPresetInput) -> Result<CullPreset> {
        let now = now_rfc3339();
        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::Other("cull preset name cannot be empty".into()));
        }
        if input.rules_json.trim().is_empty() {
            return Err(Error::Other(
                "cull preset rules_json cannot be empty".into(),
            ));
        }

        if let Some(ref id) = input.id {
            // Update existing.
            let existing = self.get_cull_preset(id)?;
            if existing.matter_id != self.matter_id {
                return Err(Error::Other(format!(
                    "cull preset {id} belongs to another matter"
                )));
            }
            // Name collision with a different row.
            let clash: Option<String> = self
                .conn
                .query_row(
                    "SELECT id FROM cull_presets WHERE matter_id = ?1 AND name = ?2 AND id != ?3",
                    params![self.matter_id, name, id],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "cull preset name already exists in matter: {name}"
                )));
            }
            self.conn.execute(
                "UPDATE cull_presets SET name = ?1, description = ?2, rules_json = ?3, \
                 updated_at = ?4, created_by = COALESCE(?5, created_by) WHERE id = ?6",
                params![
                    name,
                    input.description,
                    input.rules_json,
                    now,
                    input.created_by,
                    id
                ],
            )?;
            return self.get_cull_preset(id);
        }

        // Insert new — reject name collision.
        let clash: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM cull_presets WHERE matter_id = ?1 AND name = ?2",
                params![self.matter_id, name],
                |row| row.get(0),
            )
            .optional()?;
        if clash.is_some() {
            return Err(Error::Other(format!(
                "cull preset name already exists in matter: {name}"
            )));
        }
        let id = new_id("cpr");
        self.conn.execute(
            "INSERT INTO cull_presets (id, matter_id, name, description, rules_json, \
             created_at, updated_at, created_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                self.matter_id,
                name,
                input.description,
                input.rules_json,
                now,
                now,
                input.created_by
            ],
        )?;
        self.get_cull_preset(&id)
    }

    /// Delete a cull preset row. Does **not** clear item cull fields.
    pub fn delete_cull_preset(&self, preset_id: &str) -> Result<()> {
        let existing = self.get_cull_preset(preset_id)?;
        if existing.matter_id != self.matter_id {
            return Err(Error::Other(format!(
                "cull preset {preset_id} belongs to another matter"
            )));
        }
        self.conn
            .execute("DELETE FROM cull_presets WHERE id = ?1", params![preset_id])?;
        Ok(())
    }

    // --- Promote / review sets (schema v7) ---

    /// True when any item has a non-null `cull_status` (cull has run).
    pub fn cull_has_run(&self) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE matter_id = ?1 AND cull_status IS NOT NULL)",
            params![self.matter_id],
            |row| row.get(0),
        )?;
        Ok(n != 0)
    }

    /// True when any item has a non-null `dedup_role` (dedupe has run).
    pub fn any_dedup_role_present(&self) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE matter_id = ?1 AND dedup_role IS NOT NULL)",
            params![self.matter_id],
            |row| row.get(0),
        )?;
        Ok(n != 0)
    }

    /// Ensure a default review set exists for this matter.
    ///
    /// If a default already exists, returns it (name is not changed).
    /// Otherwise inserts `name` (default [`DEFAULT_REVIEW_SET_NAME`]) with
    /// `is_default = 1`. The partial unique index prevents double-default races.
    pub fn ensure_default_review_set(&self, name: &str) -> Result<ReviewSet> {
        if let Some(existing) = self.get_default_review_set()? {
            return Ok(existing);
        }
        let name = if name.trim().is_empty() {
            DEFAULT_REVIEW_SET_NAME
        } else {
            name.trim()
        };
        let id = new_id("rset");
        let now = now_rfc3339();
        match self.conn.execute(
            "INSERT INTO review_sets (id, matter_id, name, is_default, policy, policy_json, \
             item_count, created_at, updated_at, created_by) \
             VALUES (?1, ?2, ?3, 1, NULL, NULL, 0, ?4, ?4, NULL)",
            params![id, self.matter_id, name, now],
        ) {
            Ok(_) => self.get_review_set(&id),
            Err(e) => {
                // Race: another writer may have created the default first.
                if let Some(existing) = self.get_default_review_set()? {
                    return Ok(existing);
                }
                Err(Error::Sqlite(e))
            }
        }
    }

    /// Load the default review set for this matter, if any.
    pub fn get_default_review_set(&self) -> Result<Option<ReviewSet>> {
        self.conn
            .query_row(
                "SELECT id, matter_id, name, is_default, policy, policy_json, item_count, \
                 created_at, updated_at, created_by \
                 FROM review_sets WHERE matter_id = ?1 AND is_default = 1",
                params![self.matter_id],
                map_review_set_row,
            )
            .optional()
            .map_err(Error::from)
    }

    /// Load a review set by id.
    pub fn get_review_set(&self, set_id: &str) -> Result<ReviewSet> {
        self.conn
            .query_row(
                "SELECT id, matter_id, name, is_default, policy, policy_json, item_count, \
                 created_at, updated_at, created_by \
                 FROM review_sets WHERE id = ?1",
                params![set_id],
                map_review_set_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("review set not found: {set_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// List review sets for this matter, default first then by name.
    pub fn list_review_sets(&self) -> Result<Vec<ReviewSet>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, matter_id, name, is_default, policy, policy_json, item_count, \
             created_at, updated_at, created_by \
             FROM review_sets WHERE matter_id = ?1 \
             ORDER BY is_default DESC, name ASC",
        )?;
        let rows = stmt.query_map(params![self.matter_id], map_review_set_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Update policy snapshot + item_count after a successful promote.
    pub fn update_review_set_snapshot(
        &self,
        set_id: &str,
        policy: &str,
        policy_json: Option<&str>,
        item_count: i64,
    ) -> Result<ReviewSet> {
        let existing = self.get_review_set(set_id)?;
        if existing.matter_id != self.matter_id {
            return Err(Error::Other(format!(
                "review set {set_id} belongs to another matter"
            )));
        }
        let now = now_rfc3339();
        self.conn.execute(
            "UPDATE review_sets SET policy = ?1, policy_json = ?2, item_count = ?3, \
             updated_at = ?4 WHERE id = ?5",
            params![policy, policy_json, item_count, now, set_id],
        )?;
        self.get_review_set(set_id)
    }

    /// Clear promote membership columns for items in `set_id` (this matter).
    ///
    /// Returns the number of rows updated. Does **not** delete items or CAS.
    pub fn clear_review_membership_for_set(&self, set_id: &str) -> Result<u64> {
        let matter_id = self.matter_id.clone();
        self.with_transaction(|conn| {
            let n = conn.execute(
                "UPDATE items SET \
                    in_review = 0, review_set_id = NULL, review_order = NULL, \
                    promoted_at = NULL, promote_job_id = NULL, promote_policy = NULL \
                 WHERE matter_id = ?1 AND review_set_id = ?2",
                params![matter_id, set_id],
            )?;
            // Also clear any rows still flagged in_review without a set id
            // (defensive) when this is the default set's full recompute.
            let n2 = conn.execute(
                "UPDATE items SET \
                    in_review = 0, review_order = NULL, \
                    promoted_at = NULL, promote_job_id = NULL, promote_policy = NULL \
                 WHERE matter_id = ?1 AND in_review = 1 AND (review_set_id IS NULL OR review_set_id = ?2)",
                params![matter_id, set_id],
            )?;
            Ok((n + n2) as u64)
        })
    }

    /// Thin candidates for promote policy selection (all items in the matter).
    pub fn list_promote_candidates(&self) -> Result<Vec<PromoteCandidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_item_id, path, status, dedup_role, cull_status, role \
             FROM items WHERE matter_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![self.matter_id], map_promote_candidate_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Single-pass family-aware order over membership ids.
    ///
    /// Uses a temp table so ordering is **one** SQL query (no N+1 per parent).
    /// Returns thin rows ordered by:
    /// `COALESCE(parent_item_id, id), parent-first, path, id`.
    pub fn list_promote_ordered_membership(
        &self,
        member_ids: &[String],
    ) -> Result<Vec<PromoteCandidate>> {
        if member_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_transaction(|conn| {
            conn.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS _promote_members (id TEXT PRIMARY KEY NOT NULL);
                 DELETE FROM _promote_members;",
            )?;
            {
                let mut ins =
                    conn.prepare("INSERT OR IGNORE INTO _promote_members (id) VALUES (?1)")?;
                for id in member_ids {
                    ins.execute(params![id])?;
                }
            }
            let mut stmt = conn.prepare(
                "SELECT i.id, i.parent_item_id, i.path, i.status, i.dedup_role, i.cull_status, i.role \
                 FROM items i \
                 INNER JOIN _promote_members m ON m.id = i.id \
                 ORDER BY \
                   COALESCE(i.parent_item_id, i.id) ASC, \
                   CASE WHEN i.parent_item_id IS NULL THEN 0 ELSE 1 END ASC, \
                   i.path ASC, \
                   i.id ASC",
            )?;
            let rows = stmt.query_map([], map_promote_candidate_row)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            // Drop so next call is clean even if connection is long-lived.
            conn.execute_batch("DROP TABLE IF EXISTS _promote_members;")?;
            Ok(out)
        })
    }

    /// Direct children of the given parent ids (any matter item).
    pub fn list_direct_children_ids(&self, parent_ids: &[String]) -> Result<Vec<String>> {
        if parent_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_transaction(|conn| {
            conn.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS _promote_parents (id TEXT PRIMARY KEY NOT NULL);
                 DELETE FROM _promote_parents;",
            )?;
            {
                let mut ins =
                    conn.prepare("INSERT OR IGNORE INTO _promote_parents (id) VALUES (?1)")?;
                for id in parent_ids {
                    ins.execute(params![id])?;
                }
            }
            let mut stmt = conn.prepare(
                "SELECT i.id FROM items i \
                 INNER JOIN _promote_parents p ON p.id = i.parent_item_id \
                 WHERE i.matter_id = ?1",
            )?;
            let rows = stmt.query_map(params![self.matter_id], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            conn.execute_batch("DROP TABLE IF EXISTS _promote_parents;")?;
            Ok(out)
        })
    }

    /// Distinct parent_item_id values for the given child ids (non-null only).
    pub fn list_parent_ids_of(&self, child_ids: &[String]) -> Result<Vec<String>> {
        if child_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_transaction(|conn| {
            conn.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS _promote_children (id TEXT PRIMARY KEY NOT NULL);
                 DELETE FROM _promote_children;",
            )?;
            {
                let mut ins =
                    conn.prepare("INSERT OR IGNORE INTO _promote_children (id) VALUES (?1)")?;
                for id in child_ids {
                    ins.execute(params![id])?;
                }
            }
            let mut stmt = conn.prepare(
                "SELECT DISTINCT i.parent_item_id FROM items i \
                 INNER JOIN _promote_children c ON c.id = i.id \
                 WHERE i.matter_id = ?1 AND i.parent_item_id IS NOT NULL",
            )?;
            let rows = stmt.query_map(params![self.matter_id], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            conn.execute_batch("DROP TABLE IF EXISTS _promote_children;")?;
            Ok(out)
        })
    }

    /// Apply N promote membership updates and upsert the job checkpoint in **one**
    /// SQLite transaction (same pattern as cull / near-dup / dedupe).
    pub fn apply_promote_batch_with_checkpoint(
        &self,
        job_id: &str,
        stage: &str,
        updates: &[PromoteFieldUpdate],
        cursor_json: &str,
        completed_count: i64,
    ) -> Result<()> {
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            for u in updates {
                conn.execute(
                    "UPDATE items SET \
                        in_review = ?1, review_set_id = ?2, review_order = ?3, \
                        promoted_at = ?4, promote_job_id = ?5, promote_policy = ?6 \
                     WHERE id = ?7",
                    params![
                        u.in_review,
                        u.review_set_id,
                        u.review_order,
                        u.promoted_at,
                        u.promote_job_id,
                        u.promote_policy,
                        u.item_id,
                    ],
                )?;
            }
            jobs::put_checkpoint(conn, job_id, stage, cursor_json, completed_count, &now)?;
            Ok(())
        })
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
    logical_hash_version, extra_json, \
    dedup_role, duplicate_of_item_id, dedup_tier, dedup_group_id, \
    deduped_at, dedup_job_id, \
    in_reply_to, references_json, conversation_topic, conversation_index_hex, \
    thread_id, thread_root_item_id, thread_method, threaded_at, thread_job_id, \
    near_dup_group_id, near_dup_role, near_dup_similarity, near_dup_pivot_item_id, \
    near_dup_method, near_duped_at, near_dup_job_id, \
    cull_status, cull_reasons_json, cull_preset_id, cull_preset_name, \
    culled_at, cull_job_id, \
    in_review, review_set_id, review_order, promoted_at, promote_job_id, promote_policy";

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
        dedup_role: row.get(32)?,
        duplicate_of_item_id: row.get(33)?,
        dedup_tier: row.get(34)?,
        dedup_group_id: row.get(35)?,
        deduped_at: row.get(36)?,
        dedup_job_id: row.get(37)?,
        in_reply_to: row.get(38)?,
        references_json: row.get(39)?,
        conversation_topic: row.get(40)?,
        conversation_index_hex: row.get(41)?,
        thread_id: row.get(42)?,
        thread_root_item_id: row.get(43)?,
        thread_method: row.get(44)?,
        threaded_at: row.get(45)?,
        thread_job_id: row.get(46)?,
        near_dup_group_id: row.get(47)?,
        near_dup_role: row.get(48)?,
        near_dup_similarity: row.get(49)?,
        near_dup_pivot_item_id: row.get(50)?,
        near_dup_method: row.get(51)?,
        near_duped_at: row.get(52)?,
        near_dup_job_id: row.get(53)?,
        cull_status: row.get(54)?,
        cull_reasons_json: row.get(55)?,
        cull_preset_id: row.get(56)?,
        cull_preset_name: row.get(57)?,
        culled_at: row.get(58)?,
        cull_job_id: row.get(59)?,
        in_review: row.get(60)?,
        review_set_id: row.get(61)?,
        review_order: row.get(62)?,
        promoted_at: row.get(63)?,
        promote_job_id: row.get(64)?,
        promote_policy: row.get(65)?,
    })
}

fn map_promote_candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PromoteCandidate> {
    Ok(PromoteCandidate {
        id: row.get(0)?,
        parent_item_id: row.get(1)?,
        path: row.get(2)?,
        status: row.get(3)?,
        dedup_role: row.get(4)?,
        cull_status: row.get(5)?,
        role: row.get(6)?,
    })
}

fn map_review_set_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewSet> {
    let is_default_i: i64 = row.get(3)?;
    Ok(ReviewSet {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        name: row.get(2)?,
        is_default: is_default_i != 0,
        policy: row.get(4)?,
        policy_json: row.get(5)?,
        item_count: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        created_by: row.get(9)?,
    })
}

fn map_cull_candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CullCandidate> {
    Ok(CullCandidate {
        id: row.get(0)?,
        parent_item_id: row.get(1)?,
        family_id: row.get(2)?,
        dedup_role: row.get(3)?,
        near_dup_role: row.get(4)?,
        sent_at: row.get(5)?,
        received_at: row.get(6)?,
        created_at: row.get(7)?,
        modified_at: row.get(8)?,
        path: row.get(9)?,
        custodian: row.get(10)?,
        file_category: row.get(11)?,
        mime_type: row.get(12)?,
        size_bytes: row.get(13)?,
        status: row.get(14)?,
        native_sha256: row.get(15)?,
        text_sha256: row.get(16)?,
        role: row.get(17)?,
        imported_at: row.get(18)?,
    })
}

fn map_cull_preset_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CullPreset> {
    Ok(CullPreset {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        rules_json: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        created_by: row.get(7)?,
    })
}

fn map_dedupe_candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DedupeCandidate> {
    Ok(DedupeCandidate {
        id: row.get(0)?,
        message_id: row.get(1)?,
        logical_hash: row.get(2)?,
        path: row.get(3)?,
        imported_at: row.get(4)?,
        role: row.get(5)?,
        file_category: row.get(6)?,
        status: row.get(7)?,
        dedup_role: row.get(8)?,
    })
}

fn map_thread_candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadCandidate> {
    Ok(ThreadCandidate {
        id: row.get(0)?,
        message_id: row.get(1)?,
        in_reply_to: row.get(2)?,
        references_json: row.get(3)?,
        subject: row.get(4)?,
        conversation_index_hex: row.get(5)?,
        path: row.get(6)?,
        imported_at: row.get(7)?,
        role: row.get(8)?,
        file_category: row.get(9)?,
        status: row.get(10)?,
        thread_id: row.get(11)?,
        parent_item_id: row.get(12)?,
    })
}

fn map_neardup_candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NearDupCandidate> {
    Ok(NearDupCandidate {
        id: row.get(0)?,
        text_sha256: row.get(1)?,
        dedup_role: row.get(2)?,
        path: row.get(3)?,
        imported_at: row.get(4)?,
        role: row.get(5)?,
        parent_item_id: row.get(6)?,
        status: row.get(7)?,
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
