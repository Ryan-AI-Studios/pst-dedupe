//! In-process multi-client service tests (track 0058).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use camino::Utf8PathBuf;
use http_body_util::BodyExt;
use matter_core::{ApplyCodesInput, ItemInput, Matter, ROLE_ADMIN, ROLE_REVIEWER};
use matter_service::{open_matter_for_service, router_from_matter, validate_bind};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tempfile::tempdir;
use tower::ServiceExt;

fn utf8_tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
    (dir, path)
}

fn seed_matter() -> (tempfile::TempDir, Utf8PathBuf, String, String) {
    let (tmp, base) = utf8_tempdir();
    let root = base.join("svc");
    let matter = Matter::create(&root, "Service Matter").expect("create");
    matter.enable_multi_user("system").expect("enable");
    let admin = matter
        .create_user("Admin", ROLE_ADMIN, "admin-pass", "system")
        .expect("admin");
    let _reviewer = matter
        .create_user("Reviewer", ROLE_REVIEWER, "rev-pass", "system")
        .expect("rev");
    let item = matter
        .insert_item(ItemInput {
            status: "extracted".into(),
            path: Some("/doc".into()),
            subject: Some("Hello".into()),
            from_addr: Some("a@x.com".into()),
            mime_type: Some("message/rfc822".into()),
            ..Default::default()
        })
        .expect("item");
    // Second item for batch tests.
    let _item2 = matter
        .insert_item(ItemInput {
            status: "extracted".into(),
            path: Some("/doc2".into()),
            subject: Some("Other".into()),
            mime_type: Some("message/rfc822".into()),
            ..Default::default()
        })
        .expect("item2");
    drop(matter);
    (tmp, root, item.id, admin.id)
}

async fn json_req(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("Authorization", format!("Bearer {t}"));
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let req = if let Some(b) = body {
        builder
            .body(Body::from(serde_json::to_vec(&b).expect("ser")))
            .expect("req")
    } else {
        builder.body(Body::empty()).expect("req")
    };
    let res = app.clone().oneshot(req).await.expect("oneshot");
    let status = res.status();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, val)
}

async fn login(app: &axum::Router, name: &str, password: &str) -> String {
    let (status, body) = json_req(
        app,
        "POST",
        "/v1/login",
        None,
        Some(json!({ "name": name, "password": password })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login body={body}");
    body["token"].as_str().expect("token").to_string()
}

#[tokio::test]
async fn healthz_and_login_bearer() {
    let (_tmp, root, _item, _admin) = seed_matter();
    let matter = open_matter_for_service(&root, None).expect("open");
    let app = router_from_matter(matter);

    let (status, body) = json_req(&app, "GET", "/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);

    let token = login(&app, "Reviewer", "rev-pass").await;
    assert!(!token.is_empty());

    let (bad_status, bad) =
        json_req(&app, "GET", "/v1/items", Some("not-a-real-token"), None).await;
    assert_eq!(bad_status, StatusCode::UNAUTHORIZED);
    assert_eq!(bad["code"], "unauthorized");
}

#[tokio::test]
async fn lock_held_second_mutate_fails_and_occ_409() {
    let (_tmp, root, item_id, _admin) = seed_matter();
    let matter = open_matter_for_service(&root, None).expect("open");
    // Pre-seed a code id for apply.
    let codes = matter.list_code_definitions().expect("codes");
    let code_id = codes[0].id.clone();
    let app = router_from_matter(matter);

    let tok_a = login(&app, "Admin", "admin-pass").await;
    let tok_b = login(&app, "Reviewer", "rev-pass").await;

    let (ls, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/lock"),
        Some(&tok_a),
        Some(json!({})),
    )
    .await;
    assert_eq!(ls, StatusCode::OK);

    // B tries to lock → conflict
    let (ls2, body2) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/lock"),
        Some(&tok_b),
        Some(json!({})),
    )
    .await;
    assert_eq!(ls2, StatusCode::CONFLICT);
    assert_eq!(body2["code"], "locked");

    // A codes with expected_version 0
    let (cs, cbody) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/codes"),
        Some(&tok_a),
        Some(json!({
            "add_code_ids": [code_id],
            "expected_version": 0,
            "actor": "spoofed-admin"
        })),
    )
    .await;
    assert_eq!(cs, StatusCode::OK, "code body={cbody}");
    assert_eq!(cbody["review_versions"][0], 1);

    // Stale version from A → 409
    let (stale_s, stale_b) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/codes"),
        Some(&tok_a),
        Some(json!({
            "remove_code_ids": [code_id],
            "expected_version": 0
        })),
    )
    .await;
    assert_eq!(stale_s, StatusCode::CONFLICT);
    assert_eq!(stale_b["code"], "version_conflict");
}

