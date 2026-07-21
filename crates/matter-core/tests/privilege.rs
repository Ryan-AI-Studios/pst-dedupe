//! Integration tests for privilege workflow (schema v12 / track 0031).

use matter_core::{
    item_role, item_status, ApplyCodesInput, FilterSpec, ItemInput, Matter,
    PrivilegeLogExportParams, UpsertItemPrivilegeInput, UpsertNoteInput,
    UpsertPrivilegeProtocolInput, PRIVILEGE_LOG_COLUMNS, SCHEMA_VERSION, SCOPE_ENTIRE_MATTER,
    SCOPE_REVIEW_CORPUS,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

fn defs_by_key(matter: &Matter) -> std::collections::HashMap<String, matter_core::CodeDef> {
    matter
        .list_code_definitions()
        .expect("defs")
        .into_iter()
        .map(|d| (d.key.clone(), d))
        .collect()
}

fn insert_item(matter: &Matter, subject: &str) -> matter_core::Item {
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some(subject.into()),
            path: Some(format!("{subject}.eml")),
            in_review: Some(1),
            from_addr: Some("lawyer@firm.com".into()),
            to_addrs_json: Some(r#"["client@corp.com"]"#.into()),
            sent_at: Some("2024-06-01T12:00:00Z".into()),
            file_category: Some("email".into()),
            custodian: Some("Alice".into()),
            ..Default::default()
        })
        .expect("item")
}

#[test]
fn schema_v12_on_create() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-v12");
    let matter = Matter::create(&root, "V12").expect("create");
    assert_eq!(SCHEMA_VERSION, 35);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);

    let has: bool = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='item_privilege'",
            [],
            |row| row.get(0),
        )
        .expect("table");
    assert!(has);
}

#[test]
fn apply_privilege_code_ensures_row_withhold() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-apply");
    let matter = Matter::create(&root, "Priv").expect("create");
    let item = insert_item(&matter, "DocA");
    let by_key = defs_by_key(&matter);

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.id.clone()],
            add_code_ids: vec![by_key["privilege"].id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("apply");

    let priv_row = matter
        .get_item_privilege(&item.id)
        .expect("get")
        .expect("row");
    assert_eq!(priv_row.status, "asserted");
    assert_eq!(priv_row.withhold, 1);
    assert_eq!(priv_row.include_on_log, 1);
    assert_eq!(priv_row.basis, "attorney_client");
    assert!(matter.item_is_withheld(&item.id).expect("withheld"));

    let cache: i64 = matter
        .connection()
        .query_row(
            "SELECT privilege_withhold FROM items WHERE id = ?1",
            [item.id.as_str()],
            |row| row.get(0),
        )
        .expect("cache");
    assert_eq!(cache, 1);
}

#[test]
fn upsert_description_basis_audits() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-upsert");
    let matter = Matter::create(&root, "Priv").expect("create");
    let item = insert_item(&matter, "DocB");
    matter
        .ensure_item_privilege(&item.id, "alice")
        .expect("ensure");

    let updated = matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: item.id.clone(),
            basis: "work_product".into(),
            description: "Legal advice re contract negotiation".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "alice".into(),
        })
        .expect("upsert");
    assert_eq!(updated.basis, "work_product");
    assert_eq!(updated.description, "Legal advice re contract negotiation");

    let params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'privilege.upsert' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params).expect("json");
    assert_eq!(v["basis"], "work_product");
    assert_eq!(v["description"], "Legal advice re contract negotiation");
    assert_eq!(v["op"], "upsert");
}

#[test]
fn remove_privilege_soft_clears_retains_description() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-clear");
    let matter = Matter::create(&root, "Priv").expect("create");
    let item = insert_item(&matter, "DocC");
    let by_key = defs_by_key(&matter);

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.id.clone()],
            add_code_ids: vec![by_key["privilege"].id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("apply");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: item.id.clone(),
            basis: "attorney_client".into(),
            description: "Keep me for audit".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "tester".into(),
        })
        .expect("desc");

    matter
        .apply_codes(ApplyCodesInput {
            item_ids: vec![item.id.clone()],
            add_code_ids: vec![],
            remove_code_ids: vec![by_key["privilege"].id.clone()],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("remove");

    let priv_row = matter
        .get_item_privilege(&item.id)
        .expect("get")
        .expect("row retained");
    assert_eq!(priv_row.status, "cleared");
    assert_eq!(priv_row.withhold, 0);
    assert_eq!(priv_row.include_on_log, 0);
    assert_eq!(priv_row.description, "Keep me for audit");
    assert!(!matter.item_is_withheld(&item.id).expect("withheld"));

    let clear_audit: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'privilege.clear'",
            [],
            |row| row.get(0),
        )
        .expect("clear audit");
    assert!(clear_audit >= 1);
}

