//! Production QC run persistence and selection fingerprint (track **0041**).
//!
//! Findings live on disk (CSV pack). This module stores **run history** so
//! produce can require a fresh, passed QC over the same candidate set.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::matter::{new_id, now_rfc3339, Matter};

// ---------------------------------------------------------------------------
// Fingerprint
// ---------------------------------------------------------------------------

/// SHA-256 hex of sorted candidate item ids joined by `\n`.
///
/// Empty list → hash of the empty string (stable).
/// Order-independent: input is cloned and sorted before hashing.
///
/// Prefer [`selection_fingerprint_with_pack`] for produce-gate authorization
/// so a different QC pack cannot reuse a pass under another severity profile.
pub fn selection_fingerprint(ids: &[String]) -> String {
    selection_fingerprint_with_pack(ids, "")
}

/// SHA-256 hex of sorted candidate ids + optional QC pack id (track **0060**).
///
/// Format (stable):
/// ```text
/// <sorted ids joined by \n>
/// \n#pack=<pack_id>
/// ```
/// When `pack_id` is empty, the `#pack=` suffix is omitted so the digest matches
/// the historical selection-only fingerprint used before multi-pack QC.
pub fn selection_fingerprint_with_pack(ids: &[String], pack_id: &str) -> String {
    let mut sorted: Vec<&str> = ids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut joined = sorted.join("\n");
    let pack = pack_id.trim();
    if !pack.is_empty() {
        if !joined.is_empty() {
            joined.push('\n');
        }
        joined.push_str("#pack=");
        joined.push_str(pack);
    }
    let digest = Sha256::digest(joined.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// One row from `qc_runs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QcRunRecord {
    pub id: String,
    pub matter_id: String,
    pub profile: String,
    pub created_at: String,
    /// `true` when `error_count == 0` at insert time.
    pub passed: bool,
    pub error_count: u64,
    pub warn_count: u64,
    pub candidate_count: u64,
    pub selection_fingerprint: String,
    /// `review_corpus` or `item_ids`.
    pub scope: String,
    pub scope_json: Option<String>,
    pub report_path: Option<String>,
    pub job_id: Option<String>,
    pub rules_json: Option<String>,
}

/// Input for [`Matter::insert_qc_run`].
#[derive(Debug, Clone)]
pub struct InsertQcRunInput {
    pub profile: String,
    pub passed: bool,
    pub error_count: u64,
    pub warn_count: u64,
    pub candidate_count: u64,
    pub selection_fingerprint: String,
    pub scope: String,
    pub scope_json: Option<String>,
    pub report_path: Option<String>,
    pub job_id: Option<String>,
    pub rules_json: Option<String>,
}

// ---------------------------------------------------------------------------
// Freshness gate
// ---------------------------------------------------------------------------

/// Whether a stored QC run still authorizes produce for the current selection.
///
/// ```text
/// passed
///   && scope matches
///   && candidate_count == current.len()
///   && fingerprint == fingerprint(current, pack)
/// ```
///
/// Uses the pack id recorded on the run (`profile` column, normalized) so
/// workflow and other callers that do not pass an explicit pack still match
/// runs created with pack-aware fingerprints (track **0060**). Prefer
/// [`qc_run_is_fresh_for_pack`] when the *produce* profile binds a specific pack.
pub fn qc_run_is_fresh(
    stored: &QcRunRecord,
    current_scope: &str,
    current_candidate_ids: &[String],
) -> bool {
    let pack = crate::production_profile::normalize_qc_pack_id(&stored.profile);
    qc_run_is_fresh_for_pack(stored, current_scope, current_candidate_ids, &pack)
}

/// Like [`qc_run_is_fresh`], but fingerprints include `pack_id` so a pass under
/// one severity pack cannot authorize produce bound to another (track **0060**).
pub fn qc_run_is_fresh_for_pack(
    stored: &QcRunRecord,
    current_scope: &str,
    current_candidate_ids: &[String],
    pack_id: &str,
) -> bool {
    if !stored.passed {
        return false;
    }
    if stored.scope != current_scope {
        return false;
    }
    if stored.candidate_count != current_candidate_ids.len() as u64 {
        return false;
    }
    let fp = selection_fingerprint_with_pack(current_candidate_ids, pack_id);
    stored.selection_fingerprint == fp
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Insert a QC run row; returns the new id.
    pub fn insert_qc_run(&self, input: InsertQcRunInput) -> Result<QcRunRecord> {
        let id = new_id("qcr");
        let created_at = now_rfc3339();
        let passed_i: i64 = if input.passed { 1 } else { 0 };
        self.connection().execute(
            "INSERT INTO qc_runs \
             (id, matter_id, profile, created_at, passed, error_count, warn_count, \
              candidate_count, selection_fingerprint, scope, scope_json, report_path, \
              job_id, rules_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                id,
                self.id(),
                input.profile,
                created_at,
                passed_i,
                input.error_count as i64,
                input.warn_count as i64,
                input.candidate_count as i64,
                input.selection_fingerprint,
                input.scope,
                input.scope_json,
                input.report_path,
                input.job_id,
                input.rules_json,
            ],
        )?;
        Ok(QcRunRecord {
            id,
            matter_id: self.id().to_string(),
            profile: input.profile,
            created_at,
            passed: input.passed,
            error_count: input.error_count,
            warn_count: input.warn_count,
            candidate_count: input.candidate_count,
            selection_fingerprint: input.selection_fingerprint,
            scope: input.scope,
            scope_json: input.scope_json,
            report_path: input.report_path,
            job_id: input.job_id,
            rules_json: input.rules_json,
        })
    }

    /// Latest QC run for this matter (any scope), newest `created_at` first.
    pub fn load_latest_qc_run(&self) -> Result<Option<QcRunRecord>> {
        self.load_latest_qc_run_for_scope(None)
    }

    /// Latest QC run for this matter, optionally filtered by `scope`.
    pub fn load_latest_qc_run_for_scope(&self, scope: Option<&str>) -> Result<Option<QcRunRecord>> {
        let mut sql = String::from(
            "SELECT id, matter_id, profile, created_at, passed, error_count, warn_count, \
             candidate_count, selection_fingerprint, scope, scope_json, report_path, \
             job_id, rules_json \
             FROM qc_runs WHERE matter_id = ?1",
        );
        if scope.is_some() {
            sql.push_str(" AND scope = ?2");
        }
        sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT 1");

        let mut stmt = self.connection().prepare(&sql)?;
        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<QcRunRecord> {
            let passed_i: i64 = row.get(4)?;
            Ok(QcRunRecord {
                id: row.get(0)?,
                matter_id: row.get(1)?,
                profile: row.get(2)?,
                created_at: row.get(3)?,
                passed: passed_i != 0,
                error_count: row.get::<_, i64>(5)? as u64,
                warn_count: row.get::<_, i64>(6)? as u64,
                candidate_count: row.get::<_, i64>(7)? as u64,
                selection_fingerprint: row.get(8)?,
                scope: row.get(9)?,
                scope_json: row.get(10)?,
                report_path: row.get(11)?,
                job_id: row.get(12)?,
                rules_json: row.get(13)?,
            })
        };

        let result = if let Some(s) = scope {
            stmt.query_row(params![self.id(), s], map_row)
        } else {
            stmt.query_row(params![self.id()], map_row)
        };

        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_order_independent() {
        let a = vec!["b".into(), "a".into(), "c".into()];
        let b = vec!["c".into(), "b".into(), "a".into()];
        assert_eq!(selection_fingerprint(&a), selection_fingerprint(&b));
    }

    #[test]
    fn fingerprint_empty_stable() {
        let empty: Vec<String> = vec![];
        let fp = selection_fingerprint(&empty);
        assert_eq!(fp.len(), 64);
        // SHA-256 of empty string
        assert_eq!(
            fp,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn fingerprint_changes_when_member_added() {
        let base = vec!["a".into(), "b".into()];
        let more = vec!["a".into(), "b".into(), "c".into()];
        assert_ne!(selection_fingerprint(&base), selection_fingerprint(&more));
    }

    #[test]
    fn qc_run_is_fresh_requires_pass_scope_count_fp() {
        let ids = vec!["i1".into(), "i2".into()];
        let pack = crate::production_profile::QC_PACK_DEFAULT_V1;
        let fp = selection_fingerprint_with_pack(&ids, pack);
        let stored = QcRunRecord {
            id: "qcr1".into(),
            matter_id: "m1".into(),
            profile: pack.into(),
            created_at: "2020-01-01T00:00:00Z".into(),
            passed: true,
            error_count: 0,
            warn_count: 1,
            candidate_count: 2,
            selection_fingerprint: fp.clone(),
            scope: "review_corpus".into(),
            scope_json: None,
            report_path: None,
            job_id: None,
            rules_json: None,
        };
        assert!(qc_run_is_fresh(&stored, "review_corpus", &ids));
        assert!(qc_run_is_fresh_for_pack(
            &stored,
            "review_corpus",
            &ids,
            pack
        ));
        // Different pack → not fresh.
        assert!(!qc_run_is_fresh_for_pack(
            &stored,
            "review_corpus",
            &ids,
            crate::production_profile::QC_PACK_STRICT_PRIVILEGE_V1
        ));

        // failed
        let mut failed = stored.clone();
        failed.passed = false;
        assert!(!qc_run_is_fresh(&failed, "review_corpus", &ids));

        // scope mismatch
        assert!(!qc_run_is_fresh(&stored, "item_ids", &ids));

        // count mismatch
        let one = vec!["i1".into()];
        assert!(!qc_run_is_fresh(&stored, "review_corpus", &one));

        // fingerprint mismatch (same count, different ids)
        let other = vec!["x1".into(), "x2".into()];
        assert!(!qc_run_is_fresh(&stored, "review_corpus", &other));
    }
}
