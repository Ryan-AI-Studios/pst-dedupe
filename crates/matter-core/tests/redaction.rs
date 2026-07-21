//! Integration tests for text redaction (schema v13 / track 0032).

use matter_core::{
    build_redacted_text, display_body_digest, item_role, item_status, privilege_status,
    redaction_reason, redaction_status, utf8_char_slice, CreateHighlightInput,
    CreateRedactionInput, FilterSpec, ItemInput, ItemUpdate, Matter, REDACTED_TOKEN,
    SCHEMA_VERSION, SCOPE_ENTIRE_MATTER,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

fn insert_text_item(matter: &Matter, body: &str) -> (matter_core::Item, String) {
    let digest = matter.put_bytes(body.as_bytes()).expect("cas");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Doc".into()),
            text_sha256: Some(digest.clone()),
            path: Some("doc.txt".into()),
            ..Default::default()
        })
        .expect("item");
    (item, digest)
}

fn make_redaction(
    matter: &Matter,
    item_id: &str,
    body: &str,
    digest: &str,
    start: i64,
    end: i64,
    reason: &str,
) -> matter_core::ItemRedaction {
    let quote = utf8_char_slice(body, start as usize, end as usize)
        .expect("slice")
        .to_string();
    matter
        .create_redaction(CreateRedactionInput {
            item_id: item_id.to_string(),
            start_utf8: start,
            end_utf8: end,
            exact_quote: quote,
            display_body: body.to_string(),
            body_digest: digest.to_string(),
            reason: reason.to_string(),
            label: None,
            actor: "tester".into(),
        })
        .expect("create redaction")
}

#[test]
fn schema_v13_on_create() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-v13");
    let matter = Matter::create(&root, "V13").expect("create");
    assert_eq!(SCHEMA_VERSION, 36);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);

    let (item, _) = insert_text_item(&matter, "hello");
    assert_eq!(item.redaction_count, 0);
    assert!(item.redacted_text_sha256.is_none());
}

#[test]
fn create_list_count_original_cas_unchanged() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-create");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Alpha SECRET beta";
    let (item, digest) = insert_text_item(&matter, body);
    let original_bytes = matter.get_bytes(&digest).expect("orig cas");

    let start = 6i64;
    let end = 12i64;
    let red = make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        start,
        end,
        redaction_reason::CONFIDENTIAL,
    );
    assert_eq!(red.exact_quote, "SECRET");
    assert_eq!(red.status, redaction_status::ACTIVE);
    assert_eq!(red.reason, redaction_reason::CONFIDENTIAL);

    let listed = matter.list_redactions(&item.id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, red.id);

    let reloaded = matter.get_item(&item.id).expect("reload");
    assert_eq!(reloaded.redaction_count, 1);
    assert!(reloaded.redacted_text_sha256.is_none()); // create NULLs artifact
    assert_eq!(reloaded.text_sha256.as_deref(), Some(digest.as_str()));

    let after_bytes = matter.get_bytes(&digest).expect("cas after");
    assert_eq!(after_bytes, original_bytes);
    assert_eq!(String::from_utf8(after_bytes).unwrap(), body);
}

#[test]
fn delete_gone_and_audit() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-del");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Alpha SECRET beta";
    let (item, digest) = insert_text_item(&matter, body);
    let red = make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        6,
        12,
        redaction_reason::PII,
    );

    matter.delete_redaction(&red.id, "alice").expect("delete");
    assert!(matter.list_redactions(&item.id).expect("list").is_empty());
    assert_eq!(matter.get_item(&item.id).expect("item").redaction_count, 0);

    let params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'redaction.delete' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params).expect("json");
    assert_eq!(v["redaction_id"], red.id);
    assert_eq!(v["item_id"], item.id);
    assert_eq!(v["reason"], redaction_reason::PII);
}

#[test]
fn overlapping_ranges_merge_single_token() {
    let body = "0123456789ABCDEFGHIJ"; // 20 chars
                                       // Overlapping [10..18] and [15..21] (clamped) → one [REDACTED]
    let out = build_redacted_text(body, &[(10, 18), (15, 21)]);
    assert_eq!(out, format!("0123456789{REDACTED_TOKEN}"));
    assert_eq!(out.matches(REDACTED_TOKEN).count(), 1);
    assert!(!out.contains("ABCDEFGH"));
}

#[test]
fn true_redact_output_lacks_exact_quote() {
    let body = "Hello SECRET sauce today";
    let quote = "SECRET";
    let start = 6i64;
    let end = 12i64;
    let out = build_redacted_text(body, &[(start, end)]);
    assert!(!out.contains(quote));
    assert!(out.contains(REDACTED_TOKEN));
}