#[test]
fn export_csv_two_items_headers() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-export");
    let matter = Matter::create(&root, "Priv").expect("create");
    let a = insert_item(&matter, "ExportA");
    let b = insert_item(&matter, "ExportB");
    for id in [&a.id, &b.id] {
        matter.ensure_item_privilege(id, "exp").expect("ensure");
        matter
            .upsert_item_privilege(UpsertItemPrivilegeInput {
                item_id: id.clone(),
                basis: "attorney_client".into(),
                description: format!("desc for {id}"),
                status: "asserted".into(),
                withhold: true,
                include_on_log: true,
                actor: "exp".into(),
            })
            .expect("upsert");
    }

    let out = root.join("exports").join("priv_log.csv");
    let result = matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_REVIEW_CORPUS.into(),
            path: out.clone(),
            filter_ids: None,
        })
        .expect("export");
    assert_eq!(result.row_count, 2);
    assert_eq!(result.blank_description_count, 0);
    assert_eq!(result.withheld_count, 2);

    let text = std::fs::read_to_string(out.as_std_path()).expect("read csv");
    let mut lines = text.lines();
    let header = lines.next().expect("header");
    assert_eq!(header, PRIVILEGE_LOG_COLUMNS.join(","));
    let data: Vec<_> = lines.filter(|l| !l.is_empty()).collect();
    assert_eq!(data.len(), 2);
    assert!(data.iter().any(|l| l.contains(&a.id)));
    assert!(data.iter().any(|l| l.contains(&b.id)));

    let audit: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'privilege.log_export'",
            [],
            |row| row.get(0),
        )
        .expect("export audit");
    assert_eq!(audit, 1);

    // Export audit must include params_hash + path basename (P2-004).
    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'privilege.log_export' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("export audit params");
    let v: serde_json::Value = serde_json::from_str(&params_json).expect("params json");
    let hash = v["params_hash"].as_str().expect("params_hash present");
    assert_eq!(hash.len(), 64, "params_hash is sha256 hex");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(v["path_basename"].as_str(), Some("priv_log.csv"));
    assert_eq!(v["row_count"], 2);
    assert_eq!(v["blank_description_count"], 0);
    assert_eq!(v["withheld_count"], 2);
    assert_eq!(v["scope"], SCOPE_REVIEW_CORPUS);
    assert_eq!(v["review_only"], true);
    // Recompute expected hash (scope + empty filter_ids + full path).
    let expected_preimage = format!(
        "scope={}\nfilter_ids=\npath={}",
        SCOPE_REVIEW_CORPUS, result.path
    );
    let expected = matter_core::sha256_hex(expected_preimage.as_bytes());
    assert_eq!(hash, expected);
}

#[test]
fn attachment_inheritance_on_export() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-attach");
    let matter = Matter::create(&root, "Priv").expect("create");

    let fam = matter
        .insert_family(matter_core::FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            subject: Some("Parent subject re advice".into()),
            from_addr: Some("counsel@firm.com".into()),
            to_addrs_json: Some(r#"["ceo@corp.com"]"#.into()),
            cc_addrs_json: Some(r#"["gc@corp.com"]"#.into()),
            sent_at: Some("2024-03-15T09:00:00Z".into()),
            path: Some("inbox/parent.eml".into()),
            file_category: Some("email".into()),
            custodian: Some("Counsel".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("parent");
    let child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            path: Some("inbox/parent/memo.pdf".into()),
            file_category: Some("pdf".into()),
            // Intentionally null from/to/subject/sent for inheritance test.
            in_review: Some(1),
            ..Default::default()
        })
        .expect("child");

    matter
        .ensure_item_privilege(&child.id, "att")
        .expect("ensure");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: child.id.clone(),
            basis: "attorney_client".into(),
            description: "Attachment containing legal advice".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "att".into(),
        })
        .expect("upsert");

    let out = root.join("exports").join("attach_log.csv");
    let result = matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_ENTIRE_MATTER.into(),
            path: out.clone(),
            filter_ids: None,
        })
        .expect("export");
    assert_eq!(result.row_count, 1);

    let text = std::fs::read_to_string(out.as_std_path()).expect("read");
    let data_line = text.lines().nth(1).expect("data row");
    assert!(
        data_line.contains("counsel@firm.com"),
        "From inherited: {data_line}"
    );
    assert!(
        data_line.contains("ceo@corp.com"),
        "To inherited: {data_line}"
    );
    assert!(
        data_line.contains("Parent subject re advice"),
        "Subject inherited: {data_line}"
    );
    assert!(
        data_line.contains("2024-03-15T09:00:00Z"),
        "DocDate inherited: {data_line}"
    );
    assert!(
        data_line.contains("memo.pdf"),
        "FileName is child basename: {data_line}"
    );
    assert!(
        data_line.contains(&parent.id),
        "ParentControlNumber: {data_line}"
    );
}

