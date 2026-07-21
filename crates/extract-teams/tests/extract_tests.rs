//! Integration tests for extract-teams (track 0055 DoD-7).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use extract_teams::{
    enrich_from_metadata, html_to_plain_text, parse_teams_html, parse_teams_json,
    run_teams_extract, ConversationKeys, PstEnrichInput, TeamsExtractOutcome, TeamsExtractParams,
    JOB_KIND_TEAMS_EXTRACT,
};
use matter_core::{item_role, item_status, ItemInput, Matter};

fn fixtures_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("fixtures");
    p.push("teams");
    p
}

fn load_fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    fs::read(&path).unwrap_or_else(|e| panic!("read fixture {}: {e}", path.display()))
}

#[test]
fn conversation_id_same_day_stable_across_hours() {
    let a = ConversationKeys::from_parts(
        Some("Team Alpha"),
        Some("General"),
        Some("2024-06-01T10:00:00Z"),
        None,
    );
    let b = ConversationKeys::from_parts(
        Some("Team Alpha"),
        Some("General"),
        Some("2024-06-01T22:00:00Z"),
        None,
    );
    assert_eq!(a.conversation_id(), b.conversation_id());
}

#[test]
fn conversation_id_differs_across_utc_days() {
    let a = ConversationKeys::from_parts(
        Some("Team Alpha"),
        Some("General"),
        Some("2024-06-01T10:00:00Z"),
        None,
    );
    let b = ConversationKeys::from_parts(
        Some("Team Alpha"),
        Some("General"),
        Some("2024-06-02T11:00:00Z"),
        None,
    );
    assert_ne!(a.conversation_id(), b.conversation_id());
}

#[test]
fn xss_fixture_no_script_in_plain_text() {
    let data = String::from_utf8(load_fixture("xss_script.html")).unwrap();
    let p = parse_teams_html(&data, 50).unwrap();
    assert_eq!(p.messages.len(), 1);
    let t = &p.messages[0].plain_text;
    assert!(!t.to_lowercase().contains("<script"));
    assert!(!t.contains("alert(1)"));
    assert!(!t.to_lowercase().contains("onclick"));
    assert!(t.contains("Hello"));
    assert!(t.contains("world"));
}

#[test]
fn reaction_and_attachment_fixture() {
    let data = String::from_utf8(load_fixture("reactions_attachments.html")).unwrap();
    let p = parse_teams_html(&data, 50).unwrap();
    let t = &p.messages[0].plain_text;
    assert!(t.contains("[Reaction:"));
    assert!(t.contains("bob@example.com"));
    assert!(t.contains("[Attachment: Contract_v2.docx]"));
}

#[test]
fn multi_day_channel_two_conversation_ids() {
    let data = String::from_utf8(load_fixture("multi_day_channel.html")).unwrap();
    let p = parse_teams_html(&data, 50).unwrap();
    assert_eq!(p.messages.len(), 2);
    assert_ne!(p.messages[0].conversation_id, p.messages[1].conversation_id);
    assert_eq!(p.messages[0].conversation_bucket_date, "2024-06-01");
    assert_eq!(p.messages[1].conversation_bucket_date, "2024-06-02");
}

#[test]
fn corrupt_html_errors_no_panic() {
    let data = String::from_utf8(load_fixture("corrupt.html")).unwrap();
    let r = parse_teams_html(&data, 50);
    assert!(r.is_err());
}

