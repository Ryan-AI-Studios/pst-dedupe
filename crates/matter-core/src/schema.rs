//! Versioned SQLite schema migrations for matter.db.
//!
//! SQL is private to this crate. Callers interact through the public
//! [`crate::Matter`] API only.

use rusqlite::Connection;

use crate::error::{Error, Result};

/// Current schema version applied by this crate.
pub const SCHEMA_VERSION: u32 = 24;

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
    (12, MIGRATION_V12),
    (13, MIGRATION_V13),
    (14, MIGRATION_V14),
    (15, MIGRATION_V15),
    (16, MIGRATION_V16),
    (17, MIGRATION_V17),
    (18, MIGRATION_V18),
    (19, MIGRATION_V19),
    (20, MIGRATION_V20),
    (21, MIGRATION_V21),
    (22, MIGRATION_V22),
    (23, MIGRATION_V23),
    (24, MIGRATION_V24),
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
/// Nullable item columns + `review_sets` table. Flag-only membership Ã¢â‚¬â€ never
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

/// Schema v8: coding catalog + itemâ†”code membership (track 0027).
///
/// Matter-scoped code definitions and membership rows only â€” never deletes
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
/// metadata `filter_json`. Tantivy segments live under `index/` on disk â€” never
/// in SQLite (no FTS5 primary).
const MIGRATION_V10: &str = r#"
ALTER TABLE items ADD COLUMN fts_text_sha256 TEXT;
ALTER TABLE items ADD COLUMN fts_indexed_at TEXT;
ALTER TABLE items ADD COLUMN fts_error TEXT;
ALTER TABLE saved_searches ADD COLUMN keyword TEXT;
"#;

/// Schema v11: stand-off notes + text highlights (track 0030).
///
/// Work-product annotations beside the document â€” never rewrite CAS body text.
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

/// Schema v12: privilege claims + withhold holds + matter protocol (track 0031).
///
/// `item_privilege` is 1:1 with items when a claim exists (soft-clear retains row).
/// `privilege_protocol` is 1:1 with the matter (502(d)/502(e) notes are informational).
/// Denormalized `items.privilege_withhold` keeps list/filter chips fast for **0040**.
const MIGRATION_V12: &str = r#"
CREATE TABLE item_privilege (
    item_id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL,
    basis TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    withhold INTEGER NOT NULL,
    include_on_log INTEGER NOT NULL,
    asserted_at TEXT,
    asserted_by TEXT,
    updated_at TEXT NOT NULL,
    updated_by TEXT NOT NULL,
    extra_json TEXT
);

CREATE INDEX idx_item_privilege_matter_status
  ON item_privilege(matter_id, status);

CREATE INDEX idx_item_privilege_matter_withhold
  ON item_privilege(matter_id, withhold)
  WHERE withhold = 1;

CREATE INDEX idx_item_privilege_matter_log
  ON item_privilege(matter_id, include_on_log);

CREATE TABLE privilege_protocol (
    matter_id TEXT PRIMARY KEY NOT NULL,
    log_format TEXT NOT NULL DEFAULT 'standard',
    fre_502d_note TEXT,
    fre_502e_note TEXT,
    description_required INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL,
    updated_by TEXT NOT NULL
);

ALTER TABLE items ADD COLUMN privilege_withhold INTEGER NOT NULL DEFAULT 0;
"#;

/// Schema v13: text redaction regions + redacted produce artifact bookkeeping (track 0032).
///
/// Stand-off ranges on Review display text (same coordinate system as highlights).
/// Separate from `item_highlights` (black vs yellow; produce effect vs work product).
/// True redacted text is a **new** CAS blob; original `text_sha256` is never rewritten.
const MIGRATION_V13: &str = r#"
CREATE TABLE item_redactions (
    id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    matter_id TEXT NOT NULL,
    start_utf8 INTEGER NOT NULL,
    end_utf8 INTEGER NOT NULL,
    exact_quote TEXT NOT NULL,
    prefix TEXT,
    suffix TEXT,
    body_digest TEXT NOT NULL,
    reason TEXT NOT NULL,
    label TEXT,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_by TEXT
);

CREATE INDEX idx_item_redactions_item ON item_redactions(item_id);

CREATE INDEX idx_item_redactions_matter_status
  ON item_redactions(matter_id, status);

CREATE INDEX idx_item_redactions_matter_reason
  ON item_redactions(matter_id, reason);

ALTER TABLE items ADD COLUMN redaction_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE items ADD COLUMN redacted_text_sha256 TEXT;
ALTER TABLE items ADD COLUMN redacted_text_at TEXT;
ALTER TABLE items ADD COLUMN redacted_source_digest TEXT;
"#;

/// Schema v14: Office OOXML text extract bookkeeping (track 0033).
///
/// Nullable resume columns for `office_extract` job. Does **not** rewrite
/// native CAS; extracted plain text lands in `text_sha256` when non-empty.
const MIGRATION_V14: &str = r#"
ALTER TABLE items ADD COLUMN office_extract_status TEXT;
ALTER TABLE items ADD COLUMN office_extract_method TEXT;
ALTER TABLE items ADD COLUMN office_source_native_sha256 TEXT;
ALTER TABLE items ADD COLUMN office_extracted_at TEXT;
ALTER TABLE items ADD COLUMN office_extract_error TEXT;
"#;

