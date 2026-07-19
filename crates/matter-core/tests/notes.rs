//! Integration tests for notes / highlights (schema v11 / track 0030).

use matter_core::{
    display_body_digest, highlight_status, item_role, item_status,
    re_resolve_whitespace_normalized, resolve_highlight_against_body, utf8_char_slice,
    CreateHighlightInput, FilterSpec, ItemHighlight, ItemInput, Matter, UpsertNoteInput,
    HIGHLIGHT_DEFAULT_COLOR, NOTE_BODY_MAX_BYTES, SCHEMA_VERSION,
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

#[test]
fn schema_v11_on_create() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-v11");
    let matter = Matter::create(&root, "V11").expect("create");
    assert_eq!(SCHEMA_VERSION, 19);
    assert_eq!(matter.schema_version().expect("ver"), SCHEMA_VERSION);

    let (item, _) = insert_text_item(&matter, "hello");
    let note_count: i64 = matter
        .connection()
        .query_row(
            "SELECT note_count FROM items WHERE id = ?1",
            [item.id.as_str()],
            |row| row.get(0),
        )
        .expect("note_count");
    assert_eq!(note_count, 0);
}

#[test]
fn document_note_create_list_reopen() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-note-reopen");
    let matter = Matter::create(&root, "Notes").expect("create");
    let (item, _) = insert_text_item(&matter, "body text");

    let note = matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id.clone(),
            body: "Counsel strategy note".into(),
            highlight_id: None,
            actor: "alice".into(),
        })
        .expect("create");
    assert_eq!(note.body, "Counsel strategy note");
    assert!(note.highlight_id.is_none());

    let listed = matter.list_notes(&item.id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, note.id);

    let count: i64 = matter
        .connection()
        .query_row(
            "SELECT note_count FROM items WHERE id = ?1",
            [item.id.as_str()],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(count, 1);

    drop(matter);
    let reopened = Matter::open(&root).expect("reopen");
    let again = reopened.list_notes(&item.id).expect("list reopen");
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].body, "Counsel strategy note");
}

#[test]
fn update_note_body_audit_upsert() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-note-update");
    let matter = Matter::create(&root, "Notes").expect("create");
    let (item, _) = insert_text_item(&matter, "body");

    let note = matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id.clone(),
            body: "v1".into(),
            highlight_id: None,
            actor: "bob".into(),
        })
        .expect("create");
    let created_at = note.created_at.clone();

    std::thread::sleep(std::time::Duration::from_millis(5));

    let updated = matter
        .upsert_note(UpsertNoteInput {
            id: Some(note.id.clone()),
            item_id: item.id.clone(),
            body: "v2 revised".into(),
            highlight_id: None,
            actor: "bob".into(),
        })
        .expect("update");
    assert_eq!(updated.body, "v2 revised");
    assert!(updated.updated_at >= created_at);
    assert_eq!(updated.created_at, created_at);

    let params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'note.upsert' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params).expect("json");
    assert_eq!(v["op"], "update");
    assert_eq!(v["body"], "v2 revised");
    assert_eq!(v["note_id"], note.id);
}

#[test]
fn delete_note_audit_includes_highlight_id_when_linked() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-note-del");
    let matter = Matter::create(&root, "Notes").expect("create");
    let body = "The quick brown fox jumps.";
    let (item, digest) = insert_text_item(&matter, body);

    let start = 4i64; // "quick"
    let end = 9i64;
    let quote = utf8_char_slice(body, start as usize, end as usize)
        .unwrap()
        .to_string();
    let hl = matter
        .create_highlight(CreateHighlightInput {
            item_id: item.id.clone(),
            start_utf8: start,
            end_utf8: end,
            exact_quote: quote,
            display_body: body.to_string(),
            body_digest: digest,
            color: None,
            actor: "carol".into(),
        })
        .expect("hl");

    let note = matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id.clone(),
            body: "Passage note on fox".into(),
            highlight_id: Some(hl.id.clone()),
            actor: "carol".into(),
        })
        .expect("note");

    matter.delete_note(&note.id, "carol").expect("delete");
    assert!(matter.list_notes(&item.id).expect("list").is_empty());

    let params: String = matter
        .connection()
        .query_row(
            "SELECT params_json FROM audit_events \
             WHERE action = 'note.delete' ORDER BY seq DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("audit");
    let v: serde_json::Value = serde_json::from_str(&params).expect("json");
    assert_eq!(v["body"], "Passage note on fox");
    assert_eq!(v["highlight_id"], hl.id);
    assert_eq!(v["item_id"], item.id);
}

