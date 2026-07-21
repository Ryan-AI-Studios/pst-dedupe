//! Conversation-centric review API tests (track 0056).
//!
//! Schema stays **v34** — uses `conversation_id` + `idx_items_conversation`.

use std::collections::HashSet;

use matter_core::{
    clamp_conversation_list_limit, clamp_conversation_stream_limit, item_role, item_status,
    ApplyCodesInput, ConversationMessageRow, ItemInput, Matter, CONVERSATION_AROUND_AFTER,
    CONVERSATION_AROUND_BEFORE, CONVERSATION_LIST_MAX_LIMIT, CONVERSATION_STREAM_MAX_LIMIT,
    REPLY_SNIPPET_UNAVAILABLE, SCHEMA_VERSION,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
    (dir, path)
}

struct ChatSeed<'a> {
    id: &'a str,
    conversation_id: &'a str,
    sent_at: &'a str,
    from: &'a str,
    subject: &'a str,
    text: Option<&'a str>,
}

fn insert_chat(matter: &Matter, seed: ChatSeed<'_>) -> ConversationMessageRow {
    let text_sha = seed
        .text
        .map(|t| matter.put_bytes(t.as_bytes()).expect("put text"));
    let item = matter
        .insert_item(ItemInput {
            id: Some(seed.id.into()),
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::STANDALONE.into()),
            conversation_id: Some(seed.conversation_id.into()),
            chat_type: Some("channel".into()),
            team_name: Some("Team Alpha".into()),
            channel_name: Some("general".into()),
            conversation_bucket_date: Some("2024-06-15".into()),
            chat_export_format: Some("html".into()),
            sent_at: Some(seed.sent_at.into()),
            from_addr: Some(seed.from.into()),
            subject: Some(seed.subject.into()),
            text_sha256: text_sha,
            file_category: Some("chat".into()),
            ..Default::default()
        })
        .expect("insert chat");
    ConversationMessageRow {
        id: item.id,
        conversation_id: seed.conversation_id.into(),
        sent_at: item.sent_at,
        from_addr: item.from_addr,
        subject: item.subject,
        text_sha256: item.text_sha256,
        html_sha256: item.html_sha256,
        parent_item_id: item.parent_item_id,
        chat_type: item.chat_type,
        team_name: item.team_name,
        channel_name: item.channel_name,
        conversation_bucket_date: item.conversation_bucket_date,
        file_category: item.file_category,
        role: item.role,
        path: item.path,
        reply_snippet: None,
    }
}

#[test]
fn schema_stays_v34() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("m"), "Conv Schema").expect("create");
    assert_eq!(SCHEMA_VERSION, 34);
    assert_eq!(matter.schema_version().expect("ver"), 34);
}

#[test]
fn swiss_cheese_stream_returns_all_hit_badges_middle_only() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("swiss"), "Swiss").expect("create");
    let cid = "conv_day_2024-06-15_abc";

    let a = insert_chat(
        &matter,
        ChatSeed {
            id: "itm_a",
            conversation_id: cid,
            sent_at: "2024-06-15T10:00:00Z",
            from: "alice@ex.com",
            subject: "msg a",
            text: Some("hello context"),
        },
    );
    let b = insert_chat(
        &matter,
        ChatSeed {
            id: "itm_b",
            conversation_id: cid,
            sent_at: "2024-06-15T10:05:00Z",
            from: "bob@ex.com",
            subject: "msg b hit",
            text: Some("fraud keyword middle"),
        },
    );
    let c = insert_chat(
        &matter,
        ChatSeed {
            id: "itm_c",
            conversation_id: cid,
            sent_at: "2024-06-15T10:10:00Z",
            from: "carol@ex.com",
            subject: "msg c",
            text: Some("bye context"),
        },
    );

    // Stream is full bucket — no filter WHERE.
    let stream = matter
        .list_conversation_messages(cid, None, None, 100, false)
        .expect("stream");
    assert_eq!(stream.len(), 3);
    assert_eq!(
        stream.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec![a.id.as_str(), b.id.as_str(), c.id.as_str()]
    );

    // Hit badge set = middle only.
    let hit_set: HashSet<String> = [b.id.clone()].into_iter().collect();
    let candidates: Vec<String> = stream.iter().map(|r| r.id.clone()).collect();
    let badges = matter
        .conversation_hit_id_set(cid, &candidates, Some(&hit_set))
        .expect("badges");
    assert_eq!(badges.len(), 1);
    assert!(badges.contains(&b.id));
    assert!(!badges.contains(&a.id));
    assert!(!badges.contains(&c.id));
}

