//! `review_upsert_privilege` — withhold/basis edits when already privilege-coded.

use matter_core::{privilege_basis, privilege_status, UpsertItemPrivilegeInput};
use serde::Deserialize;

use crate::error::{map_core, CommandError};
use crate::open_root::open_matter_write;

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewUpsertPrivilegeArgs {
    pub root: String,
    pub item_id: String,
    pub basis: String,
    #[serde(default)]
    pub withhold: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
}

pub fn review_upsert_privilege_blocking(
    args: ReviewUpsertPrivilegeArgs,
) -> Result<matter_core::ItemPrivilege, CommandError> {
    if args.item_id.trim().is_empty() {
        return Err(CommandError::not_found("item not found: ".to_string()));
    }
    let basis = args.basis.trim();
    if !privilege_basis::ALL.contains(&basis) {
        return Err(CommandError::failed(format!(
            "privilege_basis must be one of {}",
            privilege_basis::ALL.join(", ")
        )));
    }
    let matter = open_matter_write(&args.root)?;
    let codes = matter
        .list_item_codes(std::slice::from_ref(&args.item_id))
        .map_err(map_core)?;
    let coded = codes
        .get(&args.item_id)
        .map(|cs| {
            cs.iter()
                .any(|c| c.group_key == "privilege" || c.key == "privilege")
        })
        .unwrap_or(false);
    if !coded {
        return Err(CommandError::failed(
            "privilege claim edits require an existing privilege code",
        ));
    }
    let description = match args.description {
        Some(d) => d,
        None => matter
            .get_item_privilege(&args.item_id)
            .map_err(map_core)?
            .map(|p| p.description)
            .unwrap_or_default(),
    };
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: args.item_id,
            basis: basis.to_string(),
            description,
            status: privilege_status::ASSERTED.into(),
            withhold: args.withhold.unwrap_or(false),
            include_on_log: true,
            actor: "chrome".into(),
            expected_version: None,
        })
        .map_err(map_core)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::{review_window_apply_blocking, ReviewWindowApplyArgs};
    use crate::create::create_matter_under;
    use matter_core::{item_role, item_status, ItemInput, Matter, DEFAULT_REVIEW_SET_NAME};
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    #[test]
    fn upsert_without_description_preserves_existing() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "PrivKeepDesc").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_0000".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("S".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id),
                    review_order: Some(0),
                    ..Default::default()
                })
                .expect("item");
        }
        let priv_id = {
            let matter = Matter::open_for_read(&root).expect("read");
            matter
                .list_code_definitions()
                .expect("defs")
                .into_iter()
                .find(|d| d.key == "privilege")
                .expect("priv")
                .id
        };
        review_window_apply_blocking(ReviewWindowApplyArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0000".into()],
            add_code_ids: vec![priv_id],
            remove_code_ids: vec![],
            propagate_family: Some(false),
            privilege_basis: Some("attorney_client".into()),
            withhold: Some(false),
            include_on_log: Some(true),
            privilege_description: Some("Legal advice".into()),
        })
        .expect("on");
        review_upsert_privilege_blocking(ReviewUpsertPrivilegeArgs {
            root: root.to_string(),
            item_id: "itm_0000".into(),
            basis: "work_product".into(),
            withhold: Some(true),
            description: None,
        })
        .expect("upsert");
        let matter = Matter::open_for_read(&root).expect("read");
        let claim = matter
            .get_item_privilege("itm_0000")
            .expect("get")
            .expect("row");
        assert_eq!(claim.basis, "work_product");
        assert_eq!(claim.withhold, 1);
        assert_eq!(claim.description, "Legal advice");
    }
}
