//! `Matter::family_members_thin` — capped family card rows (track 0112).

use camino::Utf8PathBuf;
use matter_core::{item_role, item_status, Error, ItemInput, Matter};
use tempfile::TempDir;

fn utf8_tempdir() -> (TempDir, Utf8PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let base = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    (tmp, base)
}

#[test]
fn family_members_thin_parent_and_two_children() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("family-thin");
    let matter = Matter::create(&root, "Family Thin").expect("create");
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

    let thin = matter.family_members_thin(&family.id, 100).expect("thin");
    assert_eq!(thin.members.len(), 3);
    assert!(!thin.truncated);
    let subjects: Vec<Option<String>> = thin.members.iter().map(|m| m.subject.clone()).collect();
    assert!(subjects.contains(&Some("Parent".into())));
    assert!(subjects.contains(&Some("A".into())));
    assert!(subjects.contains(&Some("B".into())));
}

#[test]
fn family_members_thin_orphan_family_id_lists_items() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("family-orphan");
    let matter = Matter::create(&root, "Family Orphan").expect("create");
    let family = matter.insert_family("").expect("family");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            subject: Some("Orphan parent".into()),
            ..Default::default()
        })
        .expect("parent");
    let fid = "fam_orphan_no_row".to_string();
    matter
        .connection()
        .pragma_update(None, "foreign_keys", false)
        .expect("fk off");
    matter
        .connection()
        .execute(
            "UPDATE items SET family_id = ?1 WHERE id = ?2",
            [fid.as_str(), parent.id.as_str()],
        )
        .expect("orphan family_id");

    match matter.list_family_members(&fid) {
        Err(Error::FamilyNotFound(_)) => {}
        other => panic!("expected FamilyNotFound from list_family_members, got {other:?}"),
    }

    let thin = matter.family_members_thin(&fid, 100).expect("thin orphan");
    assert_eq!(thin.members.len(), 1);
    assert!(!thin.truncated);
    assert_eq!(thin.members[0].subject.as_deref(), Some("Orphan parent"));
}
