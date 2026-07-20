//! Processing profiles (track 0043 / schema v23).

use matter_core::{
    builtin_profile, builtin_profiles, expand_profile_stage, parse_profile_body,
    profile_stage_plan, Matter, ProcessingProfileInput, CANONICAL_STAGE_ORDER,
    PROFILE_BODY_MAX_BYTES, PROFILE_BODY_VERSION, SCHEMA_VERSION,
};
use tempfile::tempdir;

fn open_matter() -> (tempfile::TempDir, Matter) {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().join("m");
    let path = camino::Utf8PathBuf::from_path_buf(root).expect("utf8");
    let matter = Matter::create(&path, "profile-test").expect("create");
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    (dir, matter)
}

#[test]
fn builtins_list_includes_four() {
    let builtins = builtin_profiles();
    assert_eq!(builtins.len(), 4);
    let names: Vec<_> = builtins.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"standard"));
    assert!(names.contains(&"with_ocr"));
    assert!(names.contains(&"extract_only"));
    assert!(names.contains(&"reduce_only"));
    for p in &builtins {
        assert!(p.is_builtin);
        assert!(p.id.starts_with("builtin:"));
        assert!(p.matter_id.is_none());
    }
}

#[test]
fn standard_has_ocr_disabled_and_cumulative_reset() {
    let p = builtin_profile("standard").expect("standard");
    let ocr = p.body.stages.get("ocr").expect("ocr stage");
    assert!(!ocr.enabled, "standard OCR must be off");
    assert_eq!(
        ocr.params.get("enabled").and_then(|v| v.as_bool()),
        Some(false)
    );

    for kind in ["dedupe", "thread", "cull", "neardup", "fts_index"] {
        if let Some(spec) = p.body.stages.get(kind) {
            if let Some(reset) = spec.params.get("reset") {
                assert_eq!(
                    reset.as_bool(),
                    Some(false),
                    "{kind} reset must be false in standard"
                );
            }
        }
    }

    // with_ocr enables OCR
    let w = builtin_profile("with_ocr").expect("with_ocr");
    let ocr_w = w.body.stages.get("ocr").expect("ocr");
    assert!(ocr_w.enabled);
    assert_eq!(
        ocr_w.params.get("enabled").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        ocr_w.params.get("lang").and_then(|v| v.as_str()),
        Some("eng")
    );
}

#[test]
fn order_safety_unsafe_body_still_canonical() {
    // Reverse of useful order: dedupe before classify in the JSON object.
    let json = r#"{
        "version": 1,
        "stages": {
            "promote": { "enabled": false, "params": {} },
            "dedupe": { "enabled": true, "params": { "reset": false } },
            "classify": { "enabled": true, "params": { "force": false } }
        }
    }"#;
    let body = parse_profile_body(json).expect("parse");
    let plan = profile_stage_plan(&body);
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].kind, "classify");
    assert_eq!(plan[1].kind, "dedupe");

    // Array form with reverse order must also plan classify then dedupe.
    let arr = r#"{
        "version": 1,
        "stages": [
            { "kind": "dedupe", "enabled": true, "params": {} },
            { "kind": "classify", "enabled": true, "params": {} }
        ]
    }"#;
    let body2 = parse_profile_body(arr).expect("parse arr");
    let plan2 = profile_stage_plan(&body2);
    assert_eq!(plan2[0].kind, "classify");
    assert_eq!(plan2[1].kind, "dedupe");

    // Full plan for standard matches canonical ∩ enabled.
    let std = builtin_profile("standard").expect("std");
    let plan_std = profile_stage_plan(&std.body);
    let mut expected = Vec::new();
    for &k in CANONICAL_STAGE_ORDER {
        if std.body.stages.get(k).is_some_and(|s| s.enabled) {
            expected.push(k);
        }
    }
    let got: Vec<_> = plan_std.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(got, expected);
}

