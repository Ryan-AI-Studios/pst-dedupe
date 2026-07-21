//! AI config + suggestion API tests (schema v30 / track 0051).

use matter_core::{
    catalog_content_hash, item_role, item_status, AiMatterConfig, CodeDefInput,
    InsertAiSuggestionInput, ItemInput, Matter, UpdateAiMatterConfigInput, AI_PROVIDER_MOCK,
    AI_SUGGESTION_ACCEPTED, AI_SUGGESTION_PENDING, AI_SUGGESTION_REJECTED, AI_SUGGESTION_TYPE_CODE,
    SCHEMA_VERSION,
};
use tempfile::TempDir;

fn utf8_tempdir() -> (TempDir, camino::Utf8PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    (tmp, base)
}

#[test]
fn schema_version_is_30() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI").expect("create");
    assert_eq!(SCHEMA_VERSION, 30);
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
}