#[test]
fn create_highlight_list_and_quote_match() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-hl-create");
    let matter = Matter::create(&root, "HL").expect("create");
    let body = "Alpha beta gamma";
    let (item, digest) = insert_text_item(&matter, body);
    let start = 6i64;
    let end = 10i64;
    let quote = "beta".to_string();

    let hl = matter
        .create_highlight(CreateHighlightInput {
            item_id: item.id.clone(),
            start_utf8: start,
            end_utf8: end,
            exact_quote: quote.clone(),
            display_body: body.to_string(),
            body_digest: digest.clone(),
            color: None,
            actor: "dave".into(),
        })
        .expect("create");
    assert_eq!(hl.exact_quote, quote);
    assert_eq!(hl.color, HIGHLIGHT_DEFAULT_COLOR);
    assert_eq!(hl.status, highlight_status::ACTIVE);
    assert_eq!(hl.body_digest, digest);

    let listed = matter.list_highlights(&item.id).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, hl.id);

    let resolved = resolve_highlight_against_body(&hl, body, &digest);
    assert_eq!(resolved.status, highlight_status::ACTIVE);
    assert_eq!(resolved.start_utf8, start);
    assert_eq!(resolved.end_utf8, end);
    assert!(!resolved.remapped);
}

#[test]
fn invalid_range_and_quote_mismatch_rejected() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-hl-bad");
    let matter = Matter::create(&root, "HL").expect("create");
    let body = "abcdef";
    let (item, digest) = insert_text_item(&matter, body);

    let err = matter
        .create_highlight(CreateHighlightInput {
            item_id: item.id.clone(),
            start_utf8: 3,
            end_utf8: 3,
            exact_quote: "".into(),
            display_body: body.to_string(),
            body_digest: digest.clone(),
            color: None,
            actor: "e".into(),
        })
        .expect_err("end==start");
    assert!(err.to_string().contains("invalid"), "{err}");

    let err = matter
        .create_highlight(CreateHighlightInput {
            item_id: item.id.clone(),
            start_utf8: 0,
            end_utf8: 3,
            exact_quote: "zzz".into(),
            display_body: body.to_string(),
            body_digest: digest,
            color: None,
            actor: "e".into(),
        })
        .expect_err("quote mismatch");
    assert!(err.to_string().contains("exact_quote"), "{err}");

    assert!(matter.list_highlights(&item.id).expect("list").is_empty());
}

#[test]
fn whitespace_only_body_drift_re_resolve_succeeds() {
    let body_orig = "foo  bar\n\nbaz";
    let body_new = "foo bar baz";
    let digest_orig = display_body_digest(body_orig);
    let digest_new = display_body_digest(body_new);
    assert_ne!(digest_orig, digest_new);

    // Quote spanning whitespace drift.
    let start = 0i64;
    let end = body_orig.chars().count() as i64;
    let quote = body_orig.to_string();
    let hl = ItemHighlight {
        id: "hlt_test".into(),
        item_id: "itm".into(),
        matter_id: "mat".into(),
        start_utf8: start,
        end_utf8: end,
        exact_quote: quote,
        prefix: None,
        suffix: None,
        body_digest: digest_orig,
        color: HIGHLIGHT_DEFAULT_COLOR.into(),
        status: highlight_status::ACTIVE.into(),
        created_at: "t".into(),
        updated_at: "t".into(),
        created_by: "t".into(),
    };

    let range = re_resolve_whitespace_normalized(&hl, body_new).expect("re-resolve");
    assert!(range.1 > range.0, "valid range {range:?}");
    let slice = utf8_char_slice(body_new, range.0 as usize, range.1 as usize).expect("slice");
    assert_eq!(slice, body_new);

    let resolved = resolve_highlight_against_body(&hl, body_new, &digest_new);
    assert_eq!(resolved.status, highlight_status::ACTIVE);
    assert!(resolved.remapped);
    assert_eq!(resolved.start_utf8, range.0);
    assert_eq!(resolved.end_utf8, range.1);
}