#[test]
fn ammonia_sanitize_unit() {
    let plain = html_to_plain_text(r#"x <script>alert(1)</script> y"#);
    assert!(!plain.contains("alert(1)"));
    assert!(!plain.contains('<'));
}

#[test]
fn run_teams_extract_html_fixtures() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsHtml").unwrap();

    for name in [
        "multi_day_channel.html",
        "xss_script.html",
        "reactions_attachments.html",
        "corrupt.html",
    ] {
        let data = load_fixture(name);
        let native = matter.put_bytes(&data).unwrap();
        matter
            .insert_item(ItemInput {
                path: Some(format!("export/{name}")),
                native_sha256: Some(native),
                status: item_status::EXTRACTED.into(),
                mime_type: Some("text/html".into()),
                ..Default::default()
            })
            .unwrap();
    }

    // Teams-shaped shell with no messages → true parse failure → error (not skip).
    let empty_teams = br#"<!DOCTYPE html>
<div class="conversation" data-team="T" data-channel="C" data-chat-type="channel">
</div>"#;
    let empty_sha = matter.put_bytes(empty_teams).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("export/empty_teams.html".into()),
            native_sha256: Some(empty_sha),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/html".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams::default(),
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert!(s.extracted_count >= 3, "summary={s:?}");
            assert!(s.skipped_count >= 1, "non-teams html should skip: {s:?}");
            assert!(s.error_count >= 1, "empty teams-shaped should error: {s:?}");
            assert!(s.child_count >= 4, "expected message children: {s:?}");
        }
        other => panic!("unexpected {other:?}"),
    }

    let cands = matter.list_teams_candidates(0, 100, None).unwrap();
    let multi = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/multi_day_channel.html"))
        .expect("multi parent");
    assert_eq!(multi.teams_extract_status.as_deref(), Some("ok"));
    let children = matter.list_attachments(&multi.id).unwrap();
    assert_eq!(children.len(), 2);
    let ids: std::collections::HashSet<_> = children
        .iter()
        .filter_map(|c| c.conversation_id.clone())
        .collect();
    assert_eq!(ids.len(), 2);
    for c in &children {
        assert_eq!(c.file_category.as_deref(), Some("chat"));
        assert_eq!(c.role.as_deref(), Some("chat_message"));
        assert_eq!(c.chat_export_format.as_deref(), Some("html"));
    }

    let react = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/reactions_attachments.html"))
        .expect("react parent");
    let rchildren = matter.list_attachments(&react.id).unwrap();
    assert_eq!(rchildren.len(), 1);
    let text = String::from_utf8(
        matter
            .get_bytes(rchildren[0].text_sha256.as_deref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(text.contains("[Reaction:"));
    assert!(text.contains("[Attachment: Contract_v2.docx]"));

    let xss = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/xss_script.html"))
        .expect("xss parent");
    let xchildren = matter.list_attachments(&xss.id).unwrap();
    let xt = String::from_utf8(
        matter
            .get_bytes(xchildren[0].text_sha256.as_deref().unwrap())
            .unwrap(),
    )
    .unwrap();
    assert!(!xt.to_lowercase().contains("<script"));
    assert!(!xt.contains("alert(1)"));

    // Random non-Teams .html → skipped (no item_error noise).
    let corrupt = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/corrupt.html"))
        .expect("corrupt");
    assert_eq!(corrupt.teams_extract_status.as_deref(), Some("skipped"));

    let empty = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/empty_teams.html"))
        .expect("empty teams");
    assert_eq!(empty.teams_extract_status.as_deref(), Some("error"));
}

#[test]
fn random_non_teams_html_is_skipped_not_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "NonTeamsHtml").unwrap();

    let random = b"<!DOCTYPE html><html><body><h1>Invoice</h1><p>Not Teams.</p></body></html>";
    let native = matter.put_bytes(random).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("export/random_page.html".into()),
            native_sha256: Some(native),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/html".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams::default(),
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert_eq!(s.skipped_count, 1, "{s:?}");
            assert_eq!(s.error_count, 0, "{s:?}");
            assert_eq!(s.extracted_count, 0, "{s:?}");
        }
        other => panic!("unexpected {other:?}"),
    }

    let cands = matter.list_teams_candidates(0, 10, None).unwrap();
    let leaf = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/random_page.html"))
        .expect("leaf");
    assert_eq!(leaf.teams_extract_status.as_deref(), Some("skipped"));

    // No item_error rows for quiet skip.
    let errors = matter.item_errors_for_job(&job.id).unwrap();
    assert!(
        errors.is_empty(),
        "expected no item_errors for non-teams skip, got {errors:?}"
    );
}

#[test]
fn random_non_teams_json_is_skipped_not_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "NonTeamsJson").unwrap();

    let random = br#"{"configVersion":1,"enabled":true,"paths":["a","b"]}"#;
    let native = matter.put_bytes(random).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("export/app_config.json".into()),
            native_sha256: Some(native),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("application/json".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams::default(),
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert_eq!(s.skipped_count, 1, "{s:?}");
            assert_eq!(s.error_count, 0, "{s:?}");
        }
        other => panic!("unexpected {other:?}"),
    }

    let cands = matter.list_teams_candidates(0, 10, None).unwrap();
    let leaf = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/app_config.json"))
        .expect("leaf");
    assert_eq!(leaf.teams_extract_status.as_deref(), Some("skipped"));
}

