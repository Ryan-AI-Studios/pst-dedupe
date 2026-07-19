//! Integration tests for metadata filters + saved searches (schema v9 / track 0028).

use matter_core::{
    item_role, item_status, ApplyCodesInput, FilterCondition, FilterSpec, ItemInput, Matter,
    SavedSearchInput, SCHEMA_VERSION, SCOPE_ENTIRE_MATTER, SCOPE_REVIEW_CORPUS,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

fn cond_eq(field: &str, value: &str) -> FilterCondition {
    FilterCondition {
        field: field.into(),
        op: "eq".into(),
        value: Some(serde_json::json!(value)),
        values: None,
        start: None,
        end: None,
    }
}

fn cond_contains(field: &str, value: &str) -> FilterCondition {
    FilterCondition {
        field: field.into(),
        op: "contains".into(),
        value: Some(serde_json::json!(value)),
        values: None,
        start: None,
        end: None,
    }
}

fn cond_code_any_of(keys: &[&str]) -> FilterCondition {
    FilterCondition {
        field: "code".into(),
        op: "any_of".into(),
        value: None,
        values: Some(keys.iter().map(|s| (*s).to_string()).collect()),
        start: None,
        end: None,
    }
}

fn cond_code_missing() -> FilterCondition {
    FilterCondition {
        field: "code_missing".into(),
        op: "eq".into(),
        value: Some(serde_json::Value::Bool(true)),
        values: None,
        start: None,
        end: None,
    }
}

fn cond_code_none_of(keys: &[&str]) -> FilterCondition {
    FilterCondition {
        field: "code".into(),
        op: "none_of".into(),
        value: None,
        values: Some(keys.iter().map(|s| (*s).to_string()).collect()),
        start: None,
        end: None,
    }
}

fn promote_item(
    matter: &Matter,
    item_id: &str,
    set_id: &str,
    order: i64,
) -> Result<(), matter_core::Error> {
    matter.connection().execute(
        "UPDATE items SET in_review = 1, review_set_id = ?1, review_order = ?2, \
         promoted_at = '2020-01-01T00:00:00Z' WHERE id = ?3",
        rusqlite::params![set_id, order, item_id],
    )?;
    Ok(())
}

fn setup_review_matter(name: &str) -> (tempfile::TempDir, camino::Utf8PathBuf, Matter, String) {
    let (tmp, base) = utf8_tempdir();
    let root = base.join(name);
    let matter = Matter::create(&root, name).expect("create");
    let set = matter
        .ensure_default_review_set(matter_core::DEFAULT_REVIEW_SET_NAME)
        .expect("set");
    (tmp, root, matter, set.id)
}

#[test]
fn filter_custodian_eq_matches_only() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-custodian");
    let alice = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            custodian: Some("alice@example.com".into()),
            subject: Some("A".into()),
            ..Default::default()
        })
        .expect("alice");
    let bob = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            custodian: Some("bob@example.com".into()),
            subject: Some("B".into()),
            ..Default::default()
        })
        .expect("bob");
    promote_item(&matter, &alice.id, &set_id, 1).unwrap();
    promote_item(&matter, &bob.id, &set_id, 2).unwrap();

    let spec = FilterSpec {
        conditions: vec![cond_eq("custodian", "alice@example.com")],
        ..FilterSpec::default()
    };
    let count = matter.count_items_filtered(&spec).expect("count");
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("list");
    assert_eq!(count, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, alice.id);
}

#[test]
fn filter_date_between_offset_required() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-date");
    let early = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            sent_at: Some("2023-06-15T12:00:00Z".into()),
            subject: Some("early".into()),
            ..Default::default()
        })
        .expect("early");
    let late = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            sent_at: Some("2024-06-15T12:00:00Z".into()),
            subject: Some("late".into()),
            ..Default::default()
        })
        .expect("late");
    promote_item(&matter, &early.id, &set_id, 1).unwrap();
    promote_item(&matter, &late.id, &set_id, 2).unwrap();

    let good = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "between".into(),
            value: None,
            values: None,
            start: Some("2023-01-01T00:00:00-05:00".into()),
            end: Some("2024-01-01T00:00:00-05:00".into()),
        }],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&good, 100, 0)
        .expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, early.id);

    let naive = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "between".into(),
            value: None,
            values: None,
            start: Some("2023-01-01T00:00:00".into()),
            end: Some("2024-01-01T00:00:00".into()),
        }],
        ..FilterSpec::default()
    };
    let err = matter
        .list_items_filtered_thin(&naive, 100, 0)
        .expect_err("naive rejected");
    assert!(
        err.to_string().contains("offset") || err.to_string().contains("naive"),
        "got {err}"
    );
}

