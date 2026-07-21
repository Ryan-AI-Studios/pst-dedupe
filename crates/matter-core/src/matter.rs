//! Matter directory layout and high-level store API.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::audit::{self, AuditEvent, AuditEventInput};
use crate::cas::Cas;
use crate::error::{Error, Result};
use crate::filter::{self, FilterSpec};
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
    // --- schema v13 (redaction) ---
    /// Denormalized count of `item_redactions` rows for this item.
    pub redaction_count: i64,
    /// CAS digest of last successful redacted text artifact (NULL when stale/absent).
    pub redacted_text_sha256: Option<String>,
    pub redacted_text_at: Option<String>,
    /// Display body digest the redacted artifact was built from.
    pub redacted_source_digest: Option<String>,
    // --- schema v14 (office extract) ---
    /// `ok` | `skipped` | `error` | NULL never attempted.
    pub office_extract_status: Option<String>,
    pub office_extract_method: Option<String>,
    pub office_source_native_sha256: Option<String>,
    pub office_extracted_at: Option<String>,
    pub office_extract_error: Option<String>,
    // --- schema v15 (pdf extract) ---
    /// `ok` | `low_text` | `empty` | `skipped` | `error` | NULL never attempted.
    pub pdf_extract_status: Option<String>,
    pub pdf_extract_method: Option<String>,
    pub pdf_source_native_sha256: Option<String>,
    pub pdf_extracted_at: Option<String>,
    pub pdf_extract_error: Option<String>,
    pub pdf_page_count: Option<i64>,
    /// 0/1 — empty or low-text OCR candidate (0036).
    pub pdf_needs_ocr: i64,
    // --- schema v16 (calendar / ICS) ---
    /// Raw MAPI message class or `VEVENT` / `ics`.
    pub message_class: Option<String>,
    /// RFC3339 with offset when known.
    pub cal_start_at: Option<String>,
    pub cal_end_at: Option<String>,
    /// 0/1 all-day flag.
    pub cal_all_day: Option<i64>,
    pub cal_location: Option<String>,
    pub cal_organizer: Option<String>,
    /// JSON array of `{ "addr", "name?", "role?", "partstat?" }`.
    pub cal_attendees_json: Option<String>,
    pub cal_busy_status: Option<String>,
    /// 0/1 when recurrence/RRULE present (not expanded).
    pub cal_is_recurring: Option<i64>,
    pub cal_recurrence_id: Option<String>,
    pub cal_uid: Option<String>,
    /// e.g. `pst_oxocal_v1` / `ics_icalendar_v1`.
    pub cal_extract_method: Option<String>,
    /// `ok` | `skipped` | `error` | NULL never attempted.
    pub ics_extract_status: Option<String>,
    pub ics_extract_method: Option<String>,
    pub ics_source_native_sha256: Option<String>,
    pub ics_extracted_at: Option<String>,
    pub ics_extract_error: Option<String>,
    // --- schema v17 (OCR) ---
    /// `ok` | `error` | `skipped` | `disabled` | NULL never attempted.
    pub ocr_status: Option<String>,
    pub ocr_engine: Option<String>,
    pub ocr_lang: Option<String>,
    pub ocr_text_sha256: Option<String>,
    pub ocr_source_native_sha256: Option<String>,
    pub ocr_page_count: Option<i64>,
    pub ocr_at: Option<String>,
    pub ocr_error: Option<String>,
    /// Mean confidence when engine provides it; else null.
    pub ocr_confidence: Option<f64>,
    // --- schema v18 (file category / taxonomy_v1) ---
    /// How `file_category` was decided (`message_class` / `magic` / …).
    pub category_method: Option<String>,
    /// Taxonomy id (e.g. `taxonomy_v1`).
    pub category_taxonomy: Option<String>,
    /// `ok` | `skipped` | `error` | NULL never attempted.
    pub category_status: Option<String>,
    pub category_error: Option<String>,
    /// RFC3339 timestamp of last classify apply.
    pub categorized_at: Option<String>,
    // --- schema v25 (entity / PII packs) ---
    /// Bitmask of entity hit types (see [`crate::entity_flags`]).
    pub entity_flags: i64,
    /// RFC3339 timestamp of last entity scan for this item.
    pub entity_scan_at: Option<String>,
    pub entity_scan_job_id: Option<String>,
    /// Denormalized count of `item_entity_hits` for this item.
    pub entity_hit_count: i64,
    /// Digest of body text last used for entity_scan (idempotency).
    pub entity_scanned_text_sha256: Option<String>,
    // --- schema v27 (concept clustering / default set denorm) ---
    /// Default-set concept cluster id (NULL when unclustered / non-default set only).
    pub concept_cluster_id: Option<String>,
    /// Default-set concept cluster set id.
    pub concept_cluster_set_id: Option<String>,
    /// When default-set membership was last written.
    pub concept_clustered_at: Option<String>,
    // --- schema v28 (sentiment / tone) ---
    /// Primary extreme-unit compound ∈ [-1, 1]; NULL = unscored.
    pub sentiment_compound: Option<f64>,
    /// Most negative unit compound across scored units.
    pub sentiment_compound_min: Option<f64>,
    /// Most positive unit compound across scored units.
    pub sentiment_compound_max: Option<f64>,
    /// pos/neu/neg proportions from the winning (extreme) unit.
    pub sentiment_pos: Option<f64>,
    pub sentiment_neu: Option<f64>,
    pub sentiment_neg: Option<f64>,
    /// `positive` | `neutral` | `negative`; NULL = unscored (not the same as neutral).
    pub sentiment_polarity: Option<String>,
    /// Scoring method id (e.g. `vader_lexicon_v1`).
    pub sentiment_method: Option<String>,
    /// Snapshot of pos threshold used for polarity.
    pub sentiment_pos_threshold: Option<f64>,
    /// Snapshot of neg threshold used for polarity.
    pub sentiment_neg_threshold: Option<f64>,
    /// Body `text_sha256` last used for a successful score write.
    pub sentiment_scanned_text_sha256: Option<String>,
    pub sentiment_scanned_at: Option<String>,
    pub sentiment_job_id: Option<String>,
    // --- schema v29 (semantic search) ---
    /// Body `text_sha256` last successfully embedded for the active semantic model.
    pub semantic_embedded_text_sha256: Option<String>,
    pub semantic_embedded_at: Option<String>,
    /// Number of chunks written for this item under the active model (NULL = never).
    pub semantic_chunk_count: Option<i64>,
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
    // --- schema v16 (calendar) — set by extract-pst / extract-calendar ---
    pub message_class: Option<String>,
    pub cal_start_at: Option<String>,
    pub cal_end_at: Option<String>,
    pub cal_all_day: Option<i64>,
    pub cal_location: Option<String>,
    pub cal_organizer: Option<String>,
    pub cal_attendees_json: Option<String>,
    pub cal_busy_status: Option<String>,
    pub cal_is_recurring: Option<i64>,
    pub cal_recurrence_id: Option<String>,
    pub cal_uid: Option<String>,
    pub cal_extract_method: Option<String>,
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
    /// Prior cull result when set — used for cumulative (`reset:false`) skip.
    pub cull_status: Option<String>,
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

/// Thin review-list row for the desk Review surface (0026).
///
/// Intentionally excludes body text and large participant JSON so list loads
/// stay cheap for virtualized scrolling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewListRow {
    pub id: String,
    pub review_order: Option<i64>,
    pub role: Option<String>,
    pub parent_item_id: Option<String>,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub path: Option<String>,
    pub file_category: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub text_sha256: Option<String>,
    pub html_sha256: Option<String>,
    pub dedup_role: Option<String>,
    pub cull_status: Option<String>,
    pub attachment_count: Option<i64>,
    pub family_id: Option<String>,
}

/// Code definition (matter-scoped catalog, schema v8 / track 0027; guidance v30).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeDef {
    pub id: String,
    pub matter_id: String,
    pub key: String,
    pub label: String,
    pub group_key: String,
    /// `single` (mutual exclusion within group) or `multi`.
    pub cardinality: String,
    pub color: Option<String>,
    pub sort_order: i64,
    /// 0/1 — inactive hidden from apply UI; historical membership still loads.
    pub is_active: i64,
    pub created_at: String,
    /// Operator guidance / protocol text for AI prompts (schema v30). Empty → use label.
    pub guidance: Option<String>,
}

/// Input for inserting or updating a code definition.
#[derive(Debug, Clone)]
pub struct CodeDefInput {
    /// When `Some`, update that id; when `None`, insert (key from label slug if omitted).
    pub id: Option<String>,
    /// Stable machine key. When `None` on insert, derived from `label` via slug.
    pub key: Option<String>,
    pub label: String,
    pub group_key: String,
    /// `single` or `multi`. Defaults to `multi` when empty on insert.
    pub cardinality: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub is_active: bool,
    /// Optional coding protocol / guidance text (AI prompts). `None` leaves unchanged on update.
    pub guidance: Option<String>,
}

/// Membership of a code on an item (with catalog metadata for chips).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemCodeInfo {
    pub code_id: String,
    pub key: String,
    pub label: String,
    pub group_key: String,
    pub cardinality: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub is_active: i64,
    pub set_at: String,
    pub set_by: String,
}

/// Input for [`Matter::apply_codes`] — all coding writes go through this path.
#[derive(Debug, Clone)]
pub struct ApplyCodesInput {
    /// Selected item ids (pre-expand).
    pub item_ids: Vec<String>,
    /// Code definition ids to add (single-group rules applied per target).
    pub add_code_ids: Vec<String>,
    /// Code definition ids to remove.
    pub remove_code_ids: Vec<String>,
    /// When true, expand each selection to whole family unit (parent + all children).
    pub propagate_family: bool,
    /// Actor written to membership + audit.
    pub actor: String,
}

/// Result of a successful [`Matter::apply_codes`] call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyCodesResult {
    /// Final target ids after optional family expand (sorted).
    pub target_item_ids: Vec<String>,
    pub selected_count: usize,
    pub target_count: usize,
}

/// Max UTF-8 byte length for a note body (P0).
pub const NOTE_BODY_MAX_BYTES: usize = 64 * 1024;
/// Max UTF-8 byte length for a stored highlight exact_quote.
pub const HIGHLIGHT_QUOTE_MAX_BYTES: usize = 4 * 1024;
/// Context chars captured for prefix/suffix re-resolve.
pub const HIGHLIGHT_CONTEXT_CHARS: usize = 40;
/// Default highlight paint color (yellow).
pub const HIGHLIGHT_DEFAULT_COLOR: &str = "#FFF59D";

/// Highlight status vocabulary (schema v11 / track 0030).
pub mod highlight_status {
    pub const ACTIVE: &str = "active";
    pub const STALE: &str = "stale";
}

/// Document or passage note (schema v11 / track 0030).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemNote {
    pub id: String,
    pub item_id: String,
    pub matter_id: String,
    pub body: String,
    /// When set, note is attached to a highlight (passage note).
    pub highlight_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
    pub updated_by: String,
}

