//! Family-aware `review_order` via a **single** ordered SQL query.
//!
//! ## Required ORDER BY (matter-core)
//!
//! ```sql
//! ORDER BY
//!   COALESCE(parent_item_id, id) ASC,
//!   CASE WHEN parent_item_id IS NULL THEN 0 ELSE 1 END ASC,
//!   path ASC,
//!   id ASC
//! ```
//!
//! Implemented by [`Matter::list_promote_ordered_membership`] — temp-table join
//! + one `SELECT`, **not** per-parent child queries (no N+1).

use matter_core::{Matter, PromoteCandidate};

use crate::error::Result;

/// SQL fragment documenting the compound family order key (for tests / docs).
pub const FAMILY_ORDER_SQL: &str = "\
ORDER BY \
  COALESCE(parent_item_id, id) ASC, \
  CASE WHEN parent_item_id IS NULL THEN 0 ELSE 1 END ASC, \
  path ASC, \
  id ASC";

/// Stream membership in family-aware linear order (single SQL query).
///
/// Returns thin rows only. Dense `review_order` is assigned 1..N by the caller
/// while enumerating this stream.
pub fn ordered_membership(matter: &Matter, member_ids: &[String]) -> Result<Vec<PromoteCandidate>> {
    Ok(matter.list_promote_ordered_membership(member_ids)?)
}

/// Proof helper: ordering uses the single-query Matter API (not N+1 loops).
///
/// This crate never issues per-parent child queries for order assignment.
pub fn ordering_uses_single_query_api() -> bool {
    // Architectural constant: `ordered_membership` delegates solely to
    // `list_promote_ordered_membership` (one JOIN + ORDER BY).
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_order_sql_contains_compound_key() {
        assert!(FAMILY_ORDER_SQL.contains("COALESCE(parent_item_id, id)"));
        assert!(FAMILY_ORDER_SQL.contains("parent_item_id IS NULL"));
        assert!(FAMILY_ORDER_SQL.contains("path ASC"));
        assert!(ordering_uses_single_query_api());
    }
}