#[test]
fn regenerate_writes_cas_and_bookkeeping() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-regen");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Hello SECRET sauce";
    let (item, digest) = insert_text_item(&matter, body);
    make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        6,
        12,
        redaction_reason::OTHER,
    );

    let result = matter
        .regenerate_redacted_text(&item.id, body, "bob")
        .expect("regen");
    assert_eq!(result.region_count, 1);
    assert!(!result.has_stale);
    let sha = result.redacted_text_sha256.expect("sha");
    assert_eq!(
        result.redacted_source_digest.as_deref(),
        Some(digest.as_str())
    );

    let redacted_bytes = matter.get_bytes(&sha).expect("cas redacted");
    let redacted_text = String::from_utf8(redacted_bytes).expect("utf8");
    assert!(!redacted_text.contains("SECRET"));
    assert!(redacted_text.contains(REDACTED_TOKEN));

    let reloaded = matter.get_item(&item.id).expect("item");
    assert_eq!(reloaded.redacted_text_sha256.as_deref(), Some(sha.as_str()));
    assert_eq!(
        reloaded.redacted_source_digest.as_deref(),
        Some(digest.as_str())
    );
    assert!(reloaded.redacted_text_at.is_some());
    // Original CAS intact
    assert_eq!(
        String::from_utf8(matter.get_bytes(&digest).expect("orig")).unwrap(),
        body
    );
}

#[test]
fn body_digest_change_stale_resolve() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-stale");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "The secret clause is here.";
    let (item, digest) = insert_text_item(&matter, body);
    let start = body.find("secret").unwrap();
    let start_c = body[..start].chars().count() as i64;
    let end_c = start_c + "secret".chars().count() as i64;
    make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        start_c,
        end_c,
        redaction_reason::CONFIDENTIAL,
    );

    let new_body = "Completely different body text.";
    let new_digest = display_body_digest(new_body);
    let resolved = matter
        .resolve_redactions(&item.id, new_body, &new_digest, true)
        .expect("resolve");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].status, redaction_status::STALE);

    let listed = matter.list_redactions(&item.id).expect("list");
    assert_eq!(listed[0].status, redaction_status::STALE);
}

#[test]
fn body_digest_change_nulls_redacted_sha() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-null");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Hello SECRET sauce";
    let (item, digest) = insert_text_item(&matter, body);
    make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        6,
        12,
        redaction_reason::OTHER,
    );
    matter
        .regenerate_redacted_text(&item.id, body, "bob")
        .expect("regen");
    assert!(matter
        .get_item(&item.id)
        .expect("item")
        .redacted_text_sha256
        .is_some());

    // Re-extract body → text_sha256 change severs produce pointer.
    let new_body = "Hello SECRET sauce revised";
    let new_digest = matter.put_bytes(new_body.as_bytes()).expect("new cas");
    let updated = matter
        .update_item(
            &item.id,
            ItemUpdate {
                text_sha256: Some(Some(new_digest.clone())),
                ..Default::default()
            },
        )
        .expect("update");
    assert_eq!(updated.text_sha256.as_deref(), Some(new_digest.as_str()));
    assert!(
        updated.redacted_text_sha256.is_none(),
        "body change must NULL redacted_text_sha256"
    );
    assert!(updated.redacted_text_at.is_none());
    assert!(updated.redacted_source_digest.is_none());
    // Original text CAS still present
    assert!(matter.blob_exists(&digest).expect("exists"));
}

#[test]
fn html_sha256_change_nulls_redacted_sha() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-html-null");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Hello SECRET sauce";
    let (item, digest) = insert_text_item(&matter, body);
    let html_digest = matter
        .put_bytes(b"<p>Hello SECRET sauce</p>")
        .expect("html cas");
    matter
        .update_item(
            &item.id,
            ItemUpdate {
                html_sha256: Some(Some(html_digest.clone())),
                ..Default::default()
            },
        )
        .expect("set html");
    make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        6,
        12,
        redaction_reason::OTHER,
    );
    matter
        .regenerate_redacted_text(&item.id, body, "bob")
        .expect("regen");
    assert!(matter
        .get_item(&item.id)
        .expect("item")
        .redacted_text_sha256
        .is_some());

    let new_html = matter
        .put_bytes(b"<p>Hello SECRET sauce revised</p>")
        .expect("new html");
    let updated = matter
        .update_item(
            &item.id,
            ItemUpdate {
                html_sha256: Some(Some(new_html.clone())),
                ..Default::default()
            },
        )
        .expect("update html");
    assert_eq!(updated.html_sha256.as_deref(), Some(new_html.as_str()));
    assert!(
        updated.redacted_text_sha256.is_none(),
        "html_sha256 change must NULL redacted_text_sha256"
    );
    assert!(updated.redacted_text_at.is_none());
    assert!(updated.redacted_source_digest.is_none());
}