/// Input for [`Matter::upsert_note`].
#[derive(Debug, Clone)]
pub struct UpsertNoteInput {
    /// When `Some`, update that note's body; when `None`, create.
    pub id: Option<String>,
    /// Required on create; on update must match existing (if provided).
    pub item_id: String,
    pub body: String,
    /// Optional passage link on create only (ignored on update).
    pub highlight_id: Option<String>,
    pub actor: String,
}

/// Stand-off text highlight on Review display text (schema v11 / track 0030).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemHighlight {
    pub id: String,
    pub item_id: String,
    pub matter_id: String,
    /// Inclusive UTF-8 **char** index into display body at create (or last resolve).
    pub start_utf8: i64,
    /// Exclusive UTF-8 char index; `end > start`.
    pub end_utf8: i64,
    /// Raw substring of display body at create (not re-normalized on store).
    pub exact_quote: String,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    /// Digest of display text used when created (`text_sha256` or synthetic).
    pub body_digest: String,
    pub color: String,
    /// `active` or `stale`.
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
}

/// Input for [`Matter::create_highlight`].
#[derive(Debug, Clone)]
pub struct CreateHighlightInput {
    pub item_id: String,
    /// Inclusive UTF-8 char index into [`Self::display_body`].
    pub start_utf8: i64,
    /// Exclusive UTF-8 char index.
    pub end_utf8: i64,
    /// Must equal the char-slice of `display_body` at `[start, end)`.
    pub exact_quote: String,
    /// Full display body currently shown (for validation + prefix/suffix).
    pub display_body: String,
    /// Digest of the display body (prefer item `text_sha256`, else synthetic).
    pub body_digest: String,
    pub color: Option<String>,
    pub actor: String,
}

/// Paint-ready range after digest check / whitespace re-resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHighlight {
    pub highlight_id: String,
    pub start_utf8: i64,
    pub end_utf8: i64,
    /// Effective status for paint (`active` | `stale`).
    pub status: String,
    /// True when re-resolve found a range different from stored offsets.
    pub remapped: bool,
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

/// Named saved search (schema v9+ / tracks 0028–0029).
///
/// `filter_json` is a serialized [`FilterSpec`]; load re-runs against live item state.
/// Optional `keyword` is the body FTS query (Tantivy; not compiled into SQL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSearch {
    pub id: String,
    pub matter_id: String,
    pub name: String,
    pub description: Option<String>,
    /// `review_corpus` or `entire_matter` (denormalized from FilterSpec for listing).
    pub scope: String,
    pub filter_json: String,
    /// Optional keyword / Boolean query for Tantivy FTS (schema v10).
    pub keyword: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: Option<String>,
}

/// Input for inserting or updating a saved search.
#[derive(Debug, Clone)]
pub struct SavedSearchInput {
    /// When `Some`, update that id; when `None`, insert a new row.
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    /// Serialized [`FilterSpec`] JSON (validated on upsert).
    pub filter_json: String,
    /// Optional keyword query (body FTS). Empty string stored as NULL.
    pub keyword: Option<String>,
    pub created_by: Option<String>,
}

/// One item's FTS bookkeeping assignment for transactional batch write (schema v10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtsFieldUpdate {
    pub item_id: String,
    pub fts_text_sha256: Option<String>,
    pub fts_indexed_at: Option<String>,
    pub fts_error: Option<String>,
}