/// Offset-bearing stored `sent_at` must compare as the UTC instant, not TEXT.
///
/// Stored `2023-01-01T00:00:00-05:00` == `2023-01-01T05:00:00Z`.
/// Lexical TEXT compare vs `2023-01-01T03:00:00Z` would exclude (T00 < T03);
/// correct UTC compare includes (05:00Z >= 03:00Z).
#[test]
fn filter_date_offset_bearing_stored_ts_compares_as_utc() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-date-offset-item");
    let offset_item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            // Instant = 05:00Z; lexical "…T00:00:00-05:00" sorts before "…T03:00:00Z".
            sent_at: Some("2023-01-01T00:00:00-05:00".into()),
            subject: Some("offset-bearing".into()),
            ..Default::default()
        })
        .expect("offset item");
    let late_z = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            sent_at: Some("2023-01-01T06:00:00Z".into()),
            subject: Some("late Z".into()),
            ..Default::default()
        })
        .expect("late Z");
    promote_item(&matter, &offset_item.id, &set_id, 1).unwrap();
    promote_item(&matter, &late_z.id, &set_id, 2).unwrap();

    // gte 03:00Z: offset item (05:00Z) and late_z (06:00Z) both match.
    let gte_03 = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "gte".into(),
            value: Some(serde_json::json!("2023-01-01T03:00:00Z")),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&gte_03, 100, 0)
        .expect("gte 03");
    let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
    assert!(
        ids.contains(&offset_item.id.as_str()),
        "offset-bearing 05:00Z must match gte 03:00Z (UTC); ids={ids:?}"
    );
    assert!(ids.contains(&late_z.id.as_str()));

    // gte 05:30Z: only late_z; pure lexical would also exclude offset item but
    // for the wrong reason — here we assert correct include/exclude boundary.
    let gte_0530 = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "gte".into(),
            value: Some(serde_json::json!("2023-01-01T05:30:00Z")),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&gte_0530, 100, 0)
        .expect("gte 05:30");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, late_z.id);

    // between [04:00Z, 05:30Z): only offset item (05:00Z).
    let between = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "between".into(),
            value: None,
            values: None,
            start: Some("2023-01-01T04:00:00Z".into()),
            end: Some("2023-01-01T05:30:00Z".into()),
        }],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&between, 100, 0)
        .expect("between");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, offset_item.id);
}

