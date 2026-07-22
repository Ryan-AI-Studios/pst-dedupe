//! AI config + suggestion API tests (schema v30/v31 / tracks 0051–0052).

use matter_core::{
    catalog_content_hash, item_role, item_status, AiMatterConfig, CodeDefInput,
    InsertAiCitationInput, InsertAiSuggestionInput, ItemInput, Matter, UpdateAiMatterConfigInput,
    AI_PROVIDER_MOCK, AI_SUGGESTION_ACCEPTED, AI_SUGGESTION_PENDING, AI_SUGGESTION_REJECTED,
    AI_SUGGESTION_TYPE_CODE, SCHEMA_VERSION, VERIFY_MATCHED, VERIFY_QUOTE_NOT_FOUND,
};
use tempfile::TempDir;

fn utf8_tempdir() -> (TempDir, camino::Utf8PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    (tmp, base)
}

#[test]
fn schema_version_is_31() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI").expect("create");
    assert_eq!(SCHEMA_VERSION, 38);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);
}

#[test]
fn ai_config_defaults_disabled() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI").expect("create");
    let cfg = matter.get_ai_config().expect("cfg");
    assert!(!cfg.ai_enabled);
    assert!(!cfg.ai_allow_remote);
    assert!(cfg.ai_base_url.is_none());
    assert!(cfg.ai_model.is_none());
}

#[test]
fn update_ai_config_no_key_column() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI").expect("create");
    matter
        .update_ai_config(UpdateAiMatterConfigInput {
            enabled: true,
            allow_remote: false,
            base_url: Some("http://127.0.0.1:11434/v1"),
            model: Some("llama3.2"),
            provider_kind: Some(AI_PROVIDER_MOCK),
        })
        .expect("update");
    let cfg: AiMatterConfig = matter.get_ai_config().expect("cfg");
    assert!(cfg.ai_enabled);
    assert!(!cfg.ai_allow_remote);
    assert_eq!(
        cfg.ai_base_url.as_deref(),
        Some("http://127.0.0.1:11434/v1")
    );
    assert_eq!(cfg.ai_model.as_deref(), Some("llama3.2"));
    assert_eq!(cfg.ai_provider_kind.as_deref(), Some(AI_PROVIDER_MOCK));
}

#[test]
fn code_guidance_roundtrip() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI").expect("create");
    let id = matter
        .upsert_code_definition(CodeDefInput {
            id: None,
            key: Some("custom_issue".into()),
            label: "Custom".into(),
            group_key: "issues".into(),
            cardinality: "multi".into(),
            color: None,
            sort_order: 100,
            is_active: true,
            guidance: Some("XYZZY_ONLY_APPLY_WHEN_FOO".into()),
        })
        .expect("upsert");
    let def = matter.get_code_definition(&id).expect("get");
    assert_eq!(def.guidance.as_deref(), Some("XYZZY_ONLY_APPLY_WHEN_FOO"));
    let hash = catalog_content_hash(&matter.list_code_definitions().expect("list"));
    assert_eq!(hash.len(), 64);
}

#[test]
fn accept_applies_code_reject_does_not() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some("Doc".into()),
            text_sha256: Some("abc".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot");

    let sid = matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: Some(&hot.id),
            code_name: "hot",
            confidence: Some(0.9),
            rationale: Some("mock"),
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v1",
            is_remote: false,
            text_sha256: Some("abc"),
            catalog_content_hash: Some("cat"),
            job_id: None,
        })
        .expect("insert");

    let codes_before = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes");
    assert!(codes_before[&item.id].is_empty());

    let accepted = matter
        .accept_ai_suggestion(&sid, "reviewer")
        .expect("accept");
    assert_eq!(accepted.status, AI_SUGGESTION_ACCEPTED);
    let codes = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes");
    assert_eq!(codes[&item.id].len(), 1);
    assert_eq!(codes[&item.id][0].key, "hot");

    // Second suggestion reject does not add codes.
    let sid2 = matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: None,
            code_name: "confidential",
            confidence: Some(0.5),
            rationale: None,
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v1",
            is_remote: false,
            text_sha256: Some("abc"),
            catalog_content_hash: Some("cat"),
            job_id: None,
        })
        .expect("insert2");
    let rejected = matter
        .reject_ai_suggestion(&sid2, "reviewer")
        .expect("reject");
    assert_eq!(rejected.status, AI_SUGGESTION_REJECTED);
    let codes2 = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes2");
    assert_eq!(codes2[&item.id].len(), 1); // still only hot
}