/// Thin row for FTS index candidates (no body text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FtsCandidate {
    pub id: String,
    pub subject: Option<String>,
    pub title: Option<String>,
    pub path: Option<String>,
    pub text_sha256: Option<String>,
    pub html_sha256: Option<String>,
    pub fts_text_sha256: Option<String>,
    pub role: Option<String>,
    pub parent_item_id: Option<String>,
    pub family_id: Option<String>,
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
    // --- schema v16 (calendar) ---
    pub message_class: Option<Option<String>>,
    pub cal_start_at: Option<Option<String>>,
    pub cal_end_at: Option<Option<String>>,
    pub cal_all_day: Option<Option<i64>>,
    pub cal_location: Option<Option<String>>,
    pub cal_organizer: Option<Option<String>>,
    pub cal_attendees_json: Option<Option<String>>,
    pub cal_busy_status: Option<Option<String>>,
    pub cal_is_recurring: Option<Option<i64>>,
    pub cal_recurrence_id: Option<Option<String>>,
    pub cal_uid: Option<Option<String>>,
    pub cal_extract_method: Option<Option<String>>,
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
        // Seed default coding catalog (idempotent).
        matter.seed_default_codes()?;

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
            // Seed on writer open/migrate only — avoid writes on open_for_read.
            matter.seed_default_codes()?;
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

    /// On-disk byte length of a CAS blob (metadata only; no full read).
    pub fn cas_len(&self, digest_hex: &str) -> Result<u64> {
        self.cas.blob_len(digest_hex)
    }

    /// Get raw bytes only when the on-disk length is `<= max_bytes`.
    pub fn get_bytes_capped(&self, digest_hex: &str, max_bytes: u64) -> Result<Vec<u8>> {
        self.cas.get_bytes_capped(digest_hex, max_bytes)
    }

    /// Read at most `max_bytes` from the start of a CAS blob (prefix / magic head).
    ///
    /// Unlike [`Self::get_bytes_capped`], this never loads the full object when it
    /// is larger than `max_bytes` — suitable for file-type sniffing.
    pub fn read_cas_prefix(&self, digest_hex: &str, max_bytes: usize) -> Result<Vec<u8>> {
        use std::io::Read;
        let mut file = self.cas.open_read(digest_hex)?;
        let mut buf = vec![0u8; max_bytes];
        let n = file.read(&mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Whether a blob with this digest exists.
    pub fn blob_exists(&self, digest_hex: &str) -> Result<bool> {
        self.cas.exists(digest_hex)
    }

    // --- Jobs / checkpoints ---

    /// Create a new root job in `pending` state (`parent_job_id` is `None`).
    pub fn create_job(&self, kind: &str) -> Result<Job> {
        self.create_job_with_parent(kind, None)
    }

    /// Create a job, optionally nested under a parent orchestration job.
    ///
    /// When `parent_job_id` is `Some`, the parent must exist and belong to this matter.
    pub fn create_job_with_parent(&self, kind: &str, parent_job_id: Option<&str>) -> Result<Job> {
        if let Some(parent_id) = parent_job_id {
            let parent = jobs::get_job(&self.conn, parent_id)?;
            if parent.matter_id != self.matter_id {
                return Err(Error::Other(format!(
                    "parent job {parent_id} belongs to another matter"
                )));
            }
        }
        let id = new_id("job");
        let now = now_rfc3339();
        let job = jobs::create_job(&self.conn, &id, &self.matter_id, kind, &now, parent_job_id)?;
        let mut params = serde_json::json!({ "kind": kind });
        if let Some(pid) = parent_job_id {
            params["parent_job_id"] = serde_json::Value::String(pid.to_string());
        }
        let _ = self.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "job.create".into(),
            entity: format!("job:{id}"),
            params_json: params.to_string(),
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

    /// List direct child jobs of `parent_job_id` (oldest first).
    pub fn list_child_jobs(&self, parent_job_id: &str) -> Result<Vec<Job>> {
        // Ensure parent exists (and belongs to this matter via get + check).
        let parent = jobs::get_job(&self.conn, parent_job_id)?;
        if parent.matter_id != self.matter_id {
            return Err(Error::Other(format!(
                "parent job {parent_job_id} belongs to another matter"
            )));
        }
        jobs::list_child_jobs(&self.conn, parent_job_id)
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
                in_review, review_set_id, review_order, promoted_at, promote_job_id, promote_policy, \
                message_class, cal_start_at, cal_end_at, cal_all_day, cal_location, \
                cal_organizer, cal_attendees_json, cal_busy_status, cal_is_recurring, \
                cal_recurrence_id, cal_uid, cal_extract_method\
             ) VALUES (\
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, \
                ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, \
                ?26, ?27, ?28, ?29, ?30, ?31, ?32, \
                ?33, ?34, ?35, ?36, ?37, ?38, \
                ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46, ?47, \
                ?48, ?49, ?50, ?51, ?52, ?53, ?54, \
                ?55, ?56, ?57, ?58, ?59, ?60, \
                ?61, ?62, ?63, ?64, ?65, ?66, \
                ?67, ?68, ?69, ?70, ?71, ?72, ?73, ?74, ?75, ?76, ?77, ?78\
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
                input.message_class,
                input.cal_start_at,
                input.cal_end_at,
                input.cal_all_day,
                input.cal_location,
                input.cal_organizer,
                input.cal_attendees_json,
                input.cal_busy_status,
                input.cal_is_recurring,
                input.cal_recurrence_id,
                input.cal_uid,
                input.cal_extract_method,
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
        // Defense-in-depth: body digest change severs redacted produce pointer (0032).
        // Compare before move of current digests into apply_opt2. Plain text **or**
        // HTML body change invalidates the redacted artifact (Review may display
        // either CAS).
        let text_sha256_changed = match &update.text_sha256 {
            Some(new) => new.as_ref() != current.text_sha256.as_ref(),
            None => false,
        };
        let html_sha256_changed = match &update.html_sha256 {
            Some(new) => new.as_ref() != current.html_sha256.as_ref(),
            None => false,
        };
        let body_digest_changed = text_sha256_changed || html_sha256_changed;

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
        let message_class = apply_opt2(update.message_class, current.message_class);
        let cal_start_at = apply_opt2(update.cal_start_at, current.cal_start_at);
        let cal_end_at = apply_opt2(update.cal_end_at, current.cal_end_at);
        let cal_all_day = apply_opt2(update.cal_all_day, current.cal_all_day);
        let cal_location = apply_opt2(update.cal_location, current.cal_location);
        let cal_organizer = apply_opt2(update.cal_organizer, current.cal_organizer);
        let cal_attendees_json = apply_opt2(update.cal_attendees_json, current.cal_attendees_json);
        let cal_busy_status = apply_opt2(update.cal_busy_status, current.cal_busy_status);
        let cal_is_recurring = apply_opt2(update.cal_is_recurring, current.cal_is_recurring);
        let cal_recurrence_id = apply_opt2(update.cal_recurrence_id, current.cal_recurrence_id);
        let cal_uid = apply_opt2(update.cal_uid, current.cal_uid);
        let cal_extract_method = apply_opt2(update.cal_extract_method, current.cal_extract_method);

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
                promoted_at = ?61, promote_job_id = ?62, promote_policy = ?63, \
                redacted_text_sha256 = CASE WHEN ?64 THEN NULL ELSE redacted_text_sha256 END, \
                redacted_text_at = CASE WHEN ?64 THEN NULL ELSE redacted_text_at END, \
                redacted_source_digest = CASE WHEN ?64 THEN NULL ELSE redacted_source_digest END, \
                message_class = ?65, cal_start_at = ?66, cal_end_at = ?67, cal_all_day = ?68, \
                cal_location = ?69, cal_organizer = ?70, cal_attendees_json = ?71, \
                cal_busy_status = ?72, cal_is_recurring = ?73, cal_recurrence_id = ?74, \
                cal_uid = ?75, cal_extract_method = ?76 \
             WHERE id = ?77",
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
                body_digest_changed,
                message_class,
                cal_start_at,
                cal_end_at,
                cal_all_day,
                cal_location,
                cal_organizer,
                cal_attendees_json,
                cal_busy_status,
                cal_is_recurring,
                cal_recurrence_id,
                cal_uid,
                cal_extract_method,
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
                    text_sha256, role, imported_at, cull_status \
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

    // --- Review list helpers (schema v7 / track 0026) ---

    /// Id of the default review set, if any.
    pub fn get_default_review_set_id(&self) -> Result<Option<String>> {
        Ok(self.get_default_review_set()?.map(|s| s.id))
    }

    /// Count of items in the review corpus.
    ///
    /// - `set_id = Some(id)` → items with `in_review = 1` in that set
    /// - `set_id = None` → default set if present, else all `in_review = 1`
    pub fn count_in_review(&self, set_id: Option<&str>) -> Result<u64> {
        let resolved = self.resolve_review_set_filter(set_id)?;
        let n: i64 = match resolved.as_deref() {
            Some(sid) => self.conn.query_row(
                "SELECT COUNT(*) FROM items \
                 WHERE matter_id = ?1 AND in_review = 1 AND review_set_id = ?2",
                params![self.matter_id, sid],
                |row| row.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT COUNT(*) FROM items \
                 WHERE matter_id = ?1 AND in_review = 1",
                params![self.matter_id],
                |row| row.get(0),
            )?,
        };
        Ok(n as u64)
    }

    /// Thin review rows ordered by `review_order ASC` (then `id` for stability).
    ///
    /// Does **not** load body text or large participant JSON. See
    /// [`ReviewListRow`].
    ///
    /// Set filter semantics match [`Self::count_in_review`].
    pub fn list_review_thin(
        &self,
        set_id: Option<&str>,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<ReviewListRow>> {
        let resolved = self.resolve_review_set_filter(set_id)?;
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let offset_i = offset as i64;
        // ORDER BY matches filtered list (filter::order_by_clause): NULLS LAST
        // for review_order, then imported_at, path, id (stable deep OFFSET).
        let sql = match resolved.as_deref() {
            Some(_) => {
                "SELECT id, review_order, role, parent_item_id, subject, from_addr, \
                        sent_at, received_at, path, file_category, mime_type, size_bytes, \
                        text_sha256, html_sha256, dedup_role, cull_status, attachment_count, \
                        family_id \
                 FROM items \
                 WHERE matter_id = ?1 AND in_review = 1 AND review_set_id = ?2 \
                 ORDER BY (review_order IS NULL), review_order ASC, imported_at ASC, path ASC, id ASC \
                 LIMIT ?3 OFFSET ?4"
            }
            None => {
                "SELECT id, review_order, role, parent_item_id, subject, from_addr, \
                        sent_at, received_at, path, file_category, mime_type, size_bytes, \
                        text_sha256, html_sha256, dedup_role, cull_status, attachment_count, \
                        family_id \
                 FROM items \
                 WHERE matter_id = ?1 AND in_review = 1 \
                 ORDER BY (review_order IS NULL), review_order ASC, imported_at ASC, path ASC, id ASC \
                 LIMIT ?2 OFFSET ?3"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match resolved.as_deref() {
            Some(sid) => {
                let mapped = stmt.query_map(
                    params![self.matter_id, sid, limit_i, offset_i],
                    map_review_list_row,
                )?;
                mapped.collect::<std::result::Result<Vec<_>, _>>()?
            }
            None => {
                let mapped = stmt.query_map(
                    params![self.matter_id, limit_i, offset_i],
                    map_review_list_row,
                )?;
                mapped.collect::<std::result::Result<Vec<_>, _>>()?
            }
        };
        Ok(rows)
    }

    /// Resolve optional set filter: explicit id, else default set id, else None
    /// (meaning "all `in_review = 1`").
    fn resolve_review_set_filter(&self, set_id: Option<&str>) -> Result<Option<String>> {
        if let Some(sid) = set_id {
            return Ok(Some(sid.to_string()));
        }
        self.get_default_review_set_id()
    }

    // --- Metadata filters + saved searches (schema v9 / track 0028) ---
    // --- FTS bookkeeping + id-restricted filter (schema v10 / track 0029) ---

    /// Count items matching a metadata [`FilterSpec`].
    ///
    /// For `scope = review_corpus`, uses the default review set when present
    /// (same semantics as [`Self::count_in_review`] with `set_id = None`).
    /// Family expand counts **distinct outer** ids, not hit count alone.
    pub fn count_items_filtered(&self, spec: &FilterSpec) -> Result<u64> {
        let compiled = self.compile_filter_for_matter(spec)?;
        let n: i64 = self.conn.query_row(
            &compiled.count_sql,
            params_from_iter(compiled.params.iter().cloned()),
            |row| row.get(0),
        )?;
        Ok(n as u64)
    }

    /// Thin filtered rows for the Review list (same columns as [`ReviewListRow`]).
    ///
    /// ORDER BY: `review_order` NULLS LAST, then `imported_at`, `path`, `id`.
    /// User values are bound parameters only — see [`filter::compile_filter`].
    pub fn list_items_filtered_thin(
        &self,
        spec: &FilterSpec,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<ReviewListRow>> {
        let compiled = self.compile_filter_for_matter(spec)?;
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let offset_i = offset as i64;
        let mut params = compiled.params;
        params.push(Value::Integer(limit_i));
        params.push(Value::Integer(offset_i));
        let mut stmt = self.conn.prepare(&compiled.list_sql)?;
        let rows = stmt.query_map(params_from_iter(params), map_review_list_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Count items matching [`FilterSpec`] restricted to `item_ids` (FTS ∩ filter).
    ///
    /// When `include_family` is true: intersect first (filter on FTS hits only),
    /// then expand family membership on the outer result (0029 lock).
    pub fn count_items_filtered_in_ids(
        &self,
        spec: &FilterSpec,
        item_ids: &[String],
    ) -> Result<u64> {
        if item_ids.is_empty() {
            return Ok(0);
        }
        self.with_fts_hit_temp(item_ids, || {
            let compiled = self.compile_filter_intersect_hits(spec)?;
            let n: i64 = self.conn.query_row(
                &compiled.count_sql,
                params_from_iter(compiled.params.iter().cloned()),
                |row| row.get(0),
            )?;
            Ok(n as u64)
        })
    }

    /// Thin filtered rows restricted to FTS hit ids (temp-table join).
    ///
    /// Same columns/order as [`Self::list_items_filtered_thin`]. Family expand
    /// (if requested) applies **after** FTS ∩ metadata intersection.
    pub fn list_items_filtered_thin_in_ids(
        &self,
        spec: &FilterSpec,
        item_ids: &[String],
        limit: u64,
        offset: u64,
    ) -> Result<Vec<ReviewListRow>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_fts_hit_temp(item_ids, || {
            let compiled = self.compile_filter_intersect_hits(spec)?;
            let limit_i = if limit == u64::MAX {
                i64::MAX
            } else {
                limit as i64
            };
            let offset_i = offset as i64;
            let mut params = compiled.params;
            params.push(Value::Integer(limit_i));
            params.push(Value::Integer(offset_i));
            let mut stmt = self.conn.prepare(&compiled.list_sql)?;
            let rows = stmt.query_map(params_from_iter(params), map_review_list_row)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::from)
        })
    }

    /// Eligible FTS candidates with text or HTML CAS, ordered stably.
    ///
    /// Status filter: extracted-like (`extracted` / `partial` / `normalized`).
    pub fn list_fts_candidates(&self, offset: u64, limit: u64) -> Result<Vec<FtsCandidate>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let mut stmt = self.conn.prepare(
            "SELECT id, subject, title, path, text_sha256, html_sha256, fts_text_sha256, \
                    role, parent_item_id, family_id \
             FROM items \
             WHERE matter_id = ?1 \
               AND status IN ('extracted', 'partial', 'normalized') \
               AND (text_sha256 IS NOT NULL OR html_sha256 IS NOT NULL) \
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(
            params![self.matter_id, limit_i, offset as i64],
            map_fts_candidate_row,
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Items that still have FTS bookkeeping but are no longer index-eligible
    /// (no text CAS, or status not extracted-like). The FTS job should
    /// `delete_term` these and clear `fts_*` so searches do not return ghosts.
    pub fn list_fts_orphans(&self, offset: u64, limit: u64) -> Result<Vec<String>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };
        let mut stmt = self.conn.prepare(
            "SELECT id FROM items \
             WHERE matter_id = ?1 \
               AND fts_text_sha256 IS NOT NULL \
               AND ( \
                 status NOT IN ('extracted', 'partial', 'normalized') \
                 OR (text_sha256 IS NULL AND html_sha256 IS NULL) \
               ) \
             ORDER BY id ASC \
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(params![self.matter_id, limit_i, offset as i64], |row| {
            row.get::<_, String>(0)
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Attachment child paths keyed by parent item id (role = `attachment`).
    pub fn list_attachment_names_for_parents(
        &self,
        parent_ids: &[String],
    ) -> Result<HashMap<String, Vec<String>>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        if parent_ids.is_empty() {
            return Ok(out);
        }
        // Chunk to avoid huge IN lists.
        const CHUNK: usize = 500;
        for chunk in parent_ids.chunks(CHUNK) {
            let placeholders: String = (1..=chunk.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT parent_item_id, path FROM items \
                 WHERE matter_id = ?{mat} \
                   AND IFNULL(role, '') = 'attachment' \
                   AND parent_item_id IN ({placeholders}) \
                 ORDER BY parent_item_id ASC, path ASC, id ASC",
                mat = chunk.len() + 1
            );
            let mut params: Vec<Value> = chunk.iter().map(|id| Value::Text(id.clone())).collect();
            params.push(Value::Text(self.matter_id.clone()));
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(params), |row| {
                let parent: String = row.get(0)?;
                let path: Option<String> = row.get(1)?;
                Ok((parent, path))
            })?;
            for row in rows {
                let (parent, path) = row?;
                if let Some(p) = path {
                    if !p.is_empty() {
                        out.entry(parent).or_default().push(p);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Clear all FTS bookkeeping columns for this matter (full rebuild prep).
    pub fn clear_fts_fields(&self) -> Result<u64> {
        let matter_id = self.matter_id.clone();
        self.with_transaction(|conn| {
            let n = conn.execute(
                "UPDATE items SET fts_text_sha256 = NULL, fts_indexed_at = NULL, fts_error = NULL \
                 WHERE matter_id = ?1",
                params![matter_id],
            )?;
            Ok(n as u64)
        })
    }

    /// Apply N FTS field updates and upsert the job checkpoint in **one**
    /// SQLite transaction (same pattern as near-dup / promote).
    pub fn apply_fts_batch_with_checkpoint(
        &self,
        job_id: &str,
        stage: &str,
        updates: &[FtsFieldUpdate],
        cursor_json: &str,
        completed_count: i64,
    ) -> Result<()> {
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            for u in updates {
                conn.execute(
                    "UPDATE items SET \
                        fts_text_sha256 = ?1, fts_indexed_at = ?2, fts_error = ?3 \
                     WHERE id = ?4",
                    params![u.fts_text_sha256, u.fts_indexed_at, u.fts_error, u.item_id,],
                )?;
            }
            jobs::put_checkpoint(conn, job_id, stage, cursor_json, completed_count, &now)?;
            Ok(())
        })
    }

    /// Compile a filter with this matter's id and default review-set resolution.
    fn compile_filter_for_matter(&self, spec: &FilterSpec) -> Result<filter::CompiledFilter> {
        let review_set_id = if spec.scope == filter::SCOPE_REVIEW_CORPUS {
            self.resolve_review_set_filter(None)?
        } else {
            None
        };
        filter::compile_filter(spec, &self.matter_id, review_set_id.as_deref())
    }

    /// Compile FilterSpec ∩ temp FTS hit ids; family expand after intersect when set.
    fn compile_filter_intersect_hits(&self, spec: &FilterSpec) -> Result<filter::CompiledFilter> {
        let review_set_id = if spec.scope == filter::SCOPE_REVIEW_CORPUS {
            self.resolve_review_set_filter(None)?
        } else {
            None
        };
        // Force non-family compile, then wrap with hit restriction (+ optional family).
        let mut intersect_spec = spec.clone();
        let want_family = intersect_spec.include_family;
        intersect_spec.include_family = false;
        let compiled =
            filter::compile_filter(&intersect_spec, &self.matter_id, review_set_id.as_deref())?;

        // Inject `AND i.id IN (SELECT id FROM temp_fts_hits)` into the WHERE of
        // the non-family list/count SQL.
        let list_sql = inject_fts_hit_restriction(&compiled.list_sql);
        let count_sql = inject_fts_hit_restriction(&compiled.count_sql);

        if !want_family {
            return Ok(filter::CompiledFilter {
                list_sql,
                count_sql,
                params: compiled.params,
            });
        }

        // Family expand after intersect: hits CTE = filtered ∩ FTS; outer expands.
        let matter_id = &self.matter_id;
        let mut outer_params: Vec<Value> = Vec::new();
        let mut outer_scope: Vec<String> = Vec::new();
        outer_scope.push("out.matter_id = ?".into());
        outer_params.push(Value::Text(matter_id.to_string()));
        match spec.scope.as_str() {
            filter::SCOPE_REVIEW_CORPUS => {
                outer_scope.push("out.in_review = 1".into());
                if let Some(sid) = review_set_id.as_deref() {
                    outer_scope.push("out.review_set_id = ?".into());
                    outer_params.push(Value::Text(sid.to_string()));
                }
            }
            filter::SCOPE_ENTIRE_MATTER => {
                outer_scope.push("out.status IN ('extracted', 'partial', 'normalized')".into());
            }
            _ => {}
        }
        let outer_where = outer_scope.join(" AND ");
        let order = filter::order_by_clause("out");
        let cols = filter::thin_columns_sql_qualified("out");

        // Hits WHERE params from the intersected non-family filter.
        // Extract the WHERE clause body from count_sql after injection.
        // Simpler: rebuild hits from items with same params as compiled.
        // compiled.count_sql is like: SELECT COUNT(*) FROM items i WHERE ... AND i.id IN (...)
        // We need the WHERE portion. Use list approach with CTE.
        let where_sql = extract_where_clause(&list_sql).ok_or_else(|| {
            Error::Other("failed to extract WHERE from filtered-in-ids SQL".into())
        })?;

        let mut all_params = compiled.params;
        all_params.extend(outer_params);

        let list_sql = format!(
            "WITH hits AS ( \
                 SELECT i.id, i.family_id, \
                        COALESCE(i.parent_item_id, i.id) AS family_root \
                 FROM items i \
                 WHERE {where_sql} \
             ) \
             SELECT DISTINCT {cols} \
             FROM items out \
             WHERE {outer_where} \
               AND ( \
                 (out.family_id IS NOT NULL AND out.family_id IN ( \
                     SELECT family_id FROM hits WHERE family_id IS NOT NULL \
                 )) \
                 OR out.id IN (SELECT family_root FROM hits) \
                 OR out.parent_item_id IN (SELECT family_root FROM hits) \
               ) \
             ORDER BY {order} \
             LIMIT ? OFFSET ?"
        );
        let count_sql = format!(
            "WITH hits AS ( \
                 SELECT i.id, i.family_id, \
                        COALESCE(i.parent_item_id, i.id) AS family_root \
                 FROM items i \
                 WHERE {where_sql} \
             ) \
             SELECT COUNT(*) FROM ( \
                 SELECT DISTINCT out.id \
                 FROM items out \
                 WHERE {outer_where} \
                   AND ( \
                     (out.family_id IS NOT NULL AND out.family_id IN ( \
                         SELECT family_id FROM hits WHERE family_id IS NOT NULL \
                     )) \
                     OR out.id IN (SELECT family_root FROM hits) \
                     OR out.parent_item_id IN (SELECT family_root FROM hits) \
                   ) \
             )"
        );

        Ok(filter::CompiledFilter {
            list_sql,
            count_sql,
            params: all_params,
        })
    }

    /// Populate a connection-local TEMP table of FTS hit ids for the closure.
    fn with_fts_hit_temp<T>(
        &self,
        item_ids: &[String],
        f: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS temp_fts_hits (id TEXT PRIMARY KEY NOT NULL);
             DELETE FROM temp_fts_hits;",
        )?;
        {
            let mut stmt = self
                .conn
                .prepare("INSERT OR IGNORE INTO temp_fts_hits (id) VALUES (?1)")?;
            for id in item_ids {
                stmt.execute(params![id])?;
            }
        }
        let result = f();
        let _ = self.conn.execute_batch("DELETE FROM temp_fts_hits;");
        result
    }

    /// List saved searches for this matter, ordered by name.
    pub fn list_saved_searches(&self) -> Result<Vec<SavedSearch>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, matter_id, name, description, scope, filter_json, keyword, \
                    created_at, updated_at, created_by \
             FROM saved_searches WHERE matter_id = ?1 ORDER BY name ASC",
        )?;
        let rows = stmt.query_map(params![self.matter_id], map_saved_search_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Load a saved search by id (scoped to this matter).
    pub fn get_saved_search(&self, search_id: &str) -> Result<SavedSearch> {
        self.conn
            .query_row(
                "SELECT id, matter_id, name, description, scope, filter_json, keyword, \
                        created_at, updated_at, created_by \
                 FROM saved_searches WHERE id = ?1 AND matter_id = ?2",
                params![search_id, self.matter_id],
                map_saved_search_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("saved search not found: {search_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// Insert or update a saved search. Name unique per matter (DB + app).
    ///
    /// Validates `filter_json` as a [`FilterSpec`] and audits `search.save`.
    pub fn upsert_saved_search(&self, input: SavedSearchInput) -> Result<SavedSearch> {
        let now = now_rfc3339();
        let name = input.name.trim();
        if name.is_empty() {
            return Err(Error::Other("saved search name cannot be empty".into()));
        }
        if input.filter_json.trim().is_empty() {
            return Err(Error::Other(
                "saved search filter_json cannot be empty".into(),
            ));
        }
        let spec: FilterSpec = serde_json::from_str(&input.filter_json)
            .map_err(|e| Error::Other(format!("invalid filter_json: {e}")))?;
        // Validate compile (dates, field/ops, scope).
        let _ = self.compile_filter_for_matter(&spec)?;
        let scope = spec.scope.clone();
        let filter_json = serde_json::to_string(&spec)?;
        let keyword = input
            .keyword
            .as_ref()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());

        let saved = if let Some(ref id) = input.id {
            let existing = self.get_saved_search(id)?;
            if existing.matter_id != self.matter_id {
                return Err(Error::Other(format!(
                    "saved search {id} belongs to another matter"
                )));
            }
            let clash: Option<String> = self
                .conn
                .query_row(
                    "SELECT id FROM saved_searches WHERE matter_id = ?1 AND name = ?2 AND id != ?3",
                    params![self.matter_id, name, id],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "saved search name already exists in matter: {name}"
                )));
            }
            self.conn.execute(
                "UPDATE saved_searches SET name = ?1, description = ?2, scope = ?3, \
                 filter_json = ?4, keyword = ?5, updated_at = ?6, \
                 created_by = COALESCE(?7, created_by) \
                 WHERE id = ?8",
                params![
                    name,
                    input.description,
                    scope,
                    filter_json,
                    keyword,
                    now,
                    input.created_by,
                    id
                ],
            )?;
            self.get_saved_search(id)?
        } else {
            let clash: Option<String> = self
                .conn
                .query_row(
                    "SELECT id FROM saved_searches WHERE matter_id = ?1 AND name = ?2",
                    params![self.matter_id, name],
                    |row| row.get(0),
                )
                .optional()?;
            if clash.is_some() {
                return Err(Error::Other(format!(
                    "saved search name already exists in matter: {name}"
                )));
            }
            let id = new_id("ssr");
            self.conn.execute(
                "INSERT INTO saved_searches (id, matter_id, name, description, scope, \
                 filter_json, keyword, created_at, updated_at, created_by) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    self.matter_id,
                    name,
                    input.description,
                    scope,
                    filter_json,
                    keyword,
                    now,
                    now,
                    input.created_by
                ],
            )?;
            self.get_saved_search(&id)?
        };

        let _ = self.append_audit(AuditEventInput {
            actor: input.created_by.clone().unwrap_or_else(|| "system".into()),
            action: "search.save".into(),
            entity: format!("saved_search:{}", saved.id),
            params_json: serde_json::json!({
                "name": saved.name,
                "scope": saved.scope,
                "has_keyword": saved.keyword.is_some(),
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;

        Ok(saved)
    }
    /// Delete a saved search. Does **not** affect item codes or membership.
    ///
    /// Audits `search.delete`.
    pub fn delete_saved_search(&self, search_id: &str) -> Result<()> {
        let existing = self.get_saved_search(search_id)?;
        if existing.matter_id != self.matter_id {
            return Err(Error::Other(format!(
                "saved search {search_id} belongs to another matter"
            )));
        }
        self.conn.execute(
            "DELETE FROM saved_searches WHERE id = ?1",
            params![search_id],
        )?;
        let _ = self.append_audit(AuditEventInput {
            actor: existing
                .created_by
                .clone()
                .unwrap_or_else(|| "system".into()),
            action: "search.delete".into(),
            entity: format!("saved_search:{search_id}"),
            params_json: serde_json::json!({
                "name": existing.name,
            })
            .to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
        Ok(())
    }

    // --- Coding / tags (schema v8 / track 0027) ---

    /// Seed the default code catalog. Idempotent insert-if-missing by `key`.
    pub fn seed_default_codes(&self) -> Result<()> {
        let now = now_rfc3339();
        const DEFAULTS: &[(&str, &str, &str, &str, i64)] = &[
            ("responsive", "Responsive", "responsiveness", "single", 10),
            (
                "not_responsive",
                "Not Responsive",
                "responsiveness",
                "single",
                20,
            ),
            (
                "needs_second_look",
                "Needs Second Look",
                "responsiveness",
                "single",
                30,
            ),
            ("privilege", "Privilege", "privilege", "multi", 40),
            ("hot", "Hot / Key", "issues", "multi", 50),
            ("confidential", "Confidential", "issues", "multi", 60),
        ];
        for &(key, label, group_key, cardinality, sort_order) in DEFAULTS {
            let exists: bool = self.conn.query_row(
                "SELECT COUNT(*) > 0 FROM code_definitions \
                 WHERE matter_id = ?1 AND key = ?2",
                params![self.matter_id, key],
                |row| row.get(0),
            )?;
            if exists {
                continue;
            }
            let id = new_id("cde");
            self.conn.execute(
                "INSERT INTO code_definitions \
                 (id, matter_id, key, label, group_key, cardinality, color, sort_order, is_active, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, 1, ?8)",
                params![
                    id,
                    self.matter_id,
                    key,
                    label,
                    group_key,
                    cardinality,
                    sort_order,
                    now
                ],
            )?;
        }
        Ok(())
    }

    /// List all code definitions for this matter (active and inactive), ordered.
    pub fn list_code_definitions(&self) -> Result<Vec<CodeDef>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, matter_id, key, label, group_key, cardinality, color, sort_order, \
                    is_active, created_at, guidance \
             FROM code_definitions \
             WHERE matter_id = ?1 \
             ORDER BY sort_order ASC, key ASC",
        )?;
        let rows = stmt.query_map(params![self.matter_id], map_code_def_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Insert or update a code definition. Returns the definition id.
    ///
    /// On insert with no `key`, derives a slug from `label`. Unique on
    /// `(matter_id, key)`.
    pub fn upsert_code_definition(&self, input: CodeDefInput) -> Result<String> {
        let now = now_rfc3339();
        let label = input.label.trim();
        if label.is_empty() {
            return Err(Error::Other("code label cannot be empty".into()));
        }
        let group_key = input.group_key.trim();
        if group_key.is_empty() {
            return Err(Error::Other("code group_key cannot be empty".into()));
        }
        let cardinality = match input.cardinality.trim() {
            "" => "multi".to_string(),
            "single" | "multi" => input.cardinality.trim().to_string(),
            other => {
                return Err(Error::Other(format!(
                    "invalid cardinality '{other}' (expected single|multi)"
                )));
            }
        };
        let is_active: i64 = if input.is_active { 1 } else { 0 };
        let guidance = input
            .guidance
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        if let Some(ref id) = input.id {
            let existing = self.get_code_definition(id)?;
            if existing.matter_id != self.matter_id {
                return Err(Error::Other(format!(
                    "code definition {id} belongs to another matter"
                )));
            }
            // Key is immutable on update (stable machine key).
            // guidance: Some updates; None leaves existing value.
            if input.guidance.is_some() {
                self.conn.execute(
                    "UPDATE code_definitions SET label = ?1, group_key = ?2, cardinality = ?3, \
                     color = ?4, sort_order = ?5, is_active = ?6, guidance = ?7 WHERE id = ?8",
                    params![
                        label,
                        group_key,
                        cardinality,
                        input.color,
                        input.sort_order,
                        is_active,
                        guidance,
                        id
                    ],
                )?;
            } else {
                self.conn.execute(
                    "UPDATE code_definitions SET label = ?1, group_key = ?2, cardinality = ?3, \
                     color = ?4, sort_order = ?5, is_active = ?6 WHERE id = ?7",
                    params![
                        label,
                        group_key,
                        cardinality,
                        input.color,
                        input.sort_order,
                        is_active,
                        id
                    ],
                )?;
            }
            return Ok(id.clone());
        }

        let key = match input
            .key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(k) => slugify_code_key(k),
            None => slugify_code_key(label),
        };
        if key.is_empty() {
            return Err(Error::Other(
                "code key cannot be empty after slugify".into(),
            ));
        }
        let clash: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM code_definitions WHERE matter_id = ?1 AND key = ?2",
                params![self.matter_id, key],
                |row| row.get(0),
            )
            .optional()?;
        if clash.is_some() {
            return Err(Error::Other(format!(
                "code key already exists in matter: {key}"
            )));
        }
        let id = new_id("cde");
        self.conn.execute(
            "INSERT INTO code_definitions \
             (id, matter_id, key, label, group_key, cardinality, color, sort_order, is_active, created_at, guidance) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                self.matter_id,
                key,
                label,
                group_key,
                cardinality,
                input.color,
                input.sort_order,
                is_active,
                now,
                guidance
            ],
        )?;
        Ok(id)
    }

    /// Load one code definition by id.
    pub fn get_code_definition(&self, code_id: &str) -> Result<CodeDef> {
        self.conn
            .query_row(
                "SELECT id, matter_id, key, label, group_key, cardinality, color, sort_order, \
                        is_active, created_at, guidance \
                 FROM code_definitions WHERE id = ?1",
                params![code_id],
                map_code_def_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("code definition not found: {code_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// Batch-load codes for the given item ids. Includes inactive definitions
    /// that still have membership (historical display).
    pub fn list_item_codes<S: AsRef<str>>(
        &self,
        item_ids: &[S],
    ) -> Result<HashMap<String, Vec<ItemCodeInfo>>> {
        let mut out: HashMap<String, Vec<ItemCodeInfo>> = HashMap::new();
        if item_ids.is_empty() {
            return Ok(out);
        }
        for id in item_ids {
            out.entry(id.as_ref().to_string()).or_default();
        }
        // Chunk IN-lists to stay under SQLite variable limits.
        const CHUNK: usize = 400;
        for chunk in item_ids.chunks(CHUNK) {
            let placeholders: String = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let sql = format!(
                "SELECT ic.item_id, ic.code_id, cd.key, cd.label, cd.group_key, cd.cardinality, \
                        cd.color, cd.sort_order, cd.is_active, ic.set_at, ic.set_by \
                 FROM item_codes ic \
                 INNER JOIN code_definitions cd ON cd.id = ic.code_id \
                 WHERE ic.item_id IN ({placeholders}) \
                 ORDER BY cd.sort_order ASC, cd.key ASC"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let params_iter =
                rusqlite::params_from_iter(chunk.iter().map(|s| s.as_ref().to_string()));
            let rows = stmt.query_map(params_iter, |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ItemCodeInfo {
                        code_id: row.get(1)?,
                        key: row.get(2)?,
                        label: row.get(3)?,
                        group_key: row.get(4)?,
                        cardinality: row.get(5)?,
                        color: row.get(6)?,
                        sort_order: row.get(7)?,
                        is_active: row.get(8)?,
                        set_at: row.get(9)?,
                        set_by: row.get(10)?,
                    },
                ))
            })?;
            for row in rows {
                let (item_id, info) = row?;
                out.entry(item_id).or_default().push(info);
            }
        }
        Ok(out)
    }

    /// Apply add/remove coding ops to selected items (optional whole-family expand).
    ///
    /// Single `BEGIN IMMEDIATE` transaction: membership writes + `coding.apply`
    /// audit with the **complete** sorted `item_ids` of final targets (never
    /// hashed or sampled). Failed batch leaves no partial membership.
    pub fn apply_codes(&self, input: ApplyCodesInput) -> Result<ApplyCodesResult> {
        if input.item_ids.is_empty() {
            return Err(Error::Other(
                "apply_codes requires at least one item_id".into(),
            ));
        }
        if input.add_code_ids.is_empty() && input.remove_code_ids.is_empty() {
            return Err(Error::Other(
                "apply_codes requires at least one add or remove code id".into(),
            ));
        }
        let actor = {
            let t = input.actor.trim();
            if t.is_empty() {
                "desk".to_string()
            } else {
                t.to_string()
            }
        };

        // Resolve definitions once (validate existence).
        let mut add_defs: Vec<CodeDef> = Vec::with_capacity(input.add_code_ids.len());
        for cid in &input.add_code_ids {
            let def = self.get_code_definition(cid)?;
            if def.matter_id != self.matter_id {
                return Err(Error::Other(format!(
                    "code definition {cid} belongs to another matter"
                )));
            }
            add_defs.push(def);
        }
        let mut remove_defs: Vec<CodeDef> = Vec::with_capacity(input.remove_code_ids.len());
        for cid in &input.remove_code_ids {
            let def = self.get_code_definition(cid)?;
            if def.matter_id != self.matter_id {
                return Err(Error::Other(format!(
                    "code definition {cid} belongs to another matter"
                )));
            }
            remove_defs.push(def);
        }

        // Reject conflicting single-group adds in one batch (do not silently
        // pick one via iteration order). Check before any membership/audit write.
        {
            use std::collections::BTreeMap;
            let mut by_group: BTreeMap<&str, Vec<&CodeDef>> = BTreeMap::new();
            for def in &add_defs {
                if def.cardinality == "single" {
                    let entry = by_group.entry(def.group_key.as_str()).or_default();
                    if !entry.iter().any(|d| d.id == def.id) {
                        entry.push(def);
                    }
                }
            }
            for (group_key, defs) in by_group {
                if defs.len() >= 2 {
                    let mut keys: Vec<&str> = defs.iter().map(|d| d.key.as_str()).collect();
                    keys.sort_unstable();
                    return Err(Error::Other(format!(
                        "conflicting single-group codes in one apply for group '{group_key}': {} \
                         (cardinality=single allows only one code per group)",
                        keys.join(", ")
                    )));
                }
            }
        }

        // Stable order for multi-group adds (non-conflicting).
        add_defs.sort_by(|a, b| {
            a.sort_order
                .cmp(&b.sort_order)
                .then_with(|| a.key.cmp(&b.key))
                .then_with(|| a.id.cmp(&b.id))
        });
        remove_defs.sort_by(|a, b| {
            a.sort_order
                .cmp(&b.sort_order)
                .then_with(|| a.key.cmp(&b.key))
                .then_with(|| a.id.cmp(&b.id))
        });

        // Validate selected items exist in this matter.
        for iid in &input.item_ids {
            let ok: bool = self.conn.query_row(
                "SELECT COUNT(*) > 0 FROM items WHERE id = ?1 AND matter_id = ?2",
                params![iid, self.matter_id],
                |row| row.get(0),
            )?;
            if !ok {
                return Err(Error::ItemNotFound(iid.clone()));
            }
        }

        let selected_count = input.item_ids.len();
        let mut targets: Vec<String> = if input.propagate_family {
            self.expand_family_units(&input.item_ids)?
        } else {
            input.item_ids.clone()
        };
        // Stable sorted unique list for audit + apply.
        targets.sort();
        targets.dedup();
        let target_count = targets.len();

        let add_keys: Vec<String> = add_defs.iter().map(|d| d.key.clone()).collect();
        let remove_keys: Vec<String> = remove_defs.iter().map(|d| d.key.clone()).collect();
        let now = now_rfc3339();
        let entity = if target_count == 1 {
            format!("item:{}", targets[0])
        } else {
            "batch".to_string()
        };
        let params_json = serde_json::json!({
            "item_ids": targets,
            "add": add_keys,
            "remove": remove_keys,
            "propagate_family": input.propagate_family,
            "selected_count": selected_count,
            "target_count": target_count,
        })
        .to_string();

        let add_privilege = add_defs.iter().any(|d| d.key == "privilege");
        let remove_privilege = remove_defs.iter().any(|d| d.key == "privilege");

        self.with_transaction(|conn| {
            let mut privilege_ensure_ids: Vec<String> = Vec::new();
            let mut privilege_clear_ids: Vec<String> = Vec::new();

            for item_id in &targets {
                // Adds first (with single-group clear), then removes — per spec §3.3.2.
                for def in &add_defs {
                    if def.cardinality == "single" {
                        // Remove other codes in the same group_key on this item.
                        conn.execute(
                            "DELETE FROM item_codes \
                             WHERE item_id = ?1 AND code_id IN ( \
                                 SELECT id FROM code_definitions \
                                 WHERE matter_id = ?2 AND group_key = ?3 AND id != ?4 \
                             )",
                            params![item_id, self.matter_id, def.group_key, def.id],
                        )?;
                    }
                    conn.execute(
                        "INSERT INTO item_codes (item_id, code_id, set_at, set_by) \
                         VALUES (?1, ?2, ?3, ?4) \
                         ON CONFLICT(item_id, code_id) DO UPDATE SET set_at = excluded.set_at, \
                         set_by = excluded.set_by",
                        params![item_id, def.id, now, actor],
                    )?;
                }
                for def in &remove_defs {
                    conn.execute(
                        "DELETE FROM item_codes WHERE item_id = ?1 AND code_id = ?2",
                        params![item_id, def.id],
                    )?;
                }

                // Privilege code ↔ claim lifecycle (same txn; separate audit).
                if add_privilege {
                    let ch = crate::privilege::ensure_item_privilege_conn(
                        conn,
                        &self.matter_id,
                        item_id,
                        &actor,
                        &now,
                    )?;
                    if ch == crate::privilege::PrivilegeEnsureChange::Changed {
                        privilege_ensure_ids.push(item_id.clone());
                    }
                }
                if remove_privilege {
                    let cleared = crate::privilege::soft_clear_item_privilege_conn(
                        conn,
                        &self.matter_id,
                        item_id,
                        &actor,
                        &now,
                    )?;
                    if cleared {
                        privilege_clear_ids.push(item_id.clone());
                    }
                }
            }

            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "coding.apply".into(),
                    entity: entity.clone(),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;

            // Distinct privilege audit events with full sorted item_ids.
            Matter::audit_privilege_batch_upsert(conn, &actor, &privilege_ensure_ids, &now)?;
            Matter::audit_privilege_batch_clear(conn, &actor, &privilege_clear_ids, &now)?;
            Ok(())
        })?;

        Ok(ApplyCodesResult {
            target_item_ids: targets,
            selected_count,
            target_count,
        })
    }

    // --- Notes / highlights (schema v11 / track 0030) ---

    /// List notes for an item (newest `updated_at` first).
    pub fn list_notes(&self, item_id: &str) -> Result<Vec<ItemNote>> {
        self.ensure_item_in_matter(item_id)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, item_id, matter_id, body, highlight_id, created_at, updated_at, \
                    created_by, updated_by \
             FROM item_notes \
             WHERE item_id = ?1 AND matter_id = ?2 \
             ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![item_id, self.matter_id], map_note_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Create or update a note body. Rejects blank / oversize bodies.
    ///
    /// Single SQLite transaction + `note.upsert` audit (full body in payload).
    pub fn upsert_note(&self, input: UpsertNoteInput) -> Result<ItemNote> {
        let actor = normalize_actor(&input.actor);
        let body = input.body.trim();
        if body.is_empty() {
            return Err(Error::Other(
                "note body cannot be empty or whitespace-only".into(),
            ));
        }
        if body.len() > NOTE_BODY_MAX_BYTES {
            return Err(Error::Other(format!(
                "note body exceeds max size of {NOTE_BODY_MAX_BYTES} bytes (got {})",
                body.len()
            )));
        }
        let now = now_rfc3339();

        if let Some(ref id) = input.id {
            // Update path.
            let existing = self.get_note(id)?;
            if existing.matter_id != self.matter_id {
                return Err(Error::Other(format!("note {id} belongs to another matter")));
            }
            if !input.item_id.is_empty() && input.item_id != existing.item_id {
                return Err(Error::Other(format!(
                    "note {id} belongs to item {}, not {}",
                    existing.item_id, input.item_id
                )));
            }
            let item_id = existing.item_id.clone();
            let params_json = serde_json::json!({
                "note_id": id,
                "item_id": item_id,
                "op": "update",
                "highlight_id": existing.highlight_id,
                "body": body,
            })
            .to_string();
            self.with_transaction(|conn| {
                conn.execute(
                    "UPDATE item_notes SET body = ?1, updated_at = ?2, updated_by = ?3 \
                     WHERE id = ?4",
                    params![body, now, actor, id],
                )?;
                audit::append_event(
                    conn,
                    &AuditEventInput {
                        actor: actor.clone(),
                        action: "note.upsert".into(),
                        entity: format!("note:{id}"),
                        params_json: params_json.clone(),
                        tool_version: env!("CARGO_PKG_VERSION").into(),
                    },
                    &now,
                )?;
                Ok(())
            })?;
            return self.get_note(id);
        }

        // Create path.
        self.ensure_item_in_matter(&input.item_id)?;
        if let Some(ref hid) = input.highlight_id {
            let hl = self.get_highlight(hid)?;
            if hl.item_id != input.item_id {
                return Err(Error::Other(format!(
                    "highlight {hid} belongs to a different item"
                )));
            }
        }
        let id = new_id("note");
        let item_id = input.item_id.clone();
        let highlight_id = input.highlight_id.clone();
        let params_json = serde_json::json!({
            "note_id": id,
            "item_id": item_id,
            "op": "create",
            "highlight_id": highlight_id,
            "body": body,
        })
        .to_string();
        self.with_transaction(|conn| {
            conn.execute(
                "INSERT INTO item_notes \
                 (id, item_id, matter_id, body, highlight_id, created_at, updated_at, \
                  created_by, updated_by) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    item_id,
                    self.matter_id,
                    body,
                    highlight_id,
                    now,
                    now,
                    actor,
                    actor
                ],
            )?;
            conn.execute(
                "UPDATE items SET note_count = note_count + 1 \
                 WHERE id = ?1 AND matter_id = ?2",
                params![item_id, self.matter_id],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "note.upsert".into(),
                    entity: format!("note:{id}"),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;
        self.get_note(&id)
    }

    /// Hard-delete a note. Audit retains body snapshot + `highlight_id` when set.
    pub fn delete_note(&self, note_id: &str, actor: &str) -> Result<()> {
        let actor = normalize_actor(actor);
        let existing = self.get_note(note_id)?;
        if existing.matter_id != self.matter_id {
            return Err(Error::Other(format!(
                "note {note_id} belongs to another matter"
            )));
        }
        let now = now_rfc3339();
        let params_json = serde_json::json!({
            "note_id": note_id,
            "item_id": existing.item_id,
            "body": existing.body,
            "highlight_id": existing.highlight_id,
        })
        .to_string();
        self.with_transaction(|conn| {
            conn.execute("DELETE FROM item_notes WHERE id = ?1", params![note_id])?;
            conn.execute(
                "UPDATE items SET note_count = MAX(0, note_count - 1) \
                 WHERE id = ?1 AND matter_id = ?2",
                params![existing.item_id, self.matter_id],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "note.delete".into(),
                    entity: format!("note:{note_id}"),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })
    }

    /// Load one note by id.
    pub fn get_note(&self, note_id: &str) -> Result<ItemNote> {
        self.conn
            .query_row(
                "SELECT id, item_id, matter_id, body, highlight_id, created_at, updated_at, \
                        created_by, updated_by \
                 FROM item_notes WHERE id = ?1",
                params![note_id],
                map_note_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("note not found: {note_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// List highlights for an item (creation order).
    pub fn list_highlights(&self, item_id: &str) -> Result<Vec<ItemHighlight>> {
        self.ensure_item_in_matter(item_id)?;
        let mut stmt = self.conn.prepare(
            "SELECT id, item_id, matter_id, start_utf8, end_utf8, exact_quote, prefix, suffix, \
                    body_digest, color, status, created_at, updated_at, created_by \
             FROM item_highlights \
             WHERE item_id = ?1 AND matter_id = ?2 \
             ORDER BY start_utf8 ASC, created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![item_id, self.matter_id], map_highlight_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Create a stand-off highlight. Validates range + quote match against display body.
    pub fn create_highlight(&self, input: CreateHighlightInput) -> Result<ItemHighlight> {
        let actor = normalize_actor(&input.actor);
        self.ensure_item_in_matter(&input.item_id)?;

        if input.end_utf8 <= input.start_utf8 {
            return Err(Error::Other(format!(
                "highlight range invalid: end ({}) must be > start ({})",
                input.end_utf8, input.start_utf8
            )));
        }
        if input.start_utf8 < 0 {
            return Err(Error::Other("highlight start_utf8 must be >= 0".into()));
        }
        let start = input.start_utf8 as usize;
        let end = input.end_utf8 as usize;
        let body_chars = input.display_body.chars().count();
        if end > body_chars {
            return Err(Error::Other(format!(
                "highlight end_utf8 ({end}) exceeds display body char length ({body_chars})"
            )));
        }
        let slice = match utf8_char_slice(&input.display_body, start, end) {
            Some(s) => s,
            None => {
                return Err(Error::Other(
                    "highlight range does not map to a valid char slice".into(),
                ));
            }
        };
        if slice != input.exact_quote {
            return Err(Error::Other(
                "exact_quote does not match display body at [start_utf8, end_utf8)".into(),
            ));
        }
        if input.exact_quote.len() > HIGHLIGHT_QUOTE_MAX_BYTES {
            return Err(Error::Other(format!(
                "exact_quote exceeds max size of {HIGHLIGHT_QUOTE_MAX_BYTES} bytes"
            )));
        }
        if input.body_digest.trim().is_empty() {
            return Err(Error::Other("body_digest cannot be empty".into()));
        }
        let color = input
            .color
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(HIGHLIGHT_DEFAULT_COLOR)
            .to_string();
        let prefix = utf8_char_slice(
            &input.display_body,
            start.saturating_sub(HIGHLIGHT_CONTEXT_CHARS),
            start,
        )
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
        let suffix = utf8_char_slice(
            &input.display_body,
            end,
            (end + HIGHLIGHT_CONTEXT_CHARS).min(body_chars),
        )
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

        let id = new_id("hlt");
        let now = now_rfc3339();
        let quote_for_audit = truncate_for_audit(&input.exact_quote, 512);
        let params_json = serde_json::json!({
            "highlight_id": id,
            "item_id": input.item_id,
            "start_utf8": input.start_utf8,
            "end_utf8": input.end_utf8,
            "quote": quote_for_audit,
            "color": color,
        })
        .to_string();

        self.with_transaction(|conn| {
            conn.execute(
                "INSERT INTO item_highlights \
                 (id, item_id, matter_id, start_utf8, end_utf8, exact_quote, prefix, suffix, \
                  body_digest, color, status, created_at, updated_at, created_by) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    id,
                    input.item_id,
                    self.matter_id,
                    input.start_utf8,
                    input.end_utf8,
                    input.exact_quote,
                    prefix,
                    suffix,
                    input.body_digest,
                    color,
                    highlight_status::ACTIVE,
                    now,
                    now,
                    actor
                ],
            )?;
            conn.execute(
                "UPDATE items SET highlight_count = highlight_count + 1 \
                 WHERE id = ?1 AND matter_id = ?2",
                params![input.item_id, self.matter_id],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "highlight.create".into(),
                    entity: format!("highlight:{id}"),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;
        self.get_highlight(&id)
    }

    /// Delete a highlight and **unlink** notes (`highlight_id` → NULL). Does not
    /// delete note bodies.
    pub fn delete_highlight(&self, highlight_id: &str, actor: &str) -> Result<()> {
        let actor = normalize_actor(actor);
        let existing = self.get_highlight(highlight_id)?;
        if existing.matter_id != self.matter_id {
            return Err(Error::Other(format!(
                "highlight {highlight_id} belongs to another matter"
            )));
        }
        let now = now_rfc3339();

        // Collect linked note ids for optional audit context.
        let mut linked: Vec<String> = Vec::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM item_notes WHERE highlight_id = ?1 AND matter_id = ?2")?;
            let rows = stmt.query_map(params![highlight_id, self.matter_id], |row| {
                row.get::<_, String>(0)
            })?;
            for r in rows {
                linked.push(r?);
            }
        }

        let params_json = serde_json::json!({
            "highlight_id": highlight_id,
            "item_id": existing.item_id,
            "start_utf8": existing.start_utf8,
            "end_utf8": existing.end_utf8,
            "quote": truncate_for_audit(&existing.exact_quote, 512),
            "color": existing.color,
            "unlinked_note_ids": linked,
        })
        .to_string();

        self.with_transaction(|conn| {
            conn.execute(
                "UPDATE item_notes SET highlight_id = NULL, updated_at = ?1, updated_by = ?2 \
                 WHERE highlight_id = ?3 AND matter_id = ?4",
                params![now, actor, highlight_id, self.matter_id],
            )?;
            conn.execute(
                "DELETE FROM item_highlights WHERE id = ?1",
                params![highlight_id],
            )?;
            conn.execute(
                "UPDATE items SET highlight_count = MAX(0, highlight_count - 1) \
                 WHERE id = ?1 AND matter_id = ?2",
                params![existing.item_id, self.matter_id],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "highlight.delete".into(),
                    entity: format!("highlight:{highlight_id}"),
                    params_json: params_json.clone(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })
    }

    /// Load one highlight by id.
    pub fn get_highlight(&self, highlight_id: &str) -> Result<ItemHighlight> {
        self.conn
            .query_row(
                "SELECT id, item_id, matter_id, start_utf8, end_utf8, exact_quote, prefix, suffix, \
                        body_digest, color, status, created_at, updated_at, created_by \
                 FROM item_highlights WHERE id = ?1",
                params![highlight_id],
                map_highlight_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("highlight not found: {highlight_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// Resolve highlights for paint against the current display body.
    ///
    /// When `body_digest` matches, uses stored offsets (fast path). On mismatch,
    /// applies whitespace-normalized quote re-resolve (§3.5.1). Optionally
    /// persists `status=stale` when re-resolve fails (`persist_stale`).
    pub fn resolve_highlights(
        &self,
        item_id: &str,
        display_body: &str,
        display_digest: &str,
        persist_stale: bool,
    ) -> Result<Vec<ResolvedHighlight>> {
        let highlights = self.list_highlights(item_id)?;
        let mut out = Vec::with_capacity(highlights.len());
        let mut stale_ids: Vec<String> = Vec::new();
        for hl in highlights {
            let resolved = resolve_highlight_against_body(&hl, display_body, display_digest);
            if resolved.status == highlight_status::STALE
                && hl.status != highlight_status::STALE
                && persist_stale
            {
                stale_ids.push(hl.id.clone());
            }
            out.push(resolved);
        }
        if persist_stale && !stale_ids.is_empty() {
            let now = now_rfc3339();
            self.with_transaction(|conn| {
                for id in &stale_ids {
                    conn.execute(
                        "UPDATE item_highlights SET status = ?1, updated_at = ?2 WHERE id = ?3",
                        params![highlight_status::STALE, now, id],
                    )?;
                }
                Ok(())
            })?;
        }
        Ok(out)
    }

    pub(crate) fn ensure_item_in_matter(&self, item_id: &str) -> Result<()> {
        let ok: bool = self.conn.query_row(
            "SELECT COUNT(*) > 0 FROM items WHERE id = ?1 AND matter_id = ?2",
            params![item_id, self.matter_id],
            |row| row.get(0),
        )?;
        if !ok {
            return Err(Error::ItemNotFound(item_id.to_string()));
        }
        Ok(())
    }

    /// Expand selected item ids to whole family units (parent + all direct
    /// children + same non-null `family_id` members). Does **not** expand
    /// near-dup groups or full threads.
    pub(crate) fn expand_family_units(&self, item_ids: &[String]) -> Result<Vec<String>> {
        let mut out: HashSet<String> = HashSet::new();
        for iid in item_ids {
            let (parent_item_id, family_id): (Option<String>, Option<String>) =
                self.conn.query_row(
                    "SELECT parent_item_id, family_id FROM items \
                     WHERE id = ?1 AND matter_id = ?2",
                    params![iid, self.matter_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
            let parent = parent_item_id.unwrap_or_else(|| iid.clone());
            out.insert(parent.clone());

            // All direct children of the parent.
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM items WHERE matter_id = ?1 AND parent_item_id = ?2")?;
            let children = stmt.query_map(params![self.matter_id, parent], |row| {
                row.get::<_, String>(0)
            })?;
            for c in children {
                out.insert(c?);
            }

            // Prefer also including members sharing the parent's non-null family_id.
            let parent_family: Option<String> = if family_id.is_some() && parent == *iid {
                family_id
            } else {
                self.conn
                    .query_row(
                        "SELECT family_id FROM items WHERE id = ?1 AND matter_id = ?2",
                        params![parent, self.matter_id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten()
            };
            if let Some(fid) = parent_family {
                let mut stmt = self
                    .conn
                    .prepare("SELECT id FROM items WHERE matter_id = ?1 AND family_id = ?2")?;
                let members =
                    stmt.query_map(params![self.matter_id, fid], |row| row.get::<_, String>(0))?;
                for m in members {
                    out.insert(m?);
                }
            }
        }
        Ok(out.into_iter().collect())
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
    in_review, review_set_id, review_order, promoted_at, promote_job_id, promote_policy, \
    redaction_count, redacted_text_sha256, redacted_text_at, redacted_source_digest, \
    office_extract_status, office_extract_method, office_source_native_sha256, \
    office_extracted_at, office_extract_error, \
    pdf_extract_status, pdf_extract_method, pdf_source_native_sha256, \
    pdf_extracted_at, pdf_extract_error, pdf_page_count, pdf_needs_ocr, \
    message_class, cal_start_at, cal_end_at, cal_all_day, cal_location, \
    cal_organizer, cal_attendees_json, cal_busy_status, cal_is_recurring, \
    cal_recurrence_id, cal_uid, cal_extract_method, \
    ics_extract_status, ics_extract_method, ics_source_native_sha256, \
    ics_extracted_at, ics_extract_error, \
    ocr_status, ocr_engine, ocr_lang, ocr_text_sha256, ocr_source_native_sha256, \
    ocr_page_count, ocr_at, ocr_error, ocr_confidence, \
    category_method, category_taxonomy, category_status, category_error, categorized_at, \
    entity_flags, entity_scan_at, entity_scan_job_id, entity_hit_count, entity_scanned_text_sha256, \
    concept_cluster_id, concept_cluster_set_id, concept_clustered_at, \
    sentiment_compound, sentiment_compound_min, sentiment_compound_max, \
    sentiment_pos, sentiment_neu, sentiment_neg, sentiment_polarity, sentiment_method, \
    sentiment_pos_threshold, sentiment_neg_threshold, sentiment_scanned_text_sha256, \
    sentiment_scanned_at, sentiment_job_id, \
    semantic_embedded_text_sha256, semantic_embedded_at, semantic_chunk_count";

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
        redaction_count: row.get(66)?,
        redacted_text_sha256: row.get(67)?,
        redacted_text_at: row.get(68)?,
        redacted_source_digest: row.get(69)?,
        office_extract_status: row.get(70)?,
        office_extract_method: row.get(71)?,
        office_source_native_sha256: row.get(72)?,
        office_extracted_at: row.get(73)?,
        office_extract_error: row.get(74)?,
        pdf_extract_status: row.get(75)?,
        pdf_extract_method: row.get(76)?,
        pdf_source_native_sha256: row.get(77)?,
        pdf_extracted_at: row.get(78)?,
        pdf_extract_error: row.get(79)?,
        pdf_page_count: row.get(80)?,
        pdf_needs_ocr: row.get::<_, Option<i64>>(81)?.unwrap_or(0),
        message_class: row.get(82)?,
        cal_start_at: row.get(83)?,
        cal_end_at: row.get(84)?,
        cal_all_day: row.get(85)?,
        cal_location: row.get(86)?,
        cal_organizer: row.get(87)?,
        cal_attendees_json: row.get(88)?,
        cal_busy_status: row.get(89)?,
        cal_is_recurring: row.get(90)?,
        cal_recurrence_id: row.get(91)?,
        cal_uid: row.get(92)?,
        cal_extract_method: row.get(93)?,
        ics_extract_status: row.get(94)?,
        ics_extract_method: row.get(95)?,
        ics_source_native_sha256: row.get(96)?,
        ics_extracted_at: row.get(97)?,
        ics_extract_error: row.get(98)?,
        ocr_status: row.get(99)?,
        ocr_engine: row.get(100)?,
        ocr_lang: row.get(101)?,
        ocr_text_sha256: row.get(102)?,
        ocr_source_native_sha256: row.get(103)?,
        ocr_page_count: row.get(104)?,
        ocr_at: row.get(105)?,
        ocr_error: row.get(106)?,
        ocr_confidence: row.get(107)?,
        category_method: row.get(108)?,
        category_taxonomy: row.get(109)?,
        category_status: row.get(110)?,
        category_error: row.get(111)?,
        categorized_at: row.get(112)?,
        entity_flags: row.get::<_, Option<i64>>(113)?.unwrap_or(0),
        entity_scan_at: row.get(114)?,
        entity_scan_job_id: row.get(115)?,
        entity_hit_count: row.get::<_, Option<i64>>(116)?.unwrap_or(0),
        entity_scanned_text_sha256: row.get(117)?,
        concept_cluster_id: row.get(118)?,
        concept_cluster_set_id: row.get(119)?,
        concept_clustered_at: row.get(120)?,
        sentiment_compound: row.get(121)?,
        sentiment_compound_min: row.get(122)?,
        sentiment_compound_max: row.get(123)?,
        sentiment_pos: row.get(124)?,
        sentiment_neu: row.get(125)?,
        sentiment_neg: row.get(126)?,
        sentiment_polarity: row.get(127)?,
        sentiment_method: row.get(128)?,
        sentiment_pos_threshold: row.get(129)?,
        sentiment_neg_threshold: row.get(130)?,
        sentiment_scanned_text_sha256: row.get(131)?,
        sentiment_scanned_at: row.get(132)?,
        sentiment_job_id: row.get(133)?,
        semantic_embedded_text_sha256: row.get(134)?,
        semantic_embedded_at: row.get(135)?,
        semantic_chunk_count: row.get(136)?,
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

fn map_review_list_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewListRow> {
    Ok(ReviewListRow {
        id: row.get(0)?,
        review_order: row.get(1)?,
        role: row.get(2)?,
        parent_item_id: row.get(3)?,
        subject: row.get(4)?,
        from_addr: row.get(5)?,
        sent_at: row.get(6)?,
        received_at: row.get(7)?,
        path: row.get(8)?,
        file_category: row.get(9)?,
        mime_type: row.get(10)?,
        size_bytes: row.get(11)?,
        text_sha256: row.get(12)?,
        html_sha256: row.get(13)?,
        dedup_role: row.get(14)?,
        cull_status: row.get(15)?,
        attachment_count: row.get(16)?,
        family_id: row.get(17)?,
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
        cull_status: row.get(19)?,
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

fn map_saved_search_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedSearch> {
    Ok(SavedSearch {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        scope: row.get(4)?,
        filter_json: row.get(5)?,
        keyword: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        created_by: row.get(9)?,
    })
}

fn map_fts_candidate_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FtsCandidate> {
    Ok(FtsCandidate {
        id: row.get(0)?,
        subject: row.get(1)?,
        title: row.get(2)?,
        path: row.get(3)?,
        text_sha256: row.get(4)?,
        html_sha256: row.get(5)?,
        fts_text_sha256: row.get(6)?,
        role: row.get(7)?,
        parent_item_id: row.get(8)?,
        family_id: row.get(9)?,
    })
}

/// Inject FTS hit-id restriction into non-family filter SQL.
///
/// Inserts `AND i.id IN (SELECT id FROM temp_fts_hits)` before `ORDER BY` /
/// end of statement. Expects the items alias `i` used by [`filter::compile_filter`].
fn inject_fts_hit_restriction(sql: &str) -> String {
    const CLAUSE: &str = " AND i.id IN (SELECT id FROM temp_fts_hits)";
    // Prefer inserting before ORDER BY (list) or at end (count).
    if let Some(idx) = sql.find(" ORDER BY ") {
        let mut s = String::with_capacity(sql.len() + CLAUSE.len());
        s.push_str(&sql[..idx]);
        s.push_str(CLAUSE);
        s.push_str(&sql[idx..]);
        s
    } else {
        format!("{sql}{CLAUSE}")
    }
}

/// Extract the WHERE clause body from a simple `… WHERE … [ORDER BY …] [LIMIT …]` SQL.
///
/// Returns the text after ` WHERE ` up to (not including) ` ORDER BY ` / ` LIMIT `.
fn extract_where_clause(sql: &str) -> Option<String> {
    let upper = sql.to_ascii_uppercase();
    let where_pos = upper.find(" WHERE ")?;
    let start = where_pos + " WHERE ".len();
    let rest = &sql[start..];
    let rest_upper = rest.to_ascii_uppercase();
    let end = rest_upper
        .find(" ORDER BY ")
        .or_else(|| rest_upper.find(" LIMIT "))
        .unwrap_or(rest.len());
    Some(rest[..end].trim().to_string())
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

fn map_code_def_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodeDef> {
    Ok(CodeDef {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        key: row.get(2)?,
        label: row.get(3)?,
        group_key: row.get(4)?,
        cardinality: row.get(5)?,
        color: row.get(6)?,
        sort_order: row.get(7)?,
        is_active: row.get(8)?,
        created_at: row.get(9)?,
        guidance: row.get(10)?,
    })
}

/// Stable machine key from a display label (or explicit key string).
fn slugify_code_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_us = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_us = false;
        } else if !prev_us && !out.is_empty() {
            out.push('_');
            prev_us = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

pub(crate) fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn new_id(prefix: &str) -> String {
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}_{nanos:x}_{n:x}")
}

pub(crate) fn normalize_actor(actor: &str) -> String {
    let t = actor.trim();
    if t.is_empty() {
        "desk".to_string()
    } else {
        t.to_string()
    }
}

fn truncate_for_audit(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    // Truncate on a char boundary.
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn map_note_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemNote> {
    Ok(ItemNote {
        id: row.get(0)?,
        item_id: row.get(1)?,
        matter_id: row.get(2)?,
        body: row.get(3)?,
        highlight_id: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        created_by: row.get(7)?,
        updated_by: row.get(8)?,
    })
}

fn map_highlight_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemHighlight> {
    Ok(ItemHighlight {
        id: row.get(0)?,
        item_id: row.get(1)?,
        matter_id: row.get(2)?,
        start_utf8: row.get(3)?,
        end_utf8: row.get(4)?,
        exact_quote: row.get(5)?,
        prefix: row.get(6)?,
        suffix: row.get(7)?,
        body_digest: row.get(8)?,
        color: row.get(9)?,
        status: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        created_by: row.get(13)?,
    })
}

/// SHA-256 hex digest of display body bytes (synthetic when no `text_sha256`).
pub fn display_body_digest(display_body: &str) -> String {
    crate::cas::sha256_hex(display_body.as_bytes())
}

/// Collapse every run of Unicode whitespace to a single ASCII space.
pub fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

/// Whitespace-normalized form used for quote match (collapse + trim ends).
pub fn normalize_for_quote_match(s: &str) -> String {
    collapse_whitespace(s).trim().to_string()
}

/// Slice `s` by UTF-8 **char** indices `[start, end)` (end exclusive).
pub fn utf8_char_slice(s: &str, start: usize, end: usize) -> Option<&str> {
    if end < start {
        return None;
    }
    let mut start_byte = None;
    let mut end_byte = None;
    for (i, (byte_idx, _)) in s.char_indices().enumerate() {
        if i == start {
            start_byte = Some(byte_idx);
        }
        if i == end {
            end_byte = Some(byte_idx);
            break;
        }
    }
    let char_count = s.chars().count();
    if end == char_count {
        end_byte = Some(s.len());
    }
    if start == char_count && end == char_count {
        return Some("");
    }
    let sb = start_byte?;
    let eb = end_byte?;
    s.get(sb..eb)
}

/// Build whitespace-collapsed body + map from normalized char index → raw char index.
///
/// `raw_at[i]` is the raw char index of the i-th normalized char;
/// `raw_at[norm_chars]` is `raw.chars().count()` (end sentinel).
fn build_whitespace_norm_map(raw: &str) -> (String, Vec<usize>) {
    let mut norm = String::with_capacity(raw.len());
    let mut raw_at: Vec<usize> = Vec::new();
    let mut raw_i = 0usize;
    let mut in_ws = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            if !in_ws {
                raw_at.push(raw_i);
                norm.push(' ');
                in_ws = true;
            }
        } else {
            raw_at.push(raw_i);
            norm.push(c);
            in_ws = false;
        }
        raw_i += 1;
    }
    raw_at.push(raw_i);
    (norm, raw_at)
}

fn byte_to_char_index(s: &str, byte: usize) -> usize {
    s.get(..byte).map(|p| p.chars().count()).unwrap_or(0)
}

/// Resolve one highlight against current display text (fast path + §3.5.1).
pub fn resolve_highlight_against_body(
    hl: &ItemHighlight,
    display_body: &str,
    display_digest: &str,
) -> ResolvedHighlight {
    // Fast path: digest matches → prefer stored offsets.
    if hl.body_digest == display_digest {
        let start = hl.start_utf8;
        let end = hl.end_utf8;
        let body_chars = display_body.chars().count() as i64;
        if start >= 0 && end > start && end <= body_chars {
            if let Some(slice) = utf8_char_slice(display_body, start as usize, end as usize) {
                if slice == hl.exact_quote {
                    return ResolvedHighlight {
                        highlight_id: hl.id.clone(),
                        start_utf8: start,
                        end_utf8: end,
                        status: highlight_status::ACTIVE.to_string(),
                        remapped: false,
                    };
                }
            }
        }
        // Digest matches but offsets/quote disagree — try re-resolve as repair.
    }

    match re_resolve_whitespace_normalized(hl, display_body) {
        Some((start, end)) => ResolvedHighlight {
            highlight_id: hl.id.clone(),
            start_utf8: start,
            end_utf8: end,
            status: highlight_status::ACTIVE.to_string(),
            remapped: true,
        },
        None => ResolvedHighlight {
            highlight_id: hl.id.clone(),
            start_utf8: hl.start_utf8,
            end_utf8: hl.end_utf8,
            status: highlight_status::STALE.to_string(),
            remapped: false,
        },
    }
}

/// Whitespace-normalized TextQuoteSelector-style re-resolve.
///
/// Returns raw char range on success.
pub fn re_resolve_whitespace_normalized(
    hl: &ItemHighlight,
    display_body: &str,
) -> Option<(i64, i64)> {
    let quote_n = normalize_for_quote_match(&hl.exact_quote);
    if quote_n.is_empty() {
        return None;
    }
    let (norm_body, raw_at) = build_whitespace_norm_map(display_body);
    // Context must keep boundary spaces so adjacency to the quote still matches
    // (trim would turn "alpha " into "alpha" and fail the immediate-prefix check).
    let prefix_n = hl
        .prefix
        .as_deref()
        .map(collapse_whitespace)
        .filter(|s| !s.is_empty());
    let suffix_n = hl
        .suffix
        .as_deref()
        .map(collapse_whitespace)
        .filter(|s| !s.is_empty());

    // Overlapping search: after a hit at index `i`, continue at `i+1` (one
    // normalized char), not `i + quote_len`. Otherwise quotes like "aba" in
    // "ababa" only report one hit and skip legitimate ambiguity.
    let mut hits: Vec<usize> = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = norm_body.get(search_from..).and_then(|s| s.find(&quote_n)) {
        let abs = search_from + rel;
        hits.push(abs);
        let step = norm_body[abs..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1)
            .max(1);
        search_from = abs + step;
        if search_from > norm_body.len() {
            break;
        }
    }
    if hits.is_empty() {
        return None;
    }

    let filtered: Vec<usize> = hits
        .into_iter()
        .filter(|&byte_start| {
            let ok_prefix = match &prefix_n {
                None => true,
                Some(p) => {
                    if byte_start < p.len() {
                        false
                    } else {
                        norm_body.get(byte_start - p.len()..byte_start) == Some(p.as_str())
                    }
                }
            };
            let ok_suffix = match &suffix_n {
                None => true,
                Some(s) => {
                    let after = byte_start + quote_n.len();
                    norm_body.get(after..after + s.len()) == Some(s.as_str())
                }
            };
            ok_prefix && ok_suffix
        })
        .collect();

    // Zero or ambiguous after disambiguation → stale.
    if filtered.len() != 1 {
        return None;
    }
    let byte_start = filtered[0];
    let byte_end = byte_start + quote_n.len();
    let norm_char_start = byte_to_char_index(&norm_body, byte_start);
    let norm_char_end = byte_to_char_index(&norm_body, byte_end);
    if norm_char_start >= raw_at.len() || norm_char_end >= raw_at.len() {
        return None;
    }
    let raw_start = raw_at[norm_char_start] as i64;
    // Exclusive end: next unit's raw index (or body end sentinel).
    let raw_end = raw_at[norm_char_end] as i64;
    if raw_end <= raw_start {
        return None;
    }
    Some((raw_start, raw_end))
}