/// Empty item subject must not shadow a non-empty item title (P2-001).
#[test]
fn attachment_empty_subject_prefers_item_title_over_parent() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-subj-title");
    let matter = Matter::create(&root, "Priv").expect("create");

    let fam = matter
        .insert_family(matter_core::FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            subject: Some("Parent subject should not win".into()),
            from_addr: Some("counsel@firm.com".into()),
            to_addrs_json: Some(r#"["ceo@corp.com"]"#.into()),
            sent_at: Some("2024-03-15T09:00:00Z".into()),
            path: Some("inbox/parent.eml".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("parent");
    let child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            // Empty subject must not win over title via Option::or.
            subject: Some("".into()),
            title: Some("Memo".into()),
            path: Some("inbox/parent/memo.pdf".into()),
            file_category: Some("pdf".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("child");

    matter
        .ensure_item_privilege(&child.id, "att")
        .expect("ensure");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: child.id.clone(),
            basis: "attorney_client".into(),
            description: "Attachment memo".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "att".into(),
        })
        .expect("upsert");

    let out = root.join("exports").join("subj_title.csv");
    matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_ENTIRE_MATTER.into(),
            path: out.clone(),
            filter_ids: None,
        })
        .expect("export");

    let text = std::fs::read_to_string(out.as_std_path()).expect("read");
    let data_line = text.lines().nth(1).expect("data row");
    assert!(
        data_line.contains("Memo"),
        "CSV Subject should be item title, got: {data_line}"
    );
    assert!(
        !data_line.contains("Parent subject should not win"),
        "parent subject must not win over item title: {data_line}"
    );
}

/// When item subject and title are both empty, inherit parent subject (P2-001).
#[test]
fn attachment_empty_subject_and_title_inherits_parent_subject() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-subj-parent");
    let matter = Matter::create(&root, "Priv").expect("create");

    let fam = matter
        .insert_family(matter_core::FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            subject: Some("Inherited parent subject".into()),
            path: Some("inbox/parent.eml".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("parent");
    let child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            subject: Some("".into()),
            title: Some("".into()),
            path: Some("inbox/parent/blank.pdf".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("child");

    matter
        .ensure_item_privilege(&child.id, "att")
        .expect("ensure");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: child.id.clone(),
            basis: "attorney_client".into(),
            description: "blank meta attach".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "att".into(),
        })
        .expect("upsert");

    let out = root.join("exports").join("subj_parent.csv");
    matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_ENTIRE_MATTER.into(),
            path: out.clone(),
            filter_ids: None,
        })
        .expect("export");

    let text = std::fs::read_to_string(out.as_std_path()).expect("read");
    let data_line = text.lines().nth(1).expect("data row");
    assert!(
        data_line.contains("Inherited parent subject"),
        "CSV Subject should fall back to parent: {data_line}"
    );
}

#[test]
fn blank_description_warns() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-blank");
    let matter = Matter::create(&root, "Priv").expect("create");
    let item = insert_item(&matter, "Blank");
    matter.ensure_item_privilege(&item.id, "b").expect("ensure");
    // description left empty

    let out = root.join("exports").join("blank.csv");
    let result = matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_REVIEW_CORPUS.into(),
            path: out,
            filter_ids: None,
        })
        .expect("export");
    assert_eq!(result.row_count, 1);
    assert!(result.blank_description_count >= 1);
}

