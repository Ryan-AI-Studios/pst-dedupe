//! Per-item cull rule evaluation.

use matter_core::{item_cull_status, item_dedup_role, item_near_dup_role, CullCandidate};

use crate::denist::{matches_denist, DenistList};
use crate::rules::{parse_item_instant, reason, CullRules, DateField, ListMode, MissingDatePolicy};

/// Result of evaluating rules against one item (before family pass).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemCullDecision {
    pub status: String,
    pub reasons: Vec<String>,
}

impl ItemCullDecision {
    pub fn included() -> Self {
        Self {
            status: item_cull_status::INCLUDED.into(),
            reasons: Vec::new(),
        }
    }

    pub fn culled(reasons: Vec<String>) -> Self {
        Self {
            status: item_cull_status::CULLED.into(),
            reasons,
        }
    }

    pub fn is_culled(&self) -> bool {
        self.status == item_cull_status::CULLED
    }
}

/// Evaluate all enabled cull conditions; collect **all** matching reasons.
pub fn evaluate_item(
    item: &CullCandidate,
    rules: &CullRules,
    denist: Option<&DenistList>,
) -> ItemCullDecision {
    let mut reasons: Vec<String> = Vec::new();

    // Status gate: items not in include list are culled.
    if rules.statuses.enabled {
        let ok = rules
            .statuses
            .include
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&item.status));
        if !ok {
            reasons.push(reason::STATUS.into());
        }
    }

    // Exact duplicate.
    if rules.exclude_exact_duplicates
        && item.dedup_role.as_deref() == Some(item_dedup_role::DUPLICATE)
    {
        reasons.push(reason::EXACT_DUPLICATE.into());
    }

    // Date window.
    if rules.date.enabled {
        eval_date(item, rules, &mut reasons);
    }

    // Custodians.
    if rules.custodians.enabled {
        eval_string_list(
            item.custodian.as_deref(),
            &rules.custodians.values,
            rules.custodians.mode,
            reason::CUSTODIAN,
            &mut reasons,
        );
    }

    // Path contains.
    if rules.path_contains.enabled {
        eval_path(item, rules, &mut reasons);
    }

    // File categories.
    if rules.file_categories.enabled {
        eval_string_list(
            item.file_category.as_deref(),
            &rules.file_categories.values,
            rules.file_categories.mode,
            reason::FILE_CATEGORY,
            &mut reasons,
        );
    }

    // MIME prefixes.
    if rules.mime_prefixes.enabled {
        eval_mime(item, rules, &mut reasons);
    }

    // Size.
    if rules.size_bytes.enabled {
        eval_size(item, rules, &mut reasons);
    }

    // Empty / noise.
    if rules.empty.enabled {
        eval_empty(item, rules, &mut reasons);
    }

    // Near-dup (off by default).
    if rules.near_dup.enabled {
        eval_near_dup(item, rules, &mut reasons);
    }

    // DeNIST.
    if rules.denist.enabled {
        if let Some(list) = denist {
            if matches_denist(list, item.native_sha256.as_deref()) {
                reasons.push(reason::DENIST.into());
            }
        }
    }

    if reasons.is_empty() {
        ItemCullDecision::included()
    } else {
        ItemCullDecision::culled(reasons)
    }
}

fn eval_date(item: &CullCandidate, rules: &CullRules, reasons: &mut Vec<String>) {
    let raw = pick_date_field(item, rules.date.field);
    let Some(raw) = raw else {
        match rules.date.missing_policy {
            MissingDatePolicy::Include => {}
            MissingDatePolicy::Cull => reasons.push(reason::DATE_MISSING.into()),
        }
        return;
    };
    let Some(instant) = parse_item_instant(raw) else {
        match rules.date.missing_policy {
            MissingDatePolicy::Include => {}
            MissingDatePolicy::Cull => reasons.push(reason::DATE_MISSING.into()),
        }
        return;
    };

    // start inclusive, end exclusive.
    if let Some(ref start_s) = rules.date.start {
        if let Ok(start) = crate::rules::parse_bound_instant(start_s) {
            if instant < start {
                reasons.push(reason::DATE_OUT_OF_RANGE.into());
                return;
            }
        }
    }
    if let Some(ref end_s) = rules.date.end {
        if let Ok(end) = crate::rules::parse_bound_instant(end_s) {
            if instant >= end {
                reasons.push(reason::DATE_OUT_OF_RANGE.into());
            }
        }
    }
}

fn pick_date_field(item: &CullCandidate, field: DateField) -> Option<&str> {
    match field {
        DateField::SentAt => item.sent_at.as_deref(),
        DateField::ReceivedAt => item.received_at.as_deref(),
        DateField::CreatedAt => item.created_at.as_deref(),
        DateField::BestEffort => item
            .sent_at
            .as_deref()
            .or(item.received_at.as_deref())
            .or(item.created_at.as_deref())
            .or(item.modified_at.as_deref()),
    }
}

