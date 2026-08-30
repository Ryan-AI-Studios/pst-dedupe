//! `review_document` — metadata, codes, family card, notes, neighbors (no CAS body).

use matter_core::{
    FamilyMemberThin, FilterSpec, Item, ItemCodeInfo, ItemNote, ItemPrivilege, Matter,
};
use matter_search::{
    search_keyword_for_matter, KeywordQuery, SearchError, DEFAULT_FTS_FETCH_LIMIT,
};
use serde::{Deserialize, Serialize};

use crate::error::{map_core, CommandError};
use crate::open_root::open_matter_read;

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewDocumentArgs {
    pub root: String,
    pub item_id: String,
    #[serde(default)]
    pub filter_json: Option<String>,
    #[serde(default)]
    pub keyword: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PredictionSlot {
    pub present: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewDocumentResponse {
    pub item_id: String,
    pub from_addr: Option<String>,
    pub to_addrs_json: Option<String>,
    pub cc_addrs_json: Option<String>,
    pub subject: Option<String>,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub mime_type: Option<String>,
    pub path: Option<String>,
    pub review_order: Option<i64>,
    pub attachment_count: Option<i64>,
    pub family_id: Option<String>,
    pub family_size: u64,
    pub family_truncated: bool,
    pub family_members: Vec<FamilyMemberThin>,
    pub apply_to_family_enabled: bool,
    pub codes: Vec<ItemCodeInfo>,
    pub privilege: Option<ItemPrivilege>,
    pub notes: Vec<ItemNote>,
    pub prev_id: Option<String>,
    pub next_id: Option<String>,
    pub position: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neighbors_error: Option<String>,
    pub control_number: String,
    pub bates: String,
    pub bates_note: String,
    pub prediction: PredictionSlot,
}

pub fn review_document_blocking(
    args: ReviewDocumentArgs,
) -> Result<ReviewDocumentResponse, CommandError> {
    if args.item_id.trim().is_empty() {
        return Err(CommandError::not_found("item not found: ".to_string()));
    }
    let matter = open_matter_read(&args.root)?;
    let item = matter.get_item(&args.item_id).map_err(map_core)?;
    let filter = parse_filter(args.filter_json.as_deref())?;
    let keyword = args
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let (prev_id, next_id, position, total, neighbors_error) = match keyword {
        None => {
            let n = matter
                .review_neighbors(&args.item_id, &filter, None)
                .map_err(map_core)?;
            (n.prev_id, n.next_id, n.position, n.total, None)
        }
        Some(q) => match search_keyword_for_matter(
            &matter,
            &KeywordQuery {
                query: q.to_string(),
                limit: DEFAULT_FTS_FETCH_LIMIT,
                offset: 0,
            },
        ) {
            Ok(hits) => {
                let n = matter
                    .review_neighbors(&args.item_id, &filter, Some(&hits.item_ids))
                    .map_err(map_core)?;
                (n.prev_id, n.next_id, n.position, n.total, None)
            }
            Err(SearchError::IndexMissing) | Err(SearchError::LangPackStale(_)) => {
                (None, None, 0, 0, Some("fts_unavailable".to_string()))
            }
            Err(e) => return Err(CommandError::failed(e.to_string())),
        },
    };

    let codes = matter
        .list_item_codes(std::slice::from_ref(&args.item_id))
        .map_err(map_core)?;
    let codes = codes.get(&args.item_id).cloned().unwrap_or_default();
    let privilege = matter.get_item_privilege(&args.item_id).map_err(map_core)?;
    let notes = matter.list_notes(&args.item_id).map_err(map_core)?;
    let (family_size, family_truncated, family_members, apply_to_family_enabled) =
        family_card(&matter, &item)?;

    let control_number = match item.review_order {
        Some(n) => n.to_string(),
        None => "—".into(),
    };
    let (bates, bates_note) = match matter
        .latest_control_number(&args.item_id)
        .map_err(map_core)?
    {
        Some(cn) => (cn, "from production".to_string()),
        None => ("—".into(), String::new()),
    };

    Ok(ReviewDocumentResponse {
        item_id: item.id,
        from_addr: item.from_addr,
        to_addrs_json: item.to_addrs_json,
        cc_addrs_json: item.cc_addrs_json,
        subject: item.subject,
        sent_at: item.sent_at,
        received_at: item.received_at,
        mime_type: item.mime_type,
        path: item.path,
        review_order: item.review_order,
        attachment_count: item.attachment_count,
        family_id: item.family_id,
        family_size,
        family_truncated,
        family_members,
        apply_to_family_enabled,
        codes,
        privilege,
        notes,
        prev_id,
        next_id,
        position,
        total,
        neighbors_error,
        control_number,
        bates,
        bates_note,
        prediction: PredictionSlot { present: false },
    })
}

fn parse_filter(filter_json: Option<&str>) -> Result<FilterSpec, CommandError> {
    match filter_json.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(FilterSpec::review_corpus()),
        Some(json) => serde_json::from_str(json)
            .map_err(|e| CommandError::failed(format!("invalid filter_json: {e}"))),
    }
}

fn family_card(
    matter: &Matter,
    item: &Item,
) -> Result<(u64, bool, Vec<FamilyMemberThin>, bool), CommandError> {
    let Some(fid) = item.family_id.as_deref() else {
        let self_row = FamilyMemberThin {
            id: item.id.clone(),
            parent_item_id: item.parent_item_id.clone(),
            subject: item.subject.clone(),
            role: item.role.clone(),
        };
        return Ok((1, false, vec![self_row], false));
    };
    let sizes = matter.family_sizes(&[fid.to_string()]).map_err(map_core)?;
    let family_size = sizes.get(fid).copied().unwrap_or(1).max(1);
    let thin = matter.family_members_thin(fid, 100).map_err(map_core)?;
    Ok((family_size, thin.truncated, thin.members, family_size > 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes::{
        review_code_catalog_blocking, review_window_apply_blocking, ReviewWindowApplyArgs,
        RootOnlyArgs,
    };
    use crate::create::create_matter_under;
    use matter_core::{
        item_role, item_status, FilterSpec, ItemInput, ItemUpdate, Matter, DEFAULT_REVIEW_SET_NAME,
    };
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
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
                from_addr: Some("a@example.com".into()),
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
                subject: Some("Child A".into()),
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
                subject: Some("Child B".into()),
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

    fn catalog_id(root: &str, key: &str) -> String {
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

    #[test]
    fn review_document_child_family_card() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "DocFam").expect("create");
        seed_family_three(&root);
        let doc = review_document_blocking(ReviewDocumentArgs {
            root: root.to_string(),
            item_id: "itm_0001".into(),
            filter_json: None,
            keyword: None,
        })
        .expect("doc");
        assert_eq!(doc.family_size, 3);
        assert_eq!(doc.family_members.len(), 3);
        let subjects: Vec<_> = doc
            .family_members
            .iter()
            .filter_map(|m| m.subject.clone())
            .collect();
        assert!(subjects.iter().any(|s| s == "Parent"));
        assert!(subjects.iter().any(|s| s == "Child A"));
        assert!(subjects.iter().any(|s| s == "Child B"));
        assert!(doc.apply_to_family_enabled);
        assert_eq!(doc.control_number, "1");
        assert_eq!(doc.bates, "—");
        assert_eq!(doc.bates_note, "");
        assert!(!doc.control_number.contains("ACME"));
        assert!(!doc.bates.contains("ACME0002"));
        assert!(!doc.prediction.present);
    }

    #[test]
    fn review_document_neighbors_after_uncoded_dropout() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "DocNeigh").expect("create");
        seed_family_three(&root);
        let resp = catalog_id(root.as_str(), "responsive");
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
        let filter_json = serde_json::to_string(&FilterSpec::preset_uncoded()).expect("json");
        let doc = review_document_blocking(ReviewDocumentArgs {
            root: root.to_string(),
            item_id: "itm_0000".into(),
            filter_json: Some(filter_json),
            keyword: None,
        })
        .expect("doc");
        assert_eq!(doc.next_id.as_deref(), Some("itm_0001"));
        assert_ne!(doc.position, 0);
    }

    #[test]
    fn review_document_missing_and_empty_not_found() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "DocMiss").expect("create");
        seed_family_three(&root);
        let missing = review_document_blocking(ReviewDocumentArgs {
            root: root.to_string(),
            item_id: "itm_nope".into(),
            filter_json: None,
            keyword: None,
        })
        .expect_err("missing");
        assert_eq!(missing.kind, "not_found");
        let empty = review_document_blocking(ReviewDocumentArgs {
            root: root.to_string(),
            item_id: "".into(),
            filter_json: None,
            keyword: None,
        })
        .expect_err("empty");
        assert_eq!(empty.kind, "not_found");
    }

    #[test]
    fn review_document_encrypted_kind_without_open() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = parent.join("EncDoc");
        {
            let _m = Matter::create_encrypted(&root, "EncDoc", "test-passphrase-0112")
                .expect("create encrypted");
        }
        let err = review_document_blocking(ReviewDocumentArgs {
            root: root.to_string(),
            item_id: "itm_0000".into(),
            filter_json: None,
            keyword: None,
        })
        .expect_err("encrypted");
        assert_eq!(err.kind, "encrypted");
    }
}
