//! Built-in production QC rules and default pack.

use std::collections::{HashMap, HashSet};

use matter_core::{Item, Matter};

use crate::error::Result;
use crate::params::{QcRuleConfig, QcSeverity, PROFILE_DEFAULT_PRODUCTION_QC_V1};

// ---------------------------------------------------------------------------
// Rule ids
// ---------------------------------------------------------------------------

pub const RULE_BROKEN_FAMILY_ORPHAN_CHILD: &str = "broken_family_orphan_child";
pub const RULE_BROKEN_FAMILY_INCOMPLETE_PARENT: &str = "broken_family_incomplete_parent";
pub const RULE_WITHHELD_IN_SELECTION: &str = "withheld_in_selection";
pub const RULE_WITHHELD_FAMILY_MEMBER: &str = "withheld_family_member";
pub const RULE_REDACTED_TEXT_MISSING: &str = "redacted_text_missing";
pub const RULE_MISSING_NATIVE: &str = "missing_native";
pub const RULE_MISSING_TEXT: &str = "missing_text";
pub const RULE_PDF_NEEDS_OCR: &str = "pdf_needs_ocr";
pub const RULE_ZERO_SIZE: &str = "zero_size";
pub const RULE_ITEM_STATUS_ERROR: &str = "item_status_error";
pub const RULE_EMPTY_SELECTION: &str = "empty_selection";
pub const RULE_ONLY_WITHHELD: &str = "only_withheld";

/// One finding from a rule evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QcFinding {
    pub rule_id: String,
    pub severity: QcSeverity,
    pub item_id: Option<String>,
    /// Short stable phrase only — never subject/body/paths.
    pub message: String,
}

/// Resolved severity map for evaluation (Off entries still present for lookup).
#[derive(Debug, Clone)]
pub struct ResolvedRules {
    pub profile: String,
    by_id: HashMap<String, QcSeverity>,
}

impl ResolvedRules {
    pub fn severity(&self, rule_id: &str) -> QcSeverity {
        self.by_id.get(rule_id).copied().unwrap_or(QcSeverity::Off)
    }

    pub fn is_enabled(&self, rule_id: &str) -> bool {
        self.severity(rule_id) != QcSeverity::Off
    }