#[test]
fn regenerate_uses_full_cas_ignoring_truncated_display() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-full-cas");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Hello SECRET sauce and a long trailing tail that the UI might truncate";
    let (item, digest) = insert_text_item(&matter, body);
    make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        6,
        12,
        redaction_reason::OTHER,
    );

    // Truncated display missing the tail — must not become the produce source.
    let truncated = "Hello SECRET sauce and a long trailing tail that the UI might trun";
    assert!(body.starts_with(truncated) || truncated.len() < body.len());
    let result = matter
        .regenerate_redacted_text(&item.id, truncated, "bob")
        .expect("regen from truncated display still uses full CAS");
    assert_eq!(
        result.redacted_source_digest.as_deref(),
        Some(digest.as_str())
    );
    let sha = result.redacted_text_sha256.expect("sha");
    let redacted = String::from_utf8(matter.get_bytes(&sha).expect("cas")).expect("utf8");
    assert!(!redacted.contains("SECRET"));
    // Full tail preserved after redaction (proves full body was used).
    assert!(
        redacted.contains("truncate") || redacted.contains("trailing"),
        "expected full-body tail in redacted output, got: {redacted}"
    );
    assert_eq!(
        redacted,
        build_redacted_text(body, &[(6, 12)]),
        "output must match full-body true redact"
    );
}

#[test]
fn regenerate_fails_closed_when_text_cas_missing() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-missing-cas");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Hello SECRET sauce";
    // Point text_sha256 at a digest that is not in CAS.
    let fake = "f".repeat(64);
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Doc".into()),
            text_sha256: Some(fake.clone()),
            path: Some("doc.txt".into()),
            ..Default::default()
        })
        .expect("item");
    // Create redaction against display body (validation only uses display).
    make_redaction(
        &matter,
        &item.id,
        body,
        &fake,
        6,
        12,
        redaction_reason::OTHER,
    );
    let err = matter
        .regenerate_redacted_text(&item.id, body, "bob")
        .expect_err("must fail closed");
    let msg = err.to_string();
    assert!(
        msg.contains("cannot load full text body")
            || msg.contains("not found")
            || msg.contains("Blob"),
        "{msg}"
    );
    // Must not write a partial artifact.
    let reloaded = matter.get_item(&item.id).expect("item");
    assert!(reloaded.redacted_text_sha256.is_none());
}

#[test]
fn html_only_regenerate_source_is_html_sha() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-html-only");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let display = "Hello SECRET sauce";
    let html_digest = matter
        .put_bytes(b"<p>Hello SECRET sauce</p>")
        .expect("html");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("HtmlOnly".into()),
            html_sha256: Some(html_digest.clone()),
            path: Some("doc.html".into()),
            ..Default::default()
        })
        .expect("item");
    make_redaction(
        &matter,
        &item.id,
        display,
        &html_digest,
        6,
        12,
        redaction_reason::OTHER,
    );
    let result = matter
        .regenerate_redacted_text(&item.id, display, "bob")
        .expect("regen");
    assert_eq!(
        result.redacted_source_digest.as_deref(),
        Some(html_digest.as_str())
    );

    // HTML change → filter redacted_text_stale after update nulls artifact.
    matter
        .update_item(
            &item.id,
            ItemUpdate {
                html_sha256: Some(Some(matter.put_bytes(b"<p>changed</p>").expect("new html"))),
                ..Default::default()
            },
        )
        .expect("update");
    assert!(matter
        .get_item(&item.id)
        .expect("item")
        .redacted_text_sha256
        .is_none());
}

#[test]
fn reason_privilege_sets_partial_redaction() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-priv");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Privileged advice here";
    let (item, digest) = insert_text_item(&matter, body);
    let start = body.find("advice").unwrap();
    let start_c = body[..start].chars().count() as i64;
    let end_c = start_c + "advice".chars().count() as i64;
    make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        start_c,
        end_c,
        redaction_reason::PRIVILEGE,
    );

    let priv_row = matter
        .get_item_privilege(&item.id)
        .expect("get")
        .expect("row");
    assert_eq!(priv_row.status, privilege_status::PARTIAL_REDACTION);
    assert_eq!(priv_row.withhold, 1);
    assert_eq!(priv_row.include_on_log, 1);

    let withhold: i64 = matter
        .connection()
        .query_row(
            "SELECT privilege_withhold FROM items WHERE id = ?1",
            [item.id.as_str()],
            |row| row.get(0),
        )
        .expect("cache");
    assert_eq!(withhold, 1);
}

