//! Concept clustering storage and list APIs (schema v27 / track 0048).
//!
//! Orthogonal to near-dup (`near_dup_*`). Membership lives in
//! `item_concept_membership`; optional denorm columns on `items` mirror the
//! **default** set only.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::matter::{new_id, now_rfc3339, Matter};

/// Default concept cluster set name (P0).
pub const DEFAULT_CONCEPT_SET_NAME: &str = "default";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One row from `concept_cluster_sets`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptClusterSet {
    pub id: String,
    pub matter_id: String,
    pub name: String,
    pub method: String,
    /// Requested k (target).
    pub k: i64,
    /// Actual non-empty clusters written.
    pub cluster_count: i64,
    pub params_json: String,
    pub fingerprint: Option<String>,
    pub item_count: i64,
    pub built_at: Option<String>,
    pub job_id: Option<String>,
}

/// One row from `concept_clusters` (item_count always > 0 when written by engine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptCluster {
    pub id: String,
    pub set_id: String,
    pub matter_id: String,
    /// Dense ordinal 0..cluster_count-1.
    pub ordinal: i64,
    pub label: String,
    pub label_terms_json: String,
    pub item_count: i64,
}

/// Desk / API status for a named set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ConceptClusterStatus {
    pub set_id: Option<String>,
    pub set_name: String,
    pub method: Option<String>,
    pub k: Option<i64>,
    pub cluster_count: i64,
    pub item_count: i64,
    pub built_at: Option<String>,
    pub fingerprint: Option<String>,
    pub job_id: Option<String>,
    pub is_complete: bool,
    /// Current inventory digest (sha256 of ordered id\\0text_sha256 lines).
    pub current_inventory_digest: Option<String>,
    /// True when stored fingerprint inventory portion differs from current candidates.
    pub is_stale: bool,
}

/// Candidate item for concept clustering (non-null `text_sha256`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConceptClusterCandidate {
    pub id: String,
    pub text_sha256: String,
}

/// One membership row to write (item → cluster).
#[derive(Debug, Clone)]
pub struct ConceptMembershipWrite {
    pub item_id: String,
    pub distance: Option<f64>,
}

/// One non-empty cluster payload for atomic replace.
#[derive(Debug, Clone)]
pub struct ConceptClusterWrite {
    pub ordinal: i64,
    pub label: String,
    pub label_terms_json: String,
    pub members: Vec<ConceptMembershipWrite>,
}

/// Input for [`Matter::replace_concept_cluster_set`].
#[derive(Debug, Clone)]
pub struct ReplaceConceptClusterSetInput {
    pub set_name: String,
    pub method: String,
    /// Requested k.
    pub k: i64,
    pub params_json: String,
    pub fingerprint: String,
    pub job_id: Option<String>,
    /// Non-empty clusters only (engine must drop empty first).
    pub clusters: Vec<ConceptClusterWrite>,
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Inventory digest of concept candidates: sha256 of ordered `id\\0text_sha256\\n`.
    ///
    /// Used by concept clustering fingerprint / Desk stale detection (track 0048).
    pub fn concept_cluster_inventory_digest(&self) -> Result<String> {
        use crate::sha256_hex;
        let mut stmt = self.connection().prepare(
            "SELECT id, text_sha256 FROM items \
             WHERE matter_id = ?1 \
               AND text_sha256 IS NOT NULL AND TRIM(text_sha256) != '' \
             ORDER BY id ASC",
        )?;
        let mut buf = String::new();
        let rows = stmt.query_map(params![self.id()], |row| {
            let id: String = row.get(0)?;
            let text: String = row.get(1)?;
            Ok((id, text))
        })?;
        for r in rows {
            let (id, text) = r?;
            buf.push_str(&id);
            buf.push('\0');
            buf.push_str(&text);
            buf.push('\n');
        }
        Ok(sha256_hex(buf.as_bytes()))
    }

