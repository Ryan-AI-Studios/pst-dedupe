//! Semantic search bookkeeping (schema v29 / track 0050).
//!
//! SQLite stores matter-level active-model meta, per-item embed digests, and an
//! optional `semantic_chunks` catalog. Vector bytes live on disk under
//! `{matter}/semantic/{model_id}/` (owned by `matter-semantic`).

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::filter::FilterSpec;
use crate::matter::{new_id, now_rfc3339, Matter};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Thin candidate for semantic_index job pagination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticCandidate {
    pub id: String,
    pub text_sha256: Option<String>,
    pub semantic_embedded_text_sha256: Option<String>,
    pub semantic_chunk_count: Option<i64>,
}

/// Matter-level semantic index metadata (active model namespace).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticMatterMeta {
    pub semantic_enabled: bool,
    pub semantic_model_id: Option<String>,
    pub semantic_dims: Option<i64>,
    pub semantic_chunk_params_json: Option<String>,
    pub semantic_fingerprint: Option<String>,
    pub semantic_built_at: Option<String>,
    pub semantic_job_id: Option<String>,
    pub semantic_chunk_count: i64,
}

/// One catalog row in `semantic_chunks`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticChunkRow {
    pub id: String,
    pub matter_id: String,
    pub item_id: String,
    pub ordinal: i64,
    pub start_offset: Option<i64>,
    pub end_offset: Option<i64>,
    pub text_sha256: String,
    pub model_id: String,
}

/// Write per-item embed bookkeeping after successful index.
#[derive(Debug, Clone)]
pub struct WriteItemSemanticInput<'a> {
    pub item_id: &'a str,
    pub embedded_text_sha256: &'a str,
    pub chunk_count: i64,
    pub embedded_at: &'a str,
}

/// Upsert one chunk catalog row.
#[derive(Debug, Clone)]
pub struct UpsertSemanticChunkInput<'a> {
    pub item_id: &'a str,
    pub ordinal: i64,
    pub start_offset: Option<i64>,
    pub end_offset: Option<i64>,
    pub text_sha256: &'a str,
    pub model_id: &'a str,
}