/// Schema v15: PDF text extract bookkeeping (track 0034).
///
/// Includes `pdf_needs_ocr` for empty/low-text handoff to OCR (0036).
/// Does **not** add preview/raster columns (deferred).
const MIGRATION_V15: &str = r#"
ALTER TABLE items ADD COLUMN pdf_extract_status TEXT;
ALTER TABLE items ADD COLUMN pdf_extract_method TEXT;
ALTER TABLE items ADD COLUMN pdf_source_native_sha256 TEXT;
ALTER TABLE items ADD COLUMN pdf_extracted_at TEXT;
ALTER TABLE items ADD COLUMN pdf_extract_error TEXT;
ALTER TABLE items ADD COLUMN pdf_page_count INTEGER;
ALTER TABLE items ADD COLUMN pdf_needs_ocr INTEGER NOT NULL DEFAULT 0;
"#;

/// Schema v16: Calendar appointment fields + ICS extract bookkeeping (track 0035).
///
/// Structured cal_* columns for PST appointments / ICS VEVENTs, plus
/// `ics_*` job bookkeeping (mirrors office/pdf). Does **not** expand RRULEs.
const MIGRATION_V16: &str = r#"
ALTER TABLE items ADD COLUMN message_class TEXT;
ALTER TABLE items ADD COLUMN cal_start_at TEXT;
ALTER TABLE items ADD COLUMN cal_end_at TEXT;
ALTER TABLE items ADD COLUMN cal_all_day INTEGER;
ALTER TABLE items ADD COLUMN cal_location TEXT;
ALTER TABLE items ADD COLUMN cal_organizer TEXT;
ALTER TABLE items ADD COLUMN cal_attendees_json TEXT;
ALTER TABLE items ADD COLUMN cal_busy_status TEXT;
ALTER TABLE items ADD COLUMN cal_is_recurring INTEGER;
ALTER TABLE items ADD COLUMN cal_recurrence_id TEXT;
ALTER TABLE items ADD COLUMN cal_uid TEXT;
ALTER TABLE items ADD COLUMN cal_extract_method TEXT;
ALTER TABLE items ADD COLUMN ics_extract_status TEXT;
ALTER TABLE items ADD COLUMN ics_extract_method TEXT;
ALTER TABLE items ADD COLUMN ics_source_native_sha256 TEXT;
ALTER TABLE items ADD COLUMN ics_extracted_at TEXT;
ALTER TABLE items ADD COLUMN ics_extract_error TEXT;
"#;

/// Schema v17: OCR bookkeeping (track 0036).
///
/// Records engine/lang/page stats and CAS digests for offline OCR text.
/// Consumes `pdf_needs_ocr` handoff from extract-pdf (0034).
const MIGRATION_V17: &str = r#"
ALTER TABLE items ADD COLUMN ocr_status TEXT;
ALTER TABLE items ADD COLUMN ocr_engine TEXT;
ALTER TABLE items ADD COLUMN ocr_lang TEXT;
ALTER TABLE items ADD COLUMN ocr_text_sha256 TEXT;
ALTER TABLE items ADD COLUMN ocr_source_native_sha256 TEXT;
ALTER TABLE items ADD COLUMN ocr_page_count INTEGER;
ALTER TABLE items ADD COLUMN ocr_at TEXT;
ALTER TABLE items ADD COLUMN ocr_error TEXT;
ALTER TABLE items ADD COLUMN ocr_confidence REAL;
"#;

/// Schema v18: file-category bookkeeping (track 0037).
///
/// Records method/taxonomy/status for the `taxonomy_v1` classifier.
/// `file_category` remains the single filter/cull field.
const MIGRATION_V18: &str = r#"
ALTER TABLE items ADD COLUMN category_method TEXT;
ALTER TABLE items ADD COLUMN category_taxonomy TEXT;
ALTER TABLE items ADD COLUMN category_status TEXT;
ALTER TABLE items ADD COLUMN category_error TEXT;
ALTER TABLE items ADD COLUMN categorized_at TEXT;
"#;

/// Schema v19: supporting indexes for case overview rollups (track 0038).
///
/// Speeds `GROUP BY file_category` / `custodian` and top-level role filters.
/// No overview cache table (live SQL only).
const MIGRATION_V19: &str = r#"
CREATE INDEX IF NOT EXISTS idx_items_matter_file_category ON items(matter_id, file_category);
CREATE INDEX IF NOT EXISTS idx_items_matter_custodian ON items(matter_id, custodian);
CREATE INDEX IF NOT EXISTS idx_items_matter_role ON items(matter_id, role);
"#;

/// Schema v20: production sets + items for review-set produce (track 0040).
///
/// Bates/control assignment, status, and output layout bookkeeping for
/// natives + text + Concordance DAT packaging.
const MIGRATION_V20: &str = r#"
CREATE TABLE production_sets (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    bates_prefix TEXT NOT NULL,
    next_seq INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL,
    params_json TEXT,
    output_root TEXT,
    job_id TEXT
);

