//! `review_queue_page` — windowed first-pass queue over FilterSpec / FTS.

use std::collections::{HashMap, HashSet};

use matter_core::{FilterSpec, ItemCodeInfo, Matter, ReviewListRow};
use matter_search::{compose_keyword_filter, SearchError};
use serde::{Deserialize, Serialize};

use crate::error::CommandError;
use crate::open_root::open_matter_read;

const DEFAULT_LIMIT: u64 = 500;
const MAX_LIMIT: u64 = 500;

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewQueuePageArgs {
    pub root: String,
    #[serde(default)]
    pub filter_json: Option<String>,
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub extras: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QueueRow {
    pub id: String,
    pub review_order: Option<i64>,
    pub date: Option<String>,
    pub from_addr: Option<String>,
    pub subject: Option<String>,
    pub parent_item_id: Option<String>,
    pub role: Option<String>,
    pub family_id: Option<String>,
    pub family_size: u64,
    pub resp: Option<String>,
    pub privilege_coded: bool,
    pub withhold: bool,
    pub custodian: Option<String>,
    /// Present when lead/QC extras are on (confidential membership).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidential: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewQueuePageResponse {
    pub total: u64,
    pub offset: u64,
    pub limit: u64,
    pub extras: bool,
    pub rows: Vec<QueueRow>,
}

pub fn review_queue_page_blocking(
    args: ReviewQueuePageArgs,
) -> Result<ReviewQueuePageResponse, CommandError> {
    let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    if limit > MAX_LIMIT {
        return Err(CommandError::failed(format!(
            "limit {limit} exceeds max {MAX_LIMIT}"
        )));
    }
    let offset = args.offset.unwrap_or(0);
    let extras = args.extras.unwrap_or(false);

    let matter = open_matter_read(&args.root)?;
    let filter = parse_filter(args.filter_json.as_deref())?;
    let keyword = args
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let (total, thin) = if keyword.is_none() {
        let total = matter
            .count_items_filtered(&filter)
            .map_err(|e| CommandError::failed(e.to_string()))?;
        let rows = matter
            .list_items_filtered_thin(&filter, limit, offset)
            .map_err(|e| CommandError::failed(e.to_string()))?;
        (total, rows)
    } else {
        let root = matter.root().to_path_buf();
        match compose_keyword_filter(&matter, &root, keyword, &filter, limit, offset) {
            Ok(pair) => pair,
            Err(SearchError::IndexMissing) | Err(SearchError::LangPackStale(_)) => {
                return Err(CommandError::fts_unavailable(
                    "Keyword search requires an FTS index; run fts_index in Desk/Process.",
                ));
            }
            Err(e) => return Err(CommandError::failed(e.to_string())),
        }
    };

    let rows = fill_queue_rows(&matter, &thin, extras)?;
    Ok(ReviewQueuePageResponse {
        total,
        offset,
        limit,
        extras,
        rows,
    })
}

fn parse_filter(filter_json: Option<&str>) -> Result<FilterSpec, CommandError> {
    match filter_json.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(FilterSpec::review_corpus()),
        Some(json) => serde_json::from_str(json)
            .map_err(|e| CommandError::failed(format!("invalid filter_json: {e}"))),
    }
}

