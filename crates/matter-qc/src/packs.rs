//! Named QC severity packs (track **0060**).
//!
//! Packs are pure severity tables over the built-in rule ids. Profiles bind a
//! `pack_id`; operator rule overrides still apply after the pack is resolved.

use std::collections::HashMap;

use matter_core::{
    normalize_qc_pack_id, QC_PACK_DEFAULT_V1, QC_PACK_LEGACY_DEFAULT, QC_PACK_NATIVE_HEAVY_V1,
    QC_PACK_STRICT_PRIVILEGE_V1,
};

use crate::params::{QcRuleConfig, QcSeverity, PROFILE_DEFAULT_PRODUCTION_QC_V1};
use crate::rules::{
    RULE_BROKEN_FAMILY_INCOMPLETE_PARENT, RULE_BROKEN_FAMILY_ORPHAN_CHILD, RULE_EMPTY_SELECTION,
    RULE_ITEM_STATUS_ERROR, RULE_MISSING_NATIVE, RULE_MISSING_TEXT, RULE_ONLY_WITHHELD,
    RULE_PDF_NEEDS_OCR, RULE_REDACTED_TEXT_MISSING, RULE_WITHHELD_FAMILY_MEMBER,
    RULE_WITHHELD_IN_SELECTION, RULE_ZERO_SIZE,
};

/// Re-export pack id constants for callers that depend on matter-qc only.
pub use matter_core::{
    QC_PACK_DEFAULT_V1 as PACK_DEFAULT_V1, QC_PACK_LEGACY_DEFAULT as PACK_LEGACY_DEFAULT,
    QC_PACK_NATIVE_HEAVY_V1 as PACK_NATIVE_HEAVY_V1,
    QC_PACK_STRICT_PRIVILEGE_V1 as PACK_STRICT_PRIVILEGE_V1,
};

/// Default pack `qc_default_v1` — same severities as 0041 `default_production_qc_v1`.
pub fn pack_default_v1() -> Vec<QcRuleConfig> {
    vec![
        QcRuleConfig {
            id: RULE_BROKEN_FAMILY_ORPHAN_CHILD.into(),
            severity: QcSeverity::Error,
        },
        QcRuleConfig {
            id: RULE_BROKEN_FAMILY_INCOMPLETE_PARENT.into(),
            severity: QcSeverity::Warn,
        },
        QcRuleConfig {
            id: RULE_WITHHELD_IN_SELECTION.into(),
            severity: QcSeverity::Error,
        },
        QcRuleConfig {
            id: RULE_WITHHELD_FAMILY_MEMBER.into(),
            severity: QcSeverity::Warn,
        },
        QcRuleConfig {
            id: RULE_REDACTED_TEXT_MISSING.into(),
            severity: QcSeverity::Error,
        },
        QcRuleConfig {
            id: RULE_MISSING_NATIVE.into(),
            severity: QcSeverity::Error,
        },
        QcRuleConfig {
            id: RULE_MISSING_TEXT.into(),
            severity: QcSeverity::Warn,
        },
        QcRuleConfig {
            id: RULE_PDF_NEEDS_OCR.into(),
            severity: QcSeverity::Warn,
        },
        QcRuleConfig {
            id: RULE_ZERO_SIZE.into(),
            severity: QcSeverity::Warn,
        },
        QcRuleConfig {
            id: RULE_ITEM_STATUS_ERROR.into(),
            severity: QcSeverity::Warn,
        },
        QcRuleConfig {
            id: RULE_EMPTY_SELECTION.into(),
            severity: QcSeverity::Error,
        },
        QcRuleConfig {
            id: RULE_ONLY_WITHHELD.into(),
            severity: QcSeverity::Error,
        },
    ]
}

/// Strict privilege pack: withheld family + incomplete family escalate to Error.
pub fn pack_strict_privilege_v1() -> Vec<QcRuleConfig> {
    let mut rules = pack_default_v1();
    for r in &mut rules {
        match r.id.as_str() {
            RULE_WITHHELD_IN_SELECTION
            | RULE_WITHHELD_FAMILY_MEMBER
            | RULE_BROKEN_FAMILY_INCOMPLETE_PARENT => {
                r.severity = QcSeverity::Error;
            }
            _ => {}
        }
    }
    rules
}

/// Native-heavy pack: missing native + zero size as Error; missing text Off for
/// non-document taxonomy is handled at evaluation time (base severity Warn stays
/// taxonomy-aware; Error would force all missing text to Error).
pub fn pack_native_heavy_v1() -> Vec<QcRuleConfig> {
    let mut rules = pack_default_v1();
    for r in &mut rules {
        match r.id.as_str() {
            RULE_MISSING_NATIVE | RULE_ZERO_SIZE => {
                r.severity = QcSeverity::Error;
            }
            // Soften missing_text: keep Warn so taxonomy path still allows Warn
            // for image/binary; document categories still escalate via taxonomy.
            RULE_MISSING_TEXT => {
                r.severity = QcSeverity::Warn;
            }
            _ => {}
        }
    }
    rules
}

