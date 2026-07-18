//! Family integrity pass after per-item cull evaluation.

use std::collections::{HashMap, HashSet};

use matter_core::{item_cull_status, CullCandidate};

use crate::eval::ItemCullDecision;
use crate::rules::{reason, FamilyPolicy};

/// Apply family policy to a map of item_id → decision (mutates in place).
///
/// - `KeepChildrenWithIncludedParent` (default, **absolute**): for each
///   included parent, force all direct children to included and clear reasons.
/// - `CullChildrenWithParent`: culled parent → children culled with
///   `family_with_culled_parent` (adds reason if not already culled).
/// - `Independent`: no-op.
pub fn apply_family_policy(
    candidates: &[CullCandidate],
    decisions: &mut HashMap<String, ItemCullDecision>,
    policy: FamilyPolicy,
) {
    match policy {
        FamilyPolicy::Independent => {}
        FamilyPolicy::KeepChildrenWithIncludedParent => {
            keep_children_with_included_parent(candidates, decisions);
        }
        FamilyPolicy::CullChildrenWithParent => {
            cull_children_with_parent(candidates, decisions);
        }
    }
}

fn keep_children_with_included_parent(
    candidates: &[CullCandidate],
    decisions: &mut HashMap<String, ItemCullDecision>,
) {
    // Parents that are included after item pass.
    let included_parents: HashSet<String> = candidates
        .iter()
        .filter(|c| {
            decisions
                .get(&c.id)
                .map(|d| !d.is_culled())
                .unwrap_or(false)
                && c.parent_item_id.is_none()
        })
        .map(|c| c.id.clone())
        .collect();

    // Also treat any item that has children as a potential parent even if it
    // itself has a parent_item_id (nested). Spec says *direct* children of
    // included parents — use parent_item_id link.
    let included_ids: HashSet<String> = decisions
        .iter()
        .filter(|(_, d)| !d.is_culled())
        .map(|(id, _)| id.clone())
        .collect();

    let _ = included_parents; // reserved for future parent-role filter

    for c in candidates {
        let Some(ref parent_id) = c.parent_item_id else {
            continue;
        };
        if included_ids.contains(parent_id) {
            // Absolute: force included, clear reasons (even exact_duplicate).
            decisions.insert(c.id.clone(), ItemCullDecision::included());
        }
    }
}

fn cull_children_with_parent(
    candidates: &[CullCandidate],
    decisions: &mut HashMap<String, ItemCullDecision>,
) {
    let culled_parents: HashSet<String> = decisions
        .iter()
        .filter(|(_, d)| d.is_culled())
        .map(|(id, _)| id.clone())
        .collect();

    for c in candidates {
        let Some(ref parent_id) = c.parent_item_id else {
            continue;
        };
        if !culled_parents.contains(parent_id) {
            continue;
        }
        let entry = decisions
            .entry(c.id.clone())
            .or_insert_with(ItemCullDecision::included);
        if !entry
            .reasons
            .iter()
            .any(|r| r == reason::FAMILY_WITH_CULLED_PARENT)
        {
            entry.reasons.push(reason::FAMILY_WITH_CULLED_PARENT.into());
        }
        entry.status = item_cull_status::CULLED.into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::item_dedup_role;

    fn cand(id: &str, parent: Option<&str>, dedup: Option<&str>) -> CullCandidate {
        CullCandidate {
            id: id.into(),
            parent_item_id: parent.map(|s| s.into()),
            family_id: Some("fam1".into()),
            dedup_role: dedup.map(|s| s.into()),
            near_dup_role: None,
            sent_at: None,
            received_at: None,
            created_at: None,
            modified_at: None,
            path: Some(format!("{id}.bin")),
            custodian: None,
            file_category: None,
            mime_type: None,
            size_bytes: Some(10),
            status: "extracted".into(),
            native_sha256: None,
            text_sha256: None,
            role: None,
            imported_at: "2020-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn absolute_include_child_duplicate() {
        let parent = cand("p", None, Some(item_dedup_role::UNIQUE));
        let child = cand("c", Some("p"), Some(item_dedup_role::DUPLICATE));
        let candidates = vec![parent, child];
        let mut decisions = HashMap::new();
        decisions.insert("p".into(), ItemCullDecision::included());
        decisions.insert(
            "c".into(),
            ItemCullDecision::culled(vec![reason::EXACT_DUPLICATE.into()]),
        );
        apply_family_policy(
            &candidates,
            &mut decisions,
            FamilyPolicy::KeepChildrenWithIncludedParent,
        );
        let c = decisions.get("c").unwrap();
        assert!(!c.is_culled());
        assert!(c.reasons.is_empty());
    }
}