#[test]
fn digest_change_missing_quote_is_stale() {
    let body_orig = "The secret clause is here.";
    let body_new = "Completely different body text.";
    let hl = ItemHighlight {
        id: "hlt_stale".into(),
        item_id: "itm".into(),
        matter_id: "mat".into(),
        start_utf8: 4,
        end_utf8: 10,
        exact_quote: "secret".into(),
        prefix: Some("The ".into()),
        suffix: Some(" clause".into()),
        body_digest: display_body_digest(body_orig),
        color: HIGHLIGHT_DEFAULT_COLOR.into(),
        status: highlight_status::ACTIVE.into(),
        created_at: "t".into(),
        updated_at: "t".into(),
        created_by: "t".into(),
    };
    let resolved = resolve_highlight_against_body(&hl, body_new, &display_body_digest(body_new));
    assert_eq!(resolved.status, highlight_status::STALE);
    assert!(re_resolve_whitespace_normalized(&hl, body_new).is_none());
}

#[test]
fn ambiguous_normalized_quote_disambiguated_by_prefix() {
    // Two identical normalized quotes; prefix selects the correct occurrence.
    let body = "alpha foo beta foo gamma";
    // First "foo" at chars 6..9, second at 15..18.
    let digest_orig = display_body_digest("old body so re-resolve path is used");
    let digest_new = display_body_digest(body);
    assert_ne!(digest_orig, digest_new);

    let base = |prefix: Option<&str>| ItemHighlight {
        id: "hlt_amb".into(),
        item_id: "itm".into(),
        matter_id: "mat".into(),
        start_utf8: 0,
        end_utf8: 3,
        exact_quote: "foo".into(),
        prefix: prefix.map(|s| s.to_string()),
        suffix: None,
        body_digest: digest_orig.clone(),
        color: HIGHLIGHT_DEFAULT_COLOR.into(),
        status: highlight_status::ACTIVE.into(),
        created_at: "t".into(),
        updated_at: "t".into(),
        created_by: "t".into(),
    };

    // No prefix → two hits → ambiguous → None / stale.
    let no_prefix = base(None);
    assert!(
        re_resolve_whitespace_normalized(&no_prefix, body).is_none(),
        "ambiguous without prefix must not resolve"
    );
    let stale = resolve_highlight_against_body(&no_prefix, body, &digest_new);
    assert_eq!(stale.status, highlight_status::STALE);

    // Wrong prefix → zero filtered hits → None / stale.
    let wrong = base(Some("zzz "));
    assert!(re_resolve_whitespace_normalized(&wrong, body).is_none());
    assert_eq!(
        resolve_highlight_against_body(&wrong, body, &digest_new).status,
        highlight_status::STALE
    );

    // Correct prefix for first occurrence → single range.
    let first = base(Some("alpha "));
    let range = re_resolve_whitespace_normalized(&first, body).expect("first foo");
    assert_eq!(range, (6, 9));
    let slice = utf8_char_slice(body, range.0 as usize, range.1 as usize).expect("slice");
    assert_eq!(slice, "foo");
    let resolved = resolve_highlight_against_body(&first, body, &digest_new);
    assert_eq!(resolved.status, highlight_status::ACTIVE);
    assert!(resolved.remapped);
    assert_eq!((resolved.start_utf8, resolved.end_utf8), (6, 9));

    // Correct prefix for second occurrence.
    let second = base(Some("beta "));
    let range2 = re_resolve_whitespace_normalized(&second, body).expect("second foo");
    assert_eq!(range2, (15, 18));
}

