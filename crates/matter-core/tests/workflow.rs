//! Workflows (track 0044 / schema v24).

use matter_core::item_status;
use matter_core::{
    bind_workflow, builtin_workflows, parse_workflow_body, selection_fingerprint,
    validate_workflow, workflow_definition_hash, InsertQcRunInput, ItemInput, Matter,
    WorkflowInput, WorkflowNodeType, JOB_KIND_WORKFLOW_RUN, SCHEMA_VERSION, WORKFLOW_BODY_VERSION,
};
use serde_json::json;
use tempfile::tempdir;

fn open_matter() -> (tempfile::TempDir, Matter) {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().join("m");
    let path = camino::Utf8PathBuf::from_path_buf(root).expect("utf8");
    let matter = Matter::create(&path, "workflow-test").expect("create");
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 27);
    (dir, matter)
}

fn minimal_body_json() -> String {
    r#"{
        "version": 1,
        "nodes": [
            {
                "id": "n1",
                "type": "job",
                "kind": "gap",
                "enabled": true,
                "soft_fail": false,
                "params": {}
            }
        ]
    }"#
    .into()
}

#[test]
fn schema_version_is_current() {
    let (_dir, matter) = open_matter();
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 27);
}

#[test]
fn list_workflows_unions_builtins() {
    let (_dir, matter) = open_matter();
    let listed = matter.list_workflows().expect("list");
    let builtins = builtin_workflows();
    assert_eq!(builtins.len(), 5);
    for b in &builtins {
        assert!(
            listed.iter().any(|w| w.id == b.id && w.is_builtin),
            "missing builtin {}",
            b.name
        );
    }
    // Fresh matter has only built-ins.
    assert_eq!(listed.len(), builtins.len());
}

#[test]
fn unique_workflow_name_per_matter() {
    let (_dir, matter) = open_matter();
    let body = minimal_body_json();
    let w1 = matter
        .upsert_workflow(WorkflowInput {
            id: None,
            name: "my_chain".into(),
            description: Some("test".into()),
            body_json: body.clone(),
            created_by: Some("tester".into()),
        })
        .expect("upsert");
    assert!(!w1.is_builtin);
    assert!(w1.id.starts_with("wfl"));
    assert_eq!(w1.name, "my_chain");

    let err = matter
        .upsert_workflow(WorkflowInput {
            id: None,
            name: "my_chain".into(),
            description: None,
            body_json: body,
            created_by: None,
        })
        .expect_err("duplicate name");
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn cannot_upsert_or_delete_builtin() {
    let (_dir, matter) = open_matter();
    let body = minimal_body_json();

    let reserved = matter
        .upsert_workflow(WorkflowInput {
            id: None,
            name: "ingest_then_standard".into(),
            description: None,
            body_json: body.clone(),
            created_by: None,
        })
        .expect_err("reserved name");
    assert!(reserved.to_string().contains("reserved"));

    let del = matter
        .delete_workflow("builtin:ingest_then_standard")
        .expect_err("delete builtin");
    assert!(del.to_string().contains("cannot delete"));

    let del_bare = matter
        .delete_workflow("ingest_then_standard")
        .expect_err("delete bare builtin");
    assert!(del_bare.to_string().contains("cannot delete"));
}

#[test]
fn get_workflow_resolves_builtin_and_user() {
    let (_dir, matter) = open_matter();
    let b = matter
        .get_workflow("builtin:reduce_only_chain")
        .expect("builtin id");
    assert!(b.is_builtin);
    let b2 = matter.get_workflow("reduce_only_chain").expect("bare name");
    assert_eq!(b.id, b2.id);

    let body = minimal_body_json();
    let u = matter
        .upsert_workflow(WorkflowInput {
            id: None,
            name: "custom".into(),
            description: None,
            body_json: body,
            created_by: None,
        })
        .expect("user");
    let by_id = matter.get_workflow(&u.id).expect("by id");
    let by_name = matter.get_workflow("custom").expect("by name");
    assert_eq!(by_id.id, by_name.id);
    assert!(!by_id.is_builtin);
}

#[test]
fn ast_bind_windows_path_with_quote() {
    let body = parse_workflow_body(
        r#"{
            "version": 1,
            "nodes": [
                {
                    "id": "n1",
                    "type": "job",
                    "kind": "ingest",
                    "params": { "path": "${source_path}" }
                }
            ]
        }"#,
    )
    .expect("parse");
    let path = r#"C:\Users\test\foo"bar\export"#;
    let plan = bind_workflow(&body, &json!({ "source_path": path })).expect("bind");
    let bound = plan.nodes[0]
        .params
        .get("path")
        .and_then(|v| v.as_str())
        .expect("path");
    assert_eq!(bound, path);
    let ser = serde_json::to_string(&plan.nodes[0].params).expect("ser");
    let de: serde_json::Value = serde_json::from_str(&ser).expect("de");
    assert_eq!(de.get("path").and_then(|v| v.as_str()), Some(path));
}