fn eval_string_list(
    value: Option<&str>,
    values: &[String],
    mode: ListMode,
    reason_code: &str,
    reasons: &mut Vec<String>,
) {
    let present = value.unwrap_or("");
    let in_list = values.iter().any(|v| v.eq_ignore_ascii_case(present));
    let cull = match mode {
        ListMode::Include => !in_list,
        ListMode::Exclude => in_list,
    };
    if cull {
        reasons.push(reason_code.into());
    }
}

fn eval_path(item: &CullCandidate, rules: &CullRules, reasons: &mut Vec<String>) {
    let path = item.path.as_deref().unwrap_or("");
    let path_lower = path.to_ascii_lowercase();
    let matched = rules.path_contains.patterns.iter().any(|p| {
        let pl = p.to_ascii_lowercase();
        path_lower.contains(&pl)
    });
    let cull = match rules.path_contains.mode {
        ListMode::Exclude => matched,
        ListMode::Include => !matched,
    };
    if cull {
        reasons.push(reason::PATH.into());
    }
}

fn eval_mime(item: &CullCandidate, rules: &CullRules, reasons: &mut Vec<String>) {
    let mime = item.mime_type.as_deref().unwrap_or("").to_ascii_lowercase();
    let matched = rules
        .mime_prefixes
        .values
        .iter()
        .any(|prefix| mime.starts_with(&prefix.to_ascii_lowercase()));
    let cull = match rules.mime_prefixes.mode {
        ListMode::Exclude => matched,
        ListMode::Include => !matched,
    };
    if cull {
        reasons.push(reason::MIME.into());
    }
}

fn eval_size(item: &CullCandidate, rules: &CullRules, reasons: &mut Vec<String>) {
    let size = item.size_bytes;
    let mut out = false;
    if let Some(min) = rules.size_bytes.min {
        if size.map(|s| s < min).unwrap_or(true) {
            out = true;
        }
    }
    if let Some(max) = rules.size_bytes.max {
        if size.map(|s| s > max).unwrap_or(true) {
            out = true;
        }
    }
    if out {
        reasons.push(reason::SIZE.into());
    }
}

fn eval_empty(item: &CullCandidate, rules: &CullRules, reasons: &mut Vec<String>) {
    if rules.empty.zero_size && item.size_bytes == Some(0) {
        reasons.push(reason::EMPTY.into());
        return;
    }
    if rules.empty.no_text_and_no_native
        && item.text_sha256.is_none()
        && item.native_sha256.is_none()
    {
        reasons.push(reason::EMPTY.into());
    }
}

fn eval_near_dup(item: &CullCandidate, rules: &CullRules, reasons: &mut Vec<String>) {
    let role = item.near_dup_role.as_deref();
    if rules.near_dup.cull_members && role == Some(item_near_dup_role::MEMBER) {
        reasons.push(reason::NEAR_DUP_MEMBER.into());
    }
    if rules.near_dup.keep_pivots_only
        && role != Some(item_near_dup_role::PIVOT)
        && !reasons.iter().any(|r| r == reason::NEAR_DUP_MEMBER)
    {
        // Cull everyone who is not a pivot (including unique/skipped/member).
        reasons.push(reason::NEAR_DUP_MEMBER.into());
    }
}

/// Serialize reasons to JSON array string.
pub fn reasons_to_json(reasons: &[String]) -> String {
    serde_json::to_string(reasons).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{DateRule, MissingDatePolicy};

    fn cand_with_sent(sent: &str) -> CullCandidate {
        CullCandidate {
            id: "x".into(),
            parent_item_id: None,
            family_id: None,
            dedup_role: Some(item_dedup_role::UNIQUE.into()),
            near_dup_role: None,
            sent_at: Some(sent.into()),
            received_at: None,
            created_at: None,
            modified_at: None,
            path: Some("x.eml".into()),
            custodian: None,
            file_category: None,
            mime_type: None,
            size_bytes: Some(1),
            status: "extracted".into(),
            native_sha256: None,
            text_sha256: None,
            role: None,
            imported_at: "2020-01-01T00:00:00Z".into(),
        }
    }

    fn date_rules(start: &str, end: &str) -> CullRules {
        CullRules {
            exclude_exact_duplicates: false,
            date: DateRule {
                enabled: true,
                field: DateField::SentAt,
                start: Some(start.into()),
                end: Some(end.into()),
                missing_policy: MissingDatePolicy::Include,
            },
            ..Default::default()
        }
    }

    /// start inclusive: instant == start → included.
    #[test]
    fn date_start_inclusive() {
        let rules = date_rules("2023-01-01T00:00:00Z", "2023-02-01T00:00:00Z");
        let d = evaluate_item(&cand_with_sent("2023-01-01T00:00:00Z"), &rules, None);
        assert!(!d.is_culled(), "instant == start must be included: {d:?}");
    }

    /// end exclusive: instant == end → culled (date_out_of_range).
    #[test]
    fn date_end_exclusive() {
        let rules = date_rules("2023-01-01T00:00:00Z", "2023-02-01T00:00:00Z");
        let d = evaluate_item(&cand_with_sent("2023-02-01T00:00:00Z"), &rules, None);
        assert!(d.is_culled(), "instant == end must be culled: {d:?}");
        assert!(d.reasons.iter().any(|r| r == reason::DATE_OUT_OF_RANGE));
    }
}
