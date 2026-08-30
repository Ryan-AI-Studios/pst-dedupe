//! Produce checklist helpers (track 0113).

use matter_core::{
    item_role, item_status, ApplyCodesInput, FilterSpec, ItemInput, Matter, DEFAULT_REVIEW_SET_NAME,
};
use tempfile::tempdir;

fn utf8_tmp(tmp: &tempfile::TempDir) -> camino::Utf8PathBuf {
    camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
}

fn insert_review_item(matter: &Matter, input: ItemInput) -> String {
    let mut input = input;
    input.status = item_status::EXTRACTED.into();
    if input.role.is_none() {
        input.role = Some(item_role::STANDALONE.into());
    }
    input.in_review = Some(1);
    matter.insert_item(input).expect("insert").id
}

fn seed_codes_responsive(matter: &Matter, item_id: &str) {
    matter.seed_default_codes().expect("seed");
    let resp = matter
        .list_code_definitions()
        .expect("defs")
        .into_iter()
        .find(|d| d.key == "responsive")
        .expect("responsive")
        .id;
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item_id.into()],
            add_code_ids: vec![resp],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "test".into(),
            expected_version: None,
        })
        .expect("apply");
}

fn insert_production_set(
    matter: &Matter,
    id: &str,
    status: &str,
    created_at: &str,
    prefix: &str,
    next_seq: i64,
) {
    matter
        .connection()
        .execute(
            "INSERT INTO production_sets \
             (id, matter_id, name, created_at, updated_at, bates_prefix, next_seq, status) \
             VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, matter.id(), id, created_at, prefix, next_seq, status],
        )
        .expect("insert set");
}

fn insert_production_item(
    matter: &Matter,
    set_id: &str,
    item_id: &str,
    control: &str,
    status: &str,
    produced_at: &str,
) {
    matter
        .connection()
        .execute(
            "INSERT INTO production_items \
             (production_set_id, item_id, control_number, status, produced_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![set_id, item_id, control, status, produced_at],
        )
        .expect("insert pi");
}

#[test]
fn preset_produce_responsive_filters_code_and_withhold() {
    let tmp = tempdir().expect("tempdir");
    let root = utf8_tmp(&tmp).join("preset");
    let matter = Matter::create(&root, "Preset").expect("create");
    let set = matter
        .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
        .expect("set");

    let keep = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_keep".into()),
            subject: Some("Keep".into()),
            review_set_id: Some(set.id.clone()),
            review_order: Some(0),
            ..Default::default()
        },
    );
    let withheld = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_hold".into()),
            subject: Some("Hold".into()),
            review_set_id: Some(set.id.clone()),
            review_order: Some(1),
            ..Default::default()
        },
    );
    let uncoded = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_none".into()),
            subject: Some("None".into()),
            review_set_id: Some(set.id),
            review_order: Some(2),
            ..Default::default()
        },
    );
    seed_codes_responsive(&matter, &keep);
    seed_codes_responsive(&matter, &withheld);
    matter
        .upsert_item_privilege(matter_core::UpsertItemPrivilegeInput {
            item_id: withheld.clone(),
            basis: "attorney_client".into(),
            description: "held".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "t".into(),
            expected_version: None,
        })
        .expect("withhold");

    let mut spec = FilterSpec::preset_produce_responsive();
    spec.include_family = false;
    let ids = matter.list_item_ids_filtered(&spec).expect("ids");
    assert_eq!(ids, vec![keep.clone()]);
    assert!(!ids.contains(&withheld));
    assert!(!ids.contains(&uncoded));
}

#[test]
fn list_item_ids_filtered_matches_thin_order() {
    let tmp = tempdir().expect("tempdir");
    let root = utf8_tmp(&tmp).join("ids");
    let matter = Matter::create(&root, "Ids").expect("create");
    let set = matter
        .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
        .expect("set");
    for (id, order) in [("itm_b", 1), ("itm_a", 0), ("itm_c", 2)] {
        insert_review_item(
            &matter,
            ItemInput {
                id: Some(id.into()),
                subject: Some(id.into()),
                review_set_id: Some(set.id.clone()),
                review_order: Some(order),
                ..Default::default()
            },
        );
    }
    let spec = FilterSpec::review_corpus();
    let ids = matter.list_item_ids_filtered(&spec).expect("ids");
    let thin = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("thin");
    let thin_ids: Vec<_> = thin.into_iter().map(|r| r.id).collect();
    assert_eq!(ids, thin_ids);
    assert_eq!(ids, vec!["itm_a", "itm_b", "itm_c"]);
}