#[test]
fn job_insert_does_not_write_item_codes() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            text_sha256: Some("deadbeef".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: None,
            code_name: "responsive",
            confidence: None,
            rationale: None,
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v1",
            is_remote: false,
            text_sha256: Some("deadbeef"),
            catalog_content_hash: Some("h"),
            job_id: Some("job1"),
        })
        .expect("sugg");
    let codes = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes");
    assert!(codes[&item.id].is_empty());
    let pending = matter
        .list_pending_ai_suggestions_for_item(&item.id)
        .expect("pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, AI_SUGGESTION_PENDING);
}

#[test]
fn fingerprint_skip_helper() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            text_sha256: Some("t1".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    assert!(!matter
        .has_matching_ai_suggestion_fingerprint(&item.id, "t1", "mock", "suggest_codes_v1", "c1")
        .expect("fp"));
    matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: None,
            code_name: "hot",
            confidence: None,
            rationale: None,
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v1",
            is_remote: false,
            text_sha256: Some("t1"),
            catalog_content_hash: Some("c1"),
            job_id: None,
        })
        .expect("ins");
    assert!(matter
        .has_matching_ai_suggestion_fingerprint(&item.id, "t1", "mock", "suggest_codes_v1", "c1")
        .expect("fp2"));
    assert!(!matter
        .has_matching_ai_suggestion_fingerprint(&item.id, "t1", "mock", "suggest_codes_v1", "c2")
        .expect("fp3"));
    // Template is part of the fingerprint: a v1 row must not skip a v2 job.
    assert!(
        !matter
            .has_matching_ai_suggestion_fingerprint(
                &item.id,
                "t1",
                "mock",
                "suggest_codes_v2",
                "c1"
            )
            .expect("fp v2"),
        "v1 suggestion fingerprint must not skip suggest_codes_v2"
    );
}

#[test]
fn long_quote_citation_roundtrip_no_truncate() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI cite").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            text_sha256: Some("abc".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    let sid = matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: None,
            code_name: "hot",
            confidence: Some(0.5),
            rationale: None,
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v2",
            is_remote: false,
            text_sha256: Some("abc"),
            catalog_content_hash: Some("c"),
            job_id: None,
        })
        .expect("sugg");
    let long_quote = "evidence ".repeat(100); // ~900 chars — no hard truncate
    matter
        .insert_ai_suggestion_citations(&[InsertAiCitationInput {
            suggestion_id: &sid,
            item_id: &item.id,
            ordinal: 0,
            quote: &long_quote,
            start_offset: Some(0),
            end_offset: Some(long_quote.len() as i64),
            field: "text",
            verify_status: VERIFY_MATCHED,
        }])
        .expect("cites");
    let loaded = matter.list_ai_suggestion_citations(&sid).expect("list");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].quote, long_quote);
    assert_eq!(loaded[0].quote.len(), long_quote.len());
    let sugg = matter.get_ai_suggestion(&sid).expect("get");
    assert_eq!(sugg.citations_count, 1);
}