/// Subsecond precision must be preserved on date filter compare.
///
/// Prior `SecondsFormat::Secs` text normalize collapsed `.100Z` and `.500Z`
/// to the same second, causing false matches. Epoch-ms compare must not.
#[test]
fn filter_date_subsecond_precision_preserved() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-date-subsec");
    let early_frac = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            sent_at: Some("2023-01-01T00:00:00.100Z".into()),
            subject: Some("early frac".into()),
            ..Default::default()
        })
        .expect("early frac");
    let late_frac = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            sent_at: Some("2023-01-01T00:00:00.600Z".into()),
            subject: Some("late frac".into()),
            ..Default::default()
        })
        .expect("late frac");
    promote_item(&matter, &early_frac.id, &set_id, 1).unwrap();
    promote_item(&matter, &late_frac.id, &set_id, 2).unwrap();

    // gte .500Z: only .600Z; .100Z must be excluded (false match under Secs truncate).
    let gte_500 = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "gte".into(),
            value: Some(serde_json::json!("2023-01-01T00:00:00.500Z")),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&gte_500, 100, 0)
        .expect("gte .500");
    assert_eq!(
        rows.len(),
        1,
        "only .600Z should match gte .500Z; got {:?}",
        rows
    );
    assert_eq!(rows[0].id, late_frac.id);

    // gte .000Z: both items included.
    let gte_000 = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "gte".into(),
            value: Some(serde_json::json!("2023-01-01T00:00:00.000Z")),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&gte_000, 100, 0)
        .expect("gte .000");
    let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&early_frac.id.as_str()));
    assert!(ids.contains(&late_frac.id.as_str()));

    // between [.100Z, .600Z): only early_frac (.100 inclusive, .600 exclusive end).
    let between = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "between".into(),
            value: None,
            values: None,
            start: Some("2023-01-01T00:00:00.100Z".into()),
            end: Some("2023-01-01T00:00:00.600Z".into()),
        }],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&between, 100, 0)
        .expect("between subsec");
    assert_eq!(
        rows.len(),
        1,
        "exclusive end must exclude .600Z; got {:?}",
        rows
    );
    assert_eq!(rows[0].id, early_frac.id);

    // between [.000Z, .100Z): empty (start inclusive would need item < .100).
    let between_empty = FilterSpec {
        conditions: vec![FilterCondition {
            field: "sent_at".into(),
            op: "between".into(),
            value: None,
            values: None,
            start: Some("2023-01-01T00:00:00.000Z".into()),
            end: Some("2023-01-01T00:00:00.100Z".into()),
        }],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&between_empty, 100, 0)
        .expect("between empty end");
    assert!(
        rows.is_empty(),
        "exclusive end at .100Z must exclude item at .100Z; got {:?}",
        rows
    );
}

#[test]
fn filter_code_any_of_responsive() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-code-any");
    let yes = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            subject: Some("yes".into()),
            ..Default::default()
        })
        .expect("yes");
    let no = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            subject: Some("no".into()),
            ..Default::default()
        })
        .expect("no");
    promote_item(&matter, &yes.id, &set_id, 1).unwrap();
    promote_item(&matter, &no.id, &set_id, 2).unwrap();

    let defs = matter.list_code_definitions().expect("defs");
    let resp = defs.iter().find(|d| d.key == "responsive").expect("resp");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![yes.id.clone()],
            add_code_ids: vec![resp.id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("code");

    let spec = FilterSpec {
        conditions: vec![cond_code_any_of(&["responsive"])],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, yes.id);
}

#[test]
fn filter_code_none_of_and_uncoded() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-uncoded");
    let coded = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            subject: Some("coded".into()),
            ..Default::default()
        })
        .expect("coded");
    let bare = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            subject: Some("bare".into()),
            ..Default::default()
        })
        .expect("bare");
    promote_item(&matter, &coded.id, &set_id, 1).unwrap();
    promote_item(&matter, &bare.id, &set_id, 2).unwrap();

    let defs = matter.list_code_definitions().expect("defs");
    let priv_code = defs.iter().find(|d| d.key == "privilege").expect("priv");
    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![coded.id.clone()],
            add_code_ids: vec![priv_code.id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("code");

    let uncoded = FilterSpec {
        conditions: vec![cond_code_missing()],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&uncoded, 100, 0)
        .expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, bare.id);

    let none_priv = FilterSpec {
        conditions: vec![cond_code_none_of(&["privilege"])],
        ..FilterSpec::default()
    };
    let rows2 = matter
        .list_items_filtered_thin(&none_priv, 100, 0)
        .expect("list");
    assert_eq!(rows2.len(), 1);
    assert_eq!(rows2[0].id, bare.id);
}

#[test]
fn filter_scope_review_corpus_excludes_non_review() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-scope");
    let in_rev = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            custodian: Some("alice@example.com".into()),
            subject: Some("in".into()),
            ..Default::default()
        })
        .expect("in");
    let _out = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            custodian: Some("alice@example.com".into()),
            subject: Some("out".into()),
            ..Default::default()
        })
        .expect("out");
    promote_item(&matter, &in_rev.id, &set_id, 1).unwrap();

    let spec = FilterSpec {
        scope: SCOPE_REVIEW_CORPUS.into(),
        conditions: vec![cond_eq("custodian", "alice@example.com")],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, in_rev.id);
}