CREATE TABLE production_items (
    production_set_id TEXT NOT NULL REFERENCES production_sets(id),
    item_id TEXT NOT NULL REFERENCES items(id),
    control_number TEXT NOT NULL,
    native_relpath TEXT,
    text_relpath TEXT,
    status TEXT NOT NULL,
    skip_reason TEXT,
    error TEXT,
    produced_at TEXT,
    PRIMARY KEY (production_set_id, item_id)
);

CREATE UNIQUE INDEX idx_production_items_control
    ON production_items(production_set_id, control_number);
CREATE INDEX idx_production_sets_matter ON production_sets(matter_id);
CREATE INDEX idx_production_items_item ON production_items(item_id);
"#;

/// Schema v21: production QC run history for pre-produce gate (track 0041).
///
/// Stores pass/fail counts and a selection fingerprint so produce can refuse
/// stale QC (selection changed after the last pass).
const MIGRATION_V21: &str = r#"
CREATE TABLE qc_runs (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    profile TEXT NOT NULL,
    created_at TEXT NOT NULL,
    passed INTEGER NOT NULL,
    error_count INTEGER NOT NULL,
    warn_count INTEGER NOT NULL,
    candidate_count INTEGER NOT NULL,
    selection_fingerprint TEXT NOT NULL,
    scope TEXT NOT NULL,
    scope_json TEXT,
    report_path TEXT,
    job_id TEXT,
    rules_json TEXT
);

CREATE INDEX idx_qc_runs_matter_created ON qc_runs(matter_id, created_at);
"#;

