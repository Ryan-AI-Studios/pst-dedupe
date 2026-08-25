//! Integration tests for track 0090 embedded-msg-hash/v1 identity.

use pst_dedup_cli::grouping_cli::parse_identity_level;
use pst_dedup_cli::scan::{run_scan, ScanOptions};
use pst_reader::PstFile;
use pst_writer::{write_unicode_pst, WriteAttachment, WriteMessage, WritePstOpts};
use tempfile::TempDir;

fn parent_with_embed(nested_subject: &str, nested_body: &str) -> WriteMessage {
    let nested = WriteMessage {
        message_id: None,
        subject: nested_subject.into(),
        sender: Some("nested@example.com".into()),
        display_to: Some("bob@example.com".into()),
        body_plain: Some(nested_body.into()),
        submit_time: Some(200),
        source_folder_path: Some("Inbox".into()),
        ..WriteMessage::default()
    };
    let mut parent = WriteMessage {
        message_id: None, // Tier-2.5 content binding
        subject: "Parent Same".into(),
        sender: Some("alice@example.com".into()),
        display_to: Some("bob@example.com".into()),
        body_plain: Some("same parent body".into()),
        submit_time: Some(100),
        source_folder_path: Some("Inbox".into()),
        ..WriteMessage::default()
    };
    parent.attachments = vec![WriteAttachment {
        filename: "embedded.msg".into(),
        mime: Some("message/rfc822".into()),
        size: 0,
        attach_method: Some(5),
        data: None,
        embedded_message: Some(Box::new(nested)),
        ..WriteAttachment::default()
    }];
    parent
}

#[test]
fn nested_subject_change_splits_at_body_recip_attach() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("embed_subject.pst");
    let messages = vec![
        parent_with_embed("Nested A", "same nested body"),
        parent_with_embed("Nested B", "same nested body"),
    ];
    write_unicode_pst(&path, messages, &[], &WritePstOpts::default()).expect("write");

    let mut opts = ScanOptions {
        retain_rows: true,
        retain_candidates: true,
        ..ScanOptions::default()
    };
    opts.grouping.identity = parse_identity_level("body-recip").expect("parse");
    let out_br = run_scan(std::slice::from_ref(&path), &opts).expect("scan body-recip");
    assert_eq!(
        out_br.summary.unique, 1,
        "body-recip ignores nested subject; unique={} grouping={:?}",
        out_br.summary.unique, out_br.summary.grouping
    );

    opts.grouping.identity = parse_identity_level("body-recip-attach").expect("parse");
    let out_att = run_scan(std::slice::from_ref(&path), &opts).expect("scan attach");
    assert_eq!(
        out_att.summary.unique, 2,
        "body-recip-attach must split on nested subject; unique={} grouping={:?}",
        out_att.summary.unique, out_att.summary.grouping
    );
    assert!(
        out_att.summary.grouping.strong_hash_embedded_parsed >= 2,
        "expected embedded parses; stats={:?}",
        out_att.summary.grouping
    );
    assert_eq!(
        out_att.summary.grouping.strong_hash_attach_digested, 0,
        "method-5 embeds must not inflate stream attach_digested; stats={:?}",
        out_att.summary.grouping
    );
}

#[test]
fn nested_body_change_splits_at_body_recip_attach() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("embed_body.pst");
    let messages = vec![
        parent_with_embed("Same Nested", "nested body A"),
        parent_with_embed("Same Nested", "nested body B"),
    ];
    write_unicode_pst(&path, messages, &[], &WritePstOpts::default()).expect("write");

    let mut opts = ScanOptions {
        retain_rows: true,
        retain_candidates: true,
        ..ScanOptions::default()
    };
    opts.grouping.identity = parse_identity_level("body-recip").expect("parse");
    let out_br = run_scan(std::slice::from_ref(&path), &opts).expect("scan body-recip");
    assert_eq!(
        out_br.summary.unique, 1,
        "body-recip collapses; unique={}",
        out_br.summary.unique
    );

    opts.grouping.identity = parse_identity_level("body-recip-attach").expect("parse");
    let out_att = run_scan(std::slice::from_ref(&path), &opts).expect("scan attach");
    assert_eq!(
        out_att.summary.unique, 2,
        "body-recip-attach must split on nested body; unique={} grouping={:?}",
        out_att.summary.unique, out_att.summary.grouping
    );
}

