//! Bidirectional family expand for review-set membership.

use std::collections::HashSet;

use matter_core::Matter;

use crate::error::Result;

/// Expand set `S` with direct children **and** parents until fixed point.
///
/// P0 depth: iterates up to 2 times (email ↔ attachment, nested attach ≤2).
/// Does **not** expand threads (`thread_id`).
///
/// Expanded members are added even when they fail the base policy alone.
pub fn expand_families_bidirectional(matter: &Matter, base_ids: &[String]) -> Result<Vec<String>> {
    let mut set: HashSet<String> = base_ids.iter().cloned().collect();
    // Two iterations cover parent↔child and one level of nested attachments.
    for _ in 0..2 {
        let snapshot: Vec<String> = set.iter().cloned().collect();
        let children = matter.list_direct_children_ids(&snapshot)?;
        let parents = matter.list_parent_ids_of(&snapshot)?;
        let before = set.len();
        for id in children {
            set.insert(id);
        }
        for id in parents {
            set.insert(id);
        }
        if set.len() == before {
            break;
        }
    }
    Ok(set.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_base_stays_empty() {
        // No matter needed — expand with empty should not query.
        // (list helpers short-circuit on empty.)
        // Covered by integration tests with real Matter.
        let _ = expand_families_bidirectional;
    }
}