#[test]
fn include_family_parent_subject_returns_attachments() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-fam-parent");
    let family = matter.insert_family("email_attachments").expect("fam");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            subject: Some("Project X kickoff".into()),
            path: Some("parent.eml".into()),
            ..Default::default()
        })
        .expect("parent");
    let child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("invoice.pdf".into()), // does NOT match Project X
            path: Some("invoice.pdf".into()),
            ..Default::default()
        })
        .expect("child");
    promote_item(&matter, &parent.id, &set_id, 1).unwrap();
    promote_item(&matter, &child.id, &set_id, 2).unwrap();

    let flat = FilterSpec {
        include_family: false,
        conditions: vec![cond_contains("subject", "Project X")],
        ..FilterSpec::default()
    };
    let flat_rows = matter
        .list_items_filtered_thin(&flat, 100, 0)
        .expect("flat");
    assert_eq!(flat_rows.len(), 1);
    assert_eq!(flat_rows[0].id, parent.id);

    let fam = FilterSpec {
        include_family: true,
        conditions: vec![cond_contains("subject", "Project X")],
        ..FilterSpec::default()
    };
    let fam_rows = matter.list_items_filtered_thin(&fam, 100, 0).expect("fam");
    let ids: Vec<_> = fam_rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(fam_rows.len(), 2, "ids={ids:?}");
    assert!(ids.contains(&parent.id.as_str()));
    assert!(
        ids.contains(&child.id.as_str()),
        "attachment must be included"
    );
}

#[test]
fn include_family_attachment_hit_returns_parent_and_siblings() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-fam-child");
    let family = matter.insert_family("email_attachments").expect("fam");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            subject: Some("Cover note".into()),
            path: Some("parent.eml".into()),
            ..Default::default()
        })
        .expect("parent");
    let hit = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("SECRET_TOKEN_xyz".into()),
            path: Some("secret.bin".into()),
            ..Default::default()
        })
        .expect("hit");
    let sib = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("other.pdf".into()),
            path: Some("other.pdf".into()),
            ..Default::default()
        })
        .expect("sib");
    promote_item(&matter, &parent.id, &set_id, 1).unwrap();
    promote_item(&matter, &hit.id, &set_id, 2).unwrap();
    promote_item(&matter, &sib.id, &set_id, 3).unwrap();

    let spec = FilterSpec {
        include_family: true,
        conditions: vec![cond_contains("subject", "SECRET_TOKEN")],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("list");
    let ids: std::collections::HashSet<_> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(parent.id.as_str()));
    assert!(ids.contains(hit.id.as_str()));
    assert!(ids.contains(sib.id.as_str()));
}

#[test]
fn filter_malicious_path_quote_parameterized() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-sql-inject");
    let safe = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            path: Some("inbox/normal.eml".into()),
            subject: Some("safe".into()),
            ..Default::default()
        })
        .expect("safe");
    let evil = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            path: Some("inbox/foo' OR '1'='1.eml".into()),
            subject: Some("evil".into()),
            ..Default::default()
        })
        .expect("evil");
    promote_item(&matter, &safe.id, &set_id, 1).unwrap();
    promote_item(&matter, &evil.id, &set_id, 2).unwrap();

    let spec = FilterSpec {
        conditions: vec![cond_contains("path", "foo' OR '1'='1")],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("list must not error");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, evil.id);
}

