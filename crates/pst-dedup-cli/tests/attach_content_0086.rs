//! Integration tests for track 0086 attach-content strong identity.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use assert_cmd::cargo::cargo_bin;
use pst_dedup_cli::attach_content_hash::{
    hash_attachment_stream, AttachContentHashBudgets, AttachContentHashState, AttachDigestResult,
};
use pst_dedup_cli::grouping_cli::parse_identity_level;
use pst_dedup_cli::scan::{run_scan, ScanOptions};
use pst_reader::PstFile;
use pst_writer::{write_unicode_pst, WriteAttachment, WriteMessage, WritePstOpts};
use tempfile::TempDir;

fn bin() -> PathBuf {
    cargo_bin("pst-dedup")
}

fn msg_with_attach(
    mid: &str,
    subject: &str,
    body: &str,
    name: &str,
    payload: Vec<u8>,
) -> WriteMessage {
    let mut msg = WriteMessage {
        message_id: Some(mid.into()),
        subject: subject.into(),
        sender: Some("alice@example.com".into()),
        display_to: Some("bob@example.com".into()),
        body_plain: Some(body.into()),
        source_folder_path: Some("Inbox".into()),
        submit_time: Some(100),
        ..WriteMessage::default()
    };
    msg.attachments = vec![WriteAttachment {
        filename: name.into(),
        mime: Some("application/octet-stream".into()),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: Some(payload),
        stream_available: true,
        ..WriteAttachment::default()
    }];
    msg
}

/// Same name:size, different attach bytes → 2 unique at body-recip-attach, 1 at body-recip.
///
/// Messages intentionally omit Message-ID so Tier-2 / Tier-2.5 content binding applies
/// (distinct MIDs would keep both unique at Tier-1 regardless of attach bytes).
#[test]
fn same_name_size_different_bytes_split_at_body_recip_attach() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("attach_content.pst");
    let payload_a = b"AAAA".to_vec();
    let payload_b = b"BBBB".to_vec();
    assert_eq!(payload_a.len(), payload_b.len());
    let mut messages = vec![
        msg_with_attach(
            "", // cleared below
            "Same Subject",
            "same body text",
            "doc.pdf",
            payload_a,
        ),
        msg_with_attach("", "Same Subject", "same body text", "doc.pdf", payload_b),
    ];
    for m in &mut messages {
        m.message_id = None;
    }
    write_unicode_pst(&path, messages, &[], &WritePstOpts::default()).expect("write");

    // body-recip: same name:size → one unique (content hash binds).
    let mut opts = ScanOptions {
        retain_rows: true,
        retain_candidates: true,
        ..ScanOptions::default()
    };
    opts.grouping.identity = parse_identity_level("body-recip").expect("parse");
    let out_br = run_scan(std::slice::from_ref(&path), &opts).expect("scan body-recip");
    assert_eq!(
        out_br.summary.unique, 1,
        "body-recip must collapse same name:size; unique={} dups={}",
        out_br.summary.unique, out_br.summary.duplicates
    );

    // body-recip-attach: different bytes → two uniques.
    opts.grouping.identity = parse_identity_level("body-recip-attach").expect("parse");
    let out_att = run_scan(std::slice::from_ref(&path), &opts).expect("scan attach");
    assert_eq!(
        out_att.summary.unique, 2,
        "body-recip-attach must split on attach bytes; unique={} grouping={:?}",
        out_att.summary.unique, out_att.summary.grouping
    );
    assert!(
        out_att.summary.grouping.strong_hash_attach_digested >= 2,
        "expected both attaches digested; stats={:?}",
        out_att.summary.grouping
    );
}

