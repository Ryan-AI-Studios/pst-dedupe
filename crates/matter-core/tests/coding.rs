//! Integration tests for coding / tags (schema v8 / track 0027).

use matter_core::{
    item_role, item_status, ApplyCodesInput, CodeDefInput, Error, ItemInput, Matter,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

fn defs_by_key(matter: &Matter) -> std::collections::HashMap<String, matter_core::CodeDef> {
    matter
        .list_code_definitions()
        .expect("defs")
        .into_iter()
        .map(|d| (d.key.clone(), d))
        .collect()
}

#[test]
fn coding_seed_defaults_idempotent() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-seed");
    let matter = Matter::create(&root, "Coding Seed").expect("create");

    let defs1 = matter.list_code_definitions().expect("defs1");
    assert_eq!(defs1.len(), 6);
    let keys: Vec<_> = defs1.iter().map(|d| d.key.as_str()).collect();
    for k in [
        "responsive",
        "not_responsive",
        "needs_second_look",
        "privilege",
        "hot",
        "confidential",
    ] {
        assert!(keys.contains(&k), "missing {k}");
    }

    matter.seed_default_codes().expect("reseed");
    let defs2 = matter.list_code_definitions().expect("defs2");
    assert_eq!(defs2.len(), 6);

    let err = matter
        .upsert_code_definition(CodeDefInput {
            id: None,
            key: Some("responsive".into()),
            label: "Dup".into(),
            group_key: "responsiveness".into(),
            cardinality: "single".into(),
            color: None,
            sort_order: 99,
            is_active: true,
        })
        .expect_err("dup key");
    assert!(err.to_string().contains("already exists"), "got {err}");
}

#[test]
fn coding_single_group_mutual_exclusion() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-single");
    let matter = Matter::create(&root, "Coding Single").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Doc".into()),
            ..Default::default()
        })
        .expect("item");
    let by_key = defs_by_key(&matter);

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.id.clone()],
            add_code_ids: vec![by_key["responsive"].id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("add responsive");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.id.clone()],
            add_code_ids: vec![by_key["not_responsive"].id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("add not_responsive");

    let codes = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes");
    let keys: Vec<_> = codes[&item.id].iter().map(|c| c.key.as_str()).collect();
    assert_eq!(keys, vec!["not_responsive"]);
}

#[test]
fn coding_multi_group_allows_both_issues() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-multi");
    let matter = Matter::create(&root, "Coding Multi").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Doc".into()),
            ..Default::default()
        })
        .expect("item");
    let by_key = defs_by_key(&matter);

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.id.clone()],
            add_code_ids: vec![by_key["hot"].id.clone(), by_key["confidential"].id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("add issues");

    let codes = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes");
    let mut keys: Vec<_> = codes[&item.id].iter().map(|c| c.key.clone()).collect();
    keys.sort();
    assert_eq!(keys, vec!["confidential".to_string(), "hot".to_string()]);
}