#[tokio::test]
async fn batch_feed_subset_and_actor_spoof_ignored() {
    let (_tmp, root, item_id, _admin) = seed_matter();
    let matter = open_matter_for_service(&root, None).expect("open");
    let thin = matter.list_items_thin(None, 10).expect("list");
    assert!(thin.len() >= 2);
    let ids: Vec<String> = thin.iter().map(|t| t.id.clone()).collect();
    let app = router_from_matter(matter);

    let tok = login(&app, "Reviewer", "rev-pass").await;
    let (bs, bbody) = json_req(
        &app,
        "POST",
        "/v1/batches",
        Some(&tok),
        Some(json!({
            "name": "batch1",
            "item_ids": [ids[0].clone()],
        })),
    )
    .await;
    assert_eq!(bs, StatusCode::CREATED, "batch={bbody}");
    let batch_id = bbody["id"].as_str().expect("id").to_string();

    let (fs, feed) = json_req(
        &app,
        "GET",
        &format!("/v1/batches/{batch_id}/items"),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(fs, StatusCode::OK);
    let arr = feed.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["item_id"], ids[0]);

    // Checkout + membership assert via note mutate with spoofed actor
    let (_co, _) = json_req(
        &app,
        "POST",
        &format!("/v1/batches/{batch_id}/checkout"),
        Some(&tok),
        None,
    )
    .await;

    let (ls, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{}/lock", ids[0]),
        Some(&tok),
        Some(json!({})),
    )
    .await;
    assert_eq!(ls, StatusCode::OK);

    let (ns, nbody) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{}/notes", ids[0]),
        Some(&tok),
        Some(json!({
            "body": "note from reviewer",
            "expected_version": 0,
            "actor": "totally-not-the-reviewer"
        })),
    )
    .await;
    assert_eq!(ns, StatusCode::OK, "note={nbody}");
    // updated_by must be session user id, not spoofed body actor
    assert_ne!(nbody["updated_by"], "totally-not-the-reviewer");

    // Foreign item (not in checked-out batch) mutate must fail closed.
    assert!(ids.len() >= 2, "seed_matter inserts two items");
    let (ns2, nbody2) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{}/notes", ids[1]),
        Some(&tok),
        Some(json!({
            "body": "should fail",
            "expected_version": 0
        })),
    )
    .await;
    assert_eq!(
        ns2,
        StatusCode::FORBIDDEN,
        "out-of-batch mutate must be forbidden: {nbody2}"
    );
    assert_eq!(nbody2["code"], "forbidden");

    // Global list under batch checkout must not return foreign items.
    let (ls, list) = json_req(&app, "GET", "/v1/items?limit=100", Some(&tok), None).await;
    assert_eq!(ls, StatusCode::OK);
    let arr = list.as_array().expect("array");
    assert!(arr.iter().all(|row| row["id"] == ids[0]));
    assert!(!arr.iter().any(|row| row["id"] == ids[1]));

    // Body endpoint available for in-batch item.
    let (bs, body) = json_req(
        &app,
        "GET",
        &format!("/v1/items/{}/body", ids[0]),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(bs, StatusCode::OK, "body={body}");
    assert_eq!(body["item_id"], ids[0]);
    assert!(body.get("text").is_some());
    assert!(body.get("review_version").is_some());

    // Check-in then re-checkout works.
    let (ci, _) = json_req(
        &app,
        "POST",
        &format!("/v1/batches/{batch_id}/checkin"),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(ci, StatusCode::NO_CONTENT);
    let (co2, cob) = json_req(
        &app,
        "POST",
        &format!("/v1/batches/{batch_id}/checkout"),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(co2, StatusCode::OK, "re-checkout={cob}");
    let _ = item_id;
}

#[tokio::test]
async fn admin_force_unlock_while_service_holds_matter() {
    let (_tmp, root, item_id, _admin) = seed_matter();
    let matter = open_matter_for_service(&root, None).expect("open");
    let app = router_from_matter(matter);

    let rev = login(&app, "Reviewer", "rev-pass").await;
    let admin = login(&app, "Admin", "admin-pass").await;

    let (ls, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/lock"),
        Some(&rev),
        Some(json!({})),
    )
    .await;
    assert_eq!(ls, StatusCode::OK);

    // Reviewer cannot force-unlock
    let (fs, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/force-unlock"),
        Some(&rev),
        None,
    )
    .await;
    assert_eq!(fs, StatusCode::FORBIDDEN);

    // Admin force-unlock succeeds without second matter open
    let (as_, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/force-unlock"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(as_, StatusCode::NO_CONTENT);

    // Second reviewer can now lock
    let (ls2, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/lock"),
        Some(&admin),
        Some(json!({})),
    )
    .await;
    assert_eq!(ls2, StatusCode::OK);
}

#[tokio::test]
async fn sample_qc_create_and_record() {
    let (_tmp, root, item_id, _admin) = seed_matter();
    let matter = open_matter_for_service(&root, None).expect("open");
    let codes = matter.list_code_definitions().expect("codes");
    let code_id = codes[0].id.clone();
    // Code the item as system-path before service strict (use admin after open).
    // open_matter_for_service enables strict — use multi_user APIs after login.
    let app = router_from_matter(matter);
    let tok = login(&app, "Admin", "admin-pass").await;
    let (_l, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/lock"),
        Some(&tok),
        Some(json!({})),
    )
    .await;
    let (_c, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/codes"),
        Some(&tok),
        Some(json!({
            "add_code_ids": [code_id],
            "expected_version": 0
        })),
    )
    .await;

    let (ss, sbody) = json_req(
        &app,
        "POST",
        "/v1/qc/samples",
        Some(&tok),
        Some(json!({
            "name": "sample",
            "sample_n": 1,
            "seed": 99
        })),
    )
    .await;
    assert_eq!(ss, StatusCode::CREATED, "sample={sbody}");
    let sample_id = sbody["id"].as_str().expect("sid").to_string();
    let sampled = sbody["item_ids"][0].as_str().expect("iid").to_string();

    let (rs, rbody) = json_req(
        &app,
        "POST",
        &format!("/v1/qc/samples/{sample_id}/items/{sampled}"),
        Some(&tok),
        Some(json!({ "outcome": "agree", "notes": "ok" })),
    )
    .await;
    assert_eq!(rs, StatusCode::OK, "record={rbody}");
    assert_eq!(rbody["outcome"], "agree");

    let (rep_s, rep) = json_req(
        &app,
        "GET",
        &format!("/v1/qc/samples/{sample_id}"),
        Some(&tok),
        None,
    )
    .await;
    assert_eq!(rep_s, StatusCode::OK, "report={rep}");
    assert_eq!(rep["sample"]["id"], sample_id);
    assert_eq!(rep["summary"]["agree"], 1);
    assert_eq!(rep["items"][0]["outcome"], "agree");
}

#[tokio::test]
async fn serve_requires_multi_user_enabled() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("solo");
    let m = Matter::create(&root, "Solo").expect("create");
    drop(m);
    match open_matter_for_service(&root, None) {
        Ok(_) => panic!("must fail without multi_user"),
        Err(err) => {
            let msg = err.to_string();
            assert!(
                msg.contains("multi-user") || msg.contains("bootstrap"),
                "unexpected error: {msg}"
            );
        }
    }
}

#[test]
fn bind_safety_unit() {
    let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7749);
    assert!(validate_bind(loopback, false).is_ok());
    let lan = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)), 7749);
    assert!(validate_bind(lan, false).is_err());
    assert!(validate_bind(lan, true).is_ok());
}

