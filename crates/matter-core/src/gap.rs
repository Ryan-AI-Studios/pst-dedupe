//! Gap analysis roster + expected-doc storage (track **0042**).
//!
//! Collection gap uses [`expected_custodians`] / optional [`expected_sources`].
//! Opposing DAT imports land in [`gap_imports`] + [`gap_expected_docs`].
//! Run history is [`gap_runs`] (findings live on disk under `exports/gap/`).

use std::collections::HashMap;
use std::path::Path;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::matter::{collapse_whitespace, new_id, now_rfc3339, Matter};

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Normalize a custodian name for roster match: trim, collapse whitespace, lowercase.
pub fn normalize_custodian_name(s: &str) -> String {
    collapse_whitespace(s.trim()).to_lowercase()
}

/// Normalize an expected-source label: same rules as custodians.
pub fn normalize_source_label(s: &str) -> String {
    normalize_custodian_name(s)
}

// ---------------------------------------------------------------------------
// Expected custodians
// ---------------------------------------------------------------------------

/// One row from `expected_custodians`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedCustodian {
    pub id: String,
    pub matter_id: String,
    pub name_norm: String,
    pub display_name: String,
    pub notes: Option<String>,
    pub active: bool,
    pub created_at: String,
}

/// Result of a roster CSV import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportExpectedCustodiansResult {
    pub inserted: u64,
    pub updated: u64,
    pub total_rows: u64,
    pub skipped_empty: u64,
}

// ---------------------------------------------------------------------------
// Expected sources (optional)
// ---------------------------------------------------------------------------

/// One row from `expected_sources`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedSource {
    pub id: String,
    pub matter_id: String,
    pub label: String,
    pub label_norm: String,
    pub path_hint: Option<String>,
    pub kind: Option<String>,
    pub notes: Option<String>,
    pub active: bool,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Gap import / expected docs / runs
// ---------------------------------------------------------------------------

/// One row from `gap_imports`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapImportRecord {
    pub id: String,
    pub matter_id: String,
    pub kind: String,
    pub path: String,
    pub imported_at: String,
    pub row_count: u64,
    pub column_map_json: Option<String>,
    pub error_count: Option<u64>,
}

/// One row from `gap_expected_docs` (no subject by design).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GapExpectedDoc {
    pub id: String,
    pub import_id: String,
    pub control_number: Option<String>,
    pub sha256: Option<String>,
    pub message_id: Option<String>,
    pub item_id: Option<String>,
    pub logical_hash: Option<String>,
    pub custodian: Option<String>,
    pub file_name: Option<String>,
    pub file_category: Option<String>,
    pub mime_type: Option<String>,
    pub file_ext: Option<String>,
    pub date_sent: Option<String>,
    pub date_received: Option<String>,
    pub date_created: Option<String>,
}

/// Fields for inserting an expected doc (id assigned by API).
#[derive(Debug, Clone, Default)]
pub struct GapExpectedDocInput {
    pub control_number: Option<String>,
    pub sha256: Option<String>,
    pub message_id: Option<String>,
    pub item_id: Option<String>,
    pub logical_hash: Option<String>,
    pub custodian: Option<String>,
    pub file_name: Option<String>,
    pub file_category: Option<String>,
    pub mime_type: Option<String>,
    pub file_ext: Option<String>,
    pub date_sent: Option<String>,
    pub date_received: Option<String>,
    pub date_created: Option<String>,
}

/// Input for [`Matter::insert_gap_import`].
#[derive(Debug, Clone)]
pub struct InsertGapImportInput {
    pub kind: String,
    pub path: String,
    pub row_count: u64,
    pub column_map_json: Option<String>,
    pub error_count: Option<u64>,
}

/// One row from `gap_runs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapRunRecord {
    pub id: String,
    pub matter_id: String,
    pub kind: String,
    pub params_json: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_count: u64,
    pub warn_count: u64,
    pub finding_count: u64,
    pub report_path: Option<String>,
    pub job_id: Option<String>,
    pub summary_json: Option<String>,
}

/// Input for [`Matter::insert_gap_run`].
#[derive(Debug, Clone)]
pub struct InsertGapRunInput {
    pub kind: String,
    pub params_json: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_count: u64,
    pub warn_count: u64,
    pub finding_count: u64,
    pub report_path: Option<String>,
    pub job_id: Option<String>,
    pub summary_json: Option<String>,
}

