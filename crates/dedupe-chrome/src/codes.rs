//! Code catalog, privilege-change preview, and apply_codes wrap.

use std::collections::{HashMap, HashSet};

use matter_core::{
    privilege_basis, privilege_status, ApplyCodesInput, ApplyCodesResult, CodeDef, ItemCodeInfo,
    UpsertItemPrivilegeInput,
};
use serde::{Deserialize, Serialize};

use crate::error::{map_core, CommandError};
use crate::open_root::{open_matter_read, open_matter_write};

#[derive(Debug, Clone, Deserialize)]
pub struct RootOnlyArgs {
    pub root: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeCatalogEntry {
    pub id: String,
    pub key: String,
    pub label: String,
    pub group_key: String,
    pub cardinality: String,
    pub sort_order: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewCodesPreviewArgs {
    pub root: String,
    pub item_ids: Vec<String>,
    #[serde(default)]
    pub add_code_ids: Vec<String>,
    #[serde(default)]
    pub remove_code_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReviewCodesPreviewResponse {
    pub privilege_would_change: u64,
    pub selected_count: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewApplyCodesArgs {
    pub root: String,
    pub item_ids: Vec<String>,
    #[serde(default)]
    pub add_code_ids: Vec<String>,
    #[serde(default)]
    pub remove_code_ids: Vec<String>,
    /// Ignored: queue apply always forces `propagate_family = false`.
    #[serde(default)]
    pub propagate_family: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewWindowApplyArgs {
    pub root: String,
    pub item_ids: Vec<String>,
    #[serde(default)]
    pub add_code_ids: Vec<String>,
    #[serde(default)]
    pub remove_code_ids: Vec<String>,
    #[serde(default)]
    pub propagate_family: Option<bool>,
    #[serde(default)]
    pub privilege_basis: Option<String>,
    #[serde(default)]
    pub withhold: Option<bool>,
    #[serde(default)]
    pub include_on_log: Option<bool>,
    #[serde(default)]
    pub privilege_description: Option<String>,
}

fn catalog_entries(defs: Vec<CodeDef>) -> Vec<CodeCatalogEntry> {
    defs.into_iter()
        .filter(|d| d.is_active != 0)
        .map(|d| CodeCatalogEntry {
            id: d.id,
            key: d.key,
            label: d.label,
            group_key: d.group_key,
            cardinality: d.cardinality,
            sort_order: d.sort_order,
            is_active: true,
        })
        .collect()
}

pub fn review_code_catalog_blocking(
    args: RootOnlyArgs,
) -> Result<Vec<CodeCatalogEntry>, CommandError> {
    {
        let matter = open_matter_read(&args.root)?;
        let defs = matter.list_code_definitions().map_err(map_core)?;
        if defs.iter().any(|d| d.is_active != 0) {
            return Ok(catalog_entries(defs));
        }
    }
    let matter = open_matter_write(&args.root)?;
    matter.seed_default_codes().map_err(map_core)?;
    let defs = matter.list_code_definitions().map_err(map_core)?;
    Ok(catalog_entries(defs))
}

pub fn review_codes_preview_blocking(
    args: ReviewCodesPreviewArgs,
) -> Result<ReviewCodesPreviewResponse, CommandError> {
    let matter = open_matter_read(&args.root)?;
    let selected_count = args.item_ids.len() as u64;
    if args.item_ids.is_empty() {
        return Ok(ReviewCodesPreviewResponse {
            privilege_would_change: 0,
            selected_count: 0,
        });
    }
    let defs = matter
        .list_code_definitions()
        .map_err(|e| CommandError::failed(e.to_string()))?;
    let by_id: HashMap<String, CodeDef> = defs.into_iter().map(|d| (d.id.clone(), d)).collect();

    let add_defs: Vec<&CodeDef> = args
        .add_code_ids
        .iter()
        .filter_map(|id| by_id.get(id))
        .collect();
    let remove_defs: Vec<&CodeDef> = args
        .remove_code_ids
        .iter()
        .filter_map(|id| by_id.get(id))
        .collect();

    let touches_privilege = add_defs
        .iter()
        .chain(remove_defs.iter())
        .any(|d| d.group_key == "privilege" || d.key == "privilege");
    if !touches_privilege {
        return Ok(ReviewCodesPreviewResponse {
            privilege_would_change: 0,
            selected_count,
        });
    }

    let current = matter
        .list_item_codes(&args.item_ids)
        .map_err(|e| CommandError::failed(e.to_string()))?;

    let mut would_change = 0u64;
    for id in &args.item_ids {
        let codes = current.get(id).cloned().unwrap_or_default();
        let before = privilege_membership(&codes);
        let after = simulate_privilege_membership(&codes, &add_defs, &remove_defs);
        if before != after {
            would_change += 1;
        }
    }
    Ok(ReviewCodesPreviewResponse {
        privilege_would_change: would_change,
        selected_count,
    })
}

fn privilege_membership(codes: &[ItemCodeInfo]) -> HashSet<String> {
    codes
        .iter()
        .filter(|c| c.group_key == "privilege" || c.key == "privilege")
        .map(|c| c.code_id.clone())
        .collect()
}

fn simulate_privilege_membership(
    codes: &[ItemCodeInfo],
    add_defs: &[&CodeDef],
    remove_defs: &[&CodeDef],
) -> HashSet<String> {
    let mut set = privilege_membership(codes);
    // Same order as Matter::apply_codes: adds first, then removes.
    for d in add_defs {
        if d.group_key == "privilege" || d.key == "privilege" {
            set.insert(d.id.clone());
        }
    }
    for d in remove_defs {
        if d.group_key == "privilege" || d.key == "privilege" {
            set.remove(&d.id);
        }
    }
    set
}

pub fn review_apply_codes_blocking(
    args: ReviewApplyCodesArgs,
) -> Result<ApplyCodesResult, CommandError> {
    let matter = open_matter_write(&args.root)?;
    // Queue apply always disables family propagate.
    let _ = args.propagate_family;
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: args.item_ids,
            add_code_ids: args.add_code_ids,
            remove_code_ids: args.remove_code_ids,
            propagate_family: false,
            actor: "chrome".into(),
            expected_version: None,
        })
        .map_err(|e| CommandError::failed(e.to_string()))
}

fn is_privilege_def(d: &CodeDef) -> bool {
    d.group_key == "privilege" || d.key == "privilege"
}

fn match_def<'a>(defs: &'a [CodeDef], token: &str) -> Option<&'a CodeDef> {
    defs.iter().find(|d| d.id == token || d.key == token)
}

fn resolve_code_ids(defs: &[CodeDef], tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .map(|t| {
            match_def(defs, t)
                .map(|d| d.id.clone())
                .unwrap_or_else(|| t.clone())
        })
        .collect()
}

pub fn review_window_apply_blocking(
    args: ReviewWindowApplyArgs,
) -> Result<ApplyCodesResult, CommandError> {
    let propagate_family = args.propagate_family.unwrap_or(false);
    let matter = open_matter_write(&args.root)?;
    let defs = matter.list_code_definitions().map_err(map_core)?;
    let adding_privilege = args
        .add_code_ids
        .iter()
        .any(|t| match_def(&defs, t).is_some_and(is_privilege_def));
    let privilege_id = defs
        .iter()
        .find(|d| is_privilege_def(d))
        .map(|d| d.id.clone());

    if adding_privilege {
        let basis = args
            .privilege_basis
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match basis {
            Some(b) if privilege_basis::ALL.contains(&b) => {}
            _ => {
                return Err(CommandError::failed(
                    "privilege_basis is required when adding the privilege code",
                ));
            }
        }
    }

    let add_code_ids = resolve_code_ids(&defs, &args.add_code_ids);
    let remove_code_ids = resolve_code_ids(&defs, &args.remove_code_ids);
    let removing_privilege = args
        .remove_code_ids
        .iter()
        .any(|t| match_def(&defs, t).is_some_and(is_privilege_def));

    let result = matter
        .apply_codes(ApplyCodesInput {
            item_ids: args.item_ids,
            add_code_ids,
            remove_code_ids,
            propagate_family,
            actor: "chrome".into(),
            expected_version: None,
        })
        .map_err(map_core)?;

    let targets = result.target_item_ids.clone();

    if adding_privilege {
        let basis = args
            .privilege_basis
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(privilege_basis::ATTORNEY_CLIENT);
        let withhold = args.withhold.unwrap_or(false);
        let include_on_log = args.include_on_log.unwrap_or(true);
        for id in &targets {
            let description = match &args.privilege_description {
                Some(d) => d.clone(),
                None => matter
                    .get_item_privilege(id)
                    .ok()
                    .flatten()
                    .map(|p| p.description)
                    .unwrap_or_default(),
            };
            if let Err(e) = matter.upsert_item_privilege(UpsertItemPrivilegeInput {
                item_id: id.clone(),
                basis: basis.to_string(),
                description: description.clone(),
                status: privilege_status::ASSERTED.into(),
                withhold,
                include_on_log,
                actor: "chrome".into(),
                expected_version: None,
            }) {
                if let Some(priv_id) = privilege_id.clone() {
                    let _ = matter.apply_codes(ApplyCodesInput {
                        item_ids: targets.clone(),
                        add_code_ids: vec![],
                        remove_code_ids: vec![priv_id],
                        propagate_family: false,
                        actor: "chrome".into(),
                        expected_version: None,
                    });
                }
                for tid in &targets {
                    let _ = matter.clear_item_privilege(tid, "chrome");
                }
                return Err(map_core(e));
            }
        }
    }

    if removing_privilege {
        for id in &targets {
            if let Err(e) = matter.clear_item_privilege(id, "chrome") {
                return Err(map_core(e));
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::create_matter_under;
    use matter_core::{
        item_role, item_status, ApplyCodesInput, ItemInput, ItemUpdate, Matter,
        DEFAULT_REVIEW_SET_NAME,
    };
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    fn matter_with_three(root: &camino::Utf8Path) -> (String, String, String, String) {
        let matter = Matter::open(root).expect("open");
        let set = matter
            .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
            .expect("set");
        matter.seed_default_codes().expect("seed");
        for i in 0..3 {
            matter
                .insert_item(ItemInput {
                    id: Some(format!("itm_{i:04}")),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some(format!("S{i}")),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(i as i64),
                    ..Default::default()
                })
                .expect("item");
        }
        let defs = matter.list_code_definitions().expect("defs");
        let priv_id = defs
            .iter()
            .find(|d| d.key == "privilege")
            .expect("priv")
            .id
            .clone();
        let resp_id = defs
            .iter()
            .find(|d| d.key == "responsive")
            .expect("resp")
            .id
            .clone();
        let conf_id = defs
            .iter()
            .find(|d| d.key == "confidential")
            .expect("conf")
            .id
            .clone();
        matter
            .apply_codes(ApplyCodesInput {
                item_ids: vec!["itm_0000".into()],
                add_code_ids: vec![priv_id.clone()],
                remove_code_ids: vec![],
                propagate_family: false,
                actor: "chrome-test".into(),
                expected_version: None,
            })
            .expect("precode");
        (priv_id, resp_id, conf_id, set.id)
    }

    #[test]
    fn preview_privilege_would_change_two_of_three() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "PrevPriv").expect("create");
        let (priv_id, _resp, _conf, _) = matter_with_three(&root);
        let preview = review_codes_preview_blocking(ReviewCodesPreviewArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0000".into(), "itm_0001".into(), "itm_0002".into()],
            add_code_ids: vec![priv_id],
            remove_code_ids: vec![],
        })
        .expect("preview");
        assert_eq!(preview.selected_count, 3);
        assert_eq!(preview.privilege_would_change, 2);
    }

    #[test]
    fn preview_responsive_and_confidential_zero_privilege_change() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "PrevNonPriv").expect("create");
        let (_priv, resp_id, conf_id, _) = matter_with_three(&root);
        let ids = vec!["itm_0000".into(), "itm_0001".into(), "itm_0002".into()];
        let r = review_codes_preview_blocking(ReviewCodesPreviewArgs {
            root: root.to_string(),
            item_ids: ids.clone(),
            add_code_ids: vec![resp_id],
            remove_code_ids: vec![],
        })
        .expect("resp");
        assert_eq!(r.privilege_would_change, 0);

