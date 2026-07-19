//! Collection gap: expected custodians vs matter inventory.

use matter_core::{CustodianInventoryRow, ExpectedCustodian, Matter};
use serde::{Deserialize, Serialize};

use crate::date_coverage::GapSeverity;
use crate::error::Result;

pub const FINDING_MISSING_CUSTODIAN: &str = "missing_custodian";
pub const FINDING_UNEXPECTED_CUSTODIAN: &str = "unexpected_custodian";

/// One collection-gap finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterFinding {
    pub finding_id: String,
    pub severity: GapSeverity,
    pub custodian: String,
    pub name_norm: String,
    pub message: String,
    pub item_count: u64,
}

/// Inventory + missing/unexpected findings.
#[derive(Debug, Clone, Default)]
pub struct CollectionGapAnalysis {
    pub inventory: Vec<CustodianInventoryRow>,
    pub findings: Vec<RosterFinding>,
    pub missing: Vec<RosterFinding>,
    pub unexpected: Vec<RosterFinding>,
}

/// Analyze expected custodians vs present inventory.
///
/// `missing_custodian` severity is **always warn** (locked).
pub fn analyze_collection_roster(
    expected: &[ExpectedCustodian],
    inventory: &[CustodianInventoryRow],
    flag_unexpected: bool,
) -> CollectionGapAnalysis {
    let present: std::collections::HashMap<&str, u64> = inventory
        .iter()
        .filter(|r| !r.name_norm.is_empty())
        .map(|r| (r.name_norm.as_str(), r.item_count))
        .collect();

    let mut out = CollectionGapAnalysis {
        inventory: inventory.to_vec(),
        ..Default::default()
    };

    let mut expected_norms = std::collections::HashSet::new();
    for e in expected.iter().filter(|e| e.active) {
        expected_norms.insert(e.name_norm.clone());
        let count = present.get(e.name_norm.as_str()).copied().unwrap_or(0);
        if count == 0 {
            let f = RosterFinding {
                finding_id: FINDING_MISSING_CUSTODIAN.into(),
                severity: GapSeverity::Warn, // locked
                custodian: e.display_name.clone(),
                name_norm: e.name_norm.clone(),
                message: format!(
                    "expected custodian '{}' has zero matching items",
                    e.display_name
                ),
                item_count: 0,
            };
            out.missing.push(f.clone());
            out.findings.push(f);
        }
    }

    if flag_unexpected {
        for row in inventory {
            if row.name_norm.is_empty() {
                continue;
            }
            if !expected_norms.contains(&row.name_norm) {
                let f = RosterFinding {
                    finding_id: FINDING_UNEXPECTED_CUSTODIAN.into(),
                    severity: GapSeverity::Warn,
                    custodian: row.custodian.clone(),
                    name_norm: row.name_norm.clone(),
                    message: format!(
                        "matter custodian '{}' is not on expected roster ({} items)",
                        row.custodian, row.item_count
                    ),
                    item_count: row.item_count,
                };
                out.unexpected.push(f.clone());
                out.findings.push(f);
            }
        }
    }

    out
}

/// Load expected + inventory from matter and analyze.
pub fn run_roster_analysis(
    matter: &Matter,
    flag_unexpected: bool,
) -> Result<CollectionGapAnalysis> {
    let expected = matter.list_expected_custodians(true)?;
    let inventory = matter.custodian_inventory()?;
    Ok(analyze_collection_roster(
        &expected,
        &inventory,
        flag_unexpected,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::normalize_custodian_name;

    fn exp(display: &str) -> ExpectedCustodian {
        ExpectedCustodian {
            id: "x".into(),
            matter_id: "m".into(),
            name_norm: normalize_custodian_name(display),
            display_name: display.into(),
            notes: None,
            active: true,
            created_at: "t".into(),
        }
    }

    fn inv(name: &str, n: u64) -> CustodianInventoryRow {
        CustodianInventoryRow {
            custodian: name.into(),
            name_norm: normalize_custodian_name(name),
            item_count: n,
        }
    }

    #[test]
    fn missing_is_warn() {
        let a = analyze_collection_roster(&[exp("Alice")], &[], true);
        assert_eq!(a.missing.len(), 1);
        assert_eq!(a.missing[0].severity, GapSeverity::Warn);
        assert_eq!(a.missing[0].finding_id, FINDING_MISSING_CUSTODIAN);
    }

    #[test]
    fn present_not_missing() {
        let a = analyze_collection_roster(&[exp("Alice")], &[inv("alice", 3)], true);
        assert!(a.missing.is_empty());
    }

    #[test]
    fn unexpected_when_enabled() {
        let a = analyze_collection_roster(&[exp("Alice")], &[inv("Bob", 1)], true);
        assert_eq!(a.unexpected.len(), 1);
        assert_eq!(a.unexpected[0].finding_id, FINDING_UNEXPECTED_CUSTODIAN);
    }
}