    pub fn to_configs(&self) -> Vec<QcRuleConfig> {
        let mut out: Vec<_> = self
            .by_id
            .iter()
            .map(|(id, sev)| QcRuleConfig {
                id: id.clone(),
                severity: *sev,
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
}

/// Default pack `default_production_qc_v1`.
pub fn default_rule_pack() -> Vec<QcRuleConfig> {
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
        // Base severity unused for taxonomy path; Off still disables.
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

/// Merge operator overrides over the default pack.
///
/// Unknown rule ids in overrides are accepted (forward-compatible) but ignored
/// by the evaluator if no matching rule exists.
pub fn resolve_rules(overrides: &[QcRuleConfig]) -> ResolvedRules {
    let mut by_id: HashMap<String, QcSeverity> = default_rule_pack()
        .into_iter()
        .map(|r| (r.id, r.severity))
        .collect();
    for r in overrides {
        by_id.insert(r.id.clone(), r.severity);
    }
    ResolvedRules {
        profile: PROFILE_DEFAULT_PRODUCTION_QC_V1.into(),
        by_id,
    }
}

/// Categories where missing text is an **error** (case-insensitive).
fn missing_text_is_error_category(cat: Option<&str>) -> bool {
    matches!(
        cat.map(|c| c.to_ascii_lowercase()).as_deref(),
        Some("email") | Some("document") | Some("spreadsheet") | Some("presentation") | Some("pdf")
    )
}

/// Whether item can use export-only EML (produce-like).
pub fn is_email_like(item: &Item) -> bool {
    let cat = item
        .file_category
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if cat == "email" || cat == "message" || cat == "mail" {
        return true;
    }
    let mime = item.mime_type.as_deref().unwrap_or("").to_ascii_lowercase();
    mime.starts_with("message/")
        || mime.contains("outlook")
        || item.message_id.is_some()
        || item.from_addr.is_some()
}

fn digest_present(d: Option<&str>) -> bool {
    d.map(|s| !s.trim().is_empty()).unwrap_or(false)
}

fn usable_text(item: &Item) -> bool {
    digest_present(item.text_sha256.as_deref())
        || digest_present(item.redacted_text_sha256.as_deref())
}

/// Evaluate all enabled rules against the candidate set.
pub fn evaluate_candidates(
    matter: &Matter,
    candidate_ids: &[String],
    rules: &ResolvedRules,
) -> Result<Vec<QcFinding>> {
    let mut findings = Vec::new();
    let candidate_set: HashSet<&str> = candidate_ids.iter().map(String::as_str).collect();

    // Set-level: empty selection
    if rules.is_enabled(RULE_EMPTY_SELECTION) && candidate_ids.is_empty() {
        findings.push(QcFinding {
            rule_id: RULE_EMPTY_SELECTION.into(),
            severity: rules.severity(RULE_EMPTY_SELECTION),
            item_id: None,
            message: "empty selection".into(),
        });
        return Ok(findings);
    }

    // Preload items + withheld flags
    let mut items: Vec<Item> = Vec::with_capacity(candidate_ids.len());
    let mut withheld_map: HashMap<String, bool> = HashMap::new();
    for id in candidate_ids {
        let item = matter.get_item(id)?;
        let withheld = matter.item_is_withheld(id)?;
        withheld_map.insert(id.clone(), withheld);
        items.push(item);
    }

    // Set-level: only withheld
    if rules.is_enabled(RULE_ONLY_WITHHELD)
        && !candidate_ids.is_empty()
        && candidate_ids
            .iter()
            .all(|id| *withheld_map.get(id).unwrap_or(&false))
    {
        findings.push(QcFinding {
            rule_id: RULE_ONLY_WITHHELD.into(),
            severity: rules.severity(RULE_ONLY_WITHHELD),
            item_id: None,
            message: "all candidates withheld".into(),
        });
    }

    for item in &items {
        let id = item.id.as_str();
        let is_withheld = *withheld_map.get(id).unwrap_or(&false);

        // orphan child
        if rules.is_enabled(RULE_BROKEN_FAMILY_ORPHAN_CHILD) {
            if let Some(parent) = item.parent_item_id.as_deref() {
                if !candidate_set.contains(parent) {
                    findings.push(QcFinding {
                        rule_id: RULE_BROKEN_FAMILY_ORPHAN_CHILD.into(),
                        severity: rules.severity(RULE_BROKEN_FAMILY_ORPHAN_CHILD),
                        item_id: Some(id.into()),
                        message: "orphan child: parent not in selection".into(),
                    });
                }
            }
        }

        // incomplete parent: any non-withheld child not in set
        if rules.is_enabled(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT)
            && has_missing_non_withheld_child(matter, id, &candidate_set)?
        {
            findings.push(QcFinding {
                rule_id: RULE_BROKEN_FAMILY_INCOMPLETE_PARENT.into(),
                severity: rules.severity(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT),
                item_id: Some(id.into()),
                message: "incomplete family: non-withheld child missing from selection".into(),
            });
        }

        // withheld in selection
        if rules.is_enabled(RULE_WITHHELD_IN_SELECTION) && is_withheld {
            findings.push(QcFinding {
                rule_id: RULE_WITHHELD_IN_SELECTION.into(),
                severity: rules.severity(RULE_WITHHELD_IN_SELECTION),
                item_id: Some(id.into()),
                message: "withheld item in selection".into(),
            });
        }

        // withheld family member (candidate not withheld, parent or child is)
        if rules.is_enabled(RULE_WITHHELD_FAMILY_MEMBER)
            && !is_withheld
            && family_has_withheld_relative(matter, item)?
        {
            findings.push(QcFinding {
                rule_id: RULE_WITHHELD_FAMILY_MEMBER.into(),
                severity: rules.severity(RULE_WITHHELD_FAMILY_MEMBER),
                item_id: Some(id.into()),
                message: "family member withheld".into(),
            });
        }

        // redacted text missing
        if rules.is_enabled(RULE_REDACTED_TEXT_MISSING)
            && item.redaction_count > 0
            && !digest_present(item.redacted_text_sha256.as_deref())
        {
            findings.push(QcFinding {
                rule_id: RULE_REDACTED_TEXT_MISSING.into(),
                severity: rules.severity(RULE_REDACTED_TEXT_MISSING),
                item_id: Some(id.into()),
                message: "redaction without redacted text artifact".into(),
            });
        }

        // missing native (non-email)
        if rules.is_enabled(RULE_MISSING_NATIVE)
            && !digest_present(item.native_sha256.as_deref())
            && !is_email_like(item)
        {
            findings.push(QcFinding {
                rule_id: RULE_MISSING_NATIVE.into(),
                severity: rules.severity(RULE_MISSING_NATIVE),
                item_id: Some(id.into()),
                message: "missing native for non-email item".into(),
            });
        }

        // missing text (taxonomy-aware)
        if rules.is_enabled(RULE_MISSING_TEXT) && !usable_text(item) {
            let configured = rules.severity(RULE_MISSING_TEXT);
            // Off already filtered by is_enabled.
            // If configured Error → force error; if Warn → use taxonomy;
            // if somehow other, use taxonomy under warn floor.
            let taxonomy = if missing_text_is_error_category(item.file_category.as_deref()) {
                QcSeverity::Error
            } else {
                QcSeverity::Warn
            };
            let severity = match configured {
                QcSeverity::Off => continue,
                QcSeverity::Error => QcSeverity::Error,
                QcSeverity::Warn => taxonomy,
            };
            findings.push(QcFinding {
                rule_id: RULE_MISSING_TEXT.into(),
                severity,
                item_id: Some(id.into()),
                message: "missing usable text".into(),
            });
        }

        // pdf needs ocr
        if rules.is_enabled(RULE_PDF_NEEDS_OCR) && item.pdf_needs_ocr == 1 {
            findings.push(QcFinding {
                rule_id: RULE_PDF_NEEDS_OCR.into(),
                severity: rules.severity(RULE_PDF_NEEDS_OCR),
                item_id: Some(id.into()),
                message: "pdf needs ocr".into(),
            });
        }

        // zero size
        if rules.is_enabled(RULE_ZERO_SIZE) {
            if let Some(sz) = item.size_bytes {
                if sz == 0 {
                    findings.push(QcFinding {
                        rule_id: RULE_ZERO_SIZE.into(),
                        severity: rules.severity(RULE_ZERO_SIZE),
                        item_id: Some(id.into()),
                        message: "zero size_bytes".into(),
                    });
                }
            }
        }

        // item status error/partial
        if rules.is_enabled(RULE_ITEM_STATUS_ERROR) {
            let st = item.status.to_ascii_lowercase();
            if st == "error" || st == "partial" {
                findings.push(QcFinding {
                    rule_id: RULE_ITEM_STATUS_ERROR.into(),
                    severity: rules.severity(RULE_ITEM_STATUS_ERROR),
                    item_id: Some(id.into()),
                    message: format!("item status {st}"),
                });
            }
        }
    }

    Ok(findings)
}

fn has_missing_non_withheld_child(
    matter: &Matter,
    parent_id: &str,
    candidate_set: &HashSet<&str>,
) -> Result<bool> {
    let mut stmt = matter
        .connection()
        .prepare("SELECT id FROM items WHERE matter_id = ?1 AND parent_item_id = ?2")?;
    let rows = stmt.query_map(rusqlite::params![matter.id(), parent_id], |row| {
        row.get::<_, String>(0)
    })?;
    for r in rows {
        let child_id = r?;
        if candidate_set.contains(child_id.as_str()) {
            continue;
        }
        if matter.item_is_withheld(&child_id)? {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

fn family_has_withheld_relative(matter: &Matter, item: &Item) -> Result<bool> {
    if let Some(parent) = item.parent_item_id.as_deref() {
        if matter.item_is_withheld(parent)? {
            return Ok(true);
        }
    }
    let mut stmt = matter
        .connection()
        .prepare("SELECT id FROM items WHERE matter_id = ?1 AND parent_item_id = ?2")?;
    let rows = stmt.query_map(rusqlite::params![matter.id(), item.id], |row| {
        row.get::<_, String>(0)
    })?;
    for r in rows {
        let child_id = r?;
        if matter.item_is_withheld(&child_id)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pack_has_orphan_error() {
        let r = resolve_rules(&[]);
        assert_eq!(
            r.severity(RULE_BROKEN_FAMILY_ORPHAN_CHILD),
            QcSeverity::Error
        );
        assert_eq!(
            r.severity(RULE_BROKEN_FAMILY_INCOMPLETE_PARENT),
            QcSeverity::Warn
        );
    }

    #[test]
    fn override_off_disables() {
        let r = resolve_rules(&[QcRuleConfig {
            id: RULE_ZERO_SIZE.into(),
            severity: QcSeverity::Off,
        }]);
        assert!(!r.is_enabled(RULE_ZERO_SIZE));
    }

    #[test]
    fn missing_text_error_categories() {
        assert!(missing_text_is_error_category(Some("email")));
        assert!(missing_text_is_error_category(Some("PDF")));
        assert!(missing_text_is_error_category(Some("document")));
        assert!(!missing_text_is_error_category(Some("image")));
        assert!(!missing_text_is_error_category(None));
    }
}