/// Schema v22: gap analysis roster + opposing DAT expected docs (track 0042).
///
/// Expected custodians/sources for collection gap; gap_imports / gap_expected_docs
/// for opposing load-file set-diff; gap_runs for report history.
const MIGRATION_V22: &str = r#"
CREATE TABLE expected_custodians (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    name_norm TEXT NOT NULL,
    display_name TEXT NOT NULL,
    notes TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_expected_custodians_matter_name
    ON expected_custodians(matter_id, name_norm);
CREATE INDEX idx_expected_custodians_matter
    ON expected_custodians(matter_id);

CREATE TABLE expected_sources (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    label TEXT NOT NULL,
    label_norm TEXT NOT NULL,
    path_hint TEXT,
    kind TEXT,
    notes TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_expected_sources_matter_label
    ON expected_sources(matter_id, label_norm);
CREATE INDEX idx_expected_sources_matter
    ON expected_sources(matter_id);

CREATE TABLE gap_imports (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    row_count INTEGER NOT NULL,
    column_map_json TEXT,
    error_count INTEGER
);
CREATE INDEX idx_gap_imports_matter ON gap_imports(matter_id);

CREATE TABLE gap_expected_docs (
    id TEXT PRIMARY KEY NOT NULL,
    import_id TEXT NOT NULL REFERENCES gap_imports(id),
    control_number TEXT,
    sha256 TEXT,
    message_id TEXT,
    item_id TEXT,
    logical_hash TEXT,
    custodian TEXT,
    file_name TEXT,
    file_category TEXT,
    mime_type TEXT,
    file_ext TEXT,
    date_sent TEXT,
    date_received TEXT,
    date_created TEXT
);
CREATE INDEX idx_gap_expected_docs_import ON gap_expected_docs(import_id);

CREATE TABLE gap_runs (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    kind TEXT NOT NULL,
    params_json TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    error_count INTEGER NOT NULL DEFAULT 0,
    warn_count INTEGER NOT NULL DEFAULT 0,
    finding_count INTEGER NOT NULL DEFAULT 0,
    report_path TEXT,
    job_id TEXT,
    summary_json TEXT
);
CREATE INDEX idx_gap_runs_matter_started ON gap_runs(matter_id, started_at);
"#;

/// Schema v23: named processing profiles (track 0043).
///
/// User profiles store a versioned body_json map of stage kind → {enabled, params}.
/// Built-ins live as code constants (not DB rows). Optional matter default_profile_id.
const MIGRATION_V23: &str = r#"
CREATE TABLE processing_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    name TEXT NOT NULL,
    description TEXT,
    body_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_by TEXT
);
CREATE UNIQUE INDEX idx_processing_profiles_matter_name
    ON processing_profiles(matter_id, name);
CREATE INDEX idx_processing_profiles_matter
    ON processing_profiles(matter_id);
ALTER TABLE matters ADD COLUMN default_profile_id TEXT;
"#;

/// Schema v24: workflows + job parent linkage (track 0044).
///
/// User workflows store a versioned body_json of ordered nodes. Built-ins live as
/// code constants (not DB rows). `jobs.parent_job_id` links orchestrated children
/// to `workflow_run` / `profile_run` parents.
const MIGRATION_V24: &str = r#"
CREATE TABLE workflows (
    id TEXT PRIMARY KEY NOT NULL,
    matter_id TEXT NOT NULL REFERENCES matters(id),
    name TEXT NOT NULL,
    description TEXT,
    body_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_by TEXT
);
CREATE UNIQUE INDEX idx_workflows_matter_name ON workflows(matter_id, name);
CREATE INDEX idx_workflows_matter ON workflows(matter_id);
ALTER TABLE matters ADD COLUMN default_workflow_id TEXT;
ALTER TABLE jobs ADD COLUMN parent_job_id TEXT REFERENCES jobs(id);
CREATE INDEX idx_jobs_parent ON jobs(parent_job_id);
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
        assert_eq!(v, SCHEMA_VERSION);
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
        // v13 redactions
        let has_redactions: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_redactions'",
                [],
                |row| row.get(0),
            )
            .expect("item_redactions");
        assert!(has_redactions);
        let has_redaction_count: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = 'redaction_count'",
                [],
                |row| row.get(0),
            )
            .expect("pragma");
        assert!(has_redaction_count, "expected redaction_count on items");
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

        // schema_meta already at SCHEMA_VERSION Ã¢â‚¬â€ migrate is a no-op for steps
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

    /// v1 fixture (0016-style inventory) Ã¢â€ â€™ migrate to v2 Ã¢â€ â€™ data intact + new columns.
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

        let v = migrate(&conn).expect("migrate v1Ã¢â€ â€™v6");
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

        // New nullable columns readable as NULL (pre-v2 inventory; NULL role Ã¢â€°Â¡
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

    /// v2 fixture Ã¢â€ â€™ migrate to current Ã¢â€ â€™ data intact + dedupe/thread columns.
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

        let v = migrate(&conn).expect("migrate v2Ã¢â€ â€™v6");
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

    /// v3 fixture Ã¢â€ â€™ migrate to current Ã¢â€ â€™ data intact + thread columns present.
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

        let v = migrate(&conn).expect("migrate v3Ã¢â€ â€™v6");
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

    /// v4 fixture Ã¢â€ â€™ migrate to v5 Ã¢â€ â€™ data intact + near-dup columns present.
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

        let v = migrate(&conn).expect("migrate v4Ã¢â€ â€™v6");
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

    /// v5 fixture Ã¢â€ â€™ migrate to v6 Ã¢â€ â€™ data intact + cull columns + cull_presets table.
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

        let v = migrate(&conn).expect("migrate v5Ã¢â€ â€™current");
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

    /// v6 fixture Ã¢â€ â€™ migrate to v7 Ã¢â€ â€™ data intact + review columns + review_sets.
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

    /// v7 fixture â†’ migrate to current â†’ data intact + coding tables.
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
        assert_eq!(v, SCHEMA_VERSION);

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

    /// v8 fixture â†’ migrate to v9 â†’ data intact + saved_searches + list index.
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
        assert_eq!(v, SCHEMA_VERSION);

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

    /// v9 fixture â†’ migrate to current â†’ data intact + fts_* + keyword columns.
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

    /// v10 fixture â†’ migrate to v11 â†’ data intact + notes/highlights tables + counts.
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

        let v = migrate(&conn).expect("migrate v10 to current");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, SCHEMA_VERSION);

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

        let has_priv: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_privilege'",
                [],
                |row| row.get(0),
            )
            .expect("item_privilege");
        assert!(has_priv);
    }

    /// v11 fixture â†’ migrate to v12 â†’ data intact + privilege tables + withhold cache.
    #[test]
    fn migrate_v11_to_v12_preserves_rows() {
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
        conn.execute_batch(MIGRATION_V11).expect("v11");
        conn.execute("INSERT INTO schema_meta (version) VALUES (11)", [])
            .expect("meta v11");
        assert_eq!(read_schema_version(&conn).expect("read"), 11);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v11', 'V11 Matter', '2020-01-01T00:00:00Z', 11, '/tmp/v11')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review, \
             fts_text_sha256, note_count, highlight_count) \
             VALUES ('itm_mail', 'mat_v11', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, \
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 1, \
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 0, 0)",
            [],
        )
        .expect("item");
        conn.execute(
            "INSERT INTO item_notes (id, item_id, matter_id, body, highlight_id, \
             created_at, updated_at, created_by, updated_by) \
             VALUES ('note1', 'itm_mail', 'mat_v11', 'work product', NULL, \
             '2020-01-01T00:00:02Z', '2020-01-01T00:00:02Z', 'alice', 'alice')",
            [],
        )
        .expect("note");

        let v = migrate(&conn).expect("migrate v11 to current");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, SCHEMA_VERSION);

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_mail'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("inbox/a.eml"));

        let body: String = conn
            .query_row(
                "SELECT body FROM item_notes WHERE id = 'note1'",
                [],
                |row| row.get(0),
            )
            .expect("note body");
        assert_eq!(body, "work product");

        let withhold: i64 = conn
            .query_row(
                "SELECT privilege_withhold FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("privilege_withhold");
        assert_eq!(withhold, 0);

        let has_priv: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_privilege'",
                [],
                |row| row.get(0),
            )
            .expect("item_privilege");
        assert!(has_priv);

        let has_proto: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='privilege_protocol'",
                [],
                |row| row.get(0),
            )
            .expect("privilege_protocol");
        assert!(has_proto);

        let has_red: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_redactions'",
                [],
                |row| row.get(0),
            )
            .expect("item_redactions");
        assert!(has_red);

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v11'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    /// v12 fixture â†’ migrate to v13 â†’ data intact + redaction tables + bookkeeping.
    #[test]
    fn migrate_v12_to_v13_preserves_rows() {
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
        conn.execute_batch(MIGRATION_V11).expect("v11");
        conn.execute_batch(MIGRATION_V12).expect("v12");
        conn.execute("INSERT INTO schema_meta (version) VALUES (12)", [])
            .expect("meta v12");
        assert_eq!(read_schema_version(&conn).expect("read"), 12);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v12', 'V12 Matter', '2020-01-01T00:00:00Z', 12, '/tmp/v12')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review, \
             fts_text_sha256, note_count, highlight_count, privilege_withhold) \
             VALUES ('itm_mail', 'mat_v12', NULL, NULL, 'inbox/a.eml', NULL, \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             'mid@example.com', 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'parent', 'email', 1, \
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 1, \
             'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 0, 0, 0)",
            [],
        )
        .expect("item");
        conn.execute(
            "INSERT INTO item_privilege (\
                item_id, matter_id, basis, description, status, withhold, include_on_log, \
                asserted_at, asserted_by, updated_at, updated_by, extra_json\
             ) VALUES (\
                'itm_mail', 'mat_v12', 'attorney_client', 'claim', 'asserted', 1, 1, \
                '2020-01-01T00:00:02Z', 'alice', '2020-01-01T00:00:02Z', 'alice', NULL)",
            [],
        )
        .expect("privilege");

        let v = migrate(&conn).expect("migrate v12 to v13");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, SCHEMA_VERSION);

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_mail'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("inbox/a.eml"));

        let status: String = conn
            .query_row(
                "SELECT status FROM item_privilege WHERE item_id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("priv status");
        assert_eq!(status, "asserted");

        let redaction_count: i64 = conn
            .query_row(
                "SELECT redaction_count FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("redaction_count");
        assert_eq!(redaction_count, 0);

        let redacted_sha: Option<String> = conn
            .query_row(
                "SELECT redacted_text_sha256 FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("redacted_text_sha256");
        assert!(redacted_sha.is_none());

        let has_red: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_redactions'",
                [],
                |row| row.get(0),
            )
            .expect("item_redactions");
        assert!(has_red);

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v12'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    /// v13 â†’ v14 adds office_* columns without disturbing existing item rows.
    #[test]
    fn migrate_v13_to_v14_adds_office_columns() {
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
        conn.execute_batch(MIGRATION_V11).expect("v11");
        conn.execute_batch(MIGRATION_V12).expect("v12");
        conn.execute_batch(MIGRATION_V13).expect("v13");
        conn.execute("INSERT INTO schema_meta (version) VALUES (13)", [])
            .expect("meta v13");
        assert_eq!(read_schema_version(&conn).expect("read"), 13);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v13', 'V13 Matter', '2020-01-01T00:00:00Z', 13, '/tmp/v13')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review, \
             fts_text_sha256, note_count, highlight_count, privilege_withhold, redaction_count) \
             VALUES ('itm_mail', 'mat_v13', NULL, NULL, 'docs/a.docx', \
             'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', \
             NULL, NULL, 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'attachment', 'attachment', 0, NULL, 0, NULL, 0, 0, 0, 0)",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v13 to v14");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, SCHEMA_VERSION);

        for col in [
            "office_extract_status",
            "office_extract_method",
            "office_source_native_sha256",
            "office_extracted_at",
            "office_extract_error",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected column {col}");
        }

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_mail'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("docs/a.docx"));

        let office_status: Option<String> = conn
            .query_row(
                "SELECT office_extract_status FROM items WHERE id = 'itm_mail'",
                [],
                |row| row.get(0),
            )
            .expect("office status");
        assert!(office_status.is_none());

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v13'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    /// v14 â†’ v15 adds pdf_* columns including pdf_needs_ocr DEFAULT 0.
    #[test]
    fn migrate_v14_to_v15_adds_pdf_columns() {
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
        conn.execute_batch(MIGRATION_V11).expect("v11");
        conn.execute_batch(MIGRATION_V12).expect("v12");
        conn.execute_batch(MIGRATION_V13).expect("v13");
        conn.execute_batch(MIGRATION_V14).expect("v14");
        conn.execute("INSERT INTO schema_meta (version) VALUES (14)", [])
            .expect("meta v14");
        assert_eq!(read_schema_version(&conn).expect("read"), 14);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v14', 'V14 Matter', '2020-01-01T00:00:00Z', 14, '/tmp/v14')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review, \
             fts_text_sha256, note_count, highlight_count, privilege_withhold, redaction_count) \
             VALUES ('itm_pdf', 'mat_v14', NULL, NULL, 'docs/a.pdf', \
             'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd', \
             NULL, NULL, 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'attachment', 'attachment', 0, NULL, 0, NULL, 0, 0, 0, 0)",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v14 to v15");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, SCHEMA_VERSION);

        for col in [
            "pdf_extract_status",
            "pdf_extract_method",
            "pdf_source_native_sha256",
            "pdf_extracted_at",
            "pdf_extract_error",
            "pdf_page_count",
            "pdf_needs_ocr",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected column {col}");
        }

        let needs: i64 = conn
            .query_row(
                "SELECT pdf_needs_ocr FROM items WHERE id = 'itm_pdf'",
                [],
                |row| row.get(0),
            )
            .expect("needs_ocr");
        assert_eq!(needs, 0);

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_pdf'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("docs/a.pdf"));

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v14'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    /// v15 â†’ v16 adds calendar fields + ics_* bookkeeping columns.
    #[test]
    fn migrate_v15_to_v16_adds_calendar_columns() {
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
        conn.execute_batch(MIGRATION_V11).expect("v11");
        conn.execute_batch(MIGRATION_V12).expect("v12");
        conn.execute_batch(MIGRATION_V13).expect("v13");
        conn.execute_batch(MIGRATION_V14).expect("v14");
        conn.execute_batch(MIGRATION_V15).expect("v15");
        conn.execute("INSERT INTO schema_meta (version) VALUES (15)", [])
            .expect("meta v15");
        assert_eq!(read_schema_version(&conn).expect("read"), 15);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v15', 'V15 Matter', '2020-01-01T00:00:00Z', 15, '/tmp/v15')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review, \
             fts_text_sha256, note_count, highlight_count, privilege_withhold, redaction_count, \
             pdf_needs_ocr) \
             VALUES ('itm_cal', 'mat_v15', NULL, NULL, 'meetings/a.ics', \
             'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', \
             NULL, NULL, 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'standalone', 'attachment', 0, NULL, 0, NULL, 0, 0, 0, 0, 0)",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v15 to v16");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, SCHEMA_VERSION);

        for col in [
            "message_class",
            "cal_start_at",
            "cal_end_at",
            "cal_all_day",
            "cal_location",
            "cal_organizer",
            "cal_attendees_json",
            "cal_busy_status",
            "cal_is_recurring",
            "cal_recurrence_id",
            "cal_uid",
            "cal_extract_method",
            "ics_extract_status",
            "ics_extract_method",
            "ics_source_native_sha256",
            "ics_extracted_at",
            "ics_extract_error",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected column {col}");
        }

        let start: Option<String> = conn
            .query_row(
                "SELECT cal_start_at FROM items WHERE id = 'itm_cal'",
                [],
                |row| row.get(0),
            )
            .expect("cal_start");
        assert!(start.is_none());

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_cal'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("meetings/a.ics"));

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v15'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    /// v16 â†’ v17 adds OCR bookkeeping columns.
    #[test]
    fn migrate_v16_to_v17_adds_ocr_columns() {
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
        conn.execute_batch(MIGRATION_V11).expect("v11");
        conn.execute_batch(MIGRATION_V12).expect("v12");
        conn.execute_batch(MIGRATION_V13).expect("v13");
        conn.execute_batch(MIGRATION_V14).expect("v14");
        conn.execute_batch(MIGRATION_V15).expect("v15");
        conn.execute_batch(MIGRATION_V16).expect("v16");
        conn.execute("INSERT INTO schema_meta (version) VALUES (16)", [])
            .expect("meta v16");
        assert_eq!(read_schema_version(&conn).expect("read"), 16);

        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v16', 'V16 Matter', '2020-01-01T00:00:00Z', 16, '/tmp/v16')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review, \
             fts_text_sha256, note_count, highlight_count, privilege_withhold, redaction_count, \
             pdf_needs_ocr) \
             VALUES ('itm_ocr', 'mat_v16', NULL, NULL, 'scans/a.png', \
             'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', \
             NULL, NULL, 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'standalone', 'attachment', 0, NULL, 0, NULL, 0, 0, 0, 0, 1)",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v16 to current");
        assert_eq!(v, SCHEMA_VERSION);

        for col in [
            "ocr_status",
            "ocr_engine",
            "ocr_lang",
            "ocr_text_sha256",
            "ocr_source_native_sha256",
            "ocr_page_count",
            "ocr_at",
            "ocr_error",
            "ocr_confidence",
            "category_method",
            "category_taxonomy",
            "category_status",
            "category_error",
            "categorized_at",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected column {col}");
        }

        let status: Option<String> = conn
            .query_row(
                "SELECT ocr_status FROM items WHERE id = 'itm_ocr'",
                [],
                |row| row.get(0),
            )
            .expect("ocr_status");
        assert!(status.is_none());

        let cat_status: Option<String> = conn
            .query_row(
                "SELECT category_status FROM items WHERE id = 'itm_ocr'",
                [],
                |row| row.get(0),
            )
            .expect("category_status");
        assert!(cat_status.is_none());

        let path: Option<String> = conn
            .query_row("SELECT path FROM items WHERE id = 'itm_ocr'", [], |row| {
                row.get(0)
            })
            .expect("path");
        assert_eq!(path.as_deref(), Some("scans/a.png"));

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v16'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v17_to_v18_adds_category_columns() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        // Apply through v17 only, then migrate to v18.
        for &(target, sql) in MIGRATIONS {
            if target > 17 {
                break;
            }
            conn.execute_batch(sql).expect("step");
            if target == 1 {
                conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])
                    .expect("meta");
            } else {
                conn.execute("UPDATE schema_meta SET version = ?1", [target])
                    .expect("meta");
            }
        }
        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v17', 'V17 Matter', '2020-01-01T00:00:00Z', 17, '/tmp/v17')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO items (id, matter_id, source_id, family_id, path, native_sha256, \
             logical_hash, message_id, status, size_bytes, created_at, modified_at, imported_at, \
             role, file_category, logical_hash_version, text_sha256, in_review, \
             fts_text_sha256, note_count, highlight_count, privilege_withhold, redaction_count, \
             pdf_needs_ocr) \
             VALUES ('itm_cat', 'mat_v17', NULL, NULL, 'a.pdf', \
             'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', \
             NULL, NULL, 'extracted', 10, NULL, NULL, '2020-01-01T00:00:01Z', \
             'attachment', 'attachment', 0, NULL, 0, NULL, 0, 0, 0, 0, 0)",
            [],
        )
        .expect("item");

        let v = migrate(&conn).expect("migrate v17 to current");
        assert_eq!(v, SCHEMA_VERSION);

        for col in [
            "category_method",
            "category_taxonomy",
            "category_status",
            "category_error",
            "categorized_at",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('items') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected column {col}");
        }

        let fc: Option<String> = conn
            .query_row(
                "SELECT file_category FROM items WHERE id = 'itm_cat'",
                [],
                |row| row.get(0),
            )
            .expect("fc");
        assert_eq!(fc.as_deref(), Some("attachment"));

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v17'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v18_to_v19_adds_overview_indexes() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        for &(target, sql) in MIGRATIONS {
            if target > 18 {
                break;
            }
            conn.execute_batch(sql).expect("step");
            if target == 1 {
                conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])
                    .expect("meta");
            } else {
                conn.execute("UPDATE schema_meta SET version = ?1", [target])
                    .expect("meta");
            }
        }
        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v18', 'V18 Matter', '2020-01-01T00:00:00Z', 18, '/tmp/v18')",
            [],
        )
        .expect("matter");

        let v = migrate(&conn).expect("migrate v18 to current");
        assert_eq!(v, SCHEMA_VERSION);

        for idx in [
            "idx_items_matter_file_category",
            "idx_items_matter_custodian",
            "idx_items_matter_role",
        ] {
            let has: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name=?1",
                    [idx],
                    |row| row.get(0),
                )
                .expect("idx");
            assert!(has, "expected index {idx}");
        }

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v18'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v19_to_v20_adds_production_tables() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        for &(target, sql) in MIGRATIONS {
            if target > 19 {
                break;
            }
            conn.execute_batch(sql).expect("step");
            if target == 1 {
                conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])
                    .expect("meta");
            } else {
                conn.execute("UPDATE schema_meta SET version = ?1", [target])
                    .expect("meta");
            }
        }
        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v19', 'V19 Matter', '2020-01-01T00:00:00Z', 19, '/tmp/v19')",
            [],
        )
        .expect("matter");

        let v = migrate(&conn).expect("migrate v19 to current");
        assert_eq!(v, SCHEMA_VERSION);

        for table in ["production_sets", "production_items"] {
            let has: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("table");
            assert!(has, "expected table {table}");
        }

        for col in [
            "id",
            "matter_id",
            "name",
            "created_at",
            "updated_at",
            "bates_prefix",
            "next_seq",
            "status",
            "params_json",
            "output_root",
            "job_id",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('production_sets') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected production_sets.{col}");
        }

        for col in [
            "production_set_id",
            "item_id",
            "control_number",
            "native_relpath",
            "text_relpath",
            "status",
            "skip_reason",
            "error",
            "produced_at",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('production_items') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected production_items.{col}");
        }

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v19'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v20_to_v21_adds_qc_runs() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        for &(target, sql) in MIGRATIONS {
            if target > 20 {
                break;
            }
            conn.execute_batch(sql).expect("step");
            if target == 1 {
                conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])
                    .expect("meta");
            } else {
                conn.execute("UPDATE schema_meta SET version = ?1", [target])
                    .expect("meta");
            }
        }
        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v20', 'V20 Matter', '2020-01-01T00:00:00Z', 20, '/tmp/v20')",
            [],
        )
        .expect("matter");

        let v = migrate(&conn).expect("migrate v20 to current");
        assert_eq!(v, SCHEMA_VERSION);

        let has: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='qc_runs'",
                [],
                |row| row.get(0),
            )
            .expect("table");
        assert!(has, "expected table qc_runs");

        for col in [
            "id",
            "matter_id",
            "profile",
            "created_at",
            "passed",
            "error_count",
            "warn_count",
            "candidate_count",
            "selection_fingerprint",
            "scope",
            "scope_json",
            "report_path",
            "job_id",
            "rules_json",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('qc_runs') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected qc_runs.{col}");
        }

        let has_idx: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master \
                 WHERE type='index' AND name='idx_qc_runs_matter_created'",
                [],
                |row| row.get(0),
            )
            .expect("idx");
        assert!(has_idx, "expected idx_qc_runs_matter_created");

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v20'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v21_to_v22_adds_gap_tables() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        for &(target, sql) in MIGRATIONS {
            if target > 21 {
                break;
            }
            conn.execute_batch(sql).expect("step");
            if target == 1 {
                conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])
                    .expect("meta");
            } else {
                conn.execute("UPDATE schema_meta SET version = ?1", [target])
                    .expect("meta");
            }
        }
        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v21', 'V21 Matter', '2020-01-01T00:00:00Z', 21, '/tmp/v21')",
            [],
        )
        .expect("matter");

        let v = migrate(&conn).expect("migrate v21 to current");
        assert_eq!(v, SCHEMA_VERSION);

        for table in [
            "expected_custodians",
            "expected_sources",
            "gap_imports",
            "gap_expected_docs",
            "gap_runs",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='{table}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("table");
            assert!(has, "expected table {table}");
        }

        for col in [
            "id",
            "matter_id",
            "name_norm",
            "display_name",
            "notes",
            "active",
            "created_at",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('expected_custodians') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected expected_custodians.{col}");
        }

        for col in [
            "id",
            "import_id",
            "control_number",
            "sha256",
            "message_id",
            "item_id",
            "logical_hash",
            "custodian",
            "file_name",
            "file_category",
            "mime_type",
            "file_ext",
            "date_sent",
            "date_received",
            "date_created",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('gap_expected_docs') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected gap_expected_docs.{col}");
        }

        for idx in [
            "idx_expected_custodians_matter_name",
            "idx_gap_imports_matter",
            "idx_gap_expected_docs_import",
            "idx_gap_runs_matter_started",
        ] {
            let has_idx: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='{idx}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("idx");
            assert!(has_idx, "expected {idx}");
        }

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v21'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v22_to_v23_adds_processing_profiles() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        for &(target, sql) in MIGRATIONS {
            if target > 22 {
                break;
            }
            conn.execute_batch(sql).expect("step");
            if target == 1 {
                conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])
                    .expect("meta");
            } else {
                conn.execute("UPDATE schema_meta SET version = ?1", [target])
                    .expect("meta");
            }
        }
        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v22', 'V22 Matter', '2020-01-01T00:00:00Z', 22, '/tmp/v22')",
            [],
        )
        .expect("matter");

        let v = migrate(&conn).expect("migrate v22 to current");
        assert_eq!(v, SCHEMA_VERSION);

        let has_table: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='processing_profiles'",
                [],
                |row| row.get(0),
            )
            .expect("table");
        assert!(has_table, "expected processing_profiles table");

        for col in [
            "id",
            "matter_id",
            "name",
            "description",
            "body_json",
            "created_at",
            "updated_at",
            "created_by",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('processing_profiles') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected processing_profiles.{col}");
        }

        let has_default: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('matters') WHERE name = 'default_profile_id'",
                [],
                |row| row.get(0),
            )
            .expect("col");
        assert!(has_default, "expected matters.default_profile_id");

        for idx in [
            "idx_processing_profiles_matter_name",
            "idx_processing_profiles_matter",
        ] {
            let has_idx: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='{idx}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("idx");
            assert!(has_idx, "expected {idx}");
        }

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v22'",
                [],
                |row| row.get(0),
            )
            .expect("mat schema");
        assert_eq!(ms, SCHEMA_VERSION);
    }

    #[test]
    fn migrate_v23_to_v24_adds_workflows_and_parent_job_id() {
        let conn = Connection::open_in_memory().expect("open");
        configure_connection(&conn).expect("configure");
        for &(target, sql) in MIGRATIONS {
            if target > 23 {
                break;
            }
            conn.execute_batch(sql).expect("step");
            if target == 1 {
                conn.execute("INSERT INTO schema_meta (version) VALUES (?1)", [target])
                    .expect("meta");
            } else {
                conn.execute("UPDATE schema_meta SET version = ?1", [target])
                    .expect("meta");
            }
        }
        conn.execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_v23', 'V23 Matter', '2020-01-01T00:00:00Z', 23, '/tmp/v23')",
            [],
        )
        .expect("matter");
        conn.execute(
            "INSERT INTO jobs (id, matter_id, kind, state, created_at, updated_at) \
             VALUES ('job_root', 'mat_v23', 'profile_run', 'pending', '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z')",
            [],
        )
        .expect("job");

        let v = migrate(&conn).expect("migrate v23 to v24");
        assert_eq!(v, SCHEMA_VERSION);
        assert_eq!(v, 24);

        let has_table: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='workflows'",
                [],
                |row| row.get(0),
            )
            .expect("table");
        assert!(has_table, "expected workflows table");

        for col in [
            "id",
            "matter_id",
            "name",
            "description",
            "body_json",
            "created_at",
            "updated_at",
            "created_by",
        ] {
            let has: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM pragma_table_info('workflows') WHERE name = '{col}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("col");
            assert!(has, "expected workflows.{col}");
        }

        let has_default: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('matters') WHERE name = 'default_workflow_id'",
                [],
                |row| row.get(0),
            )
            .expect("col");
        assert!(has_default, "expected matters.default_workflow_id");

        let has_parent: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('jobs') WHERE name = 'parent_job_id'",
                [],
                |row| row.get(0),
            )
            .expect("col");
        assert!(has_parent, "expected jobs.parent_job_id");

        for idx in [
            "idx_workflows_matter_name",
            "idx_workflows_matter",
            "idx_jobs_parent",
        ] {
            let has_idx: bool = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='{idx}'"
                    ),
                    [],
                    |row| row.get(0),
                )
                .expect("idx");
            assert!(has_idx, "expected {idx}");
        }

        // Existing job rows survive with NULL parent_job_id.
        let parent: Option<String> = conn
            .query_row(
                "SELECT parent_job_id FROM jobs WHERE id = 'job_root'",
                [],
                |row| row.get(0),
            )
            .expect("parent");
        assert!(parent.is_none());

        let ms: u32 = conn
            .query_row(
                "SELECT schema_version FROM matters WHERE id = 'mat_v23'",
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