#[test]
fn centered_handoff_includes_late_anchor() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("center"), "Center").expect("create");
    let cid = "conv_day_bulk";

    let mut ids = Vec::new();
    for i in 0..160 {
        let id = format!("itm_{i:04}");
        let sent = format!("2024-06-15T{:02}:{:02}:00Z", 8 + i / 60, i % 60);
        let subj = format!("m{i}");
        let body = format!("body {i}");
        insert_chat(
            &matter,
            ChatSeed {
                id: &id,
                conversation_id: cid,
                sent_at: &sent,
                from: "u@ex.com",
                subject: &subj,
                text: Some(&body),
            },
        );
        ids.push(id);
    }

    // Mid/late anchor (#120).
    let anchor = &ids[120];
    let page = matter
        .list_conversation_messages_around(cid, anchor, Some(50), Some(50), false)
        .expect("around");
    assert!(
        page.iter().any(|r| r.id == *anchor),
        "centered page must include anchor"
    );
    // Window size ≈ before + 1 + after (clamped by available messages).
    assert!(
        page.len() >= 51,
        "expected at least 50 before + anchor, got {}",
        page.len()
    );
    assert!(
        page.len() <= 101,
        "default around is 50+1+50, got {}",
        page.len()
    );

    // First page of day must NOT be assumed to contain late hits.
    let first = matter
        .list_conversation_messages(cid, None, None, 100, false)
        .expect("first");
    assert!(
        !first.iter().any(|r| r.id == *anchor),
        "first page of 100 must miss item 120 (swiss-cheese handoff risk)"
    );

    // Anchor not in conversation → error.
    let err = matter
        .list_conversation_messages_around("other_conv", anchor, None, None, false)
        .expect_err("wrong conv");
    assert!(
        err.to_string().contains("not in conversation") || err.to_string().contains("anchor"),
        "got {err}"
    );
}

#[test]
fn reply_snippet_from_parent_and_missing() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("reply"), "Reply").expect("create");
    let cid = "conv_reply";

    let parent = insert_chat(
        &matter,
        ChatSeed {
            id: "itm_parent",
            conversation_id: cid,
            sent_at: "2024-06-15T09:00:00Z",
            from: "alice@ex.com",
            subject: "parent subject",
            text: Some("The budget review is due Friday."),
        },
    );
    // Chat reply parent_item_id is a soft link (0055); set via SQL so we do not
    // require email family cohesion on insert.
    let reply = insert_chat(
        &matter,
        ChatSeed {
            id: "itm_reply",
            conversation_id: cid,
            sent_at: "2024-06-15T09:30:00Z",
            from: "bob@ex.com",
            subject: "re",
            text: Some("I agree"),
        },
    );
    matter
        .connection()
        .execute(
            "UPDATE items SET parent_item_id = ?1 WHERE id = ?2",
            rusqlite::params![parent.id, reply.id],
        )
        .expect("set reply parent");
    let orphan = insert_chat(
        &matter,
        ChatSeed {
            id: "itm_orphan",
            conversation_id: cid,
            sent_at: "2024-06-15T09:45:00Z",
            from: "carol@ex.com",
            subject: "re missing",
            text: Some("ok"),
        },
    );
    matter
        .connection()
        .execute(
            "UPDATE items SET parent_item_id = ?1 WHERE id = ?2",
            rusqlite::params!["itm_does_not_exist", orphan.id],
        )
        .expect("set dangling parent");

    let stream = matter
        .list_conversation_messages(cid, None, None, 50, true)
        .expect("stream with snippets");
    let reply_row = stream.iter().find(|r| r.id == reply.id).expect("reply");
    let snip = reply_row.reply_snippet.as_deref().expect("snippet present");
    assert!(
        snip.contains("budget review") || snip.contains("Friday"),
        "snippet should contain parent text fragment, got {snip:?}"
    );

    let orphan_row = stream.iter().find(|r| r.id == orphan.id).expect("orphan");
    assert_eq!(
        orphan_row.reply_snippet.as_deref(),
        Some(REPLY_SNIPPET_UNAVAILABLE)
    );

    let solo = matter.reply_snippet_for_parent(&parent.id).expect("solo");
    assert!(
        solo.contains("budget") || solo.contains("Friday"),
        "got {solo}"
    );
}