/// Update matter-level semantic meta after index / enable.
#[derive(Debug, Clone)]
pub struct UpdateSemanticMatterMetaInput<'a> {
    pub enabled: bool,
    pub model_id: Option<&'a str>,
    pub dims: Option<i64>,
    pub chunk_params_json: Option<&'a str>,
    pub fingerprint: Option<&'a str>,
    pub built_at: Option<&'a str>,
    pub job_id: Option<&'a str>,
    pub chunk_count: i64,
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Keyset page of semantic candidates (items with body text or prior embed).
    ///
    /// Includes prior-embed rows so digest mismatch / text clear can re-embed
    /// or clear stale vectors.
    pub fn list_semantic_candidates(
        &self,
        after_id: Option<&str>,
        limit: u64,
    ) -> Result<Vec<SemanticCandidate>> {
        let lim = limit.max(1) as i64;
        let where_clause = "matter_id = ?1 \
               AND (text_sha256 IS NOT NULL \
                    OR semantic_embedded_text_sha256 IS NOT NULL)";
        let sql = if after_id.is_some() {
            format!(
                "SELECT id, text_sha256, semantic_embedded_text_sha256, semantic_chunk_count \
             FROM items \
             WHERE {where_clause} \
               AND id > ?2 \
             ORDER BY id ASC \
             LIMIT ?3"
            )
        } else {
            format!(
                "SELECT id, text_sha256, semantic_embedded_text_sha256, semantic_chunk_count \
             FROM items \
             WHERE {where_clause} \
             ORDER BY id ASC \
             LIMIT ?2"
            )
        };
        let mut stmt = self.connection().prepare(&sql)?;
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<SemanticCandidate> {
            Ok(SemanticCandidate {
                id: row.get(0)?,
                text_sha256: row.get(1)?,
                semantic_embedded_text_sha256: row.get(2)?,
                semantic_chunk_count: row.get(3)?,
            })
        };
        let rows = if let Some(aid) = after_id {
            stmt.query_map(params![self.id(), aid, lim], map)?
        } else {
            stmt.query_map(params![self.id(), lim], map)?
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Write per-item semantic embed bookkeeping.
    pub fn write_item_semantic_meta(&self, input: WriteItemSemanticInput<'_>) -> Result<()> {
        self.ensure_item_in_matter(input.item_id)?;
        let n = self.connection().execute(
            "UPDATE items SET \
                semantic_embedded_text_sha256 = ?1, \
                semantic_embedded_at = ?2, \
                semantic_chunk_count = ?3 \
             WHERE id = ?4 AND matter_id = ?5",
            params![
                input.embedded_text_sha256,
                input.embedded_at,
                input.chunk_count,
                input.item_id,
                self.id(),
            ],
        )?;
        if n == 0 {
            return Err(Error::ItemNotFound(input.item_id.to_string()));
        }
        Ok(())
    }

    /// Clear one item's semantic embed bookkeeping (and optional catalog rows).
    pub fn clear_item_semantic(&self, item_id: &str, model_id: Option<&str>) -> Result<()> {
        self.ensure_item_in_matter(item_id)?;
        if let Some(mid) = model_id {
            self.connection().execute(
                "DELETE FROM semantic_chunks WHERE matter_id = ?1 AND item_id = ?2 AND model_id = ?3",
                params![self.id(), item_id, mid],
            )?;
        } else {
            self.connection().execute(
                "DELETE FROM semantic_chunks WHERE matter_id = ?1 AND item_id = ?2",
                params![self.id(), item_id],
            )?;
        }
        let n = self.connection().execute(
            "UPDATE items SET \
                semantic_embedded_text_sha256 = NULL, \
                semantic_embedded_at = NULL, \
                semantic_chunk_count = NULL \
             WHERE id = ?1 AND matter_id = ?2",
            params![item_id, self.id()],
        )?;
        if n == 0 {
            return Err(Error::ItemNotFound(item_id.to_string()));
        }
        Ok(())
    }

    /// Clear all item semantic meta + chunk catalog for this matter (optional model filter).
    pub fn clear_all_semantic(&self, model_id: Option<&str>) -> Result<u64> {
        if let Some(mid) = model_id {
            self.connection().execute(
                "DELETE FROM semantic_chunks WHERE matter_id = ?1 AND model_id = ?2",
                params![self.id(), mid],
            )?;
        } else {
            self.connection().execute(
                "DELETE FROM semantic_chunks WHERE matter_id = ?1",
                params![self.id()],
            )?;
        }
        let n = self.connection().execute(
            "UPDATE items SET \
                semantic_embedded_text_sha256 = NULL, \
                semantic_embedded_at = NULL, \
                semantic_chunk_count = NULL \
             WHERE matter_id = ?1",
            params![self.id()],
        )?;
        Ok(n as u64)
    }

    /// Replace chunk catalog rows for one item under `model_id`.
    pub fn replace_item_semantic_chunks(
        &self,
        item_id: &str,
        model_id: &str,
        chunks: &[UpsertSemanticChunkInput<'_>],
    ) -> Result<()> {
        self.ensure_item_in_matter(item_id)?;
        self.connection().execute(
            "DELETE FROM semantic_chunks WHERE matter_id = ?1 AND item_id = ?2 AND model_id = ?3",
            params![self.id(), item_id, model_id],
        )?;
        for c in chunks {
            let id = new_id("sch");
            self.connection().execute(
                "INSERT INTO semantic_chunks \
                    (id, matter_id, item_id, ordinal, start_offset, end_offset, text_sha256, model_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    self.id(),
                    item_id,
                    c.ordinal,
                    c.start_offset,
                    c.end_offset,
                    c.text_sha256,
                    model_id,
                ],
            )?;
        }
        Ok(())
    }

    /// Delete semantic_chunks for one item under a model.
    pub fn delete_item_semantic_chunks(&self, item_id: &str, model_id: &str) -> Result<u64> {
        self.ensure_item_in_matter(item_id)?;
        let n = self.connection().execute(
            "DELETE FROM semantic_chunks WHERE matter_id = ?1 AND item_id = ?2 AND model_id = ?3",
            params![self.id(), item_id, model_id],
        )?;
        Ok(n as u64)
    }

    /// List chunk catalog rows for a set of items under `model_id`.
    pub fn list_semantic_chunks_for_items(
        &self,
        model_id: &str,
        item_ids: &[String],
    ) -> Result<Vec<SemanticChunkRow>> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        // Chunked IN to keep SQL bounded.
        for chunk in item_ids.chunks(200) {
            let mut placeholders = String::new();
            for (i, _) in chunk.iter().enumerate() {
                if i > 0 {
                    placeholders.push(',');
                }
                placeholders.push('?');
            }
            let sql = format!(
                "SELECT id, matter_id, item_id, ordinal, start_offset, end_offset, text_sha256, model_id \
                 FROM semantic_chunks \
                 WHERE matter_id = ?1 AND model_id = ?2 AND item_id IN ({placeholders}) \
                 ORDER BY item_id ASC, ordinal ASC"
            );
            let mut stmt = self.connection().prepare(&sql)?;
            let mut params_vec: Vec<rusqlite::types::Value> = Vec::with_capacity(2 + chunk.len());
            params_vec.push(rusqlite::types::Value::Text(self.id().to_string()));
            params_vec.push(rusqlite::types::Value::Text(model_id.to_string()));
            for id in chunk {
                params_vec.push(rusqlite::types::Value::Text(id.clone()));
            }
            let rows = stmt.query_map(rusqlite::params_from_iter(params_vec), |row| {
                Ok(SemanticChunkRow {
                    id: row.get(0)?,
                    matter_id: row.get(1)?,
                    item_id: row.get(2)?,
                    ordinal: row.get(3)?,
                    start_offset: row.get(4)?,
                    end_offset: row.get(5)?,
                    text_sha256: row.get(6)?,
                    model_id: row.get(7)?,
                })
            })?;
            for r in rows {
                out.push(r?);
            }
        }
        Ok(out)
    }

    /// Load matter-level semantic meta.
    pub fn get_semantic_meta(&self) -> Result<SemanticMatterMeta> {
        self.connection()
            .query_row(
                "SELECT semantic_enabled, semantic_model_id, semantic_dims, \
                        semantic_chunk_params_json, semantic_fingerprint, \
                        semantic_built_at, semantic_job_id, semantic_chunk_count \
                 FROM matters WHERE id = ?1",
                params![self.id()],
                |row| {
                    let enabled: i64 = row.get(0)?;
                    Ok(SemanticMatterMeta {
                        semantic_enabled: enabled != 0,
                        semantic_model_id: row.get(1)?,
                        semantic_dims: row.get(2)?,
                        semantic_chunk_params_json: row.get(3)?,
                        semantic_fingerprint: row.get(4)?,
                        semantic_built_at: row.get(5)?,
                        semantic_job_id: row.get(6)?,
                        semantic_chunk_count: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    })
                },
            )
            .map_err(Error::from)
    }

    /// Update matter-level semantic meta (enable, fingerprint, counts, model).
    pub fn update_semantic_matter_meta(
        &self,
        input: UpdateSemanticMatterMetaInput<'_>,
    ) -> Result<()> {
        let n = self.connection().execute(
            "UPDATE matters SET \
                semantic_enabled = ?1, \
                semantic_model_id = ?2, \
                semantic_dims = ?3, \
                semantic_chunk_params_json = ?4, \
                semantic_fingerprint = ?5, \
                semantic_built_at = ?6, \
                semantic_job_id = ?7, \
                semantic_chunk_count = ?8 \
             WHERE id = ?9",
            params![
                if input.enabled { 1i64 } else { 0i64 },
                input.model_id,
                input.dims,
                input.chunk_params_json,
                input.fingerprint,
                input.built_at,
                input.job_id,
                input.chunk_count,
                self.id(),
            ],
        )?;
        if n == 0 {
            return Err(Error::MatterRowMissing);
        }
        Ok(())
    }

    /// Clear matter-level semantic meta (disable index bookkeeping).
    pub fn clear_semantic_matter_meta(&self) -> Result<()> {
        self.update_semantic_matter_meta(UpdateSemanticMatterMetaInput {
            enabled: false,
            model_id: None,
            dims: None,
            chunk_params_json: None,
            fingerprint: None,
            built_at: None,
            job_id: None,
            chunk_count: 0,
        })
    }

    /// Resolve [`FilterSpec`] to eligible item ids (**pre-filter** for semantic query).
    ///
    /// Uses the same SQL compile path as the Review filtered list. `limit` caps
    /// returned ids (`0` or `u64::MAX` → no practical cap).
    pub fn list_filtered_item_ids(&self, filter: &FilterSpec, limit: u64) -> Result<Vec<String>> {
        let lim = if limit == 0 { u64::MAX } else { limit };
        let rows = self.list_items_filtered_thin(filter, lim, 0)?;
        Ok(rows.into_iter().map(|r| r.id).collect())
    }

    /// Convenience: current UTC timestamp for semantic bookkeeping.
    pub fn semantic_now() -> String {
        now_rfc3339()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_meta_defaults_disabled() {
        // Unit smoke for type construction only.
        let m = SemanticMatterMeta {
            semantic_enabled: false,
            semantic_model_id: None,
            semantic_dims: None,
            semantic_chunk_params_json: None,
            semantic_fingerprint: None,
            semantic_built_at: None,
            semantic_job_id: None,
            semantic_chunk_count: 0,
        };
        assert!(!m.semantic_enabled);
        assert_eq!(m.semantic_chunk_count, 0);
    }
}
