//! `Matter::family_sizes` — batch COUNT(*) by family_id (track 0111).

use camino::Utf8PathBuf;
use matter_core::{item_role, item_status, ItemInput, Matter};
use tempfile::TempDir;

fn utf8_tempdir() -> (TempDir, Utf8PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    (tmp, base)
}

#[test]
fn family_sizes_counts_parent_and_two_children() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("family-sizes");
    let matter = Matter::create(&root, "Family Sizes").expect("create");
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
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("A".into()),
            ..Default::default()
        })
        .expect("a");
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("B".into()),
            ..Default::default()
        })
        .expect("b");

    let sizes = matter
        .family_sizes(std::slice::from_ref(&family.id))
        .expect("family_sizes");
    assert_eq!(sizes.get(&family.id), Some(&3));

    let empty = matter.family_sizes(&[]).expect("empty");
    assert!(empty.is_empty());

    let missing = matter
        .family_sizes(&["fam_does_not_exist".into()])
        .expect("missing");
    assert!(missing.is_empty());
}