#[test]
fn accept_audit_has_pointers_no_quote_cleartext() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI audit").expect("create");
    let secret_quote = "TOP_SECRET_QUOTE_MUST_NOT_APPEAR_IN_AUDIT_XYZ99";
    let body = format!("prefix {secret_quote} suffix unique_only_here");
    let digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            text_sha256: Some(digest.clone()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot");
    let q_start = body.find(secret_quote).expect("pos") as i64;
    let q_end = q_start + secret_quote.len() as i64;
    let sid = matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: Some(&hot.id),
            code_name: "hot",
            confidence: Some(0.9),
            rationale: Some("r"),
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock-model",
            prompt_template_id: "suggest_codes_v2",
            is_remote: false,
            text_sha256: Some(&digest),
            catalog_content_hash: Some("cat"),
            job_id: None,
        })
        .expect("sugg");
    matter
        .insert_ai_suggestion_citations(&[
            InsertAiCitationInput {
                suggestion_id: &sid,
                item_id: &item.id,
                ordinal: 0,
                quote: secret_quote,
                start_offset: Some(q_start),
                end_offset: Some(q_end),
                field: "text",
                verify_status: VERIFY_MATCHED,
            },
            InsertAiCitationInput {
                suggestion_id: &sid,
                item_id: &item.id,
                ordinal: 1,
                quote: "missing",
                start_offset: None,
                end_offset: None,
                field: "text",
                verify_status: VERIFY_QUOTE_NOT_FOUND,
            },
        ])
        .expect("cites");

    matter
        .accept_ai_suggestion(&sid, "reviewer")
        .expect("accept");

    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'ai_suggestion.accept' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    assert!(
        !params_json.contains(secret_quote),
        "audit must not contain quote cleartext: {params_json}"
    );
    assert!(
        !params_json.contains("\"quote\""),
        "audit must not have quote keys: {params_json}"
    );
    let v: serde_json::Value = serde_json::from_str(&params_json).expect("json");
    assert_eq!(v["suggestion_id"], sid);
    assert_eq!(v["prompt_template_id"], "suggest_codes_v2");
    assert_eq!(v["model"], "mock-model");
    assert_eq!(v["provider_kind"], AI_PROVIDER_MOCK);
    assert_eq!(v["is_remote"], false);
    assert_eq!(v["text_sha256"], digest);
    assert_eq!(v["citation_unverified"], true); // second cite not matched
    assert_eq!(v["cas_text_unavailable"], false);
    let cites = v["citations"].as_array().expect("citations arr");
    assert_eq!(cites.len(), 2);
    assert!(cites[0]["citation_id"].as_str().is_some());
    assert_eq!(cites[0]["start_offset"], q_start);
    assert_eq!(cites[0]["end_offset"], q_end);
    assert_eq!(cites[0]["verify_status"], VERIFY_MATCHED);
    assert!(!cites[0].as_object().unwrap().contains_key("quote"));
}

#[test]
fn accept_cas_unavailable_marks_citations_unverified() {
    // Digest points at a non-existent CAS blob: accept must not claim matched.
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI cas miss").expect("create");
    let fake_digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            text_sha256: Some(fake_digest.into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot");
    let sid = matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: Some(&hot.id),
            code_name: "hot",
            confidence: Some(0.9),
            rationale: None,
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v2",
            is_remote: false,
            text_sha256: Some(fake_digest),
            catalog_content_hash: Some("c"),
            job_id: None,
        })
        .expect("sugg");
    matter
        .insert_ai_suggestion_citations(&[InsertAiCitationInput {
            suggestion_id: &sid,
            item_id: &item.id,
            ordinal: 0,
            quote: "would have matched if cas existed",
            start_offset: Some(0),
            end_offset: Some(10),
            field: "text",
            verify_status: VERIFY_MATCHED,
        }])
        .expect("cites");

    matter
        .accept_ai_suggestion(&sid, "reviewer")
        .expect("accept");

    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'ai_suggestion.accept' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params_json).expect("json");
    assert_eq!(v["citation_unverified"], true);
    assert_eq!(v["cas_text_unavailable"], true);
    let cites = v["citations"].as_array().expect("cites");
    assert_eq!(cites[0]["verify_status"], VERIFY_QUOTE_NOT_FOUND);
    assert!(cites[0]["start_offset"].is_null());
    assert!(cites[0]["end_offset"].is_null());
}