#[test]
fn cancel_after_first_candidate_then_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsCancel").unwrap();

    for name in [
        "multi_day_channel.html",
        "xss_script.html",
        "reactions_attachments.html",
    ] {
        let data = load_fixture(name);
        let native = matter.put_bytes(&data).unwrap();
        matter
            .insert_item(ItemInput {
                path: Some(format!("export/{name}")),
                native_sha256: Some(native),
                status: item_status::EXTRACTED.into(),
                mime_type: Some("text/html".into()),
                ..Default::default()
            })
            .unwrap();
    }

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let cancel = AtomicBool::new(false);
    let progress_hits = AtomicU64::new(0);

    let params = TeamsExtractParams {
        batch_size: 1,
        ..TeamsExtractParams::default()
    };

    let first = run_teams_extract(
        &matter,
        &job.id,
        &params,
        Some(&|| cancel.load(Ordering::SeqCst)),
        |completed| {
            progress_hits.fetch_add(1, Ordering::SeqCst);
            // Trip cancel after the first candidate completes.
            if completed >= 1 {
                cancel.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("first run");

    match first {
        TeamsExtractOutcome::Paused(s) => {
            assert!(
                s.completed_count > 0,
                "expected partial progress before pause: {s:?}"
            );
            assert!(
                s.extracted_count >= 1 || s.skipped_count >= 1,
                "expected at least one finished leaf: {s:?}"
            );
        }
        other => panic!("expected Paused after cancel, got {other:?}"),
    }

    // Resume: force=false should continue / skip already-ok leaves.
    let second = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams {
            force: false,
            reset: false,
            batch_size: 10,
            ..TeamsExtractParams::default()
        },
        None,
        |_| {},
    )
    .expect("resume");

    match second {
        TeamsExtractOutcome::Succeeded(s) => {
            assert!(
                s.completed_count >= 3,
                "resume should finish remaining: {s:?}"
            );
            assert!(s.extracted_count >= 3, "all three fixtures ok: {s:?}");
        }
        other => panic!("expected Succeeded on resume, got {other:?}"),
    }

    let cands = matter.list_teams_candidates(0, 20, None).unwrap();
    for name in [
        "export/multi_day_channel.html",
        "export/xss_script.html",
        "export/reactions_attachments.html",
    ] {
        let leaf = cands
            .iter()
            .find(|c| c.path.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(
            leaf.teams_extract_status.as_deref(),
            Some("ok"),
            "{name} status"
        );
    }
}

#[test]
fn run_teams_extract_json_fixture() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsJson").unwrap();
    let data = load_fixture("messages.json");
    let native = matter.put_bytes(&data).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("export/messages.json".into()),
            native_sha256: Some(native),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("application/json".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams::default(),
        None,
        |_| {},
    )
    .unwrap();
    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert_eq!(s.extracted_count, 1);
            assert_eq!(s.child_count, 2);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn pst_enrich_from_synthetic_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsPst").unwrap();
    let body = "Hello <script>alert(1)</script> from Teams";
    let text_sha = matter.put_bytes(body.as_bytes()).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("Conversation History/Team Chat/Alpha/General/item1".into()),
            status: item_status::EXTRACTED.into(),
            message_class: Some("IPM.SkypeTeams.Message".into()),
            from_addr: Some("alice@example.com".into()),
            sent_at: Some("2024-06-01T10:00:00Z".into()),
            subject: Some("chat".into()),
            text_sha256: Some(text_sha),
            ..Default::default()
        })
        .unwrap();

    // Unit-level enrich check
    let msg = enrich_from_metadata(&PstEnrichInput {
        message_class: Some("IPM.SkypeTeams.Message"),
        path: Some("Conversation History/Team Chat/Alpha/General/item1"),
        from_addr: Some("alice@example.com"),
        sent_at: Some("2024-06-01T10:00:00Z"),
        subject: Some("chat"),
        existing_text: Some(body),
        ..Default::default()
    })
    .unwrap();
    assert!(!msg.plain_text.contains("alert(1)"));

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams::default(),
        None,
        |_| {},
    )
    .unwrap();
    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert!(s.extracted_count >= 1 || s.skipped_count >= 1 || s.completed_count >= 1);
            assert!(s.pst_count >= 1, "format count: {s:?}");
        }
        other => panic!("{other:?}"),
    }
    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.file_category.as_deref(), Some("chat"));
    assert!(after.conversation_id.is_some());
    assert_eq!(
        after.conversation_bucket_date.as_deref(),
        Some("2024-06-01")
    );
    assert_eq!(after.chat_export_format.as_deref(), Some("pst"));
    assert_eq!(after.teams_extract_status.as_deref(), Some("ok"));
}