#[test]
fn bulk_code_entire_day_bucket_applies_to_all_ids() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("bulk"), "Bulk").expect("create");
    let cid = "conv_bulk_code";

    for i in 0..5 {
        let id = format!("itm_b{i}");
        let sent = format!("2024-06-15T11:0{i}:00Z");
        let subj = format!("m{i}");
        insert_chat(
            &matter,
            ChatSeed {
                id: &id,
                conversation_id: cid,
                sent_at: &sent,
                from: "u@ex.com",
                subject: &subj,
                text: Some("body"),
            },
        );
    }

    let ids = matter.list_conversation_item_ids(cid).expect("ids");
    assert_eq!(ids.len(), 5);

    let defs = matter.list_code_definitions().expect("defs");
    let hot = defs.iter().find(|d| d.key == "hot").expect("hot code");

    let result = matter
        .apply_codes(ApplyCodesInput {
            item_ids: ids.clone(),
            add_code_ids: vec![hot.id.clone()],
            remove_code_ids: vec![],
            propagate_family: false,
            actor: "tester".into(),
        })
        .expect("apply");
    assert_eq!(result.target_count, 5);
    assert_eq!(result.target_item_ids.len(), 5);

    let codes = matter.list_item_codes(&ids).expect("codes");
    for id in &ids {
        let keys: Vec<_> = codes[id].iter().map(|c| c.key.as_str()).collect();
        assert!(keys.contains(&"hot"), "{id} missing hot");
    }
}

#[test]
fn list_discovery_with_hits_full_message_count() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("list"), "List").expect("create");

    let cid_hit = "conv_with_hit";
    let cid_miss = "conv_no_hit";

    insert_chat(
        &matter,
        ChatSeed {
            id: "itm_h1",
            conversation_id: cid_hit,
            sent_at: "2024-06-15T08:00:00Z",
            from: "a@ex.com",
            subject: "a",
            text: Some("context"),
        },
    );
    let hit = insert_chat(
        &matter,
        ChatSeed {
            id: "itm_h2",
            conversation_id: cid_hit,
            sent_at: "2024-06-15T08:05:00Z",
            from: "b@ex.com",
            subject: "hit",
            text: Some("keyword"),
        },
    );
    insert_chat(
        &matter,
        ChatSeed {
            id: "itm_h3",
            conversation_id: cid_hit,
            sent_at: "2024-06-15T08:10:00Z",
            from: "c@ex.com",
            subject: "c",
            text: Some("context"),
        },
    );
    insert_chat(
        &matter,
        ChatSeed {
            id: "itm_m1",
            conversation_id: cid_miss,
            sent_at: "2024-06-15T09:00:00Z",
            from: "d@ex.com",
            subject: "other day bucket",
            text: Some("nope"),
        },
    );

    // Unfiltered: both conversations.
    let all = matter.list_conversations(None, 50, 0).expect("all");
    assert_eq!(all.len(), 2);

    // Hit filter: only conv_with_hit.
    let hits = vec![hit.id.clone()];
    let filtered = matter
        .list_conversations(Some(&hits), 50, 0)
        .expect("filtered");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].conversation_id, cid_hit);
    assert_eq!(
        filtered[0].message_count, 3,
        "message_count must be full bucket, not hit-only"
    );
    assert_eq!(filtered[0].hit_count, 1);
    assert_eq!(filtered[0].bucket_date.as_deref(), Some("2024-06-15"));
    assert_eq!(filtered[0].team_name.as_deref(), Some("Team Alpha"));
    assert_eq!(filtered[0].channel_name.as_deref(), Some("general"));
}

#[test]
fn caps_clamp_enforced() {
    assert_eq!(
        clamp_conversation_list_limit(0),
        matter_core::CONVERSATION_LIST_DEFAULT_LIMIT
    );
    assert_eq!(
        clamp_conversation_list_limit(u64::MAX),
        CONVERSATION_LIST_MAX_LIMIT
    );
    assert_eq!(
        clamp_conversation_stream_limit(0),
        matter_core::CONVERSATION_STREAM_DEFAULT_LIMIT
    );
    assert_eq!(
        clamp_conversation_stream_limit(u64::MAX),
        CONVERSATION_STREAM_MAX_LIMIT
    );
    assert_eq!(CONVERSATION_AROUND_BEFORE, 50);
    assert_eq!(CONVERSATION_AROUND_AFTER, 50);

    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("caps"), "Caps").expect("create");
    let cid = "conv_caps";
    for i in 0..10 {
        let id = format!("itm_c{i}");
        let sent = format!("2024-06-15T12:0{i}:00Z");
        insert_chat(
            &matter,
            ChatSeed {
                id: &id,
                conversation_id: cid,
                sent_at: &sent,
                from: "u@ex.com",
                subject: "m",
                text: Some("t"),
            },
        );
    }
    // Oversize limit is clamped; still returns rows.
    let rows = matter
        .list_conversation_messages(cid, None, None, 10_000, false)
        .expect("clamp stream");
    assert_eq!(rows.len(), 10);
}