    /// Status for a named concept cluster set (`default` when name empty).
    pub fn concept_cluster_status(&self, set_name: &str) -> Result<ConceptClusterStatus> {
        let name = if set_name.trim().is_empty() {
            DEFAULT_CONCEPT_SET_NAME
        } else {
            set_name.trim()
        };
        let matter_id = self.id();
        let current_inventory = self.concept_cluster_inventory_digest()?;
        let row = self.connection().query_row(
            "SELECT id, method, k, cluster_count, item_count, built_at, fingerprint, job_id \
             FROM concept_cluster_sets \
             WHERE matter_id = ?1 AND name = ?2",
            params![matter_id, name],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        );
        match row {
            Ok((id, method, k, cluster_count, item_count, built_at, fingerprint, job_id)) => {
                let is_complete = built_at.is_some();
                // Fingerprint format: `{params_hex}:{inventory_digest}` (engine-owned).
                let is_stale = fingerprint
                    .as_deref()
                    .and_then(|fp| fp.rsplit_once(':').map(|(_, inv)| inv != current_inventory))
                    .unwrap_or(false);
                Ok(ConceptClusterStatus {
                    set_id: Some(id),
                    set_name: name.to_string(),
                    method: Some(method),
                    k: Some(k),
                    cluster_count,
                    item_count,
                    built_at,
                    fingerprint,
                    job_id,
                    is_complete,
                    current_inventory_digest: Some(current_inventory),
                    is_stale,
                })
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(ConceptClusterStatus {
                set_id: None,
                set_name: name.to_string(),
                method: None,
                k: None,
                cluster_count: 0,
                item_count: 0,
                built_at: None,
                fingerprint: None,
                job_id: None,
                is_complete: false,
                current_inventory_digest: Some(current_inventory),
                is_stale: false,
            }),
            Err(e) => Err(Error::from(e)),
        }
    }

    /// List clusters for a set, ordered by `item_count` DESC then ordinal ASC.
    ///
    /// Never assume `len == k` — iterate actual rows only.
    pub fn list_concept_clusters(&self, set_id: &str) -> Result<Vec<ConceptCluster>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, set_id, matter_id, ordinal, label, label_terms_json, item_count \
             FROM concept_clusters \
             WHERE set_id = ?1 \
             ORDER BY item_count DESC, ordinal ASC",
        )?;
        let rows = stmt.query_map(params![set_id], map_cluster_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Resolve set id by name for this matter, if present.
    pub fn get_concept_cluster_set_id(&self, set_name: &str) -> Result<Option<String>> {
        let name = if set_name.trim().is_empty() {
            DEFAULT_CONCEPT_SET_NAME
        } else {
            set_name.trim()
        };
        let r = self.connection().query_row(
            "SELECT id FROM concept_cluster_sets WHERE matter_id = ?1 AND name = ?2",
            params![self.id(), name],
            |row| row.get(0),
        );
        match r {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::from(e)),
        }
    }