#[test]
fn order_ids_family_together_first_occurrence_not_family_id_sort() {
    let tmp = tempdir().expect("tempdir");
    let root = utf8_tmp(&tmp).join("fam");
    let matter = Matter::create(&root, "Fam").expect("create");
    let fam_zzz = matter.insert_family("").expect("zzz");
    let fam_aaa = matter.insert_family("").expect("aaa");
    // Force opaque ids that would scramble if sorted as strings: zzz first in input.
    let zzz_id = if fam_zzz.id > fam_aaa.id {
        fam_zzz.id.clone()
    } else {
        fam_aaa.id.clone()
    };
    let aaa_id = if fam_zzz.id > fam_aaa.id {
        fam_aaa.id.clone()
    } else {
        fam_zzz.id.clone()
    };
    assert!(zzz_id > aaa_id, "need lexicographically later family first");

    let zp = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_zp".into()),
            role: Some(item_role::PARENT.into()),
            family_id: Some(zzz_id.clone()),
            review_order: Some(0),
            ..Default::default()
        },
    );
    let zc = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_zc".into()),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(zzz_id),
            parent_item_id: Some(zp.clone()),
            review_order: Some(1),
            ..Default::default()
        },
    );
    let ap = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_ap".into()),
            role: Some(item_role::PARENT.into()),
            family_id: Some(aaa_id.clone()),
            review_order: Some(2),
            ..Default::default()
        },
    );
    let ac = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_ac".into()),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(aaa_id),
            parent_item_id: Some(ap.clone()),
            review_order: Some(3),
            ..Default::default()
        },
    );
    let solo = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_solo".into()),
            family_id: None,
            review_order: Some(4),
            ..Default::default()
        },
    );

    // Child of later-lex family appears first; parent-first within family still applies.
    let input = vec![
        zc.clone(),
        zp.clone(),
        ac.clone(),
        ap.clone(),
        solo.clone(),
        "itm_unknown".into(),
    ];
    let ordered = matter.order_ids_family_together(&input).expect("order");
    assert_eq!(
        ordered,
        vec![zp, zc, ap, ac, solo],
        "first-occurrence family order; do not sort family_id; drop unknown"
    );
}

#[test]
fn count_produced_items_distinct_complete_with_errors_excludes_failed() {
    let tmp = tempdir().expect("tempdir");
    let root = utf8_tmp(&tmp).join("cnt");
    let matter = Matter::create(&root, "Cnt").expect("create");
    let a = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_a".into()),
            ..Default::default()
        },
    );
    let b = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_b".into()),
            ..Default::default()
        },
    );
    insert_production_set(
        &matter,
        "ps_complete",
        "complete",
        "2026-01-01T00:00:00Z",
        "PROD",
        3,
    );
    insert_production_set(
        &matter,
        "ps_err",
        "complete_with_errors",
        "2026-01-02T00:00:00Z",
        "PROD",
        4,
    );
    insert_production_set(
        &matter,
        "ps_fail",
        "failed",
        "2026-01-03T00:00:00Z",
        "PROD",
        9,
    );
    insert_production_set(
        &matter,
        "ps_partial",
        "partial",
        "2026-01-04T00:00:00Z",
        "PROD",
        2,
    );
    insert_production_item(
        &matter,
        "ps_complete",
        &a,
        "PROD000001",
        "ok",
        "2026-01-01T01:00:00Z",
    );
    insert_production_item(
        &matter,
        "ps_err",
        &a,
        "PROD000002",
        "ok",
        "2026-01-02T01:00:00Z",
    );
    insert_production_item(
        &matter,
        "ps_err",
        &b,
        "PROD000003",
        "ok",
        "2026-01-02T01:00:01Z",
    );
    insert_production_item(
        &matter,
        "ps_fail",
        &b,
        "PROD000099",
        "ok",
        "2026-01-03T01:00:00Z",
    );
    insert_production_item(
        &matter,
        "ps_partial",
        &b,
        "PROD000050",
        "ok",
        "2026-01-04T01:00:00Z",
    );

    let n = matter.count_produced_items().expect("count");
    assert_eq!(
        n, 2,
        "DISTINCT a+b; failed/partial excluded; errors included"
    );
}

