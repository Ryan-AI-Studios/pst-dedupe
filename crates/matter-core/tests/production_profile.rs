//! Production profiles (track 0060 / schema v38).

use matter_core::{
    builtin_production_profiles, default_production_profile_body, parse_production_profile_body,
    production_profile_body_to_json, Matter, ProductionProfileInput,
    BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1, RESERVED_PRODUCTION_PROFILE_SLUGS, SCHEMA_VERSION,
};
use serde_json::json;

fn open_matter() -> (tempfile::TempDir, Matter) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("m");
    let path = camino::Utf8PathBuf::from_path_buf(root).expect("utf8");
    let matter = Matter::create(&path, "prod-profile-test").expect("create");
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 38);
    (dir, matter)
}

#[test]
fn schema_v38_and_builtin_list() {
    let (_d, matter) = open_matter();
    let list = matter.list_production_profiles().expect("list");
    assert!(list.len() >= 3);
    assert!(list.iter().all(|p| {
        if p.is_builtin {
            p.matter_id.is_none()
        } else {
            true
        }
    }));
    let def = matter
        .get_production_profile(BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1)
        .expect("default");
    assert!(def.is_builtin);
    assert_eq!(def.slug, BUILTIN_US_CONCORDANCE_NATIVE_TEXT_V1);
    assert_eq!(builtin_production_profiles().len(), 3);
}

#[test]
fn upsert_list_delete_user_profile() {
    let (_d, matter) = open_matter();
    let body = default_production_profile_body();
    let body_json = production_profile_body_to_json(&body).expect("json");
    let p = matter
        .upsert_production_profile(ProductionProfileInput {
            id: None,
            slug: "firm_custom_v1".into(),
            label: "Firm custom".into(),
            jurisdiction_tag: Some("us_state".into()),
            body_json,
        })
        .expect("upsert");
    assert!(!p.is_builtin);
    assert_eq!(p.slug, "firm_custom_v1");

    let list = matter.list_production_profiles().expect("list");
    assert!(list.iter().any(|x| x.slug == "firm_custom_v1"));

    let got = matter
        .get_production_profile("firm_custom_v1")
        .expect("get by slug");
    assert_eq!(got.id, p.id);

    matter.delete_production_profile(&p.id).expect("delete");
    assert!(matter.get_production_profile("firm_custom_v1").is_err());
}

#[test]
fn reject_reserved_slug_and_start_at() {
    let (_d, matter) = open_matter();
    let body = default_production_profile_body();
    let body_json = production_profile_body_to_json(&body).expect("json");
    let err = matter
        .upsert_production_profile(ProductionProfileInput {
            id: None,
            slug: RESERVED_PRODUCTION_PROFILE_SLUGS[0].into(),
            label: "Nope".into(),
            jurisdiction_tag: None,
            body_json,
        })
        .expect_err("reserved");
    assert!(err.to_string().contains("reserved"));

    let mut val = serde_json::to_value(default_production_profile_body()).unwrap();
    val["bates"]["start_at"] = json!(1);
    let bad = serde_json::to_string(&val).unwrap();
    let err = parse_production_profile_body(&bad).expect_err("start_at");
    assert!(err.to_string().to_ascii_lowercase().contains("start"));
}
