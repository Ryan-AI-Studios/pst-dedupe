//! Candidate selection for production QC (mirrors matter-produce).

use std::collections::HashSet;

use matter_core::Matter;
use rusqlite::params;

use crate::error::{QcError, Result};
use crate::params::{QcParams, SCOPE_ITEM_IDS, SCOPE_REVIEW_CORPUS};

/// Select candidate item ids for QC scan.
pub fn select_item_ids(matter: &Matter, params: &QcParams) -> Result<Vec<String>> {
    match params.scope.as_str() {
        SCOPE_REVIEW_CORPUS => {
            let mut ids = list_in_review_ids(matter)?;
            if params.expand_family_for_scan {
                ids = expand_family_ids(matter, &ids)?;
            }
            Ok(ids)
        }
        SCOPE_ITEM_IDS => {
            let mut ids = params.item_ids.clone();
            let mut seen = HashSet::new();
            ids.retain(|id| seen.insert(id.clone()));
            if params.expand_family_for_scan {
                ids = expand_family_ids(matter, &ids)?;
            }
            Ok(ids)
        }
        other => Err(QcError::InvalidParams(format!("unknown scope '{other}'"))),
    }
}

fn list_in_review_ids(matter: &Matter) -> Result<Vec<String>> {
    let mut stmt = matter.connection().prepare(
        "SELECT id FROM items \
         WHERE matter_id = ?1 AND in_review = 1 \
         ORDER BY COALESCE(review_order, 999999999), id ASC",
    )?;
    let rows = stmt.query_map(params![matter.id()], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Lightweight family expand: include direct children and parents of selected.
fn expand_family_ids(matter: &Matter, base: &[String]) -> Result<Vec<String>> {
    let mut set: HashSet<String> = base.iter().cloned().collect();
    for id in base {
        let item = matter.get_item(id)?;
        if let Some(parent) = item.parent_item_id.as_deref() {
            set.insert(parent.to_string());
        }
        if let Some(fid) = item.family_id.as_deref() {
            let mut stmt = matter
                .connection()
                .prepare("SELECT id FROM items WHERE matter_id = ?1 AND family_id = ?2")?;
            let rows = stmt.query_map(params![matter.id(), fid], |row| row.get::<_, String>(0))?;
            for r in rows {
                set.insert(r?);
            }
        }
        let mut stmt = matter
            .connection()
            .prepare("SELECT id FROM items WHERE matter_id = ?1 AND parent_item_id = ?2")?;
        let rows = stmt.query_map(params![matter.id(), id], |row| row.get::<_, String>(0))?;
        for r in rows {
            set.insert(r?);
        }
    }
    let mut out: Vec<String> = set.into_iter().collect();
    out.sort();
    Ok(out)
}