    /// Load a concept cluster set row by name.
    pub fn get_concept_cluster_set(&self, set_name: &str) -> Result<Option<ConceptClusterSet>> {
        let name = if set_name.trim().is_empty() {
            DEFAULT_CONCEPT_SET_NAME
        } else {
            set_name.trim()
        };
        let r = self.connection().query_row(
            "SELECT id, matter_id, name, method, k, cluster_count, params_json, \
                    fingerprint, item_count, built_at, job_id \
             FROM concept_cluster_sets WHERE matter_id = ?1 AND name = ?2",
            params![self.id(), name],
            map_set_row,
        );
        match r {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::from(e)),
        }
    }

    /// Candidates: items with non-null `text_sha256` (keyset page).
    pub fn list_concept_cluster_candidates(
        &self,
        after_id: Option<&str>,
        limit: u64,
    ) -> Result<Vec<ConceptClusterCandidate>> {
        let lim = limit.max(1) as i64;
        let sql = if after_id.is_some() {
            "SELECT id, text_sha256 FROM items \
             WHERE matter_id = ?1 \
               AND text_sha256 IS NOT NULL AND TRIM(text_sha256) != '' \
               AND id > ?2 \
             ORDER BY id ASC LIMIT ?3"
        } else {
            "SELECT id, text_sha256 FROM items \
             WHERE matter_id = ?1 \
               AND text_sha256 IS NOT NULL AND TRIM(text_sha256) != '' \
             ORDER BY id ASC LIMIT ?2"
        };
        let mut stmt = self.connection().prepare(sql)?;
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ConceptClusterCandidate> {
            Ok(ConceptClusterCandidate {
                id: row.get(0)?,
                text_sha256: row.get(1)?,
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

    /// Count of candidates (non-null text_sha256).
    pub fn count_concept_cluster_candidates(&self) -> Result<u64> {
        let n: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM items \
             WHERE matter_id = ?1 \
               AND text_sha256 IS NOT NULL AND TRIM(text_sha256) != ''",
            params![self.id()],
            |row| row.get(0),
        )?;
        Ok(n as u64)
    }

    /// Clear a named set (membership, clusters, set row) and default denorm when applicable.
    pub fn clear_concept_cluster_set(&self, set_name: &str) -> Result<()> {
        let name = if set_name.trim().is_empty() {
            DEFAULT_CONCEPT_SET_NAME
        } else {
            set_name.trim()
        };
        let matter_id = self.id().to_string();
        self.with_transaction(|conn| {
            let set_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM concept_cluster_sets WHERE matter_id = ?1 AND name = ?2",
                    params![matter_id, name],
                    |row| row.get(0),
                )
                .ok();
            if let Some(sid) = set_id {
                conn.execute(
                    "DELETE FROM item_concept_membership WHERE set_id = ?1",
                    params![sid],
                )?;
                conn.execute(
                    "DELETE FROM concept_clusters WHERE set_id = ?1",
                    params![sid],
                )?;
                conn.execute(
                    "DELETE FROM concept_cluster_sets WHERE id = ?1",
                    params![sid],
                )?;
                if name == DEFAULT_CONCEPT_SET_NAME {
                    conn.execute(
                        "UPDATE items SET concept_cluster_id = NULL, \
                            concept_cluster_set_id = NULL, concept_clustered_at = NULL \
                         WHERE matter_id = ?1 AND concept_cluster_set_id = ?2",
                        params![matter_id, sid],
                    )?;
                }
            }
            Ok(())
        })
    }

    /// Atomic replace: clear prior membership/clusters for the set, upsert set,
    /// insert non-empty clusters + membership; set `built_at` only on commit.
    ///
    /// Default-set denorm on `items` is updated when `set_name == "default"`.
    /// Clusters with empty `members` are skipped (engine should already drop them).
    pub fn replace_concept_cluster_set(
        &self,
        input: ReplaceConceptClusterSetInput,
    ) -> Result<ConceptClusterSet> {
        let name = if input.set_name.trim().is_empty() {
            DEFAULT_CONCEPT_SET_NAME.to_string()
        } else {
            input.set_name.trim().to_string()
        };
        let matter_id = self.id().to_string();
        let is_default = name == DEFAULT_CONCEPT_SET_NAME;

        // Filter empty (defense in depth).
        let clusters: Vec<&ConceptClusterWrite> = input
            .clusters
            .iter()
            .filter(|c| !c.members.is_empty())
            .collect();
        let cluster_count = clusters.len() as i64;
        let item_count: i64 = clusters.iter().map(|c| c.members.len() as i64).sum();
        let now = now_rfc3339();

        self.with_transaction(|conn| {
            // Existing set id or new.
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM concept_cluster_sets WHERE matter_id = ?1 AND name = ?2",
                    params![matter_id, name],
                    |row| row.get(0),
                )
                .ok();
            let set_id = existing.unwrap_or_else(|| new_id("ccs"));

            // Clear prior rows for this set.
            conn.execute(
                "DELETE FROM item_concept_membership WHERE set_id = ?1",
                params![set_id],
            )?;
            conn.execute(
                "DELETE FROM concept_clusters WHERE set_id = ?1",
                params![set_id],
            )?;

            if is_default {
                // Clear prior default-set denorm for this set id (and any orphan default pointers).
                conn.execute(
                    "UPDATE items SET concept_cluster_id = NULL, \
                        concept_cluster_set_id = NULL, concept_clustered_at = NULL \
                     WHERE matter_id = ?1 AND \
                       (concept_cluster_set_id = ?2 OR concept_cluster_set_id IS NOT NULL)",
                    params![matter_id, set_id],
                )?;
            }

            conn.execute(
                "INSERT INTO concept_cluster_sets \
                 (id, matter_id, name, method, k, cluster_count, params_json, \
                  fingerprint, item_count, built_at, job_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
                 ON CONFLICT(matter_id, name) DO UPDATE SET \
                   method = excluded.method, \
                   k = excluded.k, \
                   cluster_count = excluded.cluster_count, \
                   params_json = excluded.params_json, \
                   fingerprint = excluded.fingerprint, \
                   item_count = excluded.item_count, \
                   built_at = excluded.built_at, \
                   job_id = excluded.job_id",
                params![
                    set_id,
                    matter_id,
                    name,
                    input.method,
                    input.k,
                    cluster_count,
                    input.params_json,
                    input.fingerprint,
                    item_count,
                    now,
                    input.job_id,
                ],
            )?;

            for c in &clusters {
                if c.members.is_empty() {
                    continue;
                }
                let cluster_id = new_id("ccl");
                let n = c.members.len() as i64;
                conn.execute(
                    "INSERT INTO concept_clusters \
                     (id, set_id, matter_id, ordinal, label, label_terms_json, item_count) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        cluster_id,
                        set_id,
                        matter_id,
                        c.ordinal,
                        c.label,
                        c.label_terms_json,
                        n,
                    ],
                )?;
                for m in &c.members {
                    let mid = new_id("icm");
                    conn.execute(
                        "INSERT INTO item_concept_membership \
                         (id, matter_id, set_id, item_id, cluster_id, distance) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![mid, matter_id, set_id, m.item_id, cluster_id, m.distance],
                    )?;
                    if is_default {
                        conn.execute(
                            "UPDATE items SET concept_cluster_id = ?1, \
                                concept_cluster_set_id = ?2, concept_clustered_at = ?3 \
                             WHERE id = ?4 AND matter_id = ?5",
                            params![cluster_id, set_id, now, m.item_id, matter_id],
                        )?;
                    }
                }
            }

            Ok(ConceptClusterSet {
                id: set_id,
                matter_id,
                name,
                method: input.method.clone(),
                k: input.k,
                cluster_count,
                params_json: input.params_json.clone(),
                fingerprint: Some(input.fingerprint.clone()),
                item_count,
                built_at: Some(now),
                job_id: input.job_id.clone(),
            })
        })
    }
}