#[test]
fn include_on_log_zero_and_cleared_omitted() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-omit");
    let matter = Matter::create(&root, "Priv").expect("create");
    let keep = insert_item(&matter, "Keep");
    let no_log = insert_item(&matter, "NoLog");
    let cleared = insert_item(&matter, "Cleared");

    for id in [&keep.id, &no_log.id, &cleared.id] {
        matter.ensure_item_privilege(id, "o").expect("ensure");
    }
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: no_log.id.clone(),
            basis: "attorney_client".into(),
            description: "off log".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: false,
            actor: "o".into(),
        })
        .expect("nolog");
    matter
        .clear_item_privilege(&cleared.id, "o")
        .expect("clear");

    let out = root.join("exports").join("omit.csv");
    let result = matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_ENTIRE_MATTER.into(),
            path: out.clone(),
            filter_ids: None,
        })
        .expect("export");
    assert_eq!(result.row_count, 1);
    let text = std::fs::read_to_string(out.as_std_path()).expect("read");
    assert!(text.contains(&keep.id));
    assert!(!text.contains(&no_log.id));
    assert!(!text.contains(&cleared.id));
}

#[test]
fn review_corpus_scope_excludes_non_review() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-scope");
    let matter = Matter::create(&root, "Priv").expect("create");
    let in_rev = insert_item(&matter, "InRev");
    let not_rev = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("NotRev".into()),
            path: Some("notrev.eml".into()),
            in_review: Some(0),
            from_addr: Some("a@b.com".into()),
            sent_at: Some("2024-01-01T00:00:00Z".into()),
            file_category: Some("email".into()),
            ..Default::default()
        })
        .expect("not rev");

    for id in [&in_rev.id, &not_rev.id] {
        matter.ensure_item_privilege(id, "s").expect("ensure");
        matter
            .upsert_item_privilege(UpsertItemPrivilegeInput {
                item_id: id.clone(),
                basis: "attorney_client".into(),
                description: "d".into(),
                status: "asserted".into(),
                withhold: true,
                include_on_log: true,
                actor: "s".into(),
            })
            .expect("u");
    }

    let out = root.join("exports").join("scope.csv");
    let result = matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_REVIEW_CORPUS.into(),
            path: out.clone(),
            filter_ids: None,
        })
        .expect("export");
    assert_eq!(result.row_count, 1);
    let text = std::fs::read_to_string(out.as_std_path()).expect("read");
    assert!(text.contains(&in_rev.id));
    assert!(!text.contains(&not_rev.id));
}

#[test]
fn family_consistency_detects_split() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-fam");
    let matter = Matter::create(&root, "Priv").expect("create");
    let fam = matter
        .insert_family(matter_core::FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam");
    let parent = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            family_id: Some(fam.id.clone()),
            subject: Some("P".into()),
            path: Some("p.eml".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("p");
    let child = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(parent.id.clone()),
            path: Some("p/a.pdf".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("c");

    matter
        .ensure_item_privilege(&parent.id, "f")
        .expect("ensure parent");

    let cons = matter
        .family_privilege_consistency(&parent.id)
        .expect("cons");
    assert!(!cons.consistent);
    assert!(cons.privileged_ids.contains(&parent.id));
    assert!(cons.non_privileged_ids.contains(&child.id));
}

#[test]
fn protocol_upsert_and_audit() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-proto");
    let matter = Matter::create(&root, "Priv").expect("create");

    let default = matter.get_privilege_protocol().expect("default");
    assert_eq!(default.log_format, "standard");
    assert_eq!(default.description_required, 1);

    let up = matter
        .upsert_privilege_protocol(UpsertPrivilegeProtocolInput {
            log_format: "standard".into(),
            fre_502d_note: Some("Order dated 2025-01-01, Dkt. 42".into()),
            fre_502e_note: Some("Clawback §7 ESI protocol".into()),
            description_required: true,
            actor: "counsel".into(),
        })
        .expect("upsert");
    assert!(up.fre_502d_note.as_deref().unwrap().contains("Dkt"));

    let params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'privilege.protocol_upsert' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    assert!(params.contains("502"));
}

#[test]
fn item_is_withheld_matrix() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-matrix");
    let matter = Matter::create(&root, "Priv").expect("create");
    let a = insert_item(&matter, "W1");
    let b = insert_item(&matter, "W0");
    let c = insert_item(&matter, "None");

    matter.ensure_item_privilege(&a.id, "m").expect("a");
    matter.ensure_item_privilege(&b.id, "m").expect("b");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: b.id.clone(),
            basis: "attorney_client".into(),
            description: "intentional produce".into(),
            status: "asserted".into(),
            withhold: false,
            include_on_log: true,
            actor: "m".into(),
        })
        .expect("override");

    assert!(matter.item_is_withheld(&a.id).expect("a"));
    assert!(!matter.item_is_withheld(&b.id).expect("b"));
    assert!(!matter.item_is_withheld(&c.id).expect("c"));

    let list = matter.list_withheld_item_ids().expect("list");
    assert_eq!(list, vec![a.id.clone()]);
}