#[test]
fn unknown_placeholder_fails() {
    let body = parse_workflow_body(
        r#"{
            "version": 1,
            "nodes": [
                {
                    "id": "n1",
                    "type": "job",
                    "kind": "ingest",
                    "params": { "path": "${nope}" }
                }
            ]
        }"#,
    )
    .expect("parse");
    let err = bind_workflow(&body, &json!({ "source_path": "x" })).expect_err("missing");
    assert!(err.to_string().contains("nope"));
}

#[test]
fn soft_fail_true_on_require_qc_pass_rejected() {
    let err = parse_workflow_body(
        r#"{
            "version": 1,
            "nodes": [
                {
                    "id": "g1",
                    "type": "gate",
                    "kind": "require_qc_pass",
                    "soft_fail": true,
                    "params": {}
                }
            ]
        }"#,
    )
    .expect_err("soft_fail gate");
    assert!(err.to_string().contains("soft_fail"));
}

#[test]
fn unknown_node_kind_fails() {
    let err = parse_workflow_body(
        r#"{
            "version": 1,
            "nodes": [
                {
                    "id": "n1",
                    "type": "job",
                    "kind": "powershell",
                    "params": {}
                }
            ]
        }"#,
    )
    .expect_err("unknown kind");
    assert!(err.to_string().contains("unknown workflow job kind"));
}

#[test]
fn create_job_with_parent_and_list_children() {
    let (_dir, matter) = open_matter();
    let root = matter.create_job(JOB_KIND_WORKFLOW_RUN).expect("root");
    assert!(root.parent_job_id.is_none());

    let child = matter
        .create_job_with_parent("ingest", Some(&root.id))
        .expect("child");
    assert_eq!(child.parent_job_id.as_deref(), Some(root.id.as_str()));

    let child2 = matter
        .create_job_with_parent("extract_pst", Some(&root.id))
        .expect("child2");
    assert_eq!(child2.parent_job_id.as_deref(), Some(root.id.as_str()));

    let children = matter.list_child_jobs(&root.id).expect("children");
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].id, child.id);
    assert_eq!(children[1].id, child2.id);

    // Root create_job stays parentless.
    let adhoc = matter.create_job("dedupe").expect("adhoc");
    assert!(adhoc.parent_job_id.is_none());
}

#[test]
fn parent_job_must_same_matter() {
    let dir = tempdir().expect("tempdir");
    let root_a = dir.path().join("a");
    let root_b = dir.path().join("b");
    let path_a = camino::Utf8PathBuf::from_path_buf(root_a).expect("utf8");
    let path_b = camino::Utf8PathBuf::from_path_buf(root_b).expect("utf8");
    let matter_a = Matter::create(&path_a, "A").expect("a");
    let matter_b = Matter::create(&path_b, "B").expect("b");
    let parent = matter_a.create_job("workflow_run").expect("parent");
    let err = matter_b
        .create_job_with_parent("ingest", Some(&parent.id))
        .expect_err("cross matter");
    // Parent job not found in B's DB (separate matter.db) or matter mismatch.
    let msg = err.to_string();
    assert!(
        msg.contains("not found") || msg.contains("another matter"),
        "unexpected: {msg}"
    );
}

#[test]
fn evaluate_gate_require_has_sources() {
    let (_dir, matter) = open_matter();
    let err = matter
        .evaluate_gate("require_has_sources", &json!({}))
        .expect_err("no sources");
    assert!(err.to_string().contains("no sources"));
}

#[test]
fn evaluate_gate_require_qc_pass_no_run() {
    let (_dir, matter) = open_matter();
    let err = matter
        .evaluate_gate("require_qc_pass", &json!({}))
        .expect_err("no qc");
    assert!(err.to_string().contains("no QC run"));
}