#[test]
fn accept_reverify_against_cas_sets_unverified_when_quote_missing() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI reverify").expect("create");
    let body = "document body with unique_token_aaa present once";
    let digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            text_sha256: Some(digest.clone()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot");
    let sid = matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: Some(&hot.id),
            code_name: "hot",
            confidence: Some(0.9),
            rationale: None,
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v2",
            is_remote: false,
            text_sha256: Some(&digest),
            catalog_content_hash: Some("c"),
            job_id: None,
        })
        .expect("sugg");
    // Stored as matched with wrong offsets + a missing quote.
    matter
        .insert_ai_suggestion_citations(&[
            InsertAiCitationInput {
                suggestion_id: &sid,
                item_id: &item.id,
                ordinal: 0,
                quote: "unique_token_aaa",
                start_offset: Some(0),
                end_offset: Some(4), // wrong — reverify repairs
                field: "text",
                verify_status: VERIFY_MATCHED,
            },
            InsertAiCitationInput {
                suggestion_id: &sid,
                item_id: &item.id,
                ordinal: 1,
                quote: "completely_absent_quote_zzz",
                start_offset: Some(0),
                end_offset: Some(5),
                field: "text",
                verify_status: VERIFY_MATCHED, // stale stored status
            },
        ])
        .expect("cites");

    matter
        .accept_ai_suggestion(&sid, "reviewer")
        .expect("accept");

    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'ai_suggestion.accept' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params_json).expect("json");
    assert_eq!(v["citation_unverified"], true);
    assert_eq!(v["text_sha256_stale"], false);
    let cites = v["citations"].as_array().expect("cites");
    assert_eq!(cites[0]["verify_status"], VERIFY_MATCHED);
    let expected = body.find("unique_token_aaa").expect("pos") as i64;
    assert_eq!(cites[0]["start_offset"], expected);
    assert_eq!(cites[1]["verify_status"], VERIFY_QUOTE_NOT_FOUND);
}

#[test]
fn accept_stale_text_sha256_flags_unverified_when_quotes_gone() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI stale").expect("create");
    // Current body no longer contains the original quote (re-extract).
    let old_digest = "a".repeat(64);
    let body = "brand new extracted body without prior evidence";
    let new_digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            text_sha256: Some(new_digest.clone()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot");
    let sid = matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: Some(&hot.id),
            code_name: "hot",
            confidence: Some(0.8),
            rationale: None,
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v2",
            is_remote: false,
            text_sha256: Some(&old_digest), // stale fingerprint
            catalog_content_hash: Some("c"),
            job_id: None,
        })
        .expect("sugg");
    matter
        .insert_ai_suggestion_citations(&[InsertAiCitationInput {
            suggestion_id: &sid,
            item_id: &item.id,
            ordinal: 0,
            quote: "prior_evidence_quote",
            start_offset: Some(10),
            end_offset: Some(30),
            field: "text",
            verify_status: VERIFY_MATCHED,
        }])
        .expect("cites");

    matter
        .accept_ai_suggestion(&sid, "reviewer")
        .expect("accept");

    let params_json: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'ai_suggestion.accept' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params_json).expect("json");
    assert_eq!(v["text_sha256_stale"], true);
    assert_eq!(v["citation_unverified"], true);
    assert_eq!(v["current_text_sha256"], new_digest);
    let cites = v["citations"].as_array().expect("cites");
    assert_eq!(cites[0]["verify_status"], VERIFY_QUOTE_NOT_FOUND);
}

#[test]
fn accept_is_atomic_with_audit() {
    // Happy path: code membership + accepted status + accept audit all present.
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI atomic").expect("create");
    let item = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            text_sha256: Some("x".into()),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item");
    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot");
    let sid = matter
        .insert_ai_suggestion(InsertAiSuggestionInput {
            item_id: &item.id,
            suggestion_type: AI_SUGGESTION_TYPE_CODE,
            code_id: Some(&hot.id),
            code_name: "hot",
            confidence: None,
            rationale: None,
            provider_kind: AI_PROVIDER_MOCK,
            model: "mock",
            prompt_template_id: "suggest_codes_v2",
            is_remote: false,
            text_sha256: Some("x"),
            catalog_content_hash: None,
            job_id: None,
        })
        .expect("sugg");
    matter
        .accept_ai_suggestion(&sid, "reviewer")
        .expect("accept");
    let sugg = matter.get_ai_suggestion(&sid).expect("get");
    assert_eq!(sugg.status, AI_SUGGESTION_ACCEPTED);
    let codes = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes");
    assert!(
        codes
            .get(&item.id)
            .map(|v| v.iter().any(|c| c.code_id == hot.id))
            .unwrap_or(false),
        "code applied"
    );
    let n: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'ai_suggestion.accept'",
            [],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(n, 1);
    let n_apply: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_events WHERE action = 'coding.apply'",
            [],
            |row| row.get(0),
        )
        .expect("count apply");
    assert_eq!(n_apply, 1);
}
