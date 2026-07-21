//! Integration tests for AI suggest job (tracks 0051/0052) — Mock only, no network.

use std::sync::atomic::{AtomicUsize, Ordering};

use matter_ai::{
    run_ai_suggest_codes, run_ai_suggest_codes_with_provider, AiProvider, AiProviderKind,
    AiSuggestCodesParams, AiSuggestOutcome, CompletionRequest, CompletionResponse, MockAiProvider,
    JOB_KIND_AI_SUGGEST_CODES, PROMPT_TEMPLATE_SUGGEST_CODES_V2,
};
use matter_core::{
    item_role, item_status, CodeDefInput, Matter, UpdateAiMatterConfigInput, AI_PROVIDER_MOCK,
    AI_SUGGESTION_PENDING, VERIFY_MATCHED,
};
use tempfile::TempDir;

/// Counts `complete` calls; delegates to [`MockAiProvider`].
struct CountingMock {
    inner: MockAiProvider,
    calls: AtomicUsize,
}

impl CountingMock {
    fn new() -> Self {
        Self {
            inner: MockAiProvider::new(),
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl AiProvider for CountingMock {
    fn kind(&self) -> AiProviderKind {
        self.inner.kind()
    }

    fn is_remote(&self) -> bool {
        self.inner.is_remote()
    }

    fn complete(&self, req: CompletionRequest) -> matter_ai::Result<CompletionResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.complete(req)
    }
}

fn utf8_tempdir() -> (TempDir, camino::Utf8PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let base = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    (tmp, base)
}

fn enable_mock_ai(matter: &Matter) {
    matter
        .update_ai_config(UpdateAiMatterConfigInput {
            enabled: true,
            allow_remote: false,
            base_url: None,
            model: Some("mock"),
            provider_kind: Some(AI_PROVIDER_MOCK),
        })
        .expect("enable ai");
}

fn put_text_item(matter: &Matter, subject: &str, body: &str) -> matter_core::Item {
    let digest = matter.cas().put_bytes(body.as_bytes()).expect("cas");
    matter
        .insert_item(matter_core::ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            subject: Some(subject.into()),
            text_sha256: Some(digest),
            in_review: Some(1),
            ..Default::default()
        })
        .expect("item")
}

fn run_job(matter: &Matter, params: &AiSuggestCodesParams) -> AiSuggestOutcome {
    let job = matter.create_job(JOB_KIND_AI_SUGGEST_CODES).expect("job");
    run_ai_suggest_codes(matter, &job.id, params, None, |_| {}).expect("run")
}

#[test]
fn job_kind_constant() {
    assert_eq!(JOB_KIND_AI_SUGGEST_CODES, "ai_suggest_codes");
}

#[test]
fn ai_off_fails_closed() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI off").expect("create");
    // AI disabled by default
    let item = put_text_item(&matter, "A", "this is hot document");
    let _ = item;
    let job = matter.create_job(JOB_KIND_AI_SUGGEST_CODES).expect("job");
    let result = run_ai_suggest_codes(
        &matter,
        &job.id,
        &AiSuggestCodesParams::default(),
        None,
        |_| {},
    );
    match result {
        Err(e) => {
            let s = e.to_string().to_ascii_lowercase();
            assert!(
                s.contains("disabled") || s.contains("ai"),
                "unexpected error: {e}"
            );
        }
        Ok(AiSuggestOutcome::Failed { message, .. }) => {
            assert!(
                message.to_ascii_lowercase().contains("disabled")
                    || message.to_ascii_lowercase().contains("ai"),
                "unexpected message: {message}"
            );
        }
        Ok(other) => panic!("expected fail-closed, got {other:?}"),
    }
}

#[test]
fn mock_job_writes_suggestions_not_item_codes() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI mock").expect("create");
    enable_mock_ai(&matter);

    // Idiosyncratic guidance on hot
    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot");
    matter
        .upsert_code_definition(CodeDefInput {
            id: Some(hot.id.clone()),
            key: None,
            label: hot.label.clone(),
            group_key: hot.group_key.clone(),
            cardinality: hot.cardinality.clone(),
            color: hot.color.clone(),
            sort_order: hot.sort_order,
            is_active: true,
            guidance: Some("XYZZY_HOT_PROTOCOL_ONLY".into()),
        })
        .expect("guidance");

    let item = put_text_item(
        &matter,
        "Key email",
        "This is a hot document for the review team.",
    );

    let outcome = run_job(&matter, &AiSuggestCodesParams::default());
    match outcome {
        AiSuggestOutcome::Succeeded(r) => {
            assert!(r.suggestion_rows >= 1, "expected suggestions: {r:?}");
            assert!(!r.is_remote);
            assert_eq!(r.provider_kind, "mock");
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }

    let pending = matter
        .list_pending_ai_suggestions_for_item(&item.id)
        .expect("pending");
    assert!(!pending.is_empty());
    assert_eq!(pending[0].status, AI_SUGGESTION_PENDING);
    assert_eq!(pending[0].code_name, "hot");

    let codes = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes");
    assert!(codes[&item.id].is_empty(), "job must not write item_codes");

    // Accept promotes
    let accepted = matter
        .accept_ai_suggestion(&pending[0].id, "reviewer")
        .expect("accept");
    assert_eq!(accepted.status, "accepted");
    let codes2 = matter
        .list_item_codes(std::slice::from_ref(&item.id))
        .expect("codes2");
    assert_eq!(codes2[&item.id].len(), 1);
    assert_eq!(codes2[&item.id][0].key, "hot");
}