#[test]
fn unique_name_delete_and_builtin_protected() {
    let (_dir, matter) = open_matter();
    let listed = matter.list_processing_profiles().expect("list");
    assert!(listed.len() >= 4);

    let body = r#"{
        "version": 1,
        "stages": {
            "classify": { "enabled": true, "params": { "force": false } }
        }
    }"#;
    let p1 = matter
        .upsert_processing_profile(ProcessingProfileInput {
            id: None,
            name: "my_recipe".into(),
            description: Some("test".into()),
            body_json: body.into(),
            created_by: Some("tester".into()),
        })
        .expect("upsert");
    assert!(!p1.is_builtin);
    assert!(p1.id.starts_with("pfl"));
    assert_eq!(p1.name, "my_recipe");

    let err = matter
        .upsert_processing_profile(ProcessingProfileInput {
            id: None,
            name: "my_recipe".into(),
            description: None,
            body_json: body.into(),
            created_by: None,
        })
        .expect_err("duplicate name");
    assert!(err.to_string().contains("already exists"));

    let reserved = matter
        .upsert_processing_profile(ProcessingProfileInput {
            id: None,
            name: "standard".into(),
            description: None,
            body_json: body.into(),
            created_by: None,
        })
        .expect_err("reserved");
    assert!(reserved.to_string().contains("reserved"));

    // Cannot delete builtin
    let del_builtin = matter
        .delete_processing_profile("builtin:standard")
        .expect_err("no del builtin");
    assert!(del_builtin.to_string().contains("built-in"));
    let del_name = matter
        .delete_processing_profile("standard")
        .expect_err("no del by name");
    assert!(del_name.to_string().contains("built-in"));

    matter
        .set_default_processing_profile(Some(&p1.id))
        .expect("set default");
    assert_eq!(
        matter.get_default_processing_profile_id().expect("get"),
        Some(p1.id.clone())
    );

    matter.delete_processing_profile(&p1.id).expect("delete");
    assert!(matter.get_processing_profile(&p1.id).is_err());
    // Default cleared
    assert_eq!(
        matter.get_default_processing_profile_id().expect("get"),
        None
    );
}

#[test]
fn expand_unknown_kind_fails() {
    let body = parse_profile_body(
        r#"{ "version": 1, "stages": { "classify": { "enabled": true, "params": {} } } }"#,
    )
    .expect("parse");
    let err = expand_profile_stage(&body, "ingest").expect_err("unknown");
    assert!(err.to_string().contains("unknown"));
    let err2 = expand_profile_stage(&body, "dedupe").expect_err("missing");
    assert!(err2.to_string().contains("not present") || err2.to_string().contains("dedupe"));
}

#[test]
fn array_form_normalizes() {
    let json = r#"{
        "version": 1,
        "stages": [
            { "kind": "office_extract", "enabled": true, "params": { "force": false } },
            { "kind": "classify", "enabled": true, "params": { "force": false } }
        ]
    }"#;
    let body = parse_profile_body(json).expect("parse");
    assert_eq!(body.version, PROFILE_BODY_VERSION);
    assert!(body.stages.contains_key("classify"));
    assert!(body.stages.contains_key("office_extract"));
    let plan = profile_stage_plan(&body);
    assert_eq!(plan[0].kind, "classify");
    assert_eq!(plan[1].kind, "office_extract");
}

#[test]
fn body_size_cap() {
    let huge_params = "x".repeat(PROFILE_BODY_MAX_BYTES);
    let json = format!(
        r#"{{"version":1,"stages":{{"classify":{{"enabled":true,"params":{{"pad":"{huge_params}"}}}}}}}}"#
    );
    assert!(json.len() > PROFILE_BODY_MAX_BYTES);
    let err = parse_profile_body(&json).expect_err("oversize");
    assert!(err.to_string().contains("max size"));
}

#[test]
fn version_unknown_fails() {
    let err = parse_profile_body(r#"{"version":99,"stages":{}}"#).expect_err("ver");
    assert!(err.to_string().contains("unknown profile body version"));
}

#[test]
fn get_by_builtin_id_and_name() {
    let (_dir, matter) = open_matter();
    let a = matter
        .get_processing_profile("builtin:standard")
        .expect("by id");
    let b = matter.get_processing_profile("standard").expect("by name");
    assert_eq!(a.id, b.id);
    assert_eq!(a.name, "standard");
}

#[test]
fn list_includes_user_profiles() {
    let (_dir, matter) = open_matter();
    let body = r#"{"version":1,"stages":{"classify":{"enabled":true,"params":{}}}}"#;
    matter
        .upsert_processing_profile(ProcessingProfileInput {
            id: None,
            name: "user_one".into(),
            description: None,
            body_json: body.into(),
            created_by: None,
        })
        .expect("upsert");
    let list = matter.list_processing_profiles().expect("list");
    assert!(list.iter().any(|p| p.name == "user_one" && !p.is_builtin));
    assert_eq!(list.iter().filter(|p| p.is_builtin).count(), 4);
}

#[test]
fn reduce_only_and_extract_only_plans() {
    let extract = builtin_profile("extract_only").expect("e");
    let plan = profile_stage_plan(&extract.body);
    let kinds: Vec<_> = plan.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec!["classify", "office_extract", "pdf_extract", "ics_extract"]
    );

    let reduce = builtin_profile("reduce_only").expect("r");
    let plan_r = profile_stage_plan(&reduce.body);
    let kinds_r: Vec<_> = plan_r.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(kinds_r, vec!["dedupe", "thread", "cull", "promote"]);
}