#[tokio::test]
async fn encrypted_matter_service_login_and_code() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("enc");
    let pass = "enc-passphrase-test";
    {
        let m = Matter::create_encrypted(&root, "Enc", pass).expect("create enc");
        m.enable_multi_user("system").expect("enable");
        m.create_user("Admin", ROLE_ADMIN, "admin-pass", "system")
            .expect("user");
        let item = m
            .insert_item(ItemInput {
                status: "extracted".into(),
                path: Some("/e".into()),
                subject: Some("E".into()),
                ..Default::default()
            })
            .expect("item");
        let code = m.list_code_definitions().expect("c")[0].id.clone();
        // drop before service open
        let _ = (item, code);
    }
    let matter = open_matter_for_service(&root, Some(pass)).expect("service open");
    let code_id = matter.list_code_definitions().expect("c")[0].id.clone();
    let item_id = matter.list_items_thin(None, 10).expect("list")[0]
        .id
        .clone();
    let app = router_from_matter(matter);

    let tok = login(&app, "Admin", "admin-pass").await;
    let (_l, _) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/lock"),
        Some(&tok),
        Some(json!({})),
    )
    .await;
    let (cs, cbody) = json_req(
        &app,
        "POST",
        &format!("/v1/items/{item_id}/codes"),
        Some(&tok),
        Some(json!({
            "add_code_ids": [code_id],
            "expected_version": 0
        })),
    )
    .await;
    assert_eq!(cs, StatusCode::OK, "enc code={cbody}");
}

// silence unused import warning if apply_codes not used at top level
#[allow(dead_code)]
fn _types() {
    let _: Option<ApplyCodesInput> = None;
}