fn fill_queue_rows(
    matter: &Matter,
    thin: &[ReviewListRow],
    extras: bool,
) -> Result<Vec<QueueRow>, CommandError> {
    let ids: Vec<String> = thin.iter().map(|r| r.id.clone()).collect();
    let codes = matter
        .list_item_codes(&ids)
        .map_err(|e| CommandError::failed(e.to_string()))?;

    let mut family_ids: Vec<String> = thin
        .iter()
        .filter_map(|r| r.family_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    family_ids.sort();
    let sizes = matter
        .family_sizes(&family_ids)
        .map_err(|e| CommandError::failed(e.to_string()))?;

    let privilege_map = if extras {
        matter
            .list_item_privilege(&ids)
            .map_err(|e| CommandError::failed(e.to_string()))?
    } else {
        HashMap::new()
    };

    let mut rows = Vec::with_capacity(thin.len());
    for r in thin {
        let item_codes = codes.get(&r.id).cloned().unwrap_or_default();
        let (resp, privilege_coded, confidential) = map_codes(&item_codes);
        let family_size = match &r.family_id {
            Some(fid) => sizes.get(fid).copied().unwrap_or(1),
            None => 1,
        };
        let (withhold, custodian) = if extras {
            let withhold = privilege_map
                .get(&r.id)
                .map(|p| p.withhold != 0)
                .unwrap_or(false);
            let item = matter
                .get_item(&r.id)
                .map_err(|e| CommandError::failed(e.to_string()))?;
            (withhold, item.custodian)
        } else {
            (false, None)
        };
        rows.push(QueueRow {
            id: r.id.clone(),
            review_order: r.review_order,
            date: r.sent_at.clone().or_else(|| r.received_at.clone()),
            from_addr: r.from_addr.clone(),
            subject: r.subject.clone(),
            parent_item_id: r.parent_item_id.clone(),
            role: r.role.clone(),
            family_id: r.family_id.clone(),
            family_size,
            resp,
            privilege_coded,
            withhold,
            custodian,
            confidential: if extras { Some(confidential) } else { None },
        });
    }
    Ok(rows)
}

fn map_codes(codes: &[ItemCodeInfo]) -> (Option<String>, bool, bool) {
    let mut resp: Option<String> = None;
    let mut privilege_coded = false;
    let mut confidential = false;
    for c in codes {
        if c.group_key == "responsiveness" {
            // Only seeded R/NR/NSL tokens; unknown keys → — in the UI.
            resp = match c.key.as_str() {
                "responsive" => Some("R".into()),
                "not_responsive" => Some("NR".into()),
                "needs_second_look" => Some("NSL".into()),
                _ => None,
            };
        }
        if c.group_key == "privilege" || c.key == "privilege" {
            privilege_coded = true;
        }
        if c.key == "confidential" {
            confidential = true;
        }
    }
    (resp, privilege_coded, confidential)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::create_matter_under;
    use matter_core::{
        item_role, item_status, ApplyCodesInput, CodeDefInput, FilterSpec, ItemInput, Matter,
        UpsertItemPrivilegeInput, DEFAULT_REVIEW_SET_NAME,
    };
    use std::collections::{HashMap, HashSet};
    use tempfile::tempdir;

    fn utf8_tmp(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
    }

    fn seed_1k(matter: &Matter) -> String {
        let set = matter
            .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
            .expect("set");
        for i in 0..1000 {
            matter
                .insert_item(ItemInput {
                    id: Some(format!("itm_{i:04}")),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some(format!("Subject {i}")),
                    from_addr: Some(format!("u{i}@example.com")),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(i as i64),
                    ..Default::default()
                })
                .expect("insert");
        }
        set.id
    }

    #[test]
    fn queue_page_1k_total_and_disjoint_pages() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "Queue1k").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            seed_1k(&matter);
        }
        let page0 = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(50),
            offset: Some(0),
            extras: Some(false),
        })
        .expect("page0");
        assert_eq!(page0.total, 1000);
        assert_eq!(page0.rows.len(), 50);
        assert!(!page0.extras);
        for row in &page0.rows {
            assert!(!row.withhold);
            assert!(row.custodian.is_none());
        }

        let page1 = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(50),
            offset: Some(50),
            extras: Some(false),
        })
        .expect("page1");
        assert_eq!(page1.total, 1000);
        assert_eq!(page1.rows.len(), 50);
        let ids0: HashSet<_> = page0.rows.iter().map(|r| r.id.clone()).collect();
        let ids1: HashSet<_> = page1.rows.iter().map(|r| r.id.clone()).collect();
        assert!(ids0.is_disjoint(&ids1));
        assert_eq!(page0.rows[0].id, "itm_0000");
        assert_eq!(page0.rows[0].review_order, Some(0));
    }

    #[test]
    fn empty_matter_total_zero() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueEmpty").expect("create");
        let page = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: None,
            offset: None,
            extras: None,
        })
        .expect("page");
        assert_eq!(page.total, 0);
        assert!(page.rows.is_empty());
    }

    #[test]
    fn insert_source_only_total_zero() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueSrcOnly").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            matter
                .insert_source(r"C:\exports\synth", "folder", "imported", None)
                .expect("source");
        }
        let page = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(50),
            offset: Some(0),
            extras: Some(false),
        })
        .expect("page");
        assert_eq!(page.total, 0);
    }

    #[test]
    fn unreviewed_filter_excludes_coded() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueUncoded").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            for (i, subj) in ["coded", "bare1", "bare2"].iter().enumerate() {
                matter
                    .insert_item(ItemInput {
                        id: Some(format!("itm_{i:04}")),
                        status: item_status::EXTRACTED.into(),
                        role: Some(item_role::STANDALONE.into()),
                        subject: Some((*subj).into()),
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
            matter
                .apply_codes(ApplyCodesInput {
                    item_ids: vec!["itm_0000".into()],
                    add_code_ids: vec![priv_id],
                    remove_code_ids: vec![],
                    propagate_family: false,
                    actor: "chrome-test".into(),
                    expected_version: None,
                })
                .expect("code");
        }
        let filter_json = serde_json::to_string(&FilterSpec::preset_uncoded()).expect("json");
        let page = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: Some(filter_json),
            keyword: None,
            limit: Some(50),
            offset: Some(0),
            extras: Some(false),
        })
        .expect("page");
        assert_eq!(page.total, 2);
        assert_eq!(page.rows.len(), 2);
    }

    #[test]
    fn family_size_is_three_not_attachment_count() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueFam").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            let family = matter.insert_family("").expect("family");
            let p = matter
                .insert_item(ItemInput {
                    id: Some("itm_0000".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::PARENT.into()),
                    family_id: Some(family.id.clone()),
                    subject: Some("Parent".into()),
                    attachment_count: Some(99),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(0),
                    ..Default::default()
                })
                .expect("parent");
            for (i, name) in ["A", "B"].iter().enumerate() {
                matter
                    .insert_item(ItemInput {
                        id: Some(format!("itm_{:04}", i + 1)),
                        status: item_status::EXTRACTED.into(),
                        role: Some(item_role::ATTACHMENT.into()),
                        family_id: Some(family.id.clone()),
                        parent_item_id: Some(p.id.clone()),
                        subject: Some((*name).into()),
                        in_review: Some(1),
                        review_set_id: Some(set.id.clone()),
                        review_order: Some((i + 1) as i64),
                        ..Default::default()
                    })
                    .expect("child");
            }
        }
        let page = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(50),
            offset: Some(0),
            extras: Some(false),
        })
        .expect("page");
        assert_eq!(page.total, 3);
        let parent_row = page.rows.iter().find(|r| r.id == "itm_0000").expect("p");
        assert_eq!(parent_row.family_size, 3);
    }

    #[test]
    fn extras_fills_withhold_and_custodian() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueExtras").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_0000".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("X".into()),
                    custodian: Some("Alice".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(0),
                    ..Default::default()
                })
                .expect("item");
            matter
                .upsert_item_privilege(UpsertItemPrivilegeInput {
                    item_id: "itm_0000".into(),
                    basis: "attorney_client".into(),
                    description: "advice".into(),
                    status: "asserted".into(),
                    withhold: true,
                    include_on_log: true,
                    actor: "chrome-test".into(),
                    expected_version: None,
                })
                .expect("priv");
        }
        let off = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(10),
            offset: Some(0),
            extras: Some(false),
        })
        .expect("off");
        assert!(!off.rows[0].withhold);
        assert!(off.rows[0].custodian.is_none());

        let on = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(10),
            offset: Some(0),
            extras: Some(true),
        })
        .expect("on");
        assert!(on.extras);
        assert!(on.rows[0].withhold);
        assert_eq!(on.rows[0].custodian.as_deref(), Some("Alice"));
    }

    #[test]
    fn privilege_coded_and_resp_mapping() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueCodesMap").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            for (i, subj) in ["priv", "resp", "bare"].iter().enumerate() {
                matter
                    .insert_item(ItemInput {
                        id: Some(format!("itm_{i:04}")),
                        status: item_status::EXTRACTED.into(),
                        role: Some(item_role::STANDALONE.into()),
                        subject: Some((*subj).into()),
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
            let nr_id = defs
                .iter()
                .find(|d| d.key == "not_responsive")
                .expect("nr")
                .id
                .clone();
            let nsl_id = defs
                .iter()
                .find(|d| d.key == "needs_second_look")
                .expect("nsl")
                .id
                .clone();
            matter
                .apply_codes(ApplyCodesInput {
                    item_ids: vec!["itm_0000".into()],
                    add_code_ids: vec![priv_id],
                    remove_code_ids: vec![],
                    propagate_family: false,
                    actor: "chrome-test".into(),
                    expected_version: None,
                })
                .expect("priv code");
            matter
                .apply_codes(ApplyCodesInput {
                    item_ids: vec!["itm_0001".into()],
                    add_code_ids: vec![resp_id],
                    remove_code_ids: vec![],
                    propagate_family: false,
                    actor: "chrome-test".into(),
                    expected_version: None,
                })
                .expect("resp code");
            // Extra items for NR / NSL mapping coverage.
            matter
                .insert_item(ItemInput {
                    id: Some("itm_0003".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("nr".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(3),
                    ..Default::default()
                })
                .expect("nr item");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_0004".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("nsl".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(4),
                    ..Default::default()
                })
                .expect("nsl item");
            matter
                .apply_codes(ApplyCodesInput {
                    item_ids: vec!["itm_0003".into()],
                    add_code_ids: vec![nr_id],
                    remove_code_ids: vec![],
                    propagate_family: false,
                    actor: "chrome-test".into(),
                    expected_version: None,
                })
                .expect("nr code");
            matter
                .apply_codes(ApplyCodesInput {
                    item_ids: vec!["itm_0004".into()],
                    add_code_ids: vec![nsl_id],
                    remove_code_ids: vec![],
                    propagate_family: false,
                    actor: "chrome-test".into(),
                    expected_version: None,
                })
                .expect("nsl code");
        }
        let page = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(50),
            offset: Some(0),
            extras: Some(false),
        })
        .expect("page");
        let by_id: HashMap<_, _> = page.rows.into_iter().map(|r| (r.id.clone(), r)).collect();
        let priv_row = by_id.get("itm_0000").expect("priv row");
        assert!(priv_row.privilege_coded);
        assert!(!priv_row.withhold, "extras=false keeps withhold false");
        assert!(priv_row.custodian.is_none());
        assert_ne!(priv_row.resp.as_deref(), Some("REDACT"));
        assert_ne!(priv_row.resp.as_deref(), Some("WITHHOLD"));

        let resp_row = by_id.get("itm_0001").expect("resp row");
        assert_eq!(resp_row.resp.as_deref(), Some("R"));
        assert!(!resp_row.privilege_coded);

        let bare = by_id.get("itm_0002").expect("bare");
        assert!(!bare.privilege_coded);
        assert!(bare.resp.is_none());

        assert_eq!(
            by_id.get("itm_0003").expect("nr").resp.as_deref(),
            Some("NR")
        );
        assert_eq!(
            by_id.get("itm_0004").expect("nsl").resp.as_deref(),
            Some("NSL")
        );
    }

    #[test]
    fn unknown_responsiveness_key_maps_to_none() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueUnkResp").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter.seed_default_codes().expect("seed");
            let custom_id = matter
                .upsert_code_definition(CodeDefInput {
                    id: None,
                    key: Some("maybe_responsive".into()),
                    label: "Maybe".into(),
                    group_key: "responsiveness".into(),
                    cardinality: "single".into(),
                    color: None,
                    sort_order: 99,
                    is_active: true,
                    guidance: None,
                })
                .expect("custom");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_0000".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("custom".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(0),
                    ..Default::default()
                })
                .expect("item");
            matter
                .apply_codes(ApplyCodesInput {
                    item_ids: vec!["itm_0000".into()],
                    add_code_ids: vec![custom_id],
                    remove_code_ids: vec![],
                    propagate_family: false,
                    actor: "chrome-test".into(),
                    expected_version: None,
                })
                .expect("code");
        }
        let page = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(10),
            offset: Some(0),
            extras: Some(false),
        })
        .expect("page");
        assert_eq!(page.rows.len(), 1);
        assert!(
            page.rows[0].resp.is_none(),
            "unknown responsiveness key must map to None (UI —), got {:?}",
            page.rows[0].resp
        );
    }

    #[test]
    fn limit_over_500_failed() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueLimit").expect("create");
        let err = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(501),
            offset: Some(0),
            extras: None,
        })
        .expect_err("limit");
        assert_eq!(err.kind, "failed");
    }

    #[test]
    fn keyword_without_index_fts_unavailable() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueFts").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            seed_1k(&matter);
        }
        let err = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: Some("contract".into()),
            limit: Some(50),
            offset: Some(0),
            extras: None,
        })
        .expect_err("fts");
        assert_eq!(err.kind, "fts_unavailable");
        assert!(err.message.to_lowercase().contains("fts_index"));
    }

    #[test]
    fn encrypted_kind_without_open() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = parent.join("EncQueue");
        {
            let _m = Matter::create_encrypted(&root, "EncQueue", "test-passphrase-0111")
                .expect("create encrypted");
        }
        let err = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(10),
            offset: Some(0),
            extras: None,
        })
        .expect_err("encrypted");
        assert_eq!(err.kind, "encrypted");
    }

    #[test]
    fn null_family_id_size_is_one() {
        let tmp = tempdir().expect("tempdir");
        let parent = utf8_tmp(&tmp);
        let root = create_matter_under(&parent, "QueueNullFam").expect("create");
        {
            let matter = Matter::open(&root).expect("open");
            let set = matter
                .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
                .expect("set");
            matter
                .insert_item(ItemInput {
                    id: Some("itm_0000".into()),
                    status: item_status::EXTRACTED.into(),
                    role: Some(item_role::STANDALONE.into()),
                    subject: Some("Solo".into()),
                    in_review: Some(1),
                    review_set_id: Some(set.id.clone()),
                    review_order: Some(0),
                    ..Default::default()
                })
                .expect("item");
        }
        let page = review_queue_page_blocking(ReviewQueuePageArgs {
            root: root.to_string(),
            filter_json: None,
            keyword: None,
            limit: Some(10),
            offset: Some(0),
            extras: Some(false),
        })
        .expect("page");
        assert_eq!(page.rows[0].family_size, 1);
        assert!(page.rows[0].family_id.is_none());
    }
}