#[test]
fn depth_cap_chain_no_panic_produces_digests() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("embed_depth.pst");

    // Writer max_embedded_depth=3 writes at most 3 nested levels; identity
    // depth cap is also 3 (sentinel at depth>=3). Build a chain that hits the
    // writer cap so scan still completes without panic.
    let mut leaf = WriteMessage {
        message_id: None,
        subject: "Depth leaf".into(),
        sender: Some("n@ex.com".into()),
        body_plain: Some("leaf".into()),
        submit_time: Some(1),
        source_folder_path: Some("Inbox".into()),
        ..WriteMessage::default()
    };
    for d in (0..5).rev() {
        let mut parent = WriteMessage {
            message_id: None,
            subject: format!("Depth {d}"),
            sender: Some("n@ex.com".into()),
            body_plain: Some(format!("body {d}")),
            submit_time: Some(100 + d as i64),
            source_folder_path: Some("Inbox".into()),
            ..WriteMessage::default()
        };
        parent.attachments = vec![WriteAttachment {
            filename: format!("nested{d}.msg"),
            attach_method: Some(5),
            embedded_message: Some(Box::new(leaf)),
            ..WriteAttachment::default()
        }];
        leaf = parent;
    }

    let opts_w = WritePstOpts {
        max_embedded_depth: 8,
        ..WritePstOpts::default()
    };
    write_unicode_pst(&path, vec![leaf], &[], &opts_w).expect("write");

    let mut opts = ScanOptions {
        retain_rows: true,
        ..ScanOptions::default()
    };
    opts.grouping.identity = parse_identity_level("body-recip-attach").expect("parse");
    let out = run_scan(&[path], &opts).expect("scan must not panic");
    assert_eq!(out.summary.unique, 1);
    assert!(
        out.summary.grouping.strong_hash_embedded_parsed >= 1,
        "expected at least one embedded parse before depth cap; stats={:?}",
        out.summary.grouping
    );
    assert!(
        out.summary.grouping.strong_hash_embedded_depth_limit >= 1,
        "chain deeper than MAX_EMBEDDED_MSG_DEPTH must hit depth-limit sentinel; stats={:?}",
        out.summary.grouping
    );
}

#[test]
fn nested_multi_attach_follows_attachment_table_order() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("embed_att_order.pst");
    let nested = WriteMessage {
        message_id: None,
        subject: "Nested Multi".into(),
        sender: Some("nested@example.com".into()),
        body_plain: Some("nested body".into()),
        submit_time: Some(200),
        source_folder_path: Some("Inbox".into()),
        attachments: vec![
            WriteAttachment {
                filename: "first.bin".into(),
                mime: Some("application/octet-stream".into()),
                size: 3,
                attach_method: Some(1),
                data: Some(b"one".to_vec()),
                ..WriteAttachment::default()
            },
            WriteAttachment {
                filename: "second.bin".into(),
                mime: Some("application/octet-stream".into()),
                size: 3,
                attach_method: Some(1),
                data: Some(b"two".to_vec()),
                ..WriteAttachment::default()
            },
        ],
        ..WriteMessage::default()
    };
    let mut parent = WriteMessage {
        message_id: None,
        subject: "Parent".into(),
        sender: Some("alice@example.com".into()),
        body_plain: Some("parent".into()),
        submit_time: Some(100),
        source_folder_path: Some("Inbox".into()),
        ..WriteMessage::default()
    };
    parent.attachments = vec![WriteAttachment {
        filename: "embedded.msg".into(),
        attach_method: Some(5),
        embedded_message: Some(Box::new(nested)),
        ..WriteAttachment::default()
    }];
    write_unicode_pst(&path, vec![parent], &[], &WritePstOpts::default()).expect("write");

    let mut pst = PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("msg");
    let atts = pst.list_attachments(nid).expect("list");
    let fields = pst
        .read_embedded_message_identity(nid, atts[0].nid, u64::MAX)
        .expect("read embedded");
    assert_eq!(fields.child_attachments.len(), 2);
    // Writer AttachmentTable rows are in attach-index order; identity must follow that.
    assert_eq!(
        fields.child_attachments[0].filename, "first.bin",
        "child[0] must be first attachment-table row"
    );
    assert_eq!(
        fields.child_attachments[1].filename, "second.bin",
        "child[1] must be second attachment-table row"
    );
}

#[test]
fn read_embedded_message_identity_on_writer_fixture() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("embed_reader.pst");
    let msg = parent_with_embed("Reader Nested", "reader body text");
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("msg");
    let atts = pst.list_attachments(nid).expect("list");
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].attach_method, Some(5));

    let fields = pst
        .read_embedded_message_identity(nid, atts[0].nid, u64::MAX)
        .expect("read embedded identity");
    assert_eq!(fields.subject.as_deref(), Some("Reader Nested"));
    assert!(
        fields
            .body_plain
            .as_deref()
            .is_some_and(|b| b.contains("reader body text")),
        "body={:?}",
        fields.body_plain
    );
    assert!(fields.body_sha256.is_some());
    assert!(!fields.crc_suspect);
}

#[test]
fn simple_rfc822_unit_via_cli_module() {
    // Covered in attach_content_hash unit tests; smoke that CLI crate links helpers.
    assert!(pst_dedup_cli::attach_content_hash::is_embedded_identity_attach(Some(5), None));
}

