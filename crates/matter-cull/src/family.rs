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
    // Nested chains (P→C→G): if C was culled at the item pass but P is
    // included, C is force-included; then G must also be force-included.
    // Iterate to fixpoint so deep trees converge in one family phase.
    loop {
        let included_ids: HashSet<String> = decisions
            .iter()
            .filter(|(_, d)| !d.is_culled())
            .map(|(id, _)| id.clone())
            .collect();

        let mut changed = false;
        for c in candidates {
            let Some(ref parent_id) = c.parent_item_id else {
                continue;
            };
            if !included_ids.contains(parent_id) {
                continue;
            }
            let already_included = decisions
                .get(&c.id)
                .map(|d| !d.is_culled())
                .unwrap_or(false);
            if already_included {
                // Still clear stale reasons if any (absolute include).
                if decisions
                    .get(&c.id)
                    .map(|d| !d.reasons.is_empty())
                    .unwrap_or(false)
                {
                    decisions.insert(c.id.clone(), ItemCullDecision::included());
                    changed = true;
                }
                continue;
            }
            // Absolute: force included, clear reasons (even exact_duplicate).
            decisions.insert(c.id.clone(), ItemCullDecision::included());
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

fn cull_children_with_parent(
    candidates: &[CullCandidate],
    decisions: &mut HashMap<String, ItemCullDecision>,
) {
    // Nested chains: culled grandparent → child → grandchild must all end culled.
    loop {
        let culled_parents: HashSet<String> = decisions
            .iter()
            .filter(|(_, d)| d.is_culled())
            .map(|(id, _)| id.clone())
            .collect();

        let mut changed = false;
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
            let was_culled = entry.is_culled()
                && entry
                    .reasons
                    .iter()
                    .any(|r| r == reason::FAMILY_WITH_CULLED_PARENT);
            if !entry
                .reasons
                .iter()
                .any(|r| r == reason::FAMILY_WITH_CULLED_PARENT)
            {
                entry.reasons.push(reason::FAMILY_WITH_CULLED_PARENT.into());
            }
            if entry.status != item_cull_status::CULLED {
                entry.status = item_cull_status::CULLED.into();
            }
            if !was_culled {
                changed = true;
            }
        }
        if !changed {
            break;
        }
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
            cull_status: None,
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

    /// Nested P→C→G: C force-included from P must also force-include G.
    #[test]
    fn nested_chain_keep_children_fixpoint() {
        let parent = cand("p", None, Some(item_dedup_role::UNIQUE));
        let child = cand("c", Some("p"), Some(item_dedup_role::DUPLICATE));
        let grand = cand("g", Some("c"), Some(item_dedup_role::DUPLICATE));
        let candidates = vec![parent, child, grand];
        let mut decisions = HashMap::new();
        decisions.insert("p".into(), ItemCullDecision::included());
        decisions.insert(
            "c".into(),
            ItemCullDecision::culled(vec![reason::EXACT_DUPLICATE.into()]),
        );
        decisions.insert(
            "g".into(),
            ItemCullDecision::culled(vec![reason::EXACT_DUPLICATE.into()]),
        );
        apply_family_policy(
            &candidates,
            &mut decisions,
            FamilyPolicy::KeepChildrenWithIncludedParent,
        );
        assert!(!decisions.get("c").unwrap().is_culled());
        assert!(decisions.get("c").unwrap().reasons.is_empty());
        assert!(
            !decisions.get("g").unwrap().is_culled(),
            "grandchild must be force-included once child is included (fixpoint)"
        );
        assert!(decisions.get("g").unwrap().reasons.is_empty());
    }

    /// Nested P→C→G under cull_children_with_parent: culled parent cascades.
    #[test]
    fn nested_chain_cull_children_fixpoint() {
        let parent = cand("p", None, Some(item_dedup_role::UNIQUE));
        let child = cand("c", Some("p"), Some(item_dedup_role::UNIQUE));
        let grand = cand("g", Some("c"), Some(item_dedup_role::UNIQUE));
        let candidates = vec![parent, child, grand];
        let mut decisions = HashMap::new();
        decisions.insert(
            "p".into(),
            ItemCullDecision::culled(vec![reason::DATE_OUT_OF_RANGE.into()]),
        );
        decisions.insert("c".into(), ItemCullDecision::included());
        decisions.insert("g".into(), ItemCullDecision::included());
        apply_family_policy(
            &candidates,
            &mut decisions,
            FamilyPolicy::CullChildrenWithParent,
        );
        assert!(decisions.get("c").unwrap().is_culled());
        assert!(decisions
            .get("c")
            .unwrap()
            .reasons
            .iter()
            .any(|r| r == reason::FAMILY_WITH_CULLED_PARENT));
        assert!(
            decisions.get("g").unwrap().is_culled(),
            "grandchild must be culled once child is culled (fixpoint)"
        );
        assert!(decisions
            .get("g")
            .unwrap()
            .reasons
            .iter()
            .any(|r| r == reason::FAMILY_WITH_CULLED_PARENT));
    }
}
