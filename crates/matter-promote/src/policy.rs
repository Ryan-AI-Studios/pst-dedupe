//! Promote selection policies.

use matter_core::{item_cull_status, item_dedup_role, item_status, Matter, PromoteCandidate};

use crate::error::{PromoteError, Result};

/// Resolve `auto` → cull_included or unique_only based on matter state.
pub const POLICY_AUTO: &str = "auto";
/// `cull_status = 'included'`.
pub const POLICY_CULL_INCLUDED: &str = "cull_included";
/// Unique / skipped (or all extracted when never deduped).
pub const POLICY_UNIQUE_ONLY: &str = "unique_only";
/// unique_only base + family expand (expand is usually param-driven).
pub const POLICY_UNIQUE_PLUS_FAMILY: &str = "unique_plus_family";
/// All extracted-like statuses.
pub const POLICY_ALL_EXTRACTED: &str = "all_extracted";
/// cull included + family expand.
pub const POLICY_CULL_INCLUDED_PLUS_FAMILY: &str = "cull_included_plus_family";

/// All accepted policy id strings (including `auto`).
pub const ALL_POLICY_IDS: &[&str] = &[
    POLICY_AUTO,
    POLICY_CULL_INCLUDED,
    POLICY_UNIQUE_ONLY,
    POLICY_UNIQUE_PLUS_FAMILY,
    POLICY_ALL_EXTRACTED,
    POLICY_CULL_INCLUDED_PLUS_FAMILY,
];

/// True when `id` is a known policy string.
pub fn policy_id_valid(id: &str) -> bool {
    ALL_POLICY_IDS.contains(&id)
}

/// True when the policy id itself implies family expand (in addition to param).
pub fn policy_implies_expand(resolved: &str) -> bool {
    matches!(
        resolved,
        POLICY_UNIQUE_PLUS_FAMILY | POLICY_CULL_INCLUDED_PLUS_FAMILY
    )
}

/// Resolve `auto` against matter state. Named policies pass through.
pub fn resolve_policy(matter: &Matter, requested: &str) -> Result<String> {
    if requested != POLICY_AUTO {
        if !policy_id_valid(requested) {
            return Err(PromoteError::InvalidParams(format!(
                "unknown promote policy '{requested}'"
            )));
        }
        return Ok(requested.to_string());
    }
    if matter.cull_has_run()? {
        Ok(POLICY_CULL_INCLUDED.to_string())
    } else {
        Ok(POLICY_UNIQUE_ONLY.to_string())
    }
}

/// Extracted-like statuses eligible for promote policies.
pub fn is_extracted_like(status: &str) -> bool {
    matches!(
        status,
        item_status::EXTRACTED | item_status::PARTIAL | item_status::NORMALIZED
    )
}

/// Select base membership ids for a **resolved** (non-auto) policy.
///
/// Does **not** apply family expand — caller handles expand separately.
pub fn select_base_ids(
    matter: &Matter,
    resolved_policy: &str,
    require_dedupe: bool,
) -> Result<Vec<String>> {
    let candidates = matter.list_promote_candidates()?;
    select_base_ids_from_candidates(matter, &candidates, resolved_policy, require_dedupe)
}

/// Pure selection over preloaded thin candidates (testable without re-query).
pub fn select_base_ids_from_candidates(
    matter: &Matter,
    candidates: &[PromoteCandidate],
    resolved_policy: &str,
    require_dedupe: bool,
) -> Result<Vec<String>> {
    let base_policy = match resolved_policy {
        POLICY_CULL_INCLUDED | POLICY_CULL_INCLUDED_PLUS_FAMILY => POLICY_CULL_INCLUDED,
        POLICY_UNIQUE_ONLY | POLICY_UNIQUE_PLUS_FAMILY => POLICY_UNIQUE_ONLY,
        POLICY_ALL_EXTRACTED => POLICY_ALL_EXTRACTED,
        other => {
            return Err(PromoteError::InvalidParams(format!(
                "cannot select base set for unresolved/unknown policy '{other}'"
            )));
        }
    };

    match base_policy {
        POLICY_CULL_INCLUDED => Ok(candidates
            .iter()
            .filter(|c| c.cull_status.as_deref() == Some(item_cull_status::INCLUDED))
            .map(|c| c.id.clone())
            .collect()),
        POLICY_ALL_EXTRACTED => Ok(candidates
            .iter()
            .filter(|c| is_extracted_like(&c.status))
            .map(|c| c.id.clone())
            .collect()),
        POLICY_UNIQUE_ONLY => {
            let any_dedup = candidates.iter().any(|c| c.dedup_role.is_some());
            if !any_dedup {
                if require_dedupe {
                    return Err(PromoteError::InvalidParams(
                        "require_dedupe=true but no item has dedup_role; run exact dedupe first"
                            .into(),
                    ));
                }
                // P0: never deduped → treat all extracted-like as eligible.
                return Ok(candidates
                    .iter()
                    .filter(|c| is_extracted_like(&c.status))
                    .map(|c| c.id.clone())
                    .collect());
            }
            // Confirm against live DB for require_dedupe (already true path above).
            let _ = matter;
            Ok(candidates
                .iter()
                .filter(|c| {
                    if !is_extracted_like(&c.status) {
                        return false;
                    }
                    match c.dedup_role.as_deref() {
                        Some(item_dedup_role::UNIQUE) | Some(item_dedup_role::SKIPPED) => true,
                        Some(_) => false,
                        None => true, // NULL role after partial dedupe: eligible
                    }
                })
                .map(|c| c.id.clone())
                .collect())
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracted_like_gate() {
        assert!(is_extracted_like(item_status::EXTRACTED));
        assert!(is_extracted_like(item_status::PARTIAL));
        assert!(is_extracted_like(item_status::NORMALIZED));
        assert!(!is_extracted_like(item_status::DISCOVERED));
    }
}