#[test]
fn overlapping_normalized_quote_is_ambiguous() {
    // "aba" occurs twice overlapping in "ababa" (indices 0..3 and 2..5).
    // Non-overlapping advance would only see one hit and wrongly paint active.
    let body = "ababa";
    let digest_orig = display_body_digest("old body so re-resolve path is used");
    let digest_new = display_body_digest(body);
    assert_ne!(digest_orig, digest_new);

    let base = |prefix: Option<&str>, suffix: Option<&str>| ItemHighlight {
        id: "hlt_overlap".into(),
        item_id: "itm".into(),
        matter_id: "mat".into(),
        start_utf8: 0,
        end_utf8: 3,
        exact_quote: "aba".into(),
        prefix: prefix.map(|s| s.to_string()),
        suffix: suffix.map(|s| s.to_string()),
        body_digest: digest_orig.clone(),
        color: HIGHLIGHT_DEFAULT_COLOR.into(),
        status: highlight_status::ACTIVE.into(),
        created_at: "t".into(),
        updated_at: "t".into(),
        created_by: "t".into(),
    };

    // No context → two overlapping hits → ambiguous → None / stale.
    let no_ctx = base(None, None);
    assert!(
        re_resolve_whitespace_normalized(&no_ctx, body).is_none(),
        "overlapping aba/ababa without context must not resolve"
    );
    let stale = resolve_highlight_against_body(&no_ctx, body, &digest_new);
    assert_eq!(stale.status, highlight_status::STALE);

    // Suffix disambiguates first occurrence (chars after 0..3 are "ba").
    let first = base(None, Some("ba"));
    let range = re_resolve_whitespace_normalized(&first, body).expect("first aba");
    assert_eq!(range, (0, 3));
    let slice = utf8_char_slice(body, range.0 as usize, range.1 as usize).expect("slice");
    assert_eq!(slice, "aba");
    let resolved = resolve_highlight_against_body(&first, body, &digest_new);
    assert_eq!(resolved.status, highlight_status::ACTIVE);
    assert!(resolved.remapped);
    assert_eq!((resolved.start_utf8, resolved.end_utf8), (0, 3));

    // Second overlapping occurrence has no trailing suffix in the body; empty
    // trailing context cannot uniquely select it via suffix alone. Prefix of
    // the second hit is "ab" (chars 0..2 before 2..5).
    let second = base(Some("ab"), None);
    let range2 = re_resolve_whitespace_normalized(&second, body).expect("second aba");
    assert_eq!(range2, (2, 5));
}

#[test]
fn delete_highlight_unlinks_notes() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-hl-unlink");
    let matter = Matter::create(&root, "HL").expect("create");
    let body = "one two three";
    let (item, digest) = insert_text_item(&matter, body);
    let hl = matter
        .create_highlight(CreateHighlightInput {
            item_id: item.id.clone(),
            start_utf8: 4,
            end_utf8: 7,
            exact_quote: "two".into(),
            display_body: body.to_string(),
            body_digest: digest,
            color: None,
            actor: "f".into(),
        })
        .expect("hl");
    let note = matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id.clone(),
            body: "About two".into(),
            highlight_id: Some(hl.id.clone()),
            actor: "f".into(),
        })
        .expect("note");
    assert_eq!(note.highlight_id.as_deref(), Some(hl.id.as_str()));

    matter.delete_highlight(&hl.id, "f").expect("del hl");
    assert!(matter.list_highlights(&item.id).expect("hls").is_empty());
    let notes = matter.list_notes(&item.id).expect("notes");
    assert_eq!(notes.len(), 1);
    assert!(notes[0].highlight_id.is_none(), "note unlinked, body kept");
    assert_eq!(notes[0].body, "About two");
}

#[test]
fn filter_has_notes_true() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-filter-notes");
    let matter = Matter::create(&root, "Filter").expect("create");
    let set = matter
        .ensure_default_review_set(matter_core::DEFAULT_REVIEW_SET_NAME)
        .expect("set");

    let (with_note, _) = insert_text_item(&matter, "a");
    let (without, _) = insert_text_item(&matter, "b");
    for (id, order) in [(&with_note.id, 1i64), (&without.id, 2i64)] {
        matter
            .connection()
            .execute(
                "UPDATE items SET in_review = 1, review_set_id = ?1, review_order = ?2 \
                 WHERE id = ?3",
                rusqlite::params![set.id, order, id],
            )
            .expect("promote");
    }

    matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: with_note.id.clone(),
            body: "has note".into(),
            highlight_id: None,
            actor: "g".into(),
        })
        .expect("note");

    let spec = FilterSpec::preset_has_notes();
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("list");
    let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
    assert!(ids.contains(&with_note.id.as_str()), "{ids:?}");
    assert!(!ids.contains(&without.id.as_str()), "{ids:?}");
}