/// Whether `pack_id` is a known built-in (or legacy alias).
pub fn is_known_pack_id(pack_id: &str) -> bool {
    let n = normalize_qc_pack_id(pack_id);
    matches!(
        n.as_str(),
        QC_PACK_DEFAULT_V1 | QC_PACK_STRICT_PRIVILEGE_V1 | QC_PACK_NATIVE_HEAVY_V1
    ) || pack_id.trim() == QC_PACK_LEGACY_DEFAULT
        || pack_id.trim() == PROFILE_DEFAULT_PRODUCTION_QC_V1
}

/// Resolve a pack id to its rule severity table.
///
/// Accepts legacy `default_production_qc_v1` as an alias of `qc_default_v1`.
/// Unknown packs return `None` — callers must fail closed (no silent default).
pub fn pack_rules_checked(pack_id: &str) -> Option<Vec<QcRuleConfig>> {
    if !is_known_pack_id(pack_id) {
        return None;
    }
    let normalized = normalize_qc_pack_id(pack_id);
    Some(match normalized.as_str() {
        QC_PACK_STRICT_PRIVILEGE_V1 => pack_strict_privilege_v1(),
        QC_PACK_NATIVE_HEAVY_V1 => pack_native_heavy_v1(),
        _ => pack_default_v1(),
    })
}

/// Resolve a pack id to its rule severity table.
///
/// Unknown packs fall through to empty after validation should have rejected
/// them; prefer [`pack_rules_checked`] / [`QcParams::validate_shape`].
pub fn pack_rules(pack_id: &str) -> Vec<QcRuleConfig> {
    pack_rules_checked(pack_id).unwrap_or_default()
}

/// Canonical pack id stored on `qc_runs.profile` / fingerprints.
pub fn canonical_pack_id(pack_or_profile: &str) -> String {
    let n = normalize_qc_pack_id(pack_or_profile);
    // Also accept legacy PROFILE_DEFAULT_PRODUCTION_QC_V1 constant.
    if n == PROFILE_DEFAULT_PRODUCTION_QC_V1 || n == QC_PACK_LEGACY_DEFAULT {
        return QC_PACK_DEFAULT_V1.into();
    }
    n
}

/// Known built-in pack ids (including legacy alias).
pub fn list_pack_ids() -> Vec<&'static str> {
    vec![
        QC_PACK_DEFAULT_V1,
        QC_PACK_STRICT_PRIVILEGE_V1,
        QC_PACK_NATIVE_HEAVY_V1,
        QC_PACK_LEGACY_DEFAULT,
    ]
}

/// Merge pack base + operator overrides into a severity map.
pub fn merge_pack_with_overrides(
    pack_id: &str,
    overrides: &[QcRuleConfig],
) -> HashMap<String, QcSeverity> {
    let mut by_id: HashMap<String, QcSeverity> = pack_rules(pack_id)
        .into_iter()
        .map(|r| (r.id, r.severity))
        .collect();
    for r in overrides {
        by_id.insert(r.id.clone(), r.severity);
    }
    by_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_escalates_family_rules() {
        let pack = pack_strict_privilege_v1();
        let get = |id: &str| {
            pack.iter()
                .find(|r| r.id == id)
                .map(|r| r.severity)
                .expect("rule")
        };
        assert_eq!(get(RULE_WITHHELD_FAMILY_MEMBER), QcSeverity::Error);
        assert_eq!(get(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT), QcSeverity::Error);
        // Default pack keeps these as Warn.
        let def = pack_default_v1();
        let get_d = |id: &str| {
            def.iter()
                .find(|r| r.id == id)
                .map(|r| r.severity)
                .expect("rule")
        };
        assert_eq!(get_d(RULE_WITHHELD_FAMILY_MEMBER), QcSeverity::Warn);
        assert_eq!(
            get_d(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT),
            QcSeverity::Warn
        );
    }

    #[test]
    fn legacy_alias_maps_to_default() {
        assert_eq!(
            canonical_pack_id("default_production_qc_v1"),
            QC_PACK_DEFAULT_V1
        );
        assert_eq!(canonical_pack_id(QC_PACK_DEFAULT_V1), QC_PACK_DEFAULT_V1);
    }

    #[test]
    fn native_heavy_zero_size_error() {
        let pack = pack_native_heavy_v1();
        let z = pack.iter().find(|r| r.id == RULE_ZERO_SIZE).expect("zero");
        assert_eq!(z.severity, QcSeverity::Error);
    }
}