#[test]
fn coding_batch_add_and_audit_full_ids() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-batch");
    let matter = Matter::create(&root, "Coding Batch").expect("create");
    let mut ids = Vec::new();
    for i in 0..3 {
        let it = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some(format!("Item {i}")),
                ..Default::default()
            })
            .expect("item");
        ids.push(it.id);
    }
    let by_key = defs_by_key(&matter);

    let result = matter
        .apply_codes(ApplyCodesInput {
            item_ids: ids.clone(),
            add_code_ids: vec![by_key["privilege"].id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("batch add");
    assert_eq!(result.target_count, 3);
    assert_eq!(result.selected_count, 3);

    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(result.target_item_ids, expected);

    for id in &ids {
        let codes = matter
            .list_item_codes(std::slice::from_ref(&id))
            .expect("codes");
        assert_eq!(codes[id].len(), 1);
        assert_eq!(codes[id][0].key, "privilege");
    }

    let (params, entity): (String, String) = matter
        .connection()
        .query_row(
            "SELECT params_json, entity FROM audit_events \
             WHERE action = 'coding.apply' ORDER BY seq DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("audit");
    assert_eq!(entity, "batch");
    let v: serde_json::Value = serde_json::from_str(&params).expect("json");
    let arr = v["item_ids"].as_array().expect("item_ids array");
    assert_eq!(arr.len(), 3);
    let mut audited: Vec<String> = arr
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    audited.sort();
    assert_eq!(audited, expected);
    assert_eq!(v["selected_count"], 3);
    assert_eq!(v["target_count"], 3);
    assert_eq!(v["propagate_family"], false);
    assert!(v["add"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x == "privilege"));
}

#[test]
fn coding_batch_remove() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-remove");
    let matter = Matter::create(&root, "Coding Remove").expect("create");
    let mut ids = Vec::new();
    for i in 0..3 {
        let it = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some(format!("Item {i}")),
                ..Default::default()
            })
            .expect("item");
        ids.push(it.id);
    }
    let by_key = defs_by_key(&matter);
    let hot = by_key["hot"].id.clone();

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: ids.clone(),
            add_code_ids: vec![hot.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("add");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: ids.clone(),
            add_code_ids: vec![],
            remove_code_ids: vec![hot],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("remove");

    for id in &ids {
        let codes = matter
            .list_item_codes(std::slice::from_ref(&id))
            .expect("codes");
        assert!(codes[id].is_empty(), "expected uncoded for hot");
    }
}

#[test]
fn coding_batch_transaction_no_partial_on_error() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-txn");
    let matter = Matter::create(&root, "Coding Txn").expect("create");
    let good = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Good".into()),
            ..Default::default()
        })
        .expect("good");
    let by_key = defs_by_key(&matter);

    let err = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![good.id.clone(), "itm_missing_xyz".into()],
            add_code_ids: vec![by_key["hot"].id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect_err("must fail");
    assert!(matches!(err, Error::ItemNotFound(_)));

    let codes = matter
        .list_item_codes(std::slice::from_ref(&good.id))
        .expect("codes");
    assert!(codes[&good.id].is_empty(), "no partial codes");

    let err2 = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![good.id.clone()],
            add_code_ids: vec!["cde_does_not_exist".into()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect_err("bad code");
    assert!(err2.to_string().contains("not found"));
    let codes2 = matter
        .list_item_codes(std::slice::from_ref(&good.id))
        .expect("codes2");
    assert!(codes2[&good.id].is_empty());
}

#[test]
fn coding_propagate_family_parent_and_siblings() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-family");
    let matter = Matter::create(&root, "Coding Family").expect("create");
    let family = matter.insert_family("").expect("family");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            subject: Some("Parent".into()),
            ..Default::default()
        })
        .expect("parent");
    let att_a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("A".into()),
            ..Default::default()
        })
        .expect("a");
    let att_b = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("B".into()),
            ..Default::default()
        })
        .expect("b");
    let by_key = defs_by_key(&matter);
    let priv_id = by_key["privilege"].id.clone();

    // Parent selected + propagate → parent + all children.
    let r = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![parent.id.clone()],
            add_code_ids: vec![priv_id.clone()],
            remove_code_ids: vec![],
            propagate_family: true,
            actor: "tester".into(),
        })
        .expect("prop parent");
    assert_eq!(r.target_count, 3);
    for id in [&parent.id, &att_a.id, &att_b.id] {
        let codes = matter
            .list_item_codes(std::slice::from_ref(&id))
            .expect("c");
        assert_eq!(codes[id][0].key, "privilege");
    }

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![parent.id.clone(), att_a.id.clone(), att_b.id.clone()],
            add_code_ids: vec![],
            remove_code_ids: vec![priv_id.clone()],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("clear");

    // One attachment + propagate → parent + that attachment AND all siblings.
    let r2 = matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![att_a.id.clone()],
            add_code_ids: vec![priv_id.clone()],
            remove_code_ids: vec![],
            propagate_family: true,
            actor: "tester".into(),
        })
        .expect("prop attachment");
    assert_eq!(r2.target_count, 3);
    for id in [&parent.id, &att_a.id, &att_b.id] {
        let codes = matter
            .list_item_codes(std::slice::from_ref(&id))
            .expect("c");
        assert_eq!(codes[id][0].key, "privilege", "sibling {id} must be coded");
    }

    // propagate false does not expand.
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![parent.id.clone(), att_a.id.clone(), att_b.id.clone()],
            add_code_ids: vec![],
            remove_code_ids: vec![priv_id.clone()],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("clear2");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![att_a.id.clone()],
            add_code_ids: vec![priv_id],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("no prop");
    assert_eq!(
        matter
            .list_item_codes(std::slice::from_ref(&att_a.id))
            .unwrap()[&att_a.id]
            .len(),
        1
    );
    assert!(matter
        .list_item_codes(std::slice::from_ref(&parent.id))
        .unwrap()[&parent.id]
        .is_empty());
    assert!(matter
        .list_item_codes(std::slice::from_ref(&att_b.id))
        .unwrap()[&att_b.id]
        .is_empty());
}