#[test]
fn withhold_zero_asserted_still_on_log() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-wo");
    let matter = Matter::create(&root, "Priv").expect("create");
    let item = insert_item(&matter, "WO");
    matter.ensure_item_privilege(&item.id, "w").expect("e");
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: item.id.clone(),
            basis: "attorney_client".into(),
            description: "under 502 produce".into(),
            status: "asserted".into(),
            withhold: false,
            include_on_log: true,
            actor: "w".into(),
        })
        .expect("u");

    let params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'privilege.upsert' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params).expect("json");
    assert_eq!(v["withhold"], 0);

    let out = root.join("exports").join("wo.csv");
    let result = matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_REVIEW_CORPUS.into(),
            path: out.clone(),
            filter_ids: None,
        })
        .expect("export");
    assert_eq!(result.row_count, 1);
    assert_eq!(result.withheld_count, 0);
    let text = std::fs::read_to_string(out.as_std_path()).expect("read");
    assert!(text.contains(&item.id));
    assert!(text.contains(",N,")); // Withhold N
}

#[test]
fn notes_body_not_in_csv_by_default() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-notes");
    let matter = Matter::create(&root, "Priv").expect("create");
    let item = insert_item(&matter, "NoteLeak");
    matter.ensure_item_privilege(&item.id, "n").expect("e");
    matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id.clone(),
            body: "SECRET_NOTE_BODY_SHOULD_NOT_APPEAR".into(),
            highlight_id: None,
            actor: "n".into(),
        })
        .expect("note");

    let out = root.join("exports").join("notes.csv");
    matter
        .export_privilege_log(PrivilegeLogExportParams {
            scope: SCOPE_REVIEW_CORPUS.into(),
            path: out.clone(),
            filter_ids: None,
        })
        .expect("export");
    let text = std::fs::read_to_string(out.as_std_path()).expect("read");
    assert!(!text.contains("SECRET_NOTE_BODY_SHOULD_NOT_APPEAR"));
}

#[test]
fn filter_presets_withheld_and_incomplete() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-priv-filter");
    let matter = Matter::create(&root, "Priv").expect("create");
    let ready = insert_item(&matter, "Ready");
    let incomplete = insert_item(&matter, "Incomplete");
    let clear = insert_item(&matter, "Clear");

    for id in [&ready.id, &incomplete.id] {
        matter.ensure_item_privilege(id, "f").expect("e");
    }
    matter
        .upsert_item_privilege(UpsertItemPrivilegeInput {
            item_id: ready.id.clone(),
            basis: "attorney_client".into(),
            description: "ready desc".into(),
            status: "asserted".into(),
            withhold: true,
            include_on_log: true,
            actor: "f".into(),
        })
        .expect("ready");
    // incomplete: blank description
    matter.ensure_item_privilege(&clear.id, "f").expect("e");
    matter.clear_item_privilege(&clear.id, "f").expect("clear");

    let withheld = FilterSpec::preset_withheld();
    let count = matter
        .count_items_filtered(&withheld)
        .expect("withheld count");
    let rows = matter
        .list_items_filtered_thin(&withheld, 50, 0)
        .expect("withheld");
    assert_eq!(count, 2);
    let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&ready.id.as_str()));
    assert!(ids.contains(&incomplete.id.as_str()));

    let incomplete_spec = FilterSpec::preset_privilege_log_incomplete();
    let count2 = matter
        .count_items_filtered(&incomplete_spec)
        .expect("incomplete count");
    let rows2 = matter
        .list_items_filtered_thin(&incomplete_spec, 50, 0)
        .expect("incomplete");
    assert_eq!(count2, 1);
    assert_eq!(rows2[0].id, incomplete.id);
}