#[test]
fn saved_search_upsert_load_delete_roundtrip() {
    let (_tmp, _root, matter, _set_id) = setup_review_matter("filter-saved");
    let spec = FilterSpec {
        conditions: vec![cond_eq("custodian", "alice@example.com")],
        ..FilterSpec::default()
    };
    let filter_json = serde_json::to_string(&spec).expect("json");

    let saved = matter
        .upsert_saved_search(SavedSearchInput {
            id: None,
            name: "Alice docs".into(),
            description: Some("custodian alice".into()),
            filter_json: filter_json.clone(),
            keyword: Some("confidential".into()),
            created_by: Some("tester".into()),
        })
        .expect("upsert");
    assert_eq!(saved.name, "Alice docs");
    assert_eq!(saved.scope, SCOPE_REVIEW_CORPUS);
    assert_eq!(saved.keyword.as_deref(), Some("confidential"));

    let got = matter.get_saved_search(&saved.id).expect("get");
    assert_eq!(got.filter_json, filter_json);
    assert_eq!(got.keyword.as_deref(), Some("confidential"));

    let list = matter.list_saved_searches().expect("list");
    assert_eq!(list.len(), 1);

    // Update
    let updated = matter
        .upsert_saved_search(SavedSearchInput {
            id: Some(saved.id.clone()),
            name: "Alice docs".into(),
            description: Some("updated".into()),
            filter_json,
            keyword: None,
            created_by: Some("tester".into()),
        })
        .expect("update");
    assert_eq!(updated.description.as_deref(), Some("updated"));
    assert!(updated.keyword.is_none());

    // Audit events
    let save_n: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'search.save'",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    assert!(save_n >= 2);

    matter.delete_saved_search(&saved.id).expect("delete");
    assert!(matter.list_saved_searches().expect("list").is_empty());
    let del_n: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'search.delete'",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    assert_eq!(del_n, 1);
}

/// `get_saved_search` must require `matter_id` (defense in depth).
#[test]
fn get_saved_search_scopes_by_matter_id() {
    let (_tmp, _root, matter, _set_id) = setup_review_matter("filter-saved-scope");
    let own = matter
        .upsert_saved_search(SavedSearchInput {
            id: None,
            name: "Mine".into(),
            description: None,
            filter_json: serde_json::to_string(&FilterSpec::default()).unwrap(),
            keyword: None,
            created_by: Some("tester".into()),
        })
        .expect("own");
    // Second matters row so FK allows a foreign saved_search (multi-matter-in-one-db).
    matter
        .connection()
        .execute(
            "INSERT INTO matters (id, name, created_at, schema_version, storage_root) \
             VALUES ('mat_other_not_us', 'Other', '2020-01-01T00:00:00Z', 10, '/tmp/other')",
            [],
        )
        .expect("insert other matter");
    matter
        .connection()
        .execute(
            "INSERT INTO saved_searches (id, matter_id, name, description, scope, filter_json, \
             created_at, updated_at, created_by) \
             VALUES ('ss_foreign', 'mat_other_not_us', 'Foreign', NULL, 'review_corpus', '{}', \
             '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z', NULL)",
            [],
        )
        .expect("insert foreign");

    assert_eq!(matter.get_saved_search(&own.id).expect("own").id, own.id);
    let err = matter
        .get_saved_search("ss_foreign")
        .expect_err("foreign matter_id must not resolve");
    assert!(
        err.to_string().contains("not found"),
        "expected not found, got {err}"
    );
}

#[test]
fn filter_paging_offset_disjoint_count_stable() {
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-page");
    for i in 0..10 {
        let item = matter
            .insert_item(ItemInput {
                status: item_status::EXTRACTED.into(),
                custodian: Some("alice@example.com".into()),
                subject: Some(format!("doc-{i}")),
                path: Some(format!("p/{i}")),
                ..Default::default()
            })
            .expect("item");
        promote_item(&matter, &item.id, &set_id, i).unwrap();
    }
    // One non-matching
    let other = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            custodian: Some("bob@example.com".into()),
            subject: Some("other".into()),
            ..Default::default()
        })
        .expect("other");
    promote_item(&matter, &other.id, &set_id, 99).unwrap();

    let spec = FilterSpec {
        conditions: vec![cond_eq("custodian", "alice@example.com")],
        ..FilterSpec::default()
    };
    let count = matter.count_items_filtered(&spec).expect("count");
    assert_eq!(count, 10);

    let page0 = matter.list_items_filtered_thin(&spec, 4, 0).expect("p0");
    let page1 = matter.list_items_filtered_thin(&spec, 4, 4).expect("p1");
    let page2 = matter.list_items_filtered_thin(&spec, 4, 8).expect("p2");
    assert_eq!(page0.len(), 4);
    assert_eq!(page1.len(), 4);
    assert_eq!(page2.len(), 2);

    let mut ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for p in [&page0, &page1, &page2] {
        for r in p {
            assert!(ids.insert(r.id.clone()), "duplicate across pages: {}", r.id);
        }
    }
    assert_eq!(ids.len(), 10);
    assert_eq!(matter.count_items_filtered(&spec).expect("count2"), 10);
}