#[test]
fn stream_keyset_load_more() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("keyset"), "Keyset").expect("create");
    let cid = "conv_keyset";
    for i in 0..5 {
        let id = format!("itm_k{i}");
        let sent = format!("2024-06-15T13:0{i}:00Z");
        insert_chat(
            &matter,
            ChatSeed {
                id: &id,
                conversation_id: cid,
                sent_at: &sent,
                from: "u@ex.com",
                subject: "m",
                text: Some("t"),
            },
        );
    }
    let page1 = matter
        .list_conversation_messages(cid, None, None, 2, false)
        .expect("p1");
    assert_eq!(page1.len(), 2);
    let last = page1.last().expect("last");
    let page2 = matter
        .list_conversation_messages(
            cid,
            last.sent_at.as_deref(),
            Some(last.id.as_str()),
            10,
            false,
        )
        .expect("p2");
    assert_eq!(page2.len(), 3);
    assert!(page2.iter().all(|r| r.id != last.id));
}

#[test]
fn stream_keyset_load_earlier_and_later_cover_full_set() {
    let (_tmp, base) = utf8_tempdir();
    let matter = Matter::create(base.join("keyset_both"), "KeysetBoth").expect("create");
    let cid = "conv_keyset_both";
    let mut expected_ids = Vec::new();
    for i in 0..7 {
        let id = format!("itm_b{i}");
        expected_ids.push(id.clone());
        let sent = format!("2024-06-15T14:0{i}:00Z");
        insert_chat(
            &matter,
            ChatSeed {
                id: &id,
                conversation_id: cid,
                sent_at: &sent,
                from: "u@ex.com",
                subject: "m",
                text: Some("t"),
            },
        );
    }

    // Middle page of 3 (ids 2,3,4) via after-keyset from first two, then before from that.
    let first = matter
        .list_conversation_messages(cid, None, None, 2, false)
        .expect("first");
    assert_eq!(first.len(), 2);
    let mid = matter
        .list_conversation_messages(
            cid,
            first.last().unwrap().sent_at.as_deref(),
            Some(first.last().unwrap().id.as_str()),
            3,
            false,
        )
        .expect("mid");
    assert_eq!(
        mid.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["itm_b2", "itm_b3", "itm_b4"]
    );

    // Load earlier from mid's first row → should get itm_b0, itm_b1.
    let earlier = matter
        .list_conversation_messages_before(
            cid,
            mid.first().unwrap().sent_at.as_deref(),
            Some(mid.first().unwrap().id.as_str()),
            10,
            false,
        )
        .expect("earlier");
    assert_eq!(
        earlier.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["itm_b0", "itm_b1"]
    );

    // Load later from mid's last → itm_b5, itm_b6.
    let later = matter
        .list_conversation_messages(
            cid,
            mid.last().unwrap().sent_at.as_deref(),
            Some(mid.last().unwrap().id.as_str()),
            10,
            false,
        )
        .expect("later");
    assert_eq!(
        later.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        vec!["itm_b5", "itm_b6"]
    );

    // Reconstruct full ordered set without gaps/dups.
    let mut reconstructed: Vec<String> = earlier.iter().map(|r| r.id.clone()).collect();
    reconstructed.extend(mid.iter().map(|r| r.id.clone()));
    reconstructed.extend(later.iter().map(|r| r.id.clone()));
    assert_eq!(reconstructed, expected_ids);

    // Empty earlier page at the start.
    let empty_earlier = matter
        .list_conversation_messages_before(
            cid,
            earlier.first().unwrap().sent_at.as_deref(),
            Some(earlier.first().unwrap().id.as_str()),
            10,
            false,
        )
        .expect("empty earlier");
    assert!(empty_earlier.is_empty());
}