#[test]
fn filter_has_redactions() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-filter");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Hello SECRET";
    let (with_red, digest) = insert_text_item(&matter, body);
    let _without = insert_text_item(&matter, "plain");
    make_redaction(
        &matter,
        &with_red.id,
        body,
        &digest,
        6,
        12,
        redaction_reason::PII,
    );

    let mut has = FilterSpec::preset_has_redactions();
    has.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter.list_items_filtered_thin(&has, 100, 0).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, with_red.id);

    // Stale signal: count>0 and no artifact yet
    let mut stale = FilterSpec::preset_redacted_text_stale();
    stale.scope = SCOPE_ENTIRE_MATTER.into();
    let stale_rows = matter
        .list_items_filtered_thin(&stale, 100, 0)
        .expect("stale");
    assert_eq!(stale_rows.len(), 1);
    assert_eq!(stale_rows[0].id, with_red.id);

    matter
        .regenerate_redacted_text(&with_red.id, body, "bob")
        .expect("regen");
    let stale_after = matter
        .list_items_filtered_thin(&stale, 100, 0)
        .expect("stale after");
    assert!(
        stale_after.is_empty(),
        "fresh artifact should clear redacted_text_stale"
    );
}

#[test]
fn invalid_range_and_quote_error() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-err");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Hello world";
    let (item, digest) = insert_text_item(&matter, body);

    let err = matter
        .create_redaction(CreateRedactionInput {
            item_id: item.id.clone(),
            start_utf8: 5,
            end_utf8: 5,
            exact_quote: "".into(),
            display_body: body.into(),
            body_digest: digest.clone(),
            reason: redaction_reason::OTHER.into(),
            label: None,
            actor: "a".into(),
        })
        .expect_err("empty range");
    assert!(err.to_string().contains("invalid") || err.to_string().contains("must be"));

    let err2 = matter
        .create_redaction(CreateRedactionInput {
            item_id: item.id.clone(),
            start_utf8: 0,
            end_utf8: 5,
            exact_quote: "XXXXX".into(),
            display_body: body.into(),
            body_digest: digest.clone(),
            reason: redaction_reason::OTHER.into(),
            label: None,
            actor: "a".into(),
        })
        .expect_err("quote mismatch");
    assert!(err2.to_string().contains("exact_quote"));

    let err3 = matter
        .create_redaction(CreateRedactionInput {
            item_id: item.id.clone(),
            start_utf8: 0,
            end_utf8: 5,
            exact_quote: "Hello".into(),
            display_body: body.into(),
            body_digest: digest,
            reason: "not_a_reason".into(),
            label: None,
            actor: "a".into(),
        })
        .expect_err("bad reason");
    assert!(err3.to_string().contains("reason"));
}

#[test]
fn highlight_create_does_not_create_redaction_rows() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-hl-sep");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "Hello yellow world";
    let (item, digest) = insert_text_item(&matter, body);

    matter
        .create_highlight(CreateHighlightInput {
            item_id: item.id.clone(),
            start_utf8: 6,
            end_utf8: 12,
            exact_quote: "yellow".into(),
            display_body: body.into(),
            body_digest: digest,
            color: None,
            actor: "alice".into(),
        })
        .expect("hl");

    assert_eq!(matter.list_highlights(&item.id).expect("hls").len(), 1);
    assert!(matter.list_redactions(&item.id).expect("rdx").is_empty());
    assert_eq!(matter.get_item(&item.id).expect("item").redaction_count, 0);

    let hl_count: i64 = matter
        .connection()
        .query_row(
            "SELECT highlight_count FROM items WHERE id = ?1",
            [item.id.as_str()],
            |row| row.get(0),
        )
        .expect("hl count");
    assert_eq!(hl_count, 1);
}

#[test]
fn create_invalidates_existing_artifact() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-rdx-inval");
    let matter = Matter::create(&root, "Rdx").expect("create");
    let body = "AAA BBB CCC DDD";
    let (item, digest) = insert_text_item(&matter, body);
    make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        0,
        3,
        redaction_reason::OTHER,
    );
    matter
        .regenerate_redacted_text(&item.id, body, "bob")
        .expect("regen");
    assert!(matter
        .get_item(&item.id)
        .unwrap()
        .redacted_text_sha256
        .is_some());

    // Second redaction must NULL artifact.
    make_redaction(
        &matter,
        &item.id,
        body,
        &digest,
        4,
        7,
        redaction_reason::OTHER,
    );
    assert!(matter
        .get_item(&item.id)
        .unwrap()
        .redacted_text_sha256
        .is_none());
}