#[test]
fn latest_control_number_skips_skip_prefix_and_failed() {
    let tmp = tempdir().expect("tempdir");
    let root = utf8_tmp(&tmp).join("bates");
    let matter = Matter::create(&root, "Bates").expect("create");
    let item = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_x".into()),
            ..Default::default()
        },
    );
    insert_production_set(
        &matter,
        "ps_old",
        "complete",
        "2026-01-01T00:00:00Z",
        "PROD",
        2,
    );
    insert_production_set(
        &matter,
        "ps_skip",
        "complete",
        "2026-01-02T00:00:00Z",
        "PROD",
        3,
    );
    insert_production_set(
        &matter,
        "ps_fail",
        "failed",
        "2026-01-03T00:00:00Z",
        "PROD",
        4,
    );
    insert_production_set(
        &matter,
        "ps_new",
        "complete_with_errors",
        "2026-01-04T00:00:00Z",
        "PROD",
        5,
    );
    insert_production_item(
        &matter,
        "ps_old",
        &item,
        "PROD000001",
        "ok",
        "2026-01-01T01:00:00Z",
    );
    insert_production_item(
        &matter,
        "ps_skip",
        &item,
        "SKIP_NATIVE",
        "ok",
        "2026-01-02T01:00:00Z",
    );
    insert_production_item(
        &matter,
        "ps_fail",
        &item,
        "PROD000088",
        "ok",
        "2026-01-03T01:00:00Z",
    );
    insert_production_item(
        &matter,
        "ps_new",
        &item,
        "PROD000010",
        "ok",
        "2026-01-04T01:00:00Z",
    );

    let cn = matter
        .latest_control_number(&item)
        .expect("latest")
        .expect("some");
    assert_eq!(cn, "PROD000010");
    assert!(matter
        .latest_control_number("itm_missing")
        .expect("missing")
        .is_none());
}

#[test]
fn count_privilege_log_blank_descriptions_is_read_only() {
    let tmp = tempdir().expect("tempdir");
    let root = utf8_tmp(&tmp).join("blank");
    let matter = Matter::create(&root, "Blank").expect("create");
    let set = matter
        .ensure_default_review_set(DEFAULT_REVIEW_SET_NAME)
        .expect("set");
    let id = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_b".into()),
            review_set_id: Some(set.id),
            review_order: Some(0),
            ..Default::default()
        },
    );
    matter
        .upsert_item_privilege(matter_core::UpsertItemPrivilegeInput {
            item_id: id.clone(),
            basis: "attorney_client".into(),
            description: String::new(),
            status: "asserted".into(),
            withhold: false,
            include_on_log: true,
            actor: "t".into(),
            expected_version: None,
        })
        .expect("priv");
    let n = matter
        .count_privilege_log_blank_descriptions("review_corpus", Some(&[id]))
        .expect("count");
    assert_eq!(n, 1);
    let audits: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'privilege.log_export'",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    assert_eq!(audits, 0);
}

#[test]
fn list_production_sets_thin_counts_ok_rows() {
    let tmp = tempdir().expect("tempdir");
    let root = utf8_tmp(&tmp).join("sets");
    let matter = Matter::create(&root, "Sets").expect("create");
    let item = insert_review_item(
        &matter,
        ItemInput {
            id: Some("itm_y".into()),
            ..Default::default()
        },
    );
    insert_production_set(
        &matter,
        "ps_a",
        "complete",
        "2026-01-01T00:00:00Z",
        "PROD",
        2,
    );
    insert_production_item(
        &matter,
        "ps_a",
        &item,
        "PROD000001",
        "ok",
        "2026-01-01T01:00:00Z",
    );
    let sets = matter.list_production_sets_thin().expect("list");
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].id, "ps_a");
    assert_eq!(sets[0].produced_ok_count, 1);
    assert_eq!(sets[0].bates_prefix, "PROD");
    assert_eq!(sets[0].next_seq, 2);
}