#[test]
fn evaluate_gate_require_qc_pass_fresh_then_stale_after_selection_change() {
    let (_dir, matter) = open_matter();

    // Empty review corpus: insert a passed QC run whose fingerprint matches empty set.
    let empty: Vec<String> = vec![];
    let fp = selection_fingerprint(&empty);
    matter
        .insert_qc_run(InsertQcRunInput {
            profile: "default_production_qc_v1".into(),
            passed: true,
            error_count: 0,
            warn_count: 0,
            candidate_count: 0,
            selection_fingerprint: fp,
            scope: "review_corpus".into(),
            scope_json: None,
            report_path: None,
            job_id: None,
            rules_json: None,
        })
        .expect("insert qc");

    matter
        .evaluate_gate("require_qc_pass", &json!({}))
        .expect("fresh empty selection must pass");

    // Change selection: promote an item into the review corpus.
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            subject: Some("new review item".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("insert item");

    let err = matter
        .evaluate_gate("require_qc_pass", &json!({}))
        .expect_err("stale after selection change");
    let msg = err.to_string();
    assert!(
        msg.contains("not fresh") || msg.contains("stale") || msg.contains("selection"),
        "expected freshness failure, got: {msg}"
    );
}

#[test]
fn workflow_definition_hash_is_stable_nonempty() {
    let body = parse_workflow_body(&minimal_body_json()).expect("parse");
    let h1 = workflow_definition_hash(&body);
    let h2 = workflow_definition_hash(&body);
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64);
    assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn user_workflow_crud_and_default() {
    let (_dir, matter) = open_matter();
    let body = minimal_body_json();
    let w = matter
        .upsert_workflow(WorkflowInput {
            id: None,
            name: "ops_chain".into(),
            description: Some("ops".into()),
            body_json: body.clone(),
            created_by: Some("alice".into()),
        })
        .expect("create");

    matter
        .set_default_workflow(Some(&w.id))
        .expect("set default");
    assert_eq!(
        matter.get_default_workflow_id().expect("get"),
        Some(w.id.clone())
    );

    // Update in place.
    let updated = matter
        .upsert_workflow(WorkflowInput {
            id: Some(w.id.clone()),
            name: "ops_chain".into(),
            description: Some("ops v2".into()),
            body_json: body,
            created_by: None,
        })
        .expect("update");
    assert_eq!(updated.description.as_deref(), Some("ops v2"));
    assert_eq!(updated.body.version, WORKFLOW_BODY_VERSION);

    matter.delete_workflow(&w.id).expect("delete");
    assert!(matter.get_workflow(&w.id).is_err());
    // Default cleared on delete.
    assert!(matter.get_default_workflow_id().expect("def").is_none());
}

#[test]
fn validate_workflow_empty_nodes_fails() {
    let err = validate_workflow(&matter_core::WorkflowBody {
        version: 1,
        name: None,
        description: None,
        nodes: vec![],
    })
    .expect_err("empty");
    assert!(err.to_string().contains("at least one node"));
}

#[test]
fn builtins_include_expected_shapes() {
    let ingest = matter_core::builtin_workflow("ingest_then_standard").expect("ingest");
    assert_eq!(ingest.body.nodes.len(), 3);
    assert_eq!(
        ingest.body.nodes[0].node_type,
        matter_core::WorkflowNodeType::Job
    );
    assert_eq!(ingest.body.nodes[0].kind.as_deref(), Some("ingest"));
    assert_eq!(
        ingest.body.nodes[2].node_type,
        matter_core::WorkflowNodeType::ProfileRun
    );

    let qc = matter_core::builtin_workflow("qc_then_produce").expect("qc");
    assert_eq!(qc.body.nodes.len(), 3);
    assert_eq!(
        qc.body.nodes[1].node_type,
        matter_core::WorkflowNodeType::Gate
    );
    assert_eq!(qc.body.nodes[1].kind.as_deref(), Some("require_qc_pass"));
}

#[test]
fn every_builtin_workflow_body_validates() {
    let builtins = builtin_workflows();
    assert_eq!(builtins.len(), 5, "expected five built-in workflows");
    for w in &builtins {
        validate_workflow(&w.body).unwrap_or_else(|e| {
            panic!("builtin '{}' failed validate_workflow: {e}", w.name);
        });
        // Round-trip through parse path used by checkpoints / user load.
        let json = serde_json::to_string(&w.body).expect("ser");
        parse_workflow_body(&json).unwrap_or_else(|e| {
            panic!("builtin '{}' failed parse_workflow_body: {e}", w.name);
        });
    }
}

#[test]
fn qc_then_produce_get_validate_bind_order() {
    let (_dir, matter) = open_matter();
    let w = matter
        .get_workflow("builtin:qc_then_produce")
        .expect("get_workflow");
    assert!(w.is_builtin);
    assert_eq!(w.name, "qc_then_produce");
    validate_workflow(&w.body).expect("validate");

    let plan = bind_workflow(&w.body, &json!({})).expect("bind empty run_params");
    let enabled: Vec<_> = plan.nodes.iter().filter(|n| n.enabled).collect();
    assert_eq!(enabled.len(), 3);
    assert_eq!(enabled[0].node_type, WorkflowNodeType::Job);
    assert_eq!(enabled[0].kind_or_profile, "qc");
    assert_eq!(enabled[1].node_type, WorkflowNodeType::Gate);
    assert_eq!(enabled[1].kind_or_profile, "require_qc_pass");
    assert!(!enabled[1].soft_fail, "gates always hard-fail when bound");
    assert_eq!(enabled[2].node_type, WorkflowNodeType::Job);
    assert_eq!(enabled[2].kind_or_profile, "produce");
}

#[test]
fn ingest_then_standard_bind_preserves_run_params_paths() {
    let (_dir, matter) = open_matter();
    let w = matter
        .get_workflow("builtin:ingest_then_standard")
        .expect("get");
    validate_workflow(&w.body).expect("validate");
    let path = r#"C:\matters\Acme\source"pkg\mail.pst"#;
    let plan = bind_workflow(
        &w.body,
        &json!({
            "source_path": path,
            "source_id": "src_1",
            "pst_item_id": "itm_pst"
        }),
    )
    .expect("bind");
    assert_eq!(plan.nodes.len(), 3);
    assert_eq!(
        plan.nodes[0].params.get("path").and_then(|v| v.as_str()),
        Some(path)
    );
    assert_eq!(
        plan.nodes[1]
            .params
            .get("source_id")
            .and_then(|v| v.as_str()),
        Some("src_1")
    );
    assert_eq!(plan.nodes[2].kind_or_profile, "builtin:standard");
}
