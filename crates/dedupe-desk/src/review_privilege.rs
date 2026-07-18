//! Privilege panel helpers for Review (track 0031).
//!
//! Work-product claim fields + export path helpers. Never auto-copy notes into
//! log descriptions.

use matter_core::{
    privilege_basis, privilege_status, FamilyPrivilegeConsistency, ItemPrivilege, Matter,
    PrivilegeLogExportParams, PrivilegeLogExportResult, PrivilegeProtocol,
    UpsertItemPrivilegeInput, UpsertPrivilegeProtocolInput, SCOPE_ENTIRE_MATTER,
    SCOPE_REVIEW_CORPUS,
};

use camino::Utf8Path;

/// Draft state for the privilege panel (bound to egui widgets).
#[derive(Debug, Clone)]
pub struct PrivilegePanelDraft {
    pub basis: String,
    pub status: String,
    pub withhold: bool,
    pub include_on_log: bool,
    pub description: String,
    /// Item id this draft was loaded for (detect selection change).
    pub item_id: Option<String>,
    /// Dirty vs last loaded/saved.
    pub dirty: bool,
}

impl Default for PrivilegePanelDraft {
    fn default() -> Self {
        Self {
            basis: privilege_basis::ATTORNEY_CLIENT.to_string(),
            status: privilege_status::ASSERTED.to_string(),
            withhold: true,
            include_on_log: true,
            description: String::new(),
            item_id: None,
            dirty: false,
        }
    }
}

impl PrivilegePanelDraft {
    /// Load draft from stored claim (or defaults when none).
    pub fn from_privilege(item_id: &str, priv_row: Option<&ItemPrivilege>) -> Self {
        match priv_row {
            Some(p) => Self {
                basis: p.basis.clone(),
                status: p.status.clone(),
                withhold: p.withhold != 0,
                include_on_log: p.include_on_log != 0,
                description: p.description.clone(),
                item_id: Some(item_id.to_string()),
                dirty: false,
            },
            None => Self {
                item_id: Some(item_id.to_string()),
                ..Self::default()
            },
        }
    }

    /// Build upsert input from draft.
    pub fn to_upsert_input(&self, item_id: &str, actor: &str) -> UpsertItemPrivilegeInput {
        UpsertItemPrivilegeInput {
            item_id: item_id.to_string(),
            basis: self.basis.clone(),
            description: self.description.clone(),
            status: self.status.clone(),
            withhold: self.withhold,
            include_on_log: self.include_on_log,
            actor: actor.to_string(),
        }
    }
}

/// Whether the privilege panel should be visible for the current item.
pub fn should_show_privilege_panel(
    has_privilege_code: bool,
    has_privilege_row: bool,
    force_open: bool,
) -> bool {
    has_privilege_code || has_privilege_row || force_open
}

/// Basis options for dropdown (key, label).
pub fn basis_options() -> &'static [(&'static str, &'static str)] {
    &[
        (
            privilege_basis::ATTORNEY_CLIENT,
            "Attorney-Client Privilege",
        ),
        (privilege_basis::WORK_PRODUCT, "Work Product"),
        (
            privilege_basis::ATTORNEY_CLIENT_WORK_PRODUCT,
            "Attorney-Client and Work Product",
        ),
        (privilege_basis::COMMON_INTEREST, "Common Interest"),
        (privilege_basis::OTHER, "Other (see description)"),
    ]
}

/// Status options for dropdown (key, label).
pub fn status_options() -> &'static [(&'static str, &'static str)] {
    &[
        (privilege_status::ASSERTED, "Asserted"),
        (privilege_status::UNDER_REVIEW, "Under review"),
        (privilege_status::CLEARED, "Cleared"),
        (privilege_status::PARTIAL_REDACTION, "Partial redaction"),
    ]
}

/// Family split banner text when consistency fails.
pub fn family_split_banner(cons: &FamilyPrivilegeConsistency) -> Option<String> {
    if cons.consistent {
        return None;
    }
    Some(format!(
        "Family split privilege call — privileged: {} · not privileged: {}",
        cons.privileged_ids.len(),
        cons.non_privileged_ids.len()
    ))
}

/// Focus gate: description focused blocks digit coding (same as notes).
pub fn focus_allows_coding_with_privilege(
    no_widget_focus: bool,
    note_editor_focused: bool,
    privilege_editor_focused: bool,
) -> bool {
    crate::review_notes::focus_allows_coding_shortcuts(no_widget_focus, note_editor_focused)
        && !privilege_editor_focused
}

/// Optional draft-from-note: copy latest note body into description draft only
/// when the operator confirms (never auto on export).
pub fn draft_description_from_note(current_description: &str, note_body: &str) -> String {
    let note = note_body.trim();
    if note.is_empty() {
        return current_description.to_string();
    }
    if current_description.trim().is_empty() {
        note.to_string()
    } else {
        // Append with separator so operator does not lose existing text.
        format!("{}\n\n{}", current_description.trim_end(), note)
    }
}

/// Blocking load of privilege claim + family consistency for one item.
pub fn load_privilege_panel(
    matter_root: &Utf8Path,
    item_id: &str,
) -> Result<(Option<ItemPrivilege>, FamilyPrivilegeConsistency), String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    let row = matter
        .get_item_privilege(item_id)
        .map_err(|e| e.to_string())?;
    let cons = matter
        .family_privilege_consistency(item_id)
        .map_err(|e| e.to_string())?;
    Ok((row, cons))
}

/// Blocking upsert of privilege claim.
pub fn upsert_privilege_blocking(
    matter_root: &Utf8Path,
    input: UpsertItemPrivilegeInput,
) -> Result<ItemPrivilege, String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    matter
        .upsert_item_privilege(input)
        .map_err(|e| e.to_string())
}

