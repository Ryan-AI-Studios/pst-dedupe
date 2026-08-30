//! `Matter::review_neighbors` — sort-key prev/next/position (track 0112).

use camino::Utf8PathBuf;
use matter_core::{
    item_role, item_status, ApplyCodesInput, Error, FilterSpec, ItemInput, ItemUpdate, Matter,
    DEFAULT_REVIEW_SET_NAME,
};
use tempfile::TempDir;

fn utf8_tempdir() -> (TempDir, Utf8PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    (tmp, base)
}

fn seed_three_review_items(matter: &Matter) {
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
        .expect("a");
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
        .expect("b");
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

#[test]
fn review_neighbors_order_and_dropout() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("neighbors");
    let matter = Matter::create(&root, "Neighbors").expect("create");
    seed_three_review_items(&matter);
    let spec = FilterSpec::preset_uncoded();

    let n0 = matter
        .review_neighbors("itm_0000", &spec, None)
        .expect("n0");
    assert_eq!(n0.prev_id, None);
    assert_eq!(n0.next_id.as_deref(), Some("itm_0001"));
    assert_eq!(n0.position, 1);
    assert_eq!(n0.total, 3);

    let n1 = matter
        .review_neighbors("itm_0001", &spec, None)
        .expect("n1");
    assert_eq!(n1.prev_id.as_deref(), Some("itm_0000"));
    assert_eq!(n1.next_id.as_deref(), Some("itm_0002"));
    assert_eq!(n1.position, 2);
    assert_eq!(n1.total, 3);

    let defs = matter.list_code_definitions().expect("defs");
    let resp_id = defs
        .iter()
        .find(|d| d.key == "responsive")
        .expect("resp")
        .id
        .clone();
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec!["itm_0000".into()],
            add_code_ids: vec![resp_id],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "test".into(),
            expected_version: None,
        })
        .expect("code");

    let after = matter
        .review_neighbors("itm_0000", &spec, None)
        .expect("after");
    assert_eq!(after.next_id.as_deref(), Some("itm_0001"));
    assert_ne!(after.position, 0, "dropout must not zero position");
    assert_eq!(after.total, 2);
}

#[test]
fn review_neighbors_missing_id() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("neighbors-missing");
    let matter = Matter::create(&root, "Neighbors Missing").expect("create");
    let err = matter
        .review_neighbors("itm_nope", &FilterSpec::review_corpus(), None)
        .expect_err("missing");
    match err {
        Error::ItemNotFound(id) => assert_eq!(id, "itm_nope"),
        other => panic!("expected ItemNotFound, got {other:?}"),
    }
}