#[test]
fn null_stage_entry_rejected() {
    let err =
        parse_profile_body(r#"{"version":1,"stages":{"classify":null}}"#).expect_err("null stage");
    assert!(
        err.to_string().contains("must be a JSON object"),
        "got {err}"
    );
}

#[test]
fn non_object_params_rejected() {
    let err = parse_profile_body(
        r#"{"version":1,"stages":{"classify":{"enabled":true,"params":"nope"}}}"#,
    )
    .expect_err("params string");
    assert!(
        err.to_string().contains("params must be a JSON object"),
        "got {err}"
    );
}

#[test]
fn zero_batch_size_rejected() {
    let err = parse_profile_body(
        r#"{"version":1,"stages":{"classify":{"enabled":true,"params":{"batch_size":0}}}}"#,
    )
    .expect_err("batch 0");
    let s = err.to_string();
    assert!(
        s.contains("batch_size") && (s.contains("positive") || s.contains(">= 1")),
        "got {err}"
    );
}

#[test]
fn enabled_must_be_bool() {
    let err =
        parse_profile_body(r#"{"version":1,"stages":{"classify":{"enabled":"yes","params":{}}}}"#)
            .expect_err("enabled string");
    assert!(
        err.to_string().contains("enabled must be a boolean"),
        "got {err}"
    );
}

#[test]
fn empty_cull_preset_name_rejected_when_enabled() {
    let err = parse_profile_body(
        r#"{"version":1,"stages":{"cull":{"enabled":true,"params":{"preset_name":""}}}}"#,
    )
    .expect_err("empty preset");
    assert!(err.to_string().contains("preset_name"), "got {err}");
}

#[test]
fn null_params_rejected() {
    let err =
        parse_profile_body(r#"{"version":1,"stages":{"classify":{"enabled":true,"params":null}}}"#)
            .expect_err("null params");
    assert!(
        err.to_string().contains("params must be a JSON object"),
        "got {err}"
    );
}

#[test]
fn unknown_param_field_rejected() {
    let err = parse_profile_body(
        r#"{"version":1,"stages":{"classify":{"enabled":true,"params":{"force":false,"not_a_field":1}}}}"#,
    )
    .expect_err("unknown field");
    assert!(
        err.to_string().contains("params invalid") || err.to_string().contains("unknown"),
        "got {err}"
    );
}

#[test]
fn classify_use_magic_must_be_bool() {
    let err = parse_profile_body(
        r#"{"version":1,"stages":{"classify":{"enabled":true,"params":{"use_magic":"invalid"}}}}"#,
    )
    .expect_err("use_magic type");
    assert!(err.to_string().contains("params invalid"), "got {err}");
}

#[test]
fn promote_unknown_policy_rejected() {
    let err = parse_profile_body(
        r#"{"version":1,"stages":{"promote":{"enabled":true,"params":{"policy":"nope"}}}}"#,
    )
    .expect_err("policy");
    assert!(
        err.to_string().contains("unknown promote policy"),
        "got {err}"
    );
}

#[test]
fn neardup_hash_seed_accepted() {
    let body = parse_profile_body(
        r#"{"version":1,"stages":{"neardup":{"enabled":true,"params":{"hash_seed":1,"num_hashes":16,"num_bands":4,"rows_per_band":4}}}}"#,
    )
    .expect("hash_seed ok");
    assert_eq!(body.stages["neardup"].params["hash_seed"].as_u64(), Some(1));
}

#[test]
fn ocr_enabled_empty_params_rejected() {
    let err = parse_profile_body(r#"{"version":1,"stages":{"ocr":{"enabled":true,"params":{}}}}"#)
        .expect_err("empty ocr");
    assert!(err.to_string().contains("params.enabled"), "got {err}");
}

#[test]
fn cull_inline_rules_rejected() {
    let err = parse_profile_body(
        r#"{"version":1,"stages":{"cull":{"enabled":true,"params":{"rules":{"version":1}}}}}"#,
    )
    .expect_err("rules");
    assert!(err.to_string().contains("inline rules"), "got {err}");
}