/// Assert privilege: ensure claim + apply privilege code if missing.
pub fn assert_privilege_blocking(
    matter_root: &Utf8Path,
    item_id: &str,
    actor: &str,
    privilege_code_id: Option<&str>,
) -> Result<ItemPrivilege, String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    if let Some(cid) = privilege_code_id {
        matter
            .apply_codes(matter_core::ApplyCodesInput {
                item_ids: vec![item_id.to_string()],
                add_code_ids: vec![cid.to_string()],
                remove_code_ids: vec![],
                propagate_family: false,
                actor: actor.to_string(),
            })
            .map_err(|e| e.to_string())?;
    } else {
        matter
            .ensure_item_privilege(item_id, actor)
            .map_err(|e| e.to_string())?;
    }
    matter
        .get_item_privilege(item_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "privilege row missing after assert".into())
}

/// Export privilege log to path under matter or operator-chosen path.
pub fn export_privilege_log_blocking(
    matter_root: &Utf8Path,
    scope_review_corpus: bool,
    path: &Utf8Path,
) -> Result<PrivilegeLogExportResult, String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    let scope = if scope_review_corpus {
        SCOPE_REVIEW_CORPUS
    } else {
        SCOPE_ENTIRE_MATTER
    };
    matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: scope.into(),
            path: path.to_path_buf(),
            filter_ids: None,
        })
        .map_err(|e| e.to_string())
}

/// Suggest default export path under matter `exports/`.
pub fn default_privilege_log_path(matter_root: &Utf8Path) -> camino::Utf8PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    matter_root
        .join("exports")
        .join(format!("privilege_log_{stamp}.csv"))
}

/// Load / upsert protocol (settings / workspace).
pub fn load_protocol_blocking(matter_root: &Utf8Path) -> Result<PrivilegeProtocol, String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    matter.get_privilege_protocol().map_err(|e| e.to_string())
}

pub fn upsert_protocol_blocking(
    matter_root: &Utf8Path,
    input: UpsertPrivilegeProtocolInput,
) -> Result<PrivilegeProtocol, String> {
    let matter = Matter::open(matter_root).map_err(|e| e.to_string())?;
    matter
        .upsert_privilege_protocol(input)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use matter_core::{basis_label, PRIVILEGE_LOG_COLUMNS};

    #[test]
    fn panel_visible_when_code_or_row_or_force() {
        assert!(!should_show_privilege_panel(false, false, false));
        assert!(should_show_privilege_panel(true, false, false));
        assert!(should_show_privilege_panel(false, true, false));
        assert!(should_show_privilege_panel(false, false, true));
    }

    #[test]
    fn draft_defaults_and_from_row() {
        let d = PrivilegePanelDraft::default();
        assert_eq!(d.basis, privilege_basis::ATTORNEY_CLIENT);
        assert!(d.withhold);
        assert!(d.include_on_log);

        let row = ItemPrivilege {
            item_id: "i1".into(),
            matter_id: "m".into(),
            basis: privilege_basis::WORK_PRODUCT.into(),
            description: "legal advice".into(),
            status: privilege_status::UNDER_REVIEW.into(),
            withhold: 0,
            include_on_log: 1,
            asserted_at: None,
            asserted_by: None,
            updated_at: "t".into(),
            updated_by: "a".into(),
            extra_json: None,
        };
        let d2 = PrivilegePanelDraft::from_privilege("i1", Some(&row));
        assert_eq!(d2.basis, privilege_basis::WORK_PRODUCT);
        assert!(!d2.withhold);
        assert_eq!(d2.description, "legal advice");
        assert!(!d2.dirty);
    }

    #[test]
    fn draft_from_note_never_auto_empty() {
        assert_eq!(draft_description_from_note("", ""), "");
        assert_eq!(
            draft_description_from_note("", "  note body  "),
            "note body"
        );
        assert_eq!(
            draft_description_from_note("existing", "note"),
            "existing\n\nnote"
        );
    }

    #[test]
    fn focus_gate_includes_privilege_editor() {
        assert!(focus_allows_coding_with_privilege(true, false, false));
        assert!(!focus_allows_coding_with_privilege(true, false, true));
        assert!(!focus_allows_coding_with_privilege(true, true, false));
        assert!(!focus_allows_coding_with_privilege(false, false, false));
    }

    #[test]
    fn csv_columns_match_spec() {
        assert_eq!(PRIVILEGE_LOG_COLUMNS[0], "ControlNumber");
        assert_eq!(PRIVILEGE_LOG_COLUMNS[12], "PrivilegeType");
        assert_eq!(PRIVILEGE_LOG_COLUMNS[13], "Description");
        assert_eq!(PRIVILEGE_LOG_COLUMNS.len(), 19);
    }

    #[test]
    fn family_banner_only_when_split() {
        let ok = FamilyPrivilegeConsistency {
            consistent: true,
            privileged_ids: vec!["a".into()],
            non_privileged_ids: vec![],
        };
        assert!(family_split_banner(&ok).is_none());
        let split = FamilyPrivilegeConsistency {
            consistent: false,
            privileged_ids: vec!["a".into()],
            non_privileged_ids: vec!["b".into()],
        };
        let msg = family_split_banner(&split).expect("banner");
        assert!(msg.contains("split"));
    }

    #[test]
    fn basis_label_for_ui() {
        assert_eq!(
            basis_label(privilege_basis::ATTORNEY_CLIENT),
            "Attorney-Client Privilege"
        );
    }
}