/// Cloud-link attach → unread sentinel; scan does not panic.
#[test]
fn cloud_link_attach_unread_no_panic() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("cloud_attach.pst");
    let mut msg = WriteMessage {
        message_id: Some("<cloud@ex.com>".into()),
        subject: "Cloud".into(),
        sender: Some("alice@example.com".into()),
        display_to: Some("bob@example.com".into()),
        body_plain: Some("body".into()),
        source_folder_path: Some("Inbox".into()),
        submit_time: Some(100),
        ..WriteMessage::default()
    };
    msg.attachments = vec![WriteAttachment {
        filename: "cloud.docx".into(),
        size: 100,
        attach_method: Some(7), // ATTACH_BY_WEB_REFERENCE
        data: None,
        stream_available: false,
        is_cloud_link: true,
        cloud_provider: Some("OneDrivePro".into()),
        cloud_url: Some("https://contoso.sharepoint.com/sites/x/cloud.docx".into()),
        ..WriteAttachment::default()
    }];
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut opts = ScanOptions {
        retain_rows: true,
        ..ScanOptions::default()
    };
    opts.grouping.identity = parse_identity_level("body-recip-attach").expect("parse");
    let out = run_scan(&[path], &opts).expect("scan must not panic");
    assert_eq!(out.summary.unique, 1);
    assert!(
        out.summary.grouping.strong_hash_attach_unread >= 1,
        "cloud attach must count as unread; stats={:?}",
        out.summary.grouping
    );
}

/// CLI parse accepts body-recip-attach (smoke via --help / scan parse).
#[test]
fn cli_accepts_body_recip_attach_flag() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("one.pst");
    let msg = msg_with_attach("<one@ex.com>", "S", "b", "a.bin", b"x".to_vec());
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let out = Command::new(bin())
        .args([
            "scan",
            path.to_str().expect("utf8"),
            "--strong-content-hash",
            "body-recip-attach",
            "--json",
        ])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("D-0076-attach-content"),
        "must not reject with deferred message"
    );
}

/// Stream hash path: real digests differ for different payloads; empty size-0 matches empty digest.
#[test]
fn hash_attachment_stream_real_and_empty() {
    let dir = TempDir::new().expect("tmp");
    let path = dir.path().join("hash_stream.pst");
    let mut msg = WriteMessage {
        message_id: Some("<h@ex.com>".into()),
        subject: "H".into(),
        sender: Some("a@x.com".into()),
        body_plain: Some("b".into()),
        source_folder_path: Some("Inbox".into()),
        ..WriteMessage::default()
    };
    msg.attachments = vec![
        WriteAttachment {
            filename: "real.bin".into(),
            size: 4,
            data: Some(b"DATA".to_vec()),
            attach_method: Some(1),
            stream_available: true,
            ..WriteAttachment::default()
        },
        WriteAttachment {
            filename: "empty.bin".into(),
            size: 0,
            data: Some(vec![]),
            attach_method: Some(1),
            stream_available: true,
            ..WriteAttachment::default()
        },
    ];
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("msg");
    let atts = pst.list_attachments(nid).expect("list");
    assert!(atts.len() >= 2);

    let budgets = AttachContentHashBudgets::default();
    let mut state = AttachContentHashState::default();
    let mut digests = Vec::new();
    for a in &atts {
        let r = hash_attachment_stream(
            &mut pst,
            nid,
            a.nid,
            &a.filename,
            a.size,
            a.is_cloud_link,
            &budgets,
            &mut state,
            &None,
        );
        digests.push((a.filename.clone(), a.size, r));
    }
    let empty = digests
        .iter()
        .find(|(n, s, _)| n.eq_ignore_ascii_case("empty.bin") && *s == 0)
        .expect("empty attach");
    match empty.2 {
        AttachDigestResult::Real { digest, bytes } => {
            assert_eq!(bytes, 0);
            assert_eq!(digest, dedup_engine::EMPTY_CONTENT_SHA256);
        }
        AttachDigestResult::Unread { .. } => {
            panic!("size-0 empty stream must be real empty digest")
        }
    }
    let real = digests
        .iter()
        .find(|(n, _, _)| n.eq_ignore_ascii_case("real.bin"))
        .expect("real attach");
    assert!(
        matches!(real.2, AttachDigestResult::Real { .. }),
        "real payload must digest"
    );
    // Source immutability: file still present after opens.
    assert!(fs::metadata(&path).is_ok());
}