        let c = review_codes_preview_blocking(ReviewCodesPreviewArgs {
            root: root.to_string(),
            item_ids: ids,
            add_code_ids: vec![conf_id],
            remove_code_ids: vec![],
        })
        .expect("conf");
        assert_eq!(c.privilege_would_change, 0);
    }

    #[test]
    fn preview_add_then_remove_same_privilege_code() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "PrevOverlap").expect("create");
        let (priv_id, _, _, _) = matter_with_three(&root);
        // Uncoded item: add-then-remove same id → still uncoded (matches apply_codes).
        let preview = review_codes_preview_blocking(ReviewCodesPreviewArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0001".into()],
            add_code_ids: vec![priv_id.clone()],
            remove_code_ids: vec![priv_id],
        })
        .expect("preview");
        assert_eq!(preview.privilege_would_change, 0);
    }

    #[test]
    fn apply_propagates_false_and_actor_chrome() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "ApplyChrome").expect("create");
        let (priv_id, _, _, _) = matter_with_three(&root);
        // Preview is read-only; itm_0001 stays uncoded until apply below.
        let before = {
            let matter = Matter::open_for_read(&root).expect("read");
            matter
                .list_item_codes(&["itm_0001".to_string()])
                .expect("codes")
        };
        assert!(before["itm_0001"].is_empty());

        let result = review_apply_codes_blocking(ReviewApplyCodesArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0001".into()],
            add_code_ids: vec![priv_id],
            remove_code_ids: vec![],
            // Client true is ignored; host forces false and actor "chrome".
            propagate_family: Some(true),
        })
        .expect("apply");
        assert_eq!(result.target_count, 1);
        assert_eq!(result.target_item_ids, vec!["itm_0001".to_string()]);

        let matter = Matter::open_for_read(&root).expect("read");
        let codes = matter
            .list_item_codes(&["itm_0001".to_string()])
            .expect("codes");
        assert_eq!(codes["itm_0001"].len(), 1);
        assert_eq!(codes["itm_0001"][0].key, "privilege");
        assert_eq!(codes["itm_0001"][0].set_by, "chrome");
    }

    #[test]
    fn catalog_seeds_when_empty() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "CatalogSeed").expect("create");
        let catalog = review_code_catalog_blocking(RootOnlyArgs {
            root: root.to_string(),
        })
        .expect("catalog");
        assert!(catalog.iter().any(|c| c.key == "privilege"));
        assert!(catalog.iter().any(|c| c.key == "responsive"));
    }

    #[test]
    fn catalog_read_path_after_seed() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "CatalogRead").expect("create");
        {
            let matter = Matter::open(&root).expect("write seed");
            matter.seed_default_codes().expect("seed");
        }
        let _held = Matter::open_for_read(&root).expect("hold read");
        let catalog = review_code_catalog_blocking(RootOnlyArgs {
            root: root.to_string(),
        })
        .expect("catalog read");
        assert!(catalog.iter().any(|c| c.key == "privilege"));
    }

    fn seed_family_three(root: &camino::Utf8Path) {
        let matter = Matter::open(root).expect("open");
        let family = matter.insert_family("").expect("family");
        matter
            .insert_item(ItemInput {
                id: Some("itm_0000".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::PARENT.into()),
                family_id: Some(family.id.clone()),
                subject: Some("Parent".into()),
                ..Default::default()
            })
            .expect("parent");
        matter
            .insert_item(ItemInput {
                id: Some("itm_0001".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::ATTACHMENT.into()),
                family_id: Some(family.id.clone()),
                parent_item_id: Some("itm_0000".into()),
                subject: Some("A".into()),
                ..Default::default()
            })
            .expect("c1");
        matter
            .insert_item(ItemInput {
                id: Some("itm_0002".into()),
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::ATTACHMENT.into()),
                family_id: Some(family.id.clone()),
                parent_item_id: Some("itm_0000".into()),
                subject: Some("B".into()),
                ..Default::default()
            })
            .expect("c2");
        let set = matter
            .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
            .expect("set");
        for (i, id) in ["itm_0000", "itm_0001", "itm_0002"].iter().enumerate() {
            matter
                .update_item(
                    id,
                    ItemUpdate {
                        in_review: Some(Some(1)),
                        review_set_id: Some(Some(set.id.clone())),
                        review_order: Some(Some(i as i64)),
                        ..Default::default()
                    },
                )
                .expect("promote");
        }
        matter.seed_default_codes().expect("seed");
    }

    fn code_id(root: &camino::Utf8Path, key: &str) -> String {
        let catalog = review_code_catalog_blocking(RootOnlyArgs {
            root: root.to_string(),
        })
        .expect("catalog");
        catalog
            .iter()
            .find(|c| c.key == key)
            .unwrap_or_else(|| panic!("missing {key}"))
            .id
            .clone()
    }

    fn has_code(root: &camino::Utf8Path, item_id: &str, key: &str) -> bool {
        let matter = Matter::open_for_read(root).expect("read");
        let codes = matter
            .list_item_codes(&[item_id.to_string()])
            .expect("codes");
        codes
            .get(item_id)
            .map(|cs| cs.iter().any(|c| c.key == key))
            .unwrap_or(false)
    }

    #[test]
    fn window_apply_propagate_false_and_true() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "WinProp").expect("create");
        seed_family_three(&root);
        let resp = code_id(&root, "responsive");
        review_window_apply_blocking(ReviewWindowApplyArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0000".into()],
            add_code_ids: vec![resp.clone()],
            remove_code_ids: vec![],
            propagate_family: Some(false),
            privilege_basis: None,
            withhold: None,
            include_on_log: None,
            privilege_description: None,
        })
        .expect("false");
        assert!(has_code(&root, "itm_0000", "responsive"));
        assert!(!has_code(&root, "itm_0001", "responsive"));
        assert!(!has_code(&root, "itm_0002", "responsive"));

        review_window_apply_blocking(ReviewWindowApplyArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0000".into()],
            add_code_ids: vec![resp],
            remove_code_ids: vec![],
            propagate_family: Some(true),
            privilege_basis: None,
            withhold: None,
            include_on_log: None,
            privilege_description: None,
        })
        .expect("true");
        assert!(has_code(&root, "itm_0000", "responsive"));
        assert!(has_code(&root, "itm_0001", "responsive"));
        assert!(has_code(&root, "itm_0002", "responsive"));
    }

    #[test]
    fn window_apply_privilege_without_basis_no_write() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "WinPrivNone").expect("create");
        seed_family_three(&root);
        let priv_id = code_id(&root, "privilege");
        let err = review_window_apply_blocking(ReviewWindowApplyArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0000".into()],
            add_code_ids: vec![priv_id],
            remove_code_ids: vec![],
            propagate_family: Some(false),
            privilege_basis: None,
            withhold: None,
            include_on_log: None,
            privilege_description: None,
        })
        .expect_err("basis required");
        assert_eq!(err.kind, "failed");
        assert!(!has_code(&root, "itm_0000", "privilege"));
        let matter = Matter::open_for_read(&root).expect("read");
        let claim = matter.get_item_privilege("itm_0000").expect("claim");
        assert!(claim.is_none());
    }

    #[test]
    fn window_apply_privilege_basis_withhold_false() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "WinPrivOk").expect("create");
        seed_family_three(&root);
        let priv_id = code_id(&root, "privilege");
        review_window_apply_blocking(ReviewWindowApplyArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0000".into()],
            add_code_ids: vec![priv_id],
            remove_code_ids: vec![],
            propagate_family: Some(false),
            privilege_basis: Some("attorney_client".into()),
            withhold: None,
            include_on_log: None,
            privilege_description: None,
        })
        .expect("apply");
        assert!(has_code(&root, "itm_0000", "privilege"));
        let matter = Matter::open_for_read(&root).expect("read");
        let claim = matter
            .get_item_privilege("itm_0000")
            .expect("claim")
            .expect("row");
        assert_eq!(claim.basis, "attorney_client");
        assert_eq!(claim.withhold, 0);
        assert_eq!(claim.include_on_log, 1);
        assert_eq!(claim.status, "asserted");
    }

    #[test]
    fn window_apply_responsive_no_privilege_claim() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "WinResp").expect("create");
        seed_family_three(&root);
        let resp = code_id(&root, "responsive");
        review_window_apply_blocking(ReviewWindowApplyArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0000".into()],
            add_code_ids: vec![resp],
            remove_code_ids: vec![],
            propagate_family: Some(false),
            privilege_basis: None,
            withhold: None,
            include_on_log: None,
            privilege_description: None,
        })
        .expect("apply");
        let matter = Matter::open_for_read(&root).expect("read");
        let claim = matter.get_item_privilege("itm_0000").expect("claim");
        assert!(
            claim.is_none() || claim.as_ref().is_some_and(|c| c.status == "cleared"),
            "responsive must not leave an active privilege claim: {claim:?}"
        );
    }

    #[test]
    fn window_apply_confidential_no_privilege_basis() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "WinConf").expect("create");
        seed_family_three(&root);
        let conf = code_id(&root, "confidential");
        review_window_apply_blocking(ReviewWindowApplyArgs {
            root: root.to_string(),
            item_ids: vec!["itm_0000".into()],
            add_code_ids: vec![conf],
            remove_code_ids: vec![],
            propagate_family: Some(false),
            privilege_basis: None,
            withhold: None,
            include_on_log: None,
            privilege_description: None,
        })
        .expect("conf");
        assert!(has_code(&root, "itm_0000", "confidential"));
        assert!(!has_code(&root, "itm_0000", "privilege"));
    }
}