#[test]
fn html_max_messages_exceeded_is_error_not_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsCapHtml").unwrap();

    // multi_day_channel.html has 2 messages; cap at 1 → error, no children.
    let data = load_fixture("multi_day_channel.html");
    let native = matter.put_bytes(&data).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("export/multi_day_channel.html".into()),
            native_sha256: Some(native),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/html".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams {
            max_messages_per_file: 1,
            ..TeamsExtractParams::default()
        },
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1, "{s:?}");
            assert_eq!(s.extracted_count, 0, "{s:?}");
            assert_eq!(s.child_count, 0, "no partial children: {s:?}");
            assert_eq!(s.html_count, 1, "{s:?}");
        }
        other => panic!("unexpected {other:?}"),
    }

    let cands = matter.list_teams_candidates(0, 10, None).unwrap();
    let leaf = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/multi_day_channel.html"))
        .expect("leaf");
    assert_eq!(leaf.teams_extract_status.as_deref(), Some("error"));
    let children = matter.list_attachments(&leaf.id).unwrap();
    assert!(children.is_empty(), "overflow must not create children");

    let errors = matter.item_errors_for_job(&job.id).unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e.code.contains("max_messages_exceeded")),
        "expected max_messages_exceeded item_error, got {errors:?}"
    );
}

#[test]
fn json_max_messages_exceeded_is_error_not_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsCapJson").unwrap();

    // messages.json has 2 messages; cap at 1 → error.
    let data = load_fixture("messages.json");
    let native = matter.put_bytes(&data).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("export/messages.json".into()),
            native_sha256: Some(native),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("application/json".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams {
            max_messages_per_file: 1,
            ..TeamsExtractParams::default()
        },
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1, "{s:?}");
            assert_eq!(s.extracted_count, 0, "{s:?}");
            assert_eq!(s.child_count, 0, "{s:?}");
            assert_eq!(s.json_count, 1, "{s:?}");
        }
        other => panic!("unexpected {other:?}"),
    }

    let cands = matter.list_teams_candidates(0, 10, None).unwrap();
    let leaf = cands
        .iter()
        .find(|c| c.path.as_deref() == Some("export/messages.json"))
        .expect("leaf");
    assert_eq!(leaf.teams_extract_status.as_deref(), Some("error"));
}

#[test]
fn json_different_chat_ids_same_day_distinct_conversations() {
    let raw = br#"[
      {"id":"1","body":"hi","chatId":"chat-aaa","timestamp":"2024-06-01T10:00:00Z","chatType":"1:1"},
      {"id":"2","body":"yo","chatId":"chat-bbb","timestamp":"2024-06-01T11:00:00Z","chatType":"dm"}
    ]"#;
    let msgs = parse_teams_json(raw, 50).unwrap();
    assert_eq!(msgs.len(), 2);
    assert_ne!(msgs[0].conversation_id, msgs[1].conversation_id);
    assert_eq!(msgs[0].chat_type, "one_to_one");
    assert_eq!(msgs[1].chat_type, "one_to_one");
}