#[test]
fn empty_body_rejected() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-empty-note");
    let matter = Matter::create(&root, "Notes").expect("create");
    let (item, _) = insert_text_item(&matter, "x");
    let err = matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id,
            body: "   \n\t  ".into(),
            highlight_id: None,
            actor: "h".into(),
        })
        .expect_err("empty");
    assert!(err.to_string().contains("empty"), "{err}");
}

#[test]
fn oversize_note_rejected() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-big-note");
    let matter = Matter::create(&root, "Notes").expect("create");
    let (item, _) = insert_text_item(&matter, "x");
    let big = "x".repeat(NOTE_BODY_MAX_BYTES + 1);
    let err = matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id,
            body: big,
            highlight_id: None,
            actor: "i".into(),
        })
        .expect_err("oversize");
    assert!(err.to_string().contains("max size"), "{err}");
}

#[test]
fn audit_chain_verifies_after_mutations() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-note-audit");
    let matter = Matter::create(&root, "Notes").expect("create");
    let body = "range target here";
    let (item, digest) = insert_text_item(&matter, body);

    let note = matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id.clone(),
            body: "doc note".into(),
            highlight_id: None,
            actor: "j".into(),
        })
        .expect("note");
    let hl = matter
        .create_highlight(CreateHighlightInput {
            item_id: item.id.clone(),
            start_utf8: 6,
            end_utf8: 12,
            exact_quote: "target".into(),
            display_body: body.to_string(),
            body_digest: digest,
            color: None,
            actor: "j".into(),
        })
        .expect("hl");
    let passage = matter
        .upsert_note(UpsertNoteInput {
            id: None,
            item_id: item.id.clone(),
            body: "passage".into(),
            highlight_id: Some(hl.id.clone()),
            actor: "j".into(),
        })
        .expect("passage");
    matter.delete_note(&passage.id, "j").expect("del passage");
    matter.delete_note(&note.id, "j").expect("del note");
    matter.delete_highlight(&hl.id, "j").expect("del hl");

    matter.verify_audit_chain().expect("chain ok");

    // Passage-note delete payload includes highlight_id.
    let mut found = false;
    let mut stmt = matter
        .connection()
        .prepare("SELECT params_json FROM audit_events WHERE action = 'note.delete' ORDER BY seq")
        .expect("prep");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("q");
    for r in rows {
        let p = r.expect("row");
        let v: serde_json::Value = serde_json::from_str(&p).expect("json");
        if v["body"] == "passage" {
            assert_eq!(v["highlight_id"], hl.id);
            found = true;
        }
    }
    assert!(found, "passage note.delete audit with highlight_id");
}

#[test]
fn resolve_persist_stale_optional() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-persist-stale");
    let matter = Matter::create(&root, "HL").expect("create");
    let body = "keep this phrase";
    let (item, digest) = insert_text_item(&matter, body);
    let hl = matter
        .create_highlight(CreateHighlightInput {
            item_id: item.id.clone(),
            start_utf8: 5,
            end_utf8: 9,
            exact_quote: "this".into(),
            display_body: body.to_string(),
            body_digest: digest,
            color: None,
            actor: "k".into(),
        })
        .expect("hl");

    let new_body = "gone forever now";
    let new_digest = display_body_digest(new_body);
    let resolved = matter
        .resolve_highlights(&item.id, new_body, &new_digest, true)
        .expect("resolve");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].status, highlight_status::STALE);

    let stored = matter.get_highlight(&hl.id).expect("get");
    assert_eq!(stored.status, highlight_status::STALE);
}