fn map_set_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConceptClusterSet> {
    Ok(ConceptClusterSet {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        name: row.get(2)?,
        method: row.get(3)?,
        k: row.get(4)?,
        cluster_count: row.get(5)?,
        params_json: row.get(6)?,
        fingerprint: row.get(7)?,
        item_count: row.get(8)?,
        built_at: row.get(9)?,
        job_id: row.get(10)?,
    })
}

fn map_cluster_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConceptCluster> {
    Ok(ConceptCluster {
        id: row.get(0)?,
        set_id: row.get(1)?,
        matter_id: row.get(2)?,
        ordinal: row.get(3)?,
        label: row.get(4)?,
        label_terms_json: row.get(5)?,
        item_count: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matter::{item_status, ItemInput};
    use tempfile::tempdir;

    fn temp_matter() -> (tempfile::TempDir, Matter) {
        let tmp = tempdir().expect("tmp");
        let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
        let m = Matter::create(&root, "cluster-api").expect("create");
        (tmp, m)
    }

    #[test]
    fn replace_and_list_drops_empty_and_sets_built_at() {
        let (_tmp, matter) = temp_matter();
        let a = matter
            .insert_item(ItemInput {
                path: Some("a.txt".into()),
                status: item_status::EXTRACTED.into(),
                text_sha256: Some("aa".repeat(32)),
                ..Default::default()
            })
            .expect("a");
        let b = matter
            .insert_item(ItemInput {
                path: Some("b.txt".into()),
                status: item_status::EXTRACTED.into(),
                text_sha256: Some("bb".repeat(32)),
                ..Default::default()
            })
            .expect("b");

        let set = matter
            .replace_concept_cluster_set(ReplaceConceptClusterSetInput {
                set_name: "default".into(),
                method: "tfidf_kmeans_v1".into(),
                k: 5,
                params_json: "{}".into(),
                fingerprint: "fp1".into(),
                job_id: Some("job1".into()),
                clusters: vec![
                    ConceptClusterWrite {
                        ordinal: 0,
                        label: "alpha beta".into(),
                        label_terms_json: r#"["alpha","beta"]"#.into(),
                        members: vec![ConceptMembershipWrite {
                            item_id: a.id.clone(),
                            distance: Some(0.1),
                        }],
                    },
                    ConceptClusterWrite {
                        ordinal: 1,
                        label: "gamma".into(),
                        label_terms_json: r#"["gamma"]"#.into(),
                        members: vec![ConceptMembershipWrite {
                            item_id: b.id.clone(),
                            distance: None,
                        }],
                    },
                    // empty — skipped
                    ConceptClusterWrite {
                        ordinal: 2,
                        label: "empty".into(),
                        label_terms_json: "[]".into(),
                        members: vec![],
                    },
                ],
            })
            .expect("replace");

        assert_eq!(set.k, 5);
        assert_eq!(set.cluster_count, 2);
        assert_eq!(set.item_count, 2);
        assert!(set.built_at.is_some());

        let clusters = matter.list_concept_clusters(&set.id).expect("list");
        assert_eq!(clusters.len(), 2);
        assert!(clusters.iter().all(|c| c.item_count > 0));

        let status = matter.concept_cluster_status("default").expect("status");
        assert!(status.is_complete);
        assert_eq!(status.cluster_count, 2);
        assert_eq!(status.k, Some(5));

        let item_a = matter.get_item(&a.id).expect("get");
        assert_eq!(
            item_a.concept_cluster_set_id.as_deref(),
            Some(set.id.as_str())
        );
        assert!(item_a.concept_cluster_id.is_some());
    }

    #[test]
    fn candidates_require_text_sha256() {
        let (_tmp, matter) = temp_matter();
        matter
            .insert_item(ItemInput {
                path: Some("no.txt".into()),
                status: item_status::EXTRACTED.into(),
                ..Default::default()
            })
            .expect("no");
        matter
            .insert_item(ItemInput {
                path: Some("yes.txt".into()),
                status: item_status::EXTRACTED.into(),
                text_sha256: Some("cc".repeat(32)),
                ..Default::default()
            })
            .expect("yes");
        let cands = matter
            .list_concept_cluster_candidates(None, 100)
            .expect("list");
        assert_eq!(cands.len(), 1);
        assert_eq!(matter.count_concept_cluster_candidates().expect("n"), 1);
    }
}