#[test]
fn pst_attachment_title_injected_into_body() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsPstAtt").unwrap();

    let body = "Please review the contract";
    let text_sha = matter.put_bytes(body.as_bytes()).unwrap();
    let family = matter.insert_family("email-attachments").unwrap();
    let parent = matter
        .insert_item(ItemInput {
            path: Some("Conversation History/Team Chat/Alpha/General/msg1".into()),
            status: item_status::EXTRACTED.into(),
            message_class: Some("IPM.SkypeTeams.Message".into()),
            from_addr: Some("alice@example.com".into()),
            sent_at: Some("2024-06-01T10:00:00Z".into()),
            subject: Some("chat".into()),
            text_sha256: Some(text_sha),
            attachment_count: Some(1),
            family_id: Some(family.id.clone()),
            role: Some(item_role::PARENT.into()),
            ..Default::default()
        })
        .unwrap();

    // Path must NOT match Team Chat heuristics so the attachment is not itself a candidate.
    matter
        .insert_item(ItemInput {
            path: Some("attachments/Contract.docx".into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            parent_item_id: Some(parent.id.clone()),
            family_id: Some(family.id.clone()),
            title: Some("Contract.docx".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams::default(),
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert!(s.extracted_count >= 1, "{s:?}");
            assert_eq!(s.pst_count, 1, "{s:?}");
        }
        other => panic!("{other:?}"),
    }

    let after = matter.get_item(&parent.id).unwrap();
    assert_eq!(after.teams_extract_status.as_deref(), Some("ok"));
    let text = String::from_utf8(
        matter
            .get_bytes(after.text_sha256.as_deref().expect("text_sha256"))
            .unwrap(),
    )
    .unwrap();
    assert!(
        text.contains("[Attachment: Contract.docx]"),
        "body missing attachment line: {text}"
    );
    assert!(text.contains("Please review the contract"));
}

#[test]
fn pst_missing_text_cas_is_error_not_subject_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsPstCas").unwrap();

    // Valid hex digest that is not present in CAS.
    let missing = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string();
    let item = matter
        .insert_item(ItemInput {
            path: Some("Conversation History/Team Chat/Alpha/General/item_missing".into()),
            status: item_status::EXTRACTED.into(),
            message_class: Some("IPM.SkypeTeams.Message".into()),
            from_addr: Some("alice@example.com".into()),
            sent_at: Some("2024-06-01T10:00:00Z".into()),
            subject: Some("would-be-fallback-subject".into()),
            text_sha256: Some(missing),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams::default(),
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert_eq!(s.error_count, 1, "{s:?}");
            assert_eq!(s.extracted_count, 0, "{s:?}");
            assert_eq!(s.pst_count, 1, "{s:?}");
        }
        other => panic!("{other:?}"),
    }

    let after = matter.get_item(&item.id).unwrap();
    assert_eq!(after.teams_extract_status.as_deref(), Some("error"));
    // Must not silently replace body with subject as ok.
    assert_ne!(after.file_category.as_deref(), Some("chat"));

    let errors = matter.item_errors_for_job(&job.id).unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e.code.contains("teams_cas_error") || e.message.contains("CAS")),
        "expected CAS item_error, got {errors:?}"
    );
}

#[test]
fn format_counts_in_mixed_run_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
    let matter = Matter::create(root.join("m"), "TeamsFmtCounts").unwrap();

    let html = load_fixture("xss_script.html");
    let html_sha = matter.put_bytes(&html).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("export/xss_script.html".into()),
            native_sha256: Some(html_sha),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("text/html".into()),
            ..Default::default()
        })
        .unwrap();

    let json = load_fixture("messages.json");
    let json_sha = matter.put_bytes(&json).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("export/messages.json".into()),
            native_sha256: Some(json_sha),
            status: item_status::EXTRACTED.into(),
            mime_type: Some("application/json".into()),
            ..Default::default()
        })
        .unwrap();

    let body = "pst body";
    let text_sha = matter.put_bytes(body.as_bytes()).unwrap();
    matter
        .insert_item(ItemInput {
            path: Some("Conversation History/Team Chat/Alpha/General/p1".into()),
            status: item_status::EXTRACTED.into(),
            message_class: Some("IPM.SkypeTeams.Message".into()),
            text_sha256: Some(text_sha),
            sent_at: Some("2024-06-01T10:00:00Z".into()),
            ..Default::default()
        })
        .unwrap();

    let job = matter.create_job(JOB_KIND_TEAMS_EXTRACT).unwrap();
    let outcome = run_teams_extract(
        &matter,
        &job.id,
        &TeamsExtractParams::default(),
        None,
        |_| {},
    )
    .expect("run");

    match outcome {
        TeamsExtractOutcome::Succeeded(s) => {
            assert_eq!(s.html_count, 1, "{s:?}");
            assert_eq!(s.json_count, 1, "{s:?}");
            assert_eq!(s.pst_count, 1, "{s:?}");
            assert!(s.extracted_count >= 3, "{s:?}");
        }
        other => panic!("{other:?}"),
    }
}