#[test]
fn fingerprint_skip_on_rerun() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI skip").expect("create");
    enable_mock_ai(&matter);
    let _item = put_text_item(&matter, "B", "hot confidential material here");

    let p = AiSuggestCodesParams::default();
    let o1 = run_job(&matter, &p);
    let r1 = match o1 {
        AiSuggestOutcome::Succeeded(r) => r,
        other => panic!("{other:?}"),
    };
    assert!(r1.suggestion_rows >= 1 || r1.suggested_count >= 1 || r1.completed_count >= 1);

    let o2 = run_job(&matter, &p);
    match o2 {
        AiSuggestOutcome::Succeeded(r2) => {
            assert!(
                r2.skipped_count >= 1,
                "second run should skip fingerprint match: {r2:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn remote_blocked_without_allow() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI remote").expect("create");
    matter
        .update_ai_config(UpdateAiMatterConfigInput {
            enabled: true,
            allow_remote: false,
            base_url: Some("https://api.openai.com/v1"),
            model: Some("gpt-4o-mini"),
            provider_kind: Some("openai_compatible"),
        })
        .expect("cfg");
    let _item = put_text_item(&matter, "C", "hot item");
    let job = matter.create_job(JOB_KIND_AI_SUGGEST_CODES).expect("job");
    // resolve_provider should fail RemoteBlocked before HTTP
    let err = run_ai_suggest_codes(
        &matter,
        &job.id,
        &AiSuggestCodesParams::default(),
        None,
        |_| {},
    );
    match err {
        Err(e) => {
            let s = e.to_string().to_ascii_lowercase();
            assert!(s.contains("remote") || s.contains("allow"), "got {e}");
        }
        Ok(AiSuggestOutcome::Failed { message, .. }) => {
            let s = message.to_ascii_lowercase();
            assert!(s.contains("remote") || s.contains("allow"), "{message}");
        }
        Ok(other) => panic!("expected remote block, got {other:?}"),
    }
}

#[test]
fn with_explicit_mock_provider() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI explicit").expect("create");
    enable_mock_ai(&matter);
    let item = put_text_item(&matter, "D", "needs responsive coding review");
    let job = matter.create_job(JOB_KIND_AI_SUGGEST_CODES).expect("job");
    let mock = MockAiProvider::new();
    let outcome = run_ai_suggest_codes_with_provider(
        &matter,
        &job.id,
        &AiSuggestCodesParams::default(),
        &mock,
        None,
        |_| {},
    )
    .expect("run");
    match outcome {
        AiSuggestOutcome::Succeeded(r) => {
            assert_eq!(r.completed_count, 1);
            let pending = matter
                .list_pending_ai_suggestions_for_item(&item.id)
                .expect("p");
            // "responsive" keyword in body should match
            assert!(
                pending.iter().any(|s| s.code_name.contains("responsive")),
                "pending={pending:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn withheld_item_skipped_no_provider_complete() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI withheld").expect("create");
    enable_mock_ai(&matter);

    // Normal item — mock should suggest "hot".
    let normal = put_text_item(&matter, "Normal", "this is a hot document for review");
    // Withheld item with text that would also match "hot" if sent to the model.
    let withheld = put_text_item(
        &matter,
        "Privileged",
        "this is a hot privileged attorney-client document",
    );
    matter
        .ensure_item_privilege(&withheld.id, "tester")
        .expect("withhold");
    assert!(
        matter.item_is_withheld(&withheld.id).expect("check"),
        "fixture must be withheld"
    );

    let job = matter.create_job(JOB_KIND_AI_SUGGEST_CODES).expect("job");
    let mock = CountingMock::new();
    let outcome = run_ai_suggest_codes_with_provider(
        &matter,
        &job.id,
        &AiSuggestCodesParams::default(),
        &mock,
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        AiSuggestOutcome::Succeeded(r) => {
            assert_eq!(
                r.withheld_count, 1,
                "withheld item must increment withheld_count: {r:?}"
            );
            assert_eq!(
                r.completed_count, 2,
                "both items completed (one withheld): {r:?}"
            );
            // Only the non-withheld item should hit the provider.
            assert_eq!(
                mock.call_count(),
                1,
                "provider.complete must not run for withheld item"
            );
            assert!(
                r.suggestion_rows >= 1 || r.suggested_count >= 1,
                "normal item should still get suggestions: {r:?}"
            );
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }

    let pending_withheld = matter
        .list_pending_ai_suggestions_for_item(&withheld.id)
        .expect("pending withheld");
    assert!(
        pending_withheld.is_empty(),
        "withheld item must have no AI suggestions: {pending_withheld:?}"
    );

    let pending_normal = matter
        .list_pending_ai_suggestions_for_item(&normal.id)
        .expect("pending normal");
    assert!(
        !pending_normal.is_empty(),
        "normal item should have suggestions"
    );
}

#[test]
fn mock_job_writes_citations_and_accept_audit_pointers() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI cites").expect("create");
    enable_mock_ai(&matter);
    let item = put_text_item(
        &matter,
        "Key email",
        "This is a hot document for the review team.",
    );

    let outcome = run_job(&matter, &AiSuggestCodesParams::default());
    match outcome {
        AiSuggestOutcome::Succeeded(r) => {
            assert!(r.suggestion_rows >= 1, "expected suggestions: {r:?}");
            assert_eq!(r.prompt_template_id, PROMPT_TEMPLATE_SUGGEST_CODES_V2);
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }

    let pending = matter
        .list_pending_ai_suggestions_for_item(&item.id)
        .expect("pending");
    assert!(!pending.is_empty());
    let hot = pending
        .iter()
        .find(|s| s.code_name == "hot")
        .expect("hot suggestion");
    assert!(
        hot.citations_count >= 1,
        "mock should attach citation: count={}",
        hot.citations_count
    );
    let cites = matter.list_ai_suggestion_citations(&hot.id).expect("cites");
    assert!(!cites.is_empty());
    assert!(
        cites.iter().any(|c| c.verify_status == VERIFY_MATCHED),
        "expected at least one matched citation: {cites:?}"
    );
    // Quote stored (for in-app promote) and non-empty.
    assert!(!cites[0].quote.is_empty());

    // Accept → audit has pointers only (no quote keys / cleartext field).
    matter
        .accept_ai_suggestion(&hot.id, "reviewer")
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
    // No "quote" key at all (code_name may equal a short keyword quote — that is fine).
    assert!(
        !params_json.contains("\"quote\""),
        "audit must not have quote keys: {params_json}"
    );
    let v: serde_json::Value = serde_json::from_str(&params_json).expect("json");
    assert_eq!(v["prompt_template_id"], PROMPT_TEMPLATE_SUGGEST_CODES_V2);
    let arr = v["citations"].as_array().expect("citations arr");
    assert!(!arr.is_empty());
    for c in arr {
        assert!(c.get("quote").is_none());
        assert!(c.get("citation_id").is_some());
        assert!(c.get("start_offset").is_some());
        assert!(c.get("verify_status").is_some());
    }
}

/// Cancel mid-batch → Paused + checkpoint; re-run same job_id → Succeeded (resume).
#[test]
fn cancel_to_paused_then_resume() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "AI cancel").expect("create");
    enable_mock_ai(&matter);

    // Several items so cancel can land between process_one calls.
    for i in 0..6 {
        let _ = put_text_item(
            &matter,
            &format!("Item {i}"),
            "this is a hot document for the review team",
        );
    }

    let job = matter.create_job(JOB_KIND_AI_SUGGEST_CODES).expect("job");
    let checks = AtomicUsize::new(0);
    // First cancel poll is loop start; second is first item; third is second item → pause.
    let cancel = || checks.fetch_add(1, Ordering::SeqCst) >= 2;

    let outcome = run_ai_suggest_codes(
        &matter,
        &job.id,
        &AiSuggestCodesParams {
            scope: "all".into(),
            batch_size: 10,
            max_items: 100,
            ..AiSuggestCodesParams::default()
        },
        Some(&cancel),
        |_| {},
    )
    .expect("run cancel");

    match outcome {
        AiSuggestOutcome::Paused(s) => {
            assert!(
                s.completed_count >= 1,
                "should process at least one item before pause: {s:?}"
            );
        }
        other => panic!("expected Paused after cancel, got {other:?}"),
    }

    // Checkpoint should exist for resume.
    let cp = matter
        .get_checkpoint(&job.id, matter_ai::AI_SUGGEST_STAGE)
        .expect("cp")
        .expect("paused run must write checkpoint");
    assert!(!cp.cursor_json.trim().is_empty());

    // Resume same job_id without cancel → finish remaining items.
    let resume_cancel = AtomicBool::new(false);
    let resume = || resume_cancel.load(Ordering::SeqCst);
    let outcome2 = run_ai_suggest_codes(
        &matter,
        &job.id,
        &AiSuggestCodesParams {
            scope: "all".into(),
            batch_size: 10,
            max_items: 100,
            ..AiSuggestCodesParams::default()
        },
        Some(&resume),
        |_| {},
    )
    .expect("resume");

    match outcome2 {
        AiSuggestOutcome::Succeeded(r) => {
            assert!(
                r.completed_count >= 6,
                "resume should finish all items: {r:?}"
            );
        }
        other => panic!("expected Succeeded on resume, got {other:?}"),
    }
}