#[test]
fn coding_near_dup_peer_not_auto_coded() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-neardup");
    let matter = Matter::create(&root, "Coding NearDup").expect("create");
    let a = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("A".into()),
            near_dup_group_id: Some("ndg1".into()),
            ..Default::default()
        })
        .expect("a");
    let b = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("B".into()),
            near_dup_group_id: Some("ndg1".into()),
            ..Default::default()
        })
        .expect("b");
    let by_key = defs_by_key(&matter);

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![a.id.clone()],
            add_code_ids: vec![by_key["hot"].id.clone()],
            remove_code_ids: vec![],
            // Family expand still must not pull near-dup peers.
            propagate_family: true,
            actor: "tester".into(),
        })
        .expect("code a");

    assert_eq!(
        matter.list_item_codes(std::slice::from_ref(&a.id)).unwrap()[&a.id].len(),
        1
    );
    assert!(
        matter.list_item_codes(std::slice::from_ref(&b.id)).unwrap()[&b.id].is_empty(),
        "near-dup peer must not auto-code"
    );
}

#[test]
fn coding_inactive_definition_still_displays_and_removable() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-inactive");
    let matter = Matter::create(&root, "Coding Inactive").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Doc".into()),
            ..Default::default()
        })
        .expect("item");
    let by_key = defs_by_key(&matter);
    let hot = by_key["hot"].clone();

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.id.clone()],
            add_code_ids: vec![hot.id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("add");

    matter
        .upsert_code_definition(CodeDefInput {
            id: Some(hot.id.clone()),
            key: None,
            label: hot.label.clone(),
            group_key: hot.group_key.clone(),
            cardinality: hot.cardinality.clone(),
            color: hot.color.clone(),
            sort_order: hot.sort_order,
            is_active: false,
        })
        .expect("deactivate");

    let codes = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes");
    assert_eq!(codes[&item.id].len(), 1);
    assert_eq!(codes[&item.id][0].key, "hot");
    assert_eq!(codes[&item.id][0].is_active, 0);

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.id.clone()],
            add_code_ids: vec![],
            remove_code_ids: vec![hot.id],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("remove");
    assert!(matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .unwrap()[&item.id]
        .is_empty());
}

#[test]
fn coding_audit_large_batch_full_item_ids() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-coding-large");
    let matter = Matter::create(&root, "Coding Large").expect("create");
    let mut ids = Vec::new();
    for i in 0..80 {
        let it = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                role: Some(item_role::STANDALONE.into()),
                subject: Some(format!("Item {i}")),
                ..Default::default()
            })
            .expect("item");
        ids.push(it.id);
    }
    let by_key = defs_by_key(&matter);

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: ids.clone(),
            add_code_ids: vec![by_key["confidential"].id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("large batch");

    let params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'coding.apply' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params).expect("json");
    let arr = v["item_ids"].as_array().expect("full item_ids");
    assert_eq!(
        arr.len(),
        80,
        "audit must list every target id, not a sample/hash"
    );
    assert_eq!(v["target_count"], 80);
    assert_eq!(v["selected_count"], 80);
}