#[test]
fn filter_has_notes_and_note_text() {
    use matter_core::UpsertNoteInput;

    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-has-notes");
    let with_note = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            subject: Some("noted".into()),
            ..Default::default()
        })
        .expect("with");
    let bare = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            subject: Some("bare".into()),
            ..Default::default()
        })
        .expect("bare");
    promote_item(&matter, &with_note.id, &set_id, 1).unwrap();
    promote_item(&matter, &bare.id, &set_id, 2).unwrap();

    matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: with_note.id.clone(),
            body: "special counsel phrase".into(),
            highlight_id: None,
            actor: "tester".into(),
        })
        .expect("note");

    let has = FilterSpec::preset_has_notes();
    let rows = matter.list_items_filtered_thin(&has, 100, 0).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, with_note.id);

    let text = FilterSpec {
        conditions: vec![FilterCondition {
            field: "note_text".into(),
            op: "contains".into(),
            value: Some(serde_json::json!("counsel")),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    let rows2 = matter
        .list_items_filtered_thin(&text, 100, 0)
        .expect("text");
    assert_eq!(rows2.len(), 1);
    assert_eq!(rows2[0].id, with_note.id);

    // Saved-search round-trip still FilterSpec v1.
    let j = serde_json::to_string(&has).expect("ser");
    let back: FilterSpec = serde_json::from_str(&j).expect("de");
    assert_eq!(back, has);
}

#[test]
fn filter_pdf_needs_ocr_preset() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-pdf-ocr-filter");
    let matter = Matter::create(&root, "PdfOcr").expect("create");

    let needs = matter
        .insert_item(ItemInput {
            path: Some("scan.pdf".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect("needs");
    let plain = matter
        .insert_item(ItemInput {
            path: Some("ok.pdf".into()),
            status: item_status::EXTRACTED.into(),
            ..Default::default()
        })
        .expect("plain");

    matter
        .connection()
        .execute(
            "UPDATE items SET pdf_needs_ocr = 1 WHERE id = ?1",
            rusqlite::params![needs.id],
        )
        .expect("set needs");
    matter
        .connection()
        .execute(
            "UPDATE items SET pdf_needs_ocr = 0 WHERE id = ?1",
            rusqlite::params![plain.id],
        )
        .expect("set plain");

    let mut spec = FilterSpec::preset_pdf_needs_ocr();
    spec.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, needs.id);

    let j = serde_json::to_string(&spec).expect("ser");
    let back: FilterSpec = serde_json::from_str(&j).expect("de");
    assert_eq!(back, spec);
}

#[test]
fn migration_has_review_list_order_index() {
    assert_eq!(SCHEMA_VERSION, 18);
    let (_tmp, _root, matter, _set_id) = setup_review_matter("filter-idx");
    let exists: bool = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master \
             WHERE type='index' AND name='idx_items_review_list_order'",
            [],
            |row| row.get(0),
        )
        .expect("idx");
    assert!(exists);
    let has_saved: bool = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master \
             WHERE type='table' AND name='saved_searches'",
            [],
            |row| row.get(0),
        )
        .expect("table");
    assert!(has_saved);
}

#[test]
fn include_family_outer_keeps_review_scope() {
    // Attachment outside review corpus must not appear when parent matches.
    let (_tmp, _root, matter, set_id) = setup_review_matter("filter-fam-scope");
    let family = matter.insert_family("email_attachments").expect("fam");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(family.id.clone()),
            subject: Some("Project Alpha".into()),
            ..Default::default()
        })
        .expect("parent");
    let orphan_child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(family.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("att".into()),
            ..Default::default()
        })
        .expect("child");
    promote_item(&matter, &parent.id, &set_id, 1).unwrap();
    // orphan_child intentionally NOT in review

    let spec = FilterSpec {
        include_family: true,
        conditions: vec![cond_contains("subject", "Project Alpha")],
        ..FilterSpec::default()
    };
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, parent.id);
    assert!(!rows.iter().any(|r| r.id == orphan_child.id));
}