fn parent_with_rfc822_bytes(nested_subject: &str, nested_body: &str) -> WriteMessage {
    let rfc822 = format!(
        "From: nested@example.com\r\n\
Subject: {nested_subject}\r\n\
To: bob@example.com\r\n\
Date: Mon, 02 Jan 2006 15:04:05 +0000\r\n\
Content-Type: text/plain\r\n\
\r\n\
{nested_body}\r\n"
    );
    let bytes = rfc822.into_bytes();
    let mut parent = WriteMessage {
        message_id: None,
        subject: "Parent Same".into(),
        sender: Some("alice@example.com".into()),
        display_to: Some("bob@example.com".into()),
        body_plain: Some("same parent body".into()),
        submit_time: Some(100),
        source_folder_path: Some("Inbox".into()),
        ..WriteMessage::default()
    };
    parent.attachments = vec![WriteAttachment {
        filename: "nested.eml".into(),
        mime: Some("message/rfc822".into()),
        size: bytes.len() as u32,
        attach_method: Some(1),
        data: Some(bytes),
        embedded_message: None,
        ..WriteAttachment::default()
    }];
    parent
}

#[test]
fn method1_rfc822_subject_change_splits_at_body_recip_attach() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("embed_rfc822.pst");
    let messages = vec![
        parent_with_rfc822_bytes("Nested A", "same nested body"),
        parent_with_rfc822_bytes("Nested B", "same nested body"),
    ];
    write_unicode_pst(&path, messages, &[], &WritePstOpts::default()).expect("write");

    let mut opts = ScanOptions {
        retain_rows: true,
        retain_candidates: true,
        ..ScanOptions::default()
    };
    opts.grouping.identity = parse_identity_level("body-recip").expect("parse");
    let out_br = run_scan(std::slice::from_ref(&path), &opts).expect("scan body-recip");
    assert_eq!(
        out_br.summary.unique, 1,
        "body-recip ignores rfc822 nested subject; unique={}",
        out_br.summary.unique
    );

    opts.grouping.identity = parse_identity_level("body-recip-attach").expect("parse");
    let out_att = run_scan(std::slice::from_ref(&path), &opts).expect("scan attach");
    assert_eq!(
        out_att.summary.unique, 2,
        "body-recip-attach must split on method-1 rfc822 nested subject; unique={} grouping={:?}",
        out_att.summary.unique, out_att.summary.grouping
    );
    assert!(
        out_att.summary.grouping.strong_hash_embedded_parsed >= 2,
        "expected rfc822 embedded parses; stats={:?}",
        out_att.summary.grouping
    );
    // Stats honesty: embedded parses must not inflate binary attach_digested.
    assert_eq!(
        out_att.summary.grouping.strong_hash_attach_digested, 0,
        "embedded-only fixtures must not count as stream digests; stats={:?}",
        out_att.summary.grouping
    );
}

#[test]
fn nested_body_over_per_attach_budget_is_unread() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("embed_budget.pst");
    // ~1.5 MiB nested body vs 1 KiB per-attach budget: reader preflight must
    // ResourceLimit before treating the embed as parsed identity.
    let large = "x".repeat(1_500_000);
    let msg = parent_with_embed("Budget Nested", &large);
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut opts = ScanOptions {
        retain_rows: true,
        strong_hash_attach_per_attach_max_bytes: 1024,
        ..ScanOptions::default()
    };
    opts.grouping.identity = parse_identity_level("body-recip-attach").expect("parse");
    let out = run_scan(&[path], &opts).expect("scan");
    assert!(
        out.summary.grouping.strong_hash_attach_unread >= 1,
        "large nested body over per-attach cap must unread; stats={:?}",
        out.summary.grouping
    );
    assert!(
        out.summary.grouping.strong_hash_attach_truncated >= 1,
        "budget hit must mark truncated; stats={:?}",
        out.summary.grouping
    );
    assert_eq!(
        out.summary.grouping.strong_hash_embedded_parsed, 0,
        "must not invent a partial embedded identity digest; stats={:?}",
        out.summary.grouping
    );
}

#[test]
fn method5_body_budget_preflight_resource_limit() {
    // Oversize nested PidTagBody must fail closed as ResourceLimit during the
    // budgeted PC load (block_payload_len_hint / load_pc_from_bids_with_body_budget)
    // before full body assemble into PropContext.subnodes; post-load
    // prop_value_byte_len remains defense-in-depth.
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("embed_body_preflight.pst");
    let large = "y".repeat(2_000_000);
    let msg = parent_with_embed("Preflight Nested", &large);
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("msg");
    let atts = pst.list_attachments(nid).expect("list");
    let err = pst
        .read_embedded_message_identity(nid, atts[0].nid, 1024)
        .expect_err("1KB budget must reject ~2MB body");
    assert!(
        matches!(err, pst_reader::PstError::ResourceLimit(_)),
        "expected ResourceLimit before full body assemble, got {err:?}"
    );
}