// ---------------------------------------------------------------------------
// Custodian inventory helpers (used by matter-gap collection analysis)
// ---------------------------------------------------------------------------

/// Present custodian with item count (empty custodian rolled up as empty string).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustodianInventoryRow {
    pub custodian: String,
    pub name_norm: String,
    pub item_count: u64,
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Import expected custodians from a UTF-8 CSV.
    ///
    /// Required header: `custodian`. Optional: `alias`, `notes`.
    /// Upserts by `name_norm` (reactivates if previously deactivated).
    pub fn import_expected_custodians_csv_bytes(
        &self,
        csv_bytes: &[u8],
    ) -> Result<ImportExpectedCustodiansResult> {
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(csv_bytes);
        let headers = reader
            .headers()
            .map_err(|e| Error::Other(format!("roster CSV header: {e}")))?
            .clone();
        let header_map: HashMap<String, usize> = headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.trim().to_ascii_lowercase(), i))
            .collect();
        let custodian_idx = header_map
            .get("custodian")
            .copied()
            .ok_or_else(|| Error::Other("roster CSV requires header column 'custodian'".into()))?;
        let notes_idx = header_map.get("notes").copied();
        let alias_idx = header_map.get("alias").copied();

        let mut inserted = 0u64;
        let mut updated = 0u64;
        let mut skipped_empty = 0u64;
        let mut total_rows = 0u64;

        for rec in reader.records() {
            let rec = rec.map_err(|e| Error::Other(format!("roster CSV row: {e}")))?;
            total_rows += 1;
            let display = rec.get(custodian_idx).unwrap_or("").trim();
            if display.is_empty() {
                skipped_empty += 1;
                continue;
            }
            let notes = notes_idx
                .and_then(|i| rec.get(i))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            // Alias is stored as notes prefix residual; primary key is custodian display.
            let notes = match (notes, alias_idx.and_then(|i| rec.get(i)).map(str::trim)) {
                (Some(n), Some(a)) if !a.is_empty() => Some(format!("alias={a}; {n}")),
                (None, Some(a)) if !a.is_empty() => Some(format!("alias={a}")),
                (n, _) => n,
            };
            match self.upsert_expected_custodian(display, notes.as_deref())? {
                UpsertOutcome::Inserted => inserted += 1,
                UpsertOutcome::Updated => updated += 1,
            }
        }

        Ok(ImportExpectedCustodiansResult {
            inserted,
            updated,
            total_rows,
            skipped_empty,
        })
    }

    /// Import expected custodians from a UTF-8 CSV file path.
    pub fn import_expected_custodians_csv_path(
        &self,
        path: &Path,
    ) -> Result<ImportExpectedCustodiansResult> {
        let bytes = std::fs::read(path)
            .map_err(|e| Error::Other(format!("read roster CSV {}: {e}", path.display())))?;
        self.import_expected_custodians_csv_bytes(&bytes)
    }

    /// Add or update one expected custodian by display name.
    pub fn add_expected_custodian(
        &self,
        display_name: &str,
        notes: Option<&str>,
    ) -> Result<ExpectedCustodian> {
        let display = display_name.trim();
        if display.is_empty() {
            return Err(Error::Other(
                "custodian display name must be non-empty".into(),
            ));
        }
        let _ = self.upsert_expected_custodian(display, notes)?;
        let name_norm = normalize_custodian_name(display);
        self.list_expected_custodians(true)?
            .into_iter()
            .find(|c| c.name_norm == name_norm)
            .ok_or_else(|| Error::Other("expected custodian missing after upsert".into()))
    }

    /// Deactivate (soft-remove) an expected custodian by id.
    pub fn remove_expected_custodian(&self, id: &str) -> Result<bool> {
        let n = self.connection().execute(
            "UPDATE expected_custodians SET active = 0 WHERE id = ?1 AND matter_id = ?2",
            params![id, self.id()],
        )?;
        Ok(n > 0)
    }

    /// List expected custodians. When `active_only`, inactive rows are omitted.
    pub fn list_expected_custodians(&self, active_only: bool) -> Result<Vec<ExpectedCustodian>> {
        let sql = if active_only {
            "SELECT id, matter_id, name_norm, display_name, notes, active, created_at \
             FROM expected_custodians WHERE matter_id = ?1 AND active = 1 \
             ORDER BY display_name COLLATE NOCASE, id"
        } else {
            "SELECT id, matter_id, name_norm, display_name, notes, active, created_at \
             FROM expected_custodians WHERE matter_id = ?1 \
             ORDER BY display_name COLLATE NOCASE, id"
        };
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params![self.id()], |row| {
            let active_i: i64 = row.get(5)?;
            Ok(ExpectedCustodian {
                id: row.get(0)?,
                matter_id: row.get(1)?,
                name_norm: row.get(2)?,
                display_name: row.get(3)?,
                notes: row.get(4)?,
                active: active_i != 0,
                created_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Add or update an expected source by label.
    pub fn add_expected_source(
        &self,
        label: &str,
        path_hint: Option<&str>,
        kind: Option<&str>,
        notes: Option<&str>,
    ) -> Result<ExpectedSource> {
        let label_trim = label.trim();
        if label_trim.is_empty() {
            return Err(Error::Other(
                "expected source label must be non-empty".into(),
            ));
        }
        let label_norm = normalize_source_label(label_trim);
        let now = now_rfc3339();
        let existing: Option<String> = self
            .connection()
            .query_row(
                "SELECT id FROM expected_sources WHERE matter_id = ?1 AND label_norm = ?2",
                params![self.id(), label_norm],
                |row| row.get(0),
            )
            .ok();
        if let Some(id) = existing {
            self.connection().execute(
                "UPDATE expected_sources SET label = ?1, path_hint = ?2, kind = ?3, notes = ?4, active = 1 \
                 WHERE id = ?5",
                params![label_trim, path_hint, kind, notes, id],
            )?;
        } else {
            let id = new_id("exs");
            self.connection().execute(
                "INSERT INTO expected_sources \
                 (id, matter_id, label, label_norm, path_hint, kind, notes, active, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
                params![
                    id,
                    self.id(),
                    label_trim,
                    label_norm,
                    path_hint,
                    kind,
                    notes,
                    now
                ],
            )?;
        }
        self.list_expected_sources(true)?
            .into_iter()
            .find(|s| s.label_norm == label_norm)
            .ok_or_else(|| Error::Other("expected source missing after upsert".into()))
    }

    /// List expected sources.
    pub fn list_expected_sources(&self, active_only: bool) -> Result<Vec<ExpectedSource>> {
        let sql = if active_only {
            "SELECT id, matter_id, label, label_norm, path_hint, kind, notes, active, created_at \
             FROM expected_sources WHERE matter_id = ?1 AND active = 1 \
             ORDER BY label COLLATE NOCASE, id"
        } else {
            "SELECT id, matter_id, label, label_norm, path_hint, kind, notes, active, created_at \
             FROM expected_sources WHERE matter_id = ?1 \
             ORDER BY label COLLATE NOCASE, id"
        };
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params![self.id()], |row| {
            let active_i: i64 = row.get(7)?;
            Ok(ExpectedSource {
                id: row.get(0)?,
                matter_id: row.get(1)?,
                label: row.get(2)?,
                label_norm: row.get(3)?,
                path_hint: row.get(4)?,
                kind: row.get(5)?,
                notes: row.get(6)?,
                active: active_i != 0,
                created_at: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Distinct custodians present on items + counts (empty string for null/blank).
    pub fn custodian_inventory(&self) -> Result<Vec<CustodianInventoryRow>> {
        let mut stmt = self.connection().prepare(
            "SELECT COALESCE(TRIM(custodian), ''), COUNT(*) \
             FROM items WHERE matter_id = ?1 \
             GROUP BY COALESCE(TRIM(custodian), '') \
             ORDER BY COUNT(*) DESC, 1 COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![self.id()], |row| {
            let custodian: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            let name_norm = if custodian.is_empty() {
                String::new()
            } else {
                normalize_custodian_name(&custodian)
            };
            Ok(CustodianInventoryRow {
                custodian,
                name_norm,
                item_count: count as u64,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Insert a gap import header row.
    pub fn insert_gap_import(&self, input: InsertGapImportInput) -> Result<GapImportRecord> {
        let id = new_id("gimp");
        let imported_at = now_rfc3339();
        let error_count_i = input.error_count.map(|c| c as i64);
        self.connection().execute(
            "INSERT INTO gap_imports \
             (id, matter_id, kind, path, imported_at, row_count, column_map_json, error_count) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                self.id(),
                input.kind,
                input.path,
                imported_at,
                input.row_count as i64,
                input.column_map_json,
                error_count_i,
            ],
        )?;
        Ok(GapImportRecord {
            id,
            matter_id: self.id().to_string(),
            kind: input.kind,
            path: input.path,
            imported_at,
            row_count: input.row_count,
            column_map_json: input.column_map_json,
            error_count: input.error_count,
        })
    }

    /// Batch-insert expected docs for an import (bound params only).
    pub fn insert_gap_expected_docs(
        &self,
        import_id: &str,
        docs: &[GapExpectedDocInput],
    ) -> Result<u64> {
        // Verify import belongs to this matter.
        let belongs: bool = self.connection().query_row(
            "SELECT COUNT(*) > 0 FROM gap_imports WHERE id = ?1 AND matter_id = ?2",
            params![import_id, self.id()],
            |row| row.get(0),
        )?;
        if !belongs {
            return Err(Error::Other(format!(
                "gap import '{import_id}' not found for this matter"
            )));
        }

        let mut count = 0u64;
        self.with_transaction(|conn| {
            let mut stmt = conn.prepare(
                "INSERT INTO gap_expected_docs \
                 (id, import_id, control_number, sha256, message_id, item_id, logical_hash, \
                  custodian, file_name, file_category, mime_type, file_ext, \
                  date_sent, date_received, date_created) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )?;
            for d in docs {
                let id = new_id("ged");
                stmt.execute(params![
                    id,
                    import_id,
                    d.control_number,
                    d.sha256,
                    d.message_id,
                    d.item_id,
                    d.logical_hash,
                    d.custodian,
                    d.file_name,
                    d.file_category,
                    d.mime_type,
                    d.file_ext,
                    d.date_sent,
                    d.date_received,
                    d.date_created,
                ])?;
                count += 1;
            }
            Ok(())
        })?;
        Ok(count)
    }

    /// List gap imports for this matter (newest first).
    pub fn list_gap_imports(&self) -> Result<Vec<GapImportRecord>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, kind, path, imported_at, row_count, column_map_json, error_count \
             FROM gap_imports WHERE matter_id = ?1 ORDER BY imported_at DESC, id DESC",
        )?;
        let rows = stmt.query_map(params![self.id()], |row| {
            let error_count: Option<i64> = row.get(7)?;
            Ok(GapImportRecord {
                id: row.get(0)?,
                matter_id: row.get(1)?,
                kind: row.get(2)?,
                path: row.get(3)?,
                imported_at: row.get(4)?,
                row_count: row.get::<_, i64>(5)? as u64,
                column_map_json: row.get(6)?,
                error_count: error_count.map(|c| c as u64),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// List expected docs for an import.
    pub fn list_gap_expected_docs(&self, import_id: &str) -> Result<Vec<GapExpectedDoc>> {
        let mut stmt = self.connection().prepare(
            "SELECT d.id, d.import_id, d.control_number, d.sha256, d.message_id, d.item_id, \
             d.logical_hash, d.custodian, d.file_name, d.file_category, d.mime_type, d.file_ext, \
             d.date_sent, d.date_received, d.date_created \
             FROM gap_expected_docs d \
             INNER JOIN gap_imports i ON i.id = d.import_id \
             WHERE d.import_id = ?1 AND i.matter_id = ?2 \
             ORDER BY d.id",
        )?;
        let rows = stmt.query_map(params![import_id, self.id()], map_gap_expected_doc)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Insert a gap run history row.
    pub fn insert_gap_run(&self, input: InsertGapRunInput) -> Result<GapRunRecord> {
        let id = new_id("grun");
        self.connection().execute(
            "INSERT INTO gap_runs \
             (id, matter_id, kind, params_json, started_at, finished_at, error_count, warn_count, \
              finding_count, report_path, job_id, summary_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                self.id(),
                input.kind,
                input.params_json,
                input.started_at,
                input.finished_at,
                input.error_count as i64,
                input.warn_count as i64,
                input.finding_count as i64,
                input.report_path,
                input.job_id,
                input.summary_json,
            ],
        )?;
        Ok(GapRunRecord {
            id,
            matter_id: self.id().to_string(),
            kind: input.kind,
            params_json: input.params_json,
            started_at: input.started_at,
            finished_at: input.finished_at,
            error_count: input.error_count,
            warn_count: input.warn_count,
            finding_count: input.finding_count,
            report_path: input.report_path,
            job_id: input.job_id,
            summary_json: input.summary_json,
        })
    }

    /// Latest gap run for this matter (any kind).
    pub fn load_latest_gap_run(&self) -> Result<Option<GapRunRecord>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, matter_id, kind, params_json, started_at, finished_at, error_count, \
             warn_count, finding_count, report_path, job_id, summary_json \
             FROM gap_runs WHERE matter_id = ?1 \
             ORDER BY started_at DESC, id DESC LIMIT 1",
        )?;
        let result = stmt.query_row(params![self.id()], map_gap_run);
        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Build normalized Message-ID → item id map (first-seen wins).
    ///
    /// Empty / whitespace-only MIDs are omitted so empty never matches empty.
    /// Use for bulk opposing compare (O(n+m)) instead of per-row full scans.
    pub fn message_id_index(&self) -> Result<std::collections::HashMap<String, String>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, message_id FROM items \
             WHERE matter_id = ?1 AND message_id IS NOT NULL AND TRIM(message_id) != ''",
        )?;
        let rows = stmt.query_map(params![self.id()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = std::collections::HashMap::new();
        for r in rows {
            let (id, mid) = r?;
            let norm = crate::logical_hash::normalize_message_id(&mid);
            if norm.is_empty() {
                continue;
            }
            // First-seen wins (stable-ish for multi-dup MIDs).
            map.entry(norm).or_insert(id);
        }
        Ok(map)
    }

    /// Find item id by normalized message_id (non-empty only).
    ///
    /// Prefer [`Self::message_id_index`] for bulk compares.
    pub fn find_item_id_by_message_id(&self, message_id_norm: &str) -> Result<Option<String>> {
        if message_id_norm.trim().is_empty() {
            return Ok(None);
        }
        let target = message_id_norm.to_lowercase();
        let index = self.message_id_index()?;
        Ok(index.get(&target).cloned())
    }

    /// Find item id by native_sha256 (case-insensitive hex).
    pub fn find_item_id_by_native_sha256(&self, sha256: &str) -> Result<Option<String>> {
        let sha = sha256.trim().to_lowercase();
        if sha.is_empty() {
            return Ok(None);
        }
        let mut stmt = self.connection().prepare(
            "SELECT id FROM items WHERE matter_id = ?1 AND lower(native_sha256) = ?2 LIMIT 1",
        )?;
        match stmt.query_row(params![self.id(), sha], |row| row.get(0)) {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Find item id by exact item id (existence check).
    pub fn find_item_id_exists(&self, item_id: &str) -> Result<bool> {
        let id = item_id.trim();
        if id.is_empty() {
            return Ok(false);
        }
        let found: bool = self.connection().query_row(
            "SELECT COUNT(*) > 0 FROM items WHERE matter_id = ?1 AND id = ?2",
            params![self.id(), id],
            |row| row.get(0),
        )?;
        Ok(found)
    }

    /// Find item id by logical_hash.
    pub fn find_item_id_by_logical_hash(&self, logical_hash: &str) -> Result<Option<String>> {
        let h = logical_hash.trim().to_lowercase();
        if h.is_empty() {
            return Ok(None);
        }
        let mut stmt = self.connection().prepare(
            "SELECT id FROM items WHERE matter_id = ?1 AND lower(logical_hash) = ?2 LIMIT 1",
        )?;
        match stmt.query_row(params![self.id(), h], |row| row.get(0)) {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Find item id via production_items.control_number (any production set).
    pub fn find_item_id_by_control_number(&self, control_number: &str) -> Result<Option<String>> {
        let cn = control_number.trim();
        if cn.is_empty() {
            return Ok(None);
        }
        let mut stmt = self.connection().prepare(
            "SELECT pi.item_id FROM production_items pi \
             INNER JOIN production_sets ps ON ps.id = pi.production_set_id \
             WHERE ps.matter_id = ?1 AND pi.control_number = ?2 LIMIT 1",
        )?;
        match stmt.query_row(params![self.id(), cn], |row| row.get(0)) {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Best-effort item dates for coverage analysis: sent, received, or created.
    pub fn list_item_best_dates(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, \
             COALESCE(NULLIF(TRIM(sent_at), ''), NULLIF(TRIM(received_at), ''), NULLIF(TRIM(created_at), '')) \
             FROM items WHERE matter_id = ?1 \
             AND COALESCE(NULLIF(TRIM(sent_at), ''), NULLIF(TRIM(received_at), ''), NULLIF(TRIM(created_at), '')) IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![self.id()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// List item ids in scope for opposing compare.
    pub fn list_item_ids_for_gap_scope(
        &self,
        scope: &str,
        production_set_id: Option<&str>,
    ) -> Result<Vec<String>> {
        match scope {
            "inventory" => {
                let mut stmt = self
                    .connection()
                    .prepare("SELECT id FROM items WHERE matter_id = ?1 ORDER BY id")?;
                let rows = stmt.query_map(params![self.id()], |row| row.get(0))?;
                let mut out = Vec::new();
                for r in rows {
                    out.push(r?);
                }
                Ok(out)
            }
            "in_review" => {
                let mut stmt = self.connection().prepare(
                    "SELECT id FROM items WHERE matter_id = ?1 AND in_review = 1 ORDER BY id",
                )?;
                let rows = stmt.query_map(params![self.id()], |row| row.get(0))?;
                let mut out = Vec::new();
                for r in rows {
                    out.push(r?);
                }
                Ok(out)
            }
            "production_set" | "production_set_id" => {
                let ps = production_set_id.ok_or_else(|| {
                    Error::Other("production_set scope requires production_set_id".into())
                })?;
                let mut stmt = self.connection().prepare(
                    "SELECT pi.item_id FROM production_items pi \
                     INNER JOIN production_sets ps ON ps.id = pi.production_set_id \
                     WHERE ps.matter_id = ?1 AND pi.production_set_id = ?2 \
                     ORDER BY pi.item_id",
                )?;
                let rows = stmt.query_map(params![self.id(), ps], |row| row.get(0))?;
                let mut out = Vec::new();
                for r in rows {
                    out.push(r?);
                }
                Ok(out)
            }
            other => Err(Error::Other(format!(
                "unknown gap matter_scope '{other}' (expected inventory, in_review, production_set)"
            ))),
        }
    }

    fn upsert_expected_custodian(
        &self,
        display_name: &str,
        notes: Option<&str>,
    ) -> Result<UpsertOutcome> {
        let name_norm = normalize_custodian_name(display_name);
        if name_norm.is_empty() {
            return Err(Error::Other(
                "custodian name_norm empty after normalize".into(),
            ));
        }
        let existing: Option<(String,)> = self
            .connection()
            .query_row(
                "SELECT id FROM expected_custodians WHERE matter_id = ?1 AND name_norm = ?2",
                params![self.id(), name_norm],
                |row| Ok((row.get(0)?,)),
            )
            .ok();
        if let Some((id,)) = existing {
            self.connection().execute(
                "UPDATE expected_custodians SET display_name = ?1, notes = COALESCE(?2, notes), active = 1 \
                 WHERE id = ?3",
                params![display_name, notes, id],
            )?;
            Ok(UpsertOutcome::Updated)
        } else {
            let id = new_id("exc");
            let now = now_rfc3339();
            self.connection().execute(
                "INSERT INTO expected_custodians \
                 (id, matter_id, name_norm, display_name, notes, active, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![id, self.id(), name_norm, display_name, notes, now],
            )?;
            Ok(UpsertOutcome::Inserted)
        }
    }
}

enum UpsertOutcome {
    Inserted,
    Updated,
}

fn map_gap_expected_doc(row: &rusqlite::Row<'_>) -> rusqlite::Result<GapExpectedDoc> {
    Ok(GapExpectedDoc {
        id: row.get(0)?,
        import_id: row.get(1)?,
        control_number: row.get(2)?,
        sha256: row.get(3)?,
        message_id: row.get(4)?,
        item_id: row.get(5)?,
        logical_hash: row.get(6)?,
        custodian: row.get(7)?,
        file_name: row.get(8)?,
        file_category: row.get(9)?,
        mime_type: row.get(10)?,
        file_ext: row.get(11)?,
        date_sent: row.get(12)?,
        date_received: row.get(13)?,
        date_created: row.get(14)?,
    })
}

fn map_gap_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<GapRunRecord> {
    Ok(GapRunRecord {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        kind: row.get(2)?,
        params_json: row.get(3)?,
        started_at: row.get(4)?,
        finished_at: row.get(5)?,
        error_count: row.get::<_, i64>(6)? as u64,
        warn_count: row.get::<_, i64>(7)? as u64,
        finding_count: row.get::<_, i64>(8)? as u64,
        report_path: row.get(9)?,
        job_id: row.get(10)?,
        summary_json: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matter::Matter;
    use tempfile::tempdir;

    fn temp_matter(name: &str) -> (tempfile::TempDir, Matter) {
        let tmp = tempdir().expect("temp");
        let path = camino::Utf8Path::from_path(tmp.path())
            .expect("utf8")
            .join(name);
        let matter = Matter::create(&path, name).expect("create");
        (tmp, matter)
    }

    #[test]
    fn normalize_collapses_and_folds() {
        assert_eq!(normalize_custodian_name("  John   Smith "), "john smith");
        assert_eq!(normalize_custodian_name("ALICE"), "alice");
    }

    #[test]
    fn roster_import_and_list() {
        let (_tmp, matter) = temp_matter("roster");
        let csv = b"custodian,notes\nAlice Smith,VIP\nBob Jones,\n";
        let r = matter
            .import_expected_custodians_csv_bytes(csv)
            .expect("import");
        assert_eq!(r.inserted, 2);
        assert_eq!(r.total_rows, 2);
        let list = matter.list_expected_custodians(true).expect("list");
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|c| c.name_norm == "alice smith"));

        // Upsert same name
        let csv2 = b"custodian\nAlice Smith\n";
        let r2 = matter
            .import_expected_custodians_csv_bytes(csv2)
            .expect("reimport");
        assert_eq!(r2.updated, 1);
        assert_eq!(matter.list_expected_custodians(true).unwrap().len(), 2);
    }

    #[test]
    fn remove_deactivates() {
        let (_tmp, matter) = temp_matter("roster-rm");
        let c = matter
            .add_expected_custodian("Carol", Some("n"))
            .expect("add");
        assert!(matter.remove_expected_custodian(&c.id).unwrap());
        assert!(matter.list_expected_custodians(true).unwrap().is_empty());
        assert_eq!(matter.list_expected_custodians(false).unwrap().len(), 1);
    }

    #[test]
    fn gap_import_docs_and_run() {
        let (_tmp, matter) = temp_matter("gap-store");
        let imp = matter
            .insert_gap_import(InsertGapImportInput {
                kind: "opposing_dat".into(),
                path: "fixtures/gap/sample.dat".into(),
                row_count: 1,
                column_map_json: Some("{}".into()),
                error_count: Some(0),
            })
            .expect("import");
        let n = matter
            .insert_gap_expected_docs(
                &imp.id,
                &[GapExpectedDocInput {
                    control_number: Some("PROD0001".into()),
                    sha256: Some("abc".into()),
                    ..Default::default()
                }],
            )
            .expect("docs");
        assert_eq!(n, 1);
        let docs = matter.list_gap_expected_docs(&imp.id).expect("list");
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].control_number.as_deref(), Some("PROD0001"));
        // No subject field on struct storage path.
        let run = matter
            .insert_gap_run(InsertGapRunInput {
                kind: "collection".into(),
                params_json: Some("{}".into()),
                started_at: now_rfc3339(),
                finished_at: Some(now_rfc3339()),
                error_count: 0,
                warn_count: 1,
                finding_count: 1,
                report_path: Some("exports/gap/x".into()),
                job_id: None,
                summary_json: None,
            })
            .expect("run");
        let latest = matter.load_latest_gap_run().expect("latest");
        assert_eq!(latest.unwrap().id, run.id);
    }
}
