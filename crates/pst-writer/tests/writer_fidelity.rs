//! Production PST writer fidelity tests (track 0069) — matrix §9.
//!
//! Synthetic tempfile-only fixtures; round-trips verified via `pst-reader`.

use std::io::Read;
use std::path::{Path, PathBuf};

use pst_writer::{
    write_unicode_pst, write_unicode_pst_with_streams, AttachRead, AttachStreamSource,
    AttachmentFidelityKind, FolderLayoutPolicy, NamedPropWritePlan, WriteAttachment, WriteMessage,
    WritePstOpts, WriteRecipient, WriteRecipientType,
};

fn scratch_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pst_writer_fidelity_tests");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!(
        "{name}_{}_{}.pst",
        std::process::id(),
        name.len().wrapping_mul(2654435761)
    ))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn base_msg(mid: &str, subject: &str) -> WriteMessage {
    WriteMessage {
        message_id: Some(mid.to_string()),
        subject: subject.to_string(),
        sender: Some("alice@example.com".to_string()),
        display_to: Some("bob@example.com".to_string()),
        submit_time: Some(0x01D5B035EDA780_i64),
        body_plain: Some("body".to_string()),
        ..Default::default()
    }
}

fn find_folder<'a>(
    folders: &'a [pst_reader::FolderInfo],
    name: &str,
) -> &'a pst_reader::FolderInfo {
    folders
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| panic!("folder {name} not found"))
}

fn first_message_nid(path: &Path, folder_name: &str) -> pst_reader::NodeId {
    let mut pst = pst_reader::PstFile::open(path).expect("open");
    let folders = pst.folders().expect("folders");
    let folder = find_folder(&folders, folder_name);
    assert!(
        !folder.message_nids.is_empty(),
        "folder {folder_name} has no messages"
    );
    folder.message_nids[0]
}

fn read_message_flags(path: &Path, nid: pst_reader::NodeId) -> i32 {
    let mut pst = pst_reader::PstFile::open(path).expect("open");
    let raw = pst.read_node_data(nid).expect("raw");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("pc");
    pc.get_i32(0x0E07)
        .expect("get flags")
        .expect("PidTagMessageFlags present")
}

fn read_has_attachments(path: &Path, nid: pst_reader::NodeId) -> bool {
    let mut pst = pst_reader::PstFile::open(path).expect("open");
    let extracted = pst.read_message_extract(nid).expect("extract");
    extracted.has_attachments.unwrap_or(false)
}

// ── 1: one message, one small file attach ────────────────────────────────────

#[test]
fn one_small_file_attach_list_open_flags() {
    let path = scratch_path("one_small_attach");
    cleanup(&path);

    let payload = b"hello-attach".to_vec();
    let mut msg = base_msg("<a1@ex.com>", "With attach");
    msg.attachments.push(WriteAttachment {
        filename: "note.txt".into(),
        mime: Some("text/plain".into()),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: Some(payload.clone()),
        stream_available: true,
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_written, 1);
    assert_eq!(report.attachments_failed, 0);

    let nid = first_message_nid(&path, "Unique Mail");
    assert!(read_has_attachments(&path, nid));
    let flags = read_message_flags(&path, nid);
    assert_eq!(flags & 0x1, 0x1, "MSGFLAG_READ");
    assert_eq!(flags & 0x10, 0x10, "MSGFLAG_HASATTACH");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    assert_eq!(attaches[0].filename, "note.txt");
    assert_eq!(attaches[0].size, payload.len() as u32);
    assert_eq!(attaches[0].attach_method, Some(1));

    let mut reader = pst
        .open_attachment_data(nid, attaches[0].nid)
        .expect("open data");
    let mut got = Vec::new();
    reader.read_to_end(&mut got).expect("read");
    assert_eq!(got, payload);

    cleanup(&path);
}

// ── 2: attach > 8KB via XBLOCK ───────────────────────────────────────────────

#[test]
fn large_attach_xblock_round_trip() {
    let path = scratch_path("large_attach");
    cleanup(&path);

    let payload: Vec<u8> = (0..12_000u32).map(|i| (i % 251) as u8).collect();
    assert!(payload.len() > 8176);

    let mut msg = base_msg("<a2@ex.com>", "Large attach");
    msg.attachments.push(WriteAttachment {
        filename: "big.bin".into(),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: Some(payload.clone()),
        stream_available: true,
        ..Default::default()
    });

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    let mut reader = pst
        .open_attachment_data(nid, attaches[0].nid)
        .expect("open");
    let mut got = Vec::new();
    reader.read_to_end(&mut got).expect("read");
    assert_eq!(got, payload);

    cleanup(&path);
}

// ── 3: two attaches ──────────────────────────────────────────────────────────

#[test]
fn two_attaches_both_listed() {
    let path = scratch_path("two_attaches");
    cleanup(&path);

    let a = b"aaa".to_vec();
    let b = b"bbbb".to_vec();
    let mut msg = base_msg("<a3@ex.com>", "Two");
    msg.attachments.push(WriteAttachment {
        filename: "a.txt".into(),
        size: a.len() as u32,
        attach_method: Some(1),
        data: Some(a.clone()),
        ..Default::default()
    });
    msg.attachments.push(WriteAttachment {
        filename: "b.txt".into(),
        size: b.len() as u32,
        attach_method: Some(1),
        data: Some(b.clone()),
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_written, 2);

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 2);
    let names: Vec<_> = attaches.iter().map(|a| a.filename.as_str()).collect();
    assert!(names.contains(&"a.txt"));
    assert!(names.contains(&"b.txt"));
    for att in &attaches {
        let expected = if att.filename == "a.txt" {
            a.len() as u32
        } else {
            b.len() as u32
        };
        assert_eq!(att.size, expected);
    }

    cleanup(&path);
}

// ── 4: soft fail one of two ──────────────────────────────────────────────────

#[test]
fn soft_fail_one_of_two_attaches() {
    let path = scratch_path("soft_fail");
    cleanup(&path);

    let good = b"good-bytes".to_vec();
    let mut msg = base_msg("<a4@ex.com>", "Soft fail");
    msg.attachments.push(WriteAttachment {
        filename: "good.txt".into(),
        size: good.len() as u32,
        attach_method: Some(1),
        data: Some(good.clone()),
        ..Default::default()
    });
    msg.attachments.push(WriteAttachment {
        filename: "missing.txt".into(),
        size: 99,
        attach_method: Some(1),
        data: None, // soft fail — no invent
        stream_available: true,
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_written, 1);
    assert!(report.attachments_failed >= 1);
    assert_eq!(report.messages_written, 1);

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    assert_eq!(attaches[0].filename, "good.txt");

    cleanup(&path);
}

// ── 5: parents_only ──────────────────────────────────────────────────────────

#[test]
fn parents_only_omits_attaches() {
    let path = scratch_path("parents_only");
    cleanup(&path);

    let mut msg = base_msg("<a5@ex.com>", "Parents only");
    msg.attachments.push(WriteAttachment {
        filename: "x.txt".into(),
        size: 3,
        attach_method: Some(1),
        data: Some(b"xyz".to_vec()),
        ..Default::default()
    });

    let opts = WritePstOpts {
        parents_only: true,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![msg], &[], &opts).expect("write");
    assert_eq!(report.attachments_written, 0);
    assert!(report.attachments_omitted_by_policy >= 1);

    let nid = first_message_nid(&path, "Unique Mail");
    assert!(!read_has_attachments(&path, nid));
    let flags = read_message_flags(&path, nid);
    assert_eq!(flags & 0x10, 0, "no HASATTACH bit");
    assert_eq!(flags & 0x1, 0x1, "still READ");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(nid).expect("list");
    assert!(attaches.is_empty());

    cleanup(&path);
}

// ── 6: folder path Inbox/A ───────────────────────────────────────────────────

#[test]
fn folder_path_inbox_a_under_ipm() {
    let path = scratch_path("folder_inbox_a");
    cleanup(&path);

    let mut msg = base_msg("<f6@ex.com>", "In A");
    msg.source_folder_path = Some("Inbox/A".into());

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let a = find_folder(&folders, "A");
    assert!(
        a.path.contains("Inbox") && a.path.contains("A"),
        "path={}",
        a.path
    );
    assert_eq!(a.message_nids.len(), 1);
    // Under IPM_SUBTREE
    assert!(a.path.contains("Top of Personal Folders"));

    cleanup(&path);
}

// ── 7: empty path → residual Unique Mail ─────────────────────────────────────

#[test]
fn empty_path_goes_to_residual() {
    let path = scratch_path("empty_path");
    cleanup(&path);

    let msg = base_msg("<f7@ex.com>", "Residual");
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = find_folder(&folders, "Unique Mail");
    assert_eq!(unique.message_nids.len(), 1);

    cleanup(&path);
}

// ── 8: two sources different basenames (0095 pre-seed closes D-0070) ─────────
//
// When `known_source_paths` lists ≥2 sources, prefixes are stable from message 1.

#[test]
fn multi_source_distinct_basenames() {
    let path = scratch_path("multi_src_diff");
    cleanup(&path);

    let mut m1 = base_msg("<s8a@ex.com>", "From A early");
    m1.source_path = Some(r"C:\data\alice.pst".into());
    m1.source_folder_path = Some("Inbox".into());

    let mut m2 = base_msg("<s8b@ex.com>", "From B");
    m2.source_path = Some(r"C:\data\bob.pst".into());
    m2.source_folder_path = Some("Inbox".into());

    let mut m3 = base_msg("<s8c@ex.com>", "From A late");
    m3.source_path = Some(r"C:\data\alice.pst".into());
    m3.source_folder_path = Some("Sent".into());

    let opts = WritePstOpts {
        known_source_paths: vec![r"C:\data\alice.pst".into(), r"C:\data\bob.pst".into()],
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![m1, m2, m3], &[], &opts).expect("write");
    assert!(report.folders_created >= 4);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    assert!(
        folders.iter().any(|f| f.name == "alice"),
        "alice prefix must exist from message 1 when pre-seeded"
    );
    assert!(folders.iter().any(|f| f.name == "bob"));
    // No unprefixed top-level Inbox under IPM (depth 2 = Root/ToPF/Inbox).
    let top_inboxes: Vec<_> = folders
        .iter()
        .filter(|f| f.name == "Inbox" && f.path.matches('/').count() == 2)
        .collect();
    assert!(
        top_inboxes.is_empty(),
        "pre-seed must avoid unprefixed Inbox; folders={folders:?}"
    );
    assert!(
        !folders.iter().any(|f| f.name == "Unique Mail"),
        "fully preserved tree must not allocate empty Unique Mail"
    );

    cleanup(&path);
}

// ── 9: two sources same basename ─────────────────────────────────────────────

#[test]
fn multi_source_same_basename_unique_prefixes() {
    let path = scratch_path("multi_src_same");
    cleanup(&path);

    let mut m1 = base_msg("<s9a@ex.com>", "Archive 1 early");
    m1.source_path = Some(r"C:\custodian1\archive.pst".into());
    m1.source_folder_path = Some("Inbox".into());

    let mut m2 = base_msg("<s9b@ex.com>", "Archive 2");
    m2.source_path = Some(r"C:\custodian2\archive.pst".into());
    m2.source_folder_path = Some("Inbox".into());

    let mut m3 = base_msg("<s9c@ex.com>", "Archive 1 late");
    m3.source_path = Some(r"C:\custodian1\archive.pst".into());
    m3.source_folder_path = Some("Sent".into());

    let opts = WritePstOpts {
        known_source_paths: vec![
            r"C:\custodian1\archive.pst".into(),
            r"C:\custodian2\archive.pst".into(),
        ],
        ..WritePstOpts::default()
    };
    write_unicode_pst(&path, vec![m1, m2, m3], &[], &opts).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let tops: Vec<_> = folders
        .iter()
        .filter(|f| f.path.matches('/').count() == 2 && f.name.starts_with("archive"))
        .map(|f| f.name.as_str())
        .collect();
    // Expect "archive" and "archive (2)" once both sources have been seen.
    assert!(tops.contains(&"archive"), "tops={tops:?}");
    assert!(tops.iter().any(|n| n.contains("(2)")), "tops={tops:?}");
    // Prefixed Inboxes/Sent hold the post-threshold messages.
    let archive_children: usize = folders
        .iter()
        .filter(|f| f.path.contains("archive") && (f.name == "Inbox" || f.name == "Sent"))
        .map(|f| f.message_nids.len())
        .sum();
    assert!(
        archive_children >= 2,
        "expected ≥2 msgs under source-prefixed folders; folders={folders:?}"
    );

    cleanup(&path);
}

/// Case-differing basenames must not merge under case-insensitive folder keys.
/// With known_source_paths pre-seed (0095), prefixes are stable from message 1.
#[test]
fn multi_source_case_differing_stems_unique_prefixes() {
    let path = scratch_path("multi_src_case");
    cleanup(&path);

    let mut m1 = base_msg("<sc1@ex.com>", "A early");
    m1.source_path = Some(r"C:\c1\Archive.pst".into());
    m1.source_folder_path = Some("Inbox".into());

    let mut m2 = base_msg("<sc2@ex.com>", "B");
    m2.source_path = Some(r"C:\c2\archive.pst".into());
    m2.source_folder_path = Some("Inbox".into());

    // Third source already named like a generated suffix must not collide.
    let mut m3 = base_msg("<sc3@ex.com>", "C");
    m3.source_path = Some(r"C:\c3\archive (2).pst".into());
    m3.source_folder_path = Some("Inbox".into());

    let mut m4 = base_msg("<sc1b@ex.com>", "A late");
    m4.source_path = Some(r"C:\c1\Archive.pst".into());
    m4.source_folder_path = Some("Sent".into());
    let mut m5 = base_msg("<sc2b@ex.com>", "B late");
    m5.source_path = Some(r"C:\c2\archive.pst".into());
    m5.source_folder_path = Some("Sent".into());
    let mut m6 = base_msg("<sc3b@ex.com>", "C late");
    m6.source_path = Some(r"C:\c3\archive (2).pst".into());
    m6.source_folder_path = Some("Sent".into());

    let opts = WritePstOpts {
        known_source_paths: vec![
            r"C:\c1\Archive.pst".into(),
            r"C:\c2\archive.pst".into(),
            r"C:\c3\archive (2).pst".into(),
        ],
        ..WritePstOpts::default()
    };
    write_unicode_pst(&path, vec![m1, m2, m3, m4, m5, m6], &[], &opts).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let tops: Vec<_> = folders
        .iter()
        .filter(|f| f.path.matches('/').count() == 2 && f.name.to_uppercase().contains("ARCHIVE"))
        .map(|f| f.name.clone())
        .collect();
    assert_eq!(
        tops.len(),
        3,
        "three distinct case-insensitive prefixes expected; tops={tops:?}"
    );
    // Late Sent folders under each prefix.
    let sents: Vec<_> = folders.iter().filter(|f| f.name == "Sent").collect();
    assert!(
        sents.len() >= 3,
        "expected Sent under each source prefix; sents={sents:?}"
    );

    cleanup(&path);
}

// ── 10: case collision ───────────────────────────────────────────────────────

#[test]
fn case_insensitive_folder_routing() {
    let path = scratch_path("case_fold");
    cleanup(&path);

    let mut m1 = base_msg("<c10a@ex.com>", "One");
    m1.source_folder_path = Some("Inbox/A".into());
    let mut m2 = base_msg("<c10b@ex.com>", "Two");
    m2.source_folder_path = Some("inbox/A".into());

    write_unicode_pst(&path, vec![m1, m2], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    // First-seen casing wins; only one Inbox and one A under IPM with both messages.
    let inbox = folders
        .iter()
        .filter(|f| f.name.eq_ignore_ascii_case("Inbox"))
        .collect::<Vec<_>>();
    assert_eq!(inbox.len(), 1, "one Inbox (case-insensitive)");
    assert_eq!(inbox[0].name, "Inbox", "first-seen casing");
    let a = folders
        .iter()
        .filter(|f| f.name.eq_ignore_ascii_case("A") && f.path.contains(&inbox[0].name))
        .collect::<Vec<_>>();
    assert_eq!(a.len(), 1, "single A folder under Inbox");
    assert_eq!(
        a[0].message_nids.len(),
        2,
        "both messages under the case-folded path"
    );

    cleanup(&path);
}

// ── 11: Flat policy ──────────────────────────────────────────────────────────

#[test]
fn flat_policy_single_folder() {
    let path = scratch_path("flat_policy");
    cleanup(&path);

    let mut m1 = base_msg("<f11a@ex.com>", "A");
    m1.source_folder_path = Some("Inbox/Deep".into());
    let mut m2 = base_msg("<f11b@ex.com>", "B");
    m2.source_folder_path = Some("Sent".into());

    let opts = WritePstOpts {
        folder_layout: FolderLayoutPolicy::Flat {
            folder_display_name: "All Mail".into(),
        },
        ..WritePstOpts::default()
    };
    write_unicode_pst(&path, vec![m1, m2], &[], &opts).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let all = find_folder(&folders, "All Mail");
    assert_eq!(all.message_nids.len(), 2);
    assert!(!folders.iter().any(|f| f.name == "Inbox"));

    cleanup(&path);
}

// ── 12: embedded method 0x5 shallow ──────────────────────────────────────────

#[test]
fn embedded_msg_method_5_not_silent_file() {
    let path = scratch_path("embedded_shallow");
    cleanup(&path);

    let mut nested = base_msg("<emb@ex.com>", "Nested subject");
    nested.body_plain = Some("nested body plain".into());
    nested.recipients = vec![WriteRecipient {
        recipient_type: WriteRecipientType::To,
        display_name: Some("Bob".into()),
        address_type: Some("SMTP".into()),
        email_address: Some("bob@example.com".into()),
        smtp_address: Some("bob@example.com".into()),
    }];
    let mut msg = base_msg("<f12@ex.com>", "Parent");
    msg.attachments.push(WriteAttachment {
        filename: "message.msg".into(),
        size: 0,
        attach_method: Some(5),
        data: None,
        embedded_message: Some(Box::new(nested)),
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.embedded_messages_written, 1);
    assert_eq!(report.attachments_written, 1);
    assert_eq!(report.embedded_unparsed, 0);

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    assert_eq!(attaches[0].attach_method, Some(5));
    // Must not present as a silent by-value file blob with invented bytes
    assert_ne!(attaches[0].attach_method, Some(1));
    // Size reflects nested message object (not zero / invented file length).
    assert!(
        attaches[0].size > 0,
        "embedded AttachSize should reflect nested PC size, got {}",
        attaches[0].size
    );

    // Attach PC: forbid non-empty PtypBinary 0x3701; require PtypObject 0x3701.
    let att_raw = pst
        .read_subnode_data(nid, attaches[0].nid)
        .expect("attach PC via message subnode");
    let att_pc = pst_reader::ltp::pc::PropContext::load(att_raw).expect("attach pc");
    let binary = att_pc
        .get_binary(0x3701)
        .expect("get_binary")
        .filter(|b| !b.is_empty());
    assert!(
        binary.is_none(),
        "embed must not write non-empty PidTagAttachDataBinary as a file payload"
    );
    let obj = att_pc
        .get_object(0x3701)
        .expect("get_object")
        .expect("PidTagAttachDataObject PtypObject required");
    assert!(obj.0 != 0, "object nid must be non-zero");

    // Resolve via property-primary path; nested subject/body/recipients must match.
    // DoD-1: resolved nid must equal 0x3701 object nid (not silent scan fallback).
    // Legacy files without PtypObject may still use NormalMessage scan elsewhere.
    let parent = pst.message_node_from_nbt(nid).expect("parent node");
    let nested_root = pst
        .resolve_embedded_root(&parent, attaches[0].nid)
        .expect("resolve via 0x3701 / fallback");
    assert_eq!(
        nested_root.nid.0,
        u64::from(obj.0),
        "property-primary resolve must use PidTagAttachDataObject nid"
    );
    let export = pst
        .read_export_from_message_node(&nested_root, 2, pst_reader::MAX_NESTED_EXPORT_PAYLOAD_BYTES)
        .expect("export nested");
    assert_eq!(export.subject.as_deref(), Some("Nested subject"));
    assert_eq!(export.sender.as_deref(), Some("alice@example.com"));
    assert_eq!(export.body_plain.as_deref(), Some("nested body plain"));
    assert!(
        !export.recipients.is_empty(),
        "nested recipients must round-trip non-empty"
    );
    assert_eq!(export.recipients[0].display_name.as_deref(), Some("Bob"));
    assert_eq!(
        export.recipients[0].email_address.as_deref(),
        Some("bob@example.com")
    );

    cleanup(&path);
}

// ── 13: embedded depth > MAX ─────────────────────────────────────────────────

#[test]
fn embedded_depth_cap_enforced() {
    let path = scratch_path("embed_depth");
    cleanup(&path);

    // Build chain depth 5: each message embeds the next.
    let mut leaf = base_msg("<d5@ex.com>", "Depth 5");
    for d in (0..5).rev() {
        let mut parent = base_msg(&format!("<d{d}@ex.com>"), &format!("Depth {d}"));
        parent.attachments.push(WriteAttachment {
            filename: format!("nested{d}.msg"),
            attach_method: Some(5),
            embedded_message: Some(Box::new(leaf)),
            ..Default::default()
        });
        leaf = parent;
    }

    let opts = WritePstOpts {
        max_embedded_depth: 3,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![leaf], &[], &opts).expect("write");
    assert!(
        report.embedded_depth_limit_hits > 0,
        "depth limit must fire; hits={}",
        report.embedded_depth_limit_hits
    );
    assert!(
        report.embedded_messages_written <= 3,
        "at most 3 nested written; got {}",
        report.embedded_messages_written
    );
    assert!(
        report
            .attachment_fidelity_events
            .iter()
            .any(|e| e.kind == AttachmentFidelityKind::DepthLimit),
        "per-attach ATTACH_DEPTH_LIMIT event required; events={:?}",
        report.attachment_fidelity_events
    );
    assert!(
        report
            .attachment_fidelity_events
            .iter()
            .filter(|e| e.kind == AttachmentFidelityKind::DepthLimit)
            .all(|e| e.severity == pst_writer::AttachEventSeverity::Fail
                && e.kind.as_code() == "ATTACH_DEPTH_LIMIT"),
        "depth events must be fail severity with stable code"
    );

    cleanup(&path);
}

/// 0101: writer halt at product ceiling — chain of 9 nested levels @ max 8.
#[test]
fn embedded_depth_chain_of_nine_halts_at_eight() {
    let path = scratch_path("embed_depth_9_at_8");
    cleanup(&path);

    let mut leaf = base_msg("<d9@ex.com>", "Depth 9");
    for d in (0..9).rev() {
        let mut parent = base_msg(&format!("<d{d}@ex.com>"), &format!("Depth {d}"));
        parent.attachments.push(WriteAttachment {
            filename: format!("nested{d}.msg"),
            attach_method: Some(5),
            embedded_message: Some(Box::new(leaf)),
            ..Default::default()
        });
        leaf = parent;
    }

    let opts = WritePstOpts {
        max_embedded_depth: 8,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![leaf], &[], &opts).expect("write");
    assert!(
        report.embedded_depth_limit_hits > 0,
        "depth limit must fire at ceiling; hits={}",
        report.embedded_depth_limit_hits
    );
    assert!(
        report.embedded_messages_written <= 8,
        "at most 8 nested written; got {}",
        report.embedded_messages_written
    );

    cleanup(&path);
}

// ── 14: MessageSize grows with attach bytes ──────────────────────────────────

#[test]
fn message_size_includes_attach_bytes() {
    let path_body = scratch_path("size_body_only");
    let path_attach = scratch_path("size_with_attach");
    cleanup(&path_body);
    cleanup(&path_attach);

    let body_only = base_msg("<sz0@ex.com>", "Body only");
    let payload = vec![0u8; 5000];
    let mut with_attach = base_msg("<sz1@ex.com>", "With attach");
    with_attach.body_plain = body_only.body_plain.clone();
    with_attach.attachments.push(WriteAttachment {
        filename: "blob.bin".into(),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: Some(payload),
        ..Default::default()
    });

    write_unicode_pst(&path_body, vec![body_only], &[], &WritePstOpts::default())
        .expect("write body-only");
    write_unicode_pst(
        &path_attach,
        vec![with_attach],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write with attach");

    let body_size = {
        let mut pst = pst_reader::PstFile::open(&path_body).expect("open body");
        let folders = pst.folders().expect("folders");
        let unique = find_folder(&folders, "Unique Mail");
        assert_eq!(unique.message_nids.len(), 1);
        pst.read_message_properties(unique.message_nids[0])
            .expect("p0")
            .message_size
            .expect("s0")
    };
    let attach_size = {
        let mut pst = pst_reader::PstFile::open(&path_attach).expect("open attach");
        let folders = pst.folders().expect("folders");
        let unique = find_folder(&folders, "Unique Mail");
        assert_eq!(unique.message_nids.len(), 1);
        pst.read_message_properties(unique.message_nids[0])
            .expect("p1")
            .message_size
            .expect("s1")
    };
    assert!(
        attach_size > body_size,
        "same body + attach size ({attach_size}) must be strictly greater than body-only ({body_size})"
    );
    assert!(
        attach_size > body_size + 1000,
        "attach message size ({attach_size}) should substantially exceed body-only ({body_size})"
    );

    cleanup(&path_body);
    cleanup(&path_attach);
}

// ── Stream source: success + soft fail ──────────────────────────────────────

struct MapStreamSource {
    bytes: Option<Vec<u8>>,
    fail: bool,
}

impl AttachStreamSource for MapStreamSource {
    fn open_attach(
        &mut self,
        _source_path: Option<&str>,
        _parent_nid: Option<u64>,
        _attach_nid: Option<u64>,
        _filename: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        if self.fail {
            return Err("stream open failed".into());
        }
        Ok(self.bytes.clone())
    }
}

#[test]
fn stream_source_supplies_missing_attach_data() {
    let path = scratch_path("stream_ok");
    cleanup(&path);

    let payload = b"from-stream".to_vec();
    let mut msg = base_msg("<st@ex.com>", "Streamed attach");
    msg.attachments.push(WriteAttachment {
        filename: "streamed.txt".into(),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: None,
        stream_available: true,
        ..Default::default()
    });

    let mut source = MapStreamSource {
        bytes: Some(payload.clone()),
        fail: false,
    };
    let report = write_unicode_pst_with_streams(
        &path,
        vec![msg],
        &[],
        &WritePstOpts::default(),
        Some(&mut source),
    )
    .expect("write");
    assert_eq!(report.attachments_written, 1);
    assert_eq!(report.attachments_failed, 0);

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    let mut reader = pst
        .open_attachment_data(nid, attaches[0].nid)
        .expect("open data");
    let mut got = Vec::new();
    reader.read_to_end(&mut got).expect("read");
    assert_eq!(got, payload);

    cleanup(&path);
}

#[test]
fn stream_source_err_soft_fails_attach() {
    let path = scratch_path("stream_err");
    cleanup(&path);

    let mut msg = base_msg("<sterr@ex.com>", "Stream fail");
    msg.attachments.push(WriteAttachment {
        filename: "missing.txt".into(),
        size: 10,
        attach_method: Some(1),
        data: None,
        stream_available: true,
        ..Default::default()
    });

    let mut source = MapStreamSource {
        bytes: None,
        fail: true,
    };
    let report = write_unicode_pst_with_streams(
        &path,
        vec![msg],
        &[],
        &WritePstOpts::default(),
        Some(&mut source),
    )
    .expect("write");
    assert_eq!(report.attachments_written, 0);
    assert!(report.attachments_failed >= 1);
    assert_eq!(report.messages_written, 1);

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(nid).expect("list");
    assert!(attaches.is_empty());

    cleanup(&path);
}

/// Stream returning empty bytes is a valid zero-byte payload (not a soft fail).
#[test]
fn stream_source_empty_vec_is_valid_zero_byte_attach() {
    let path = scratch_path("stream_empty");
    cleanup(&path);

    let mut msg = base_msg("<st0@ex.com>", "Stream empty");
    msg.attachments.push(WriteAttachment {
        filename: "empty-from-stream.bin".into(),
        size: 0,
        attach_method: Some(1),
        data: None,
        stream_available: true,
        ..Default::default()
    });

    let mut source = MapStreamSource {
        bytes: Some(Vec::new()),
        fail: false,
    };
    let report = write_unicode_pst_with_streams(
        &path,
        vec![msg],
        &[],
        &WritePstOpts::default(),
        Some(&mut source),
    )
    .expect("write");
    assert_eq!(report.attachments_written, 1);
    assert_eq!(report.attachments_failed, 0);

    let nid = first_message_nid(&path, "Unique Mail");
    assert!(read_has_attachments(&path, nid));
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let atts = pst.list_attachments(nid).expect("list");
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].size, 0);

    cleanup(&path);
}

// ── Inline attach MessageSize does not double-count ──────────────────────────

#[test]
fn message_size_inline_attach_not_double_counted() {
    let path_body = scratch_path("inline_att_body");
    let path_att = scratch_path("inline_att_with");
    cleanup(&path_body);
    cleanup(&path_att);

    // Small body + small attach (both heap-inline under MAX_HEAP_VALUE_SIZE).
    let body_only = base_msg("<ia0@ex.com>", "Inline size body");
    let payload = b"inline-attach-payload-xx".to_vec(); // << 3580
    let mut with_att = base_msg("<ia1@ex.com>", "Inline size att");
    with_att.body_plain = body_only.body_plain.clone();
    with_att.attachments.push(WriteAttachment {
        filename: "small.txt".into(),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: Some(payload.clone()),
        ..Default::default()
    });

    write_unicode_pst(&path_body, vec![body_only], &[], &WritePstOpts::default()).expect("body");
    write_unicode_pst(&path_att, vec![with_att], &[], &WritePstOpts::default()).expect("att");

    let body_size = {
        let mut pst = pst_reader::PstFile::open(&path_body).expect("open");
        let folders = pst.folders().expect("folders");
        let unique = find_folder(&folders, "Unique Mail");
        pst.read_message_properties(unique.message_nids[0])
            .expect("p")
            .message_size
            .expect("s")
    };
    let att_size = {
        let mut pst = pst_reader::PstFile::open(&path_att).expect("open");
        let folders = pst.folders().expect("folders");
        let unique = find_folder(&folders, "Unique Mail");
        pst.read_message_properties(unique.message_nids[0])
            .expect("p")
            .message_size
            .expect("s")
    };

    assert!(
        att_size > body_size,
        "with-attach ({att_size}) must exceed body-only ({body_size})"
    );
    // Delta is attach PC (includes inline binary once) + table overhead — not
    // 2× payload. Upper bound: payload + generous PC/table headroom.
    let delta = att_size - body_size;
    let payload_len = payload.len() as i32;
    assert!(
        delta < payload_len * 2 + 400,
        "delta ({delta}) looks like double-counting inline attach payload ({payload_len})"
    );
    assert!(
        delta as usize >= payload.len(),
        "delta ({delta}) should at least cover payload once"
    );

    cleanup(&path_body);
    cleanup(&path_att);
}

// ── Attachment table template at NBT 0x671 ───────────────────────────────────

#[test]
fn attachment_table_template_present_empty_at_0x671() {
    let path = scratch_path("att_template");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let raw = pst
        .read_node_data(pst_reader::NodeId(0x671))
        .expect("NBT template 0x671 must be readable");
    let table = pst_reader::ltp::tc::TableContext::load(raw).expect("TC load");
    assert_eq!(table.row_count(), 0, "template must have zero rows");
    assert_eq!(
        table.columns().len(),
        6,
        "attachment table template columns"
    );
    let props: Vec<u16> = table.columns().iter().map(|c| c.prop_id).collect();
    for expected in [0x0E20u16, 0x3704, 0x3705, 0x370B, 0x67F2, 0x67F3] {
        assert!(
            props.contains(&expected),
            "missing column 0x{expected:04X} in {props:?}"
        );
    }

    cleanup(&path);
}

// ── Per-message attachment table TC + RowIndex ───────────────────────────────

/// Inspect the per-message attachment-table subnode (NID type 0x11).
fn attachment_table_subnode(
    path: &Path,
    msg_nid: pst_reader::NodeId,
) -> (Vec<u8>, pst_reader::BlockId) {
    let pst = pst_reader::PstFile::open(path).expect("open");
    let msg_entry = pst.nbt().get(msg_nid).cloned().expect("message nbt");
    let mut file = std::fs::File::open(path).expect("file");
    let subs =
        pst_reader::ndb::block::list_subnode_entries(&mut file, pst.bbt(), msg_entry.bid_sub)
            .expect("message subnodes");
    let attach_tc = subs
        .iter()
        .find(|e| {
            matches!(
                e.nid.nid_type(),
                pst_reader::ndb::nid::NidType::AttachmentTable
            )
        })
        .expect("attachment table subnode");
    let data = pst_reader::ndb::block::read_block_data(
        &mut file,
        pst.bbt(),
        attach_tc.bid_data,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("attach table heap");
    (data, attach_tc.bid_sub)
}

#[test]
fn per_message_attachment_table_rows_and_row_index() {
    let path = scratch_path("msg_att_table");
    cleanup(&path);

    let payload = b"table-row-bytes".to_vec();
    let mut msg = base_msg("<tbl@ex.com>", "Att table");
    msg.attachments.push(WriteAttachment {
        filename: "row.txt".into(),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: Some(payload.clone()),
        ..Default::default()
    });

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(msg_nid).expect("list");
    assert_eq!(attaches.len(), 1);
    let attach_nid = attaches[0].nid.0 as u32;

    let (heap, bid_sub) = attachment_table_subnode(&path, msg_nid);
    assert!(
        !bid_sub.is_null(),
        "non-empty attachment TC must have bid_sub"
    );
    let mut file = std::fs::File::open(&path).expect("file");
    let pst2 = pst_reader::PstFile::open(&path).expect("open2");
    let table = pst_reader::ltp::tc::load_from_table_bids(
        heap,
        &mut file,
        pst2.bbt(),
        bid_sub,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("load attach TC");
    assert_ne!(
        table.info().hnid_rows & 0x1F,
        0,
        "hnidRows must be a NID (nidType != 0), got 0x{:08X}",
        table.info().hnid_rows
    );
    assert_eq!(table.row_count(), 1, "one attach → one table row");
    assert_eq!(
        table.get_row_id(0),
        Some(attach_nid),
        "RowIndex BTH row id must equal attach NID"
    );
    assert_eq!(
        table.get_row_u32(0, 0x67F2),
        Some(attach_nid),
        "PidTagLtpRowId column"
    );
    assert_eq!(
        table.get_row_u32(0, 0x0E20),
        Some(payload.len() as u32),
        "PidTagAttachSize"
    );
    assert_eq!(
        table.get_row_u32(0, 0x3705),
        Some(1),
        "PidTagAttachMethod ATTACH_BY_VALUE"
    );
    let fname = table
        .get_row_string(0, 0x3704)
        .expect("string")
        .expect("filename present");
    assert_eq!(fname, "row.txt");

    cleanup(&path);
}

// ── 0104: Strategy A attachment TC — multipage HN + matrix subnode ───────────

#[test]
fn attachment_tc_many_rows_round_trips() {
    let path = scratch_path("attach_tc_many");
    cleanup(&path);

    const N: usize = 200;
    let mut msg = base_msg("<att-many@ex.com>", "Many attaches");
    for i in 0..N {
        let name = format!("attach_filename_test_{i:04}.txt");
        msg.attachments.push(WriteAttachment {
            filename: name,
            size: 1,
            attach_method: Some(1),
            data: Some(b"x".to_vec()),
            ..Default::default()
        });
    }

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(msg_nid).expect("list");
    assert_eq!(attaches.len(), N);

    let (heap, bid_sub) = attachment_table_subnode(&path, msg_nid);
    assert!(!bid_sub.is_null());
    assert!(
        heap.len() > 8176,
        "200 ≥20-char filenames must exercise multi-page HN (heap {} bytes)",
        heap.len()
    );
    let mut file = std::fs::File::open(&path).expect("file");
    let pst2 = pst_reader::PstFile::open(&path).expect("open2");
    let table = pst_reader::ltp::tc::load_from_table_bids(
        heap,
        &mut file,
        pst2.bbt(),
        bid_sub,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("load attach TC");
    assert_eq!(table.row_count(), N);
    for i in 0..N {
        let expected = format!("attach_filename_test_{i:04}.txt");
        let fname = table
            .get_row_string(i, 0x3704)
            .expect("string")
            .expect("filename present");
        assert_eq!(fname, expected, "row {i}");
    }

    cleanup(&path);
}

/// >RowsPerBlock (live width 25 → Floor(8176/25)=327) so the matrix spans leaves.
#[test]
fn attachment_tc_matrix_spans_rows_per_block() {
    let path = scratch_path("attach_tc_span");
    cleanup(&path);

    const N: usize = 328;
    let mut msg = base_msg("<att-span@ex.com>", "Span matrix");
    for i in 0..N {
        msg.attachments.push(WriteAttachment {
            filename: format!("a{i}.txt"),
            size: 1,
            attach_method: Some(1),
            data: Some(b"x".to_vec()),
            ..Default::default()
        });
    }

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let attaches = pst.list_attachments(msg_nid).expect("list");
    assert_eq!(attaches.len(), N);

    let (heap, bid_sub) = attachment_table_subnode(&path, msg_nid);
    assert!(!bid_sub.is_null());
    let mut file = std::fs::File::open(&path).expect("file");
    let pst2 = pst_reader::PstFile::open(&path).expect("open2");
    let table = pst_reader::ltp::tc::load_from_table_bids(
        heap,
        &mut file,
        pst2.bbt(),
        bid_sub,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("load attach TC");
    assert_eq!(
        table.info().rgib[3],
        25,
        "re-verify attach TC row_width before RowsPerBlock"
    );
    assert_eq!(table.row_count(), N);
    // RowsPerBlock = Floor(8176/25) = 327 — sample both sides of the leaf edge.
    let edge_last = table
        .get_row_string(326, 0x3704)
        .expect("string")
        .expect("filename present");
    assert_eq!(edge_last, "a326.txt", "last row of first matrix leaf");
    let edge_first = table
        .get_row_string(327, 0x3704)
        .expect("string")
        .expect("filename present");
    assert_eq!(edge_first, "a327.txt", "first row of second matrix leaf");

    cleanup(&path);
}

#[test]
fn attachment_tc_long_filename_cell_nid() {
    let path = scratch_path("attach_tc_longcell");
    cleanup(&path);

    // UTF-16 bytes = 2 * chars; MAX_HEAP_VALUE_SIZE = 2048 → 1025 chars diverts.
    let long = "N".repeat(1025);
    let mut msg = base_msg("<att-longcell@ex.com>", "Long filename");
    msg.attachments.push(WriteAttachment {
        filename: long.clone(),
        size: 1,
        attach_method: Some(1),
        data: Some(b"x".to_vec()),
        ..Default::default()
    });

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let (heap, bid_sub) = attachment_table_subnode(&path, msg_nid);
    assert!(!bid_sub.is_null(), "cell-NID attach TC must have bid_sub");
    let mut file = std::fs::File::open(&path).expect("file");
    let pst2 = pst_reader::PstFile::open(&path).expect("open2");
    let subs = pst_reader::ndb::block::list_subnode_entries(&mut file, pst2.bbt(), bid_sub)
        .expect("attach table SLBLOCK");
    assert!(
        subs.len() >= 2,
        "filename cell + matrix: got {}",
        subs.len()
    );
    assert!(
        subs.windows(2).all(|w| w[0].nid.0 < w[1].nid.0),
        "SLBLOCK NIDs must be strictly increasing: {:?}",
        subs.iter().map(|e| e.nid.0).collect::<Vec<_>>()
    );
    let table = pst_reader::ltp::tc::load_from_table_bids(
        heap,
        &mut file,
        pst2.bbt(),
        bid_sub,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("load attach TC");
    assert_eq!(table.row_count(), 1);
    let fname = table
        .get_row_string(0, 0x3704)
        .expect("string")
        .expect("filename present");
    assert_eq!(fname, long);

    cleanup(&path);
}

// ── Recipient table template at NBT 0x692 (0082) ──────────────────────────────

#[test]
fn recipient_table_template_present_empty_at_0x692() {
    let path = scratch_path("recip_template");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let raw = pst
        .read_node_data(pst_reader::NodeId(0x692))
        .expect("NBT template 0x692 must be readable");
    let table = pst_reader::ltp::tc::TableContext::load(raw).expect("TC load");
    assert_eq!(table.row_count(), 0, "template must have zero rows");
    assert_eq!(
        table.columns().len(),
        15,
        "recipient table template: 14 MUST + SmtpAddress"
    );
    let props: Vec<u16> = table.columns().iter().map(|c| c.prop_id).collect();
    for expected in [
        0x0C15u16, 0x0E0F, 0x0FF9, 0x0FFE, 0x0FFF, 0x3001, 0x3002, 0x3003, 0x300B, 0x3900, 0x39FE,
        0x39FF, 0x3A40, 0x67F2, 0x67F3,
    ] {
        assert!(
            props.contains(&expected),
            "missing column 0x{expected:04X} in {props:?}"
        );
    }

    cleanup(&path);
}

// ── Every message has recipient subnode 0x692 (zero-row OK) ───────────────────

#[test]
fn every_message_has_recipient_table_subnode_0x692() {
    let path = scratch_path("msg_recip_empty");
    cleanup(&path);

    // No structured recipients → still emit empty TC (MS-PST MUST).
    let msg = base_msg("<recip0@ex.com>", "No recip rows");
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let table_raw = pst
        .read_subnode_data(msg_nid, pst_reader::NodeId(0x692))
        .expect("message subnode 0x692 recipient table");
    let table = pst_reader::ltp::tc::TableContext::load(table_raw).expect("TC");
    assert_eq!(table.row_count(), 0, "zero-row recipient TC still present");
    assert_eq!(table.columns().len(), 15);

    let recips = pst.list_recipients(msg_nid).expect("list_recipients");
    assert!(recips.is_empty());

    // Zero attaches → no per-message attachment table (MS-PST optional).
    let msg_entry = pst.nbt().get(msg_nid).cloned().expect("message nbt");
    let mut file = std::fs::File::open(&path).expect("file");
    let subs =
        pst_reader::ndb::block::list_subnode_entries(&mut file, pst.bbt(), msg_entry.bid_sub)
            .expect("message subnodes");
    assert!(
        subs.iter().all(|e| {
            !matches!(
                e.nid.nid_type(),
                pst_reader::ndb::nid::NidType::AttachmentTable
            )
        }),
        "zero-attach message must omit NID 0x671"
    );

    cleanup(&path);
}

// ── Display* present + empty recipient table: never invent rows (0082 P2-3) ───

/// Message with DisplayTo but **empty** structured recipients:
/// - `list_recipients` / extract.recipients stay empty
/// - Display* still present on extract / props
/// - reader does **not** invent TC rows from Display*
#[test]
fn display_to_with_empty_recipients_does_not_invent_rows() {
    let path = scratch_path("msg_recip_display_only");
    cleanup(&path);

    let mut msg = base_msg("<recip-disp@ex.com>", "Display only, empty table");
    msg.display_to = Some("Alice <alice@example.com>; Bob <bob@example.com>".into());
    msg.display_cc = Some("Carol <carol@example.com>".into());
    msg.recipients = vec![]; // explicit empty — writer emits zero-row TC
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");

    let recips = pst.list_recipients(msg_nid).expect("list_recipients");
    assert!(
        recips.is_empty(),
        "empty structured table must not invent rows from Display*, got {recips:?}"
    );

    let extract = pst.read_message_extract(msg_nid).expect("extract");
    assert_eq!(
        extract.display_to.as_deref(),
        Some("Alice <alice@example.com>; Bob <bob@example.com>")
    );
    assert_eq!(
        extract.display_cc.as_deref(),
        Some("Carol <carol@example.com>")
    );
    assert!(
        extract.recipients.is_empty(),
        "extract.recipients must stay empty when TC has no rows"
    );

    let props = pst.read_message_properties(msg_nid).expect("props");
    assert_eq!(
        props.display_to.as_deref(),
        Some("Alice <alice@example.com>; Bob <bob@example.com>")
    );
    assert!(props.recipients.is_empty());

    cleanup(&path);
}

/// Soft-fail: unreadable / missing message NID returns empty recipients (not hard error).
#[test]
fn list_recipients_soft_fail_missing_nid_returns_empty() {
    let path = scratch_path("msg_recip_soft_fail");
    cleanup(&path);

    let msg = base_msg("<recip-soft@ex.com>", "soft fail probe");
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    // Non-existent message NID: soft path → Ok(empty), never invents Display*.
    let missing = pst_reader::NodeId(0x00FF_FF00);
    let recips = pst
        .list_recipients(missing)
        .expect("list_recipients soft-fails to empty");
    assert!(recips.is_empty());

    cleanup(&path);
}

// ── Multi-recipient To/Cc round-trip via list_recipients ─────────────────────

#[test]
fn multi_recipient_to_cc_round_trip() {
    let path = scratch_path("msg_recip_multi");
    cleanup(&path);

    let mut msg = base_msg("<recip1@ex.com>", "To+Cc");
    msg.display_to = Some("Alice <alice@example.com>".into());
    msg.display_cc = Some("Bob <bob@example.com>".into());
    msg.recipients = vec![
        WriteRecipient {
            recipient_type: WriteRecipientType::To,
            display_name: Some("Alice".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("alice@example.com".into()),
            smtp_address: Some("alice@example.com".into()),
        },
        WriteRecipient {
            recipient_type: WriteRecipientType::Cc,
            display_name: Some("Bob".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("bob@example.com".into()),
            smtp_address: Some("bob@example.com".into()),
        },
    ];

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(msg_nid).expect("list_recipients");
    assert_eq!(recips.len(), 2, "To+Cc rows");
    assert_eq!(recips[0].recipient_type, pst_reader::RecipientType::To);
    assert_eq!(
        recips[0].email_address.as_deref(),
        Some("alice@example.com")
    );
    assert_eq!(recips[0].smtp_address.as_deref(), Some("alice@example.com"));
    assert_eq!(recips[1].recipient_type, pst_reader::RecipientType::Cc);
    assert_eq!(recips[1].email_address.as_deref(), Some("bob@example.com"));

    let extract = pst.read_message_extract(msg_nid).expect("extract");
    assert_eq!(
        extract.display_to.as_deref(),
        Some("Alice <alice@example.com>")
    );
    assert_eq!(extract.display_cc.as_deref(), Some("Bob <bob@example.com>"));

    cleanup(&path);
}

// ── BCC off: Bcc not in table, no DisplayBcc prop ────────────────────────────

#[test]
fn bcc_omitted_by_default() {
    let path = scratch_path("msg_recip_bcc_off");
    cleanup(&path);

    let mut msg = base_msg("<recip2@ex.com>", "BCC off");
    msg.display_to = Some("alice@example.com".into());
    msg.display_bcc = Some("secret@example.com".into());
    msg.recipients = vec![
        WriteRecipient {
            recipient_type: WriteRecipientType::To,
            display_name: Some("Alice".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("alice@example.com".into()),
            smtp_address: Some("alice@example.com".into()),
        },
        WriteRecipient {
            recipient_type: WriteRecipientType::Bcc,
            display_name: Some("Secret".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("secret@example.com".into()),
            smtp_address: Some("secret@example.com".into()),
        },
    ];

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(msg_nid).expect("list");
    assert_eq!(recips.len(), 1, "Bcc row filtered when flag off");
    assert_eq!(recips[0].recipient_type, pst_reader::RecipientType::To);
    assert_ne!(
        recips[0].email_address.as_deref(),
        Some("secret@example.com")
    );

    let extract = pst.read_message_extract(msg_nid).expect("extract");
    assert!(
        extract
            .display_bcc
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty(),
        "PidTagDisplayBcc must not be written when include_bcc_recipients=false"
    );

    cleanup(&path);
}

// ── BCC on: Bcc rows + DisplayBcc present ────────────────────────────────────

#[test]
fn bcc_included_when_flag_on() {
    let path = scratch_path("msg_recip_bcc_on");
    cleanup(&path);

    let mut msg = base_msg("<recip3@ex.com>", "BCC on");
    msg.display_to = Some("alice@example.com".into());
    msg.display_bcc = Some("secret@example.com".into());
    msg.recipients = vec![
        WriteRecipient {
            recipient_type: WriteRecipientType::To,
            display_name: Some("Alice".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("alice@example.com".into()),
            smtp_address: Some("alice@example.com".into()),
        },
        WriteRecipient {
            recipient_type: WriteRecipientType::Bcc,
            display_name: Some("Secret".into()),
            address_type: Some("SMTP".into()),
            email_address: Some("secret@example.com".into()),
            smtp_address: Some("secret@example.com".into()),
        },
    ];

    let opts = WritePstOpts {
        include_bcc_recipients: true,
        ..WritePstOpts::default()
    };
    write_unicode_pst(&path, vec![msg], &[], &opts).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(msg_nid).expect("list");
    assert_eq!(recips.len(), 2, "To+Bcc when flag on");
    assert!(
        recips
            .iter()
            .any(|r| r.recipient_type == pst_reader::RecipientType::Bcc
                && r.email_address.as_deref() == Some("secret@example.com")),
        "Bcc row present: {recips:?}"
    );

    let extract = pst.read_message_extract(msg_nid).expect("extract");
    assert_eq!(
        extract.display_bcc.as_deref(),
        Some("secret@example.com"),
        "PidTagDisplayBcc written when include_bcc_recipients=true"
    );

    cleanup(&path);
}

// ── EX-typed row without SMTP recovers address_type EX + email DN ────────────

#[test]
fn ex_typed_recipient_without_smtp_round_trip() {
    let path = scratch_path("msg_recip_ex");
    cleanup(&path);

    let dn = "/o=First Organization/ou=Exchange Administrative Group/cn=Recipients/cn=alice";
    let mut msg = base_msg("<recip4@ex.com>", "EX only");
    msg.recipients = vec![WriteRecipient {
        recipient_type: WriteRecipientType::To,
        display_name: Some("Alice Example (noisy)".into()),
        address_type: Some("EX".into()),
        email_address: Some(dn.into()),
        smtp_address: None,
    }];

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(msg_nid).expect("list");
    assert_eq!(recips.len(), 1);
    assert_eq!(recips[0].recipient_type, pst_reader::RecipientType::To);
    assert_eq!(recips[0].address_type.as_deref(), Some("EX"));
    assert_eq!(recips[0].email_address.as_deref(), Some(dn));
    assert!(
        recips[0].smtp_address.is_none(),
        "SmtpAddress must stay absent when source had none"
    );
    // Identity cascade prefers EX DN over display noise.
    let key = recips[0].identity_key().expect("identity");
    assert!(
        key.contains("/O=") || key.contains("/o="),
        "identity key should be EX DN form, got {key}"
    );

    cleanup(&path);
}

// ── MessageSize uses real attachment-table heap size ─────────────────────────

#[test]
fn message_size_uses_real_attachment_table_size() {
    // Relative check: body-only vs body+attach still holds after removing the
    // fabricated +64 table overhead; attach path must remain strictly larger.
    let path_body = scratch_path("msz_real_body");
    let path_att = scratch_path("msz_real_att");
    cleanup(&path_body);
    cleanup(&path_att);

    let body_only = base_msg("<msz0@ex.com>", "Body");
    let payload = vec![7u8; 200];
    let mut with_att = base_msg("<msz1@ex.com>", "Body+att");
    with_att.body_plain = body_only.body_plain.clone();
    with_att.attachments.push(WriteAttachment {
        filename: "p.bin".into(),
        size: payload.len() as u32,
        attach_method: Some(1),
        data: Some(payload),
        ..Default::default()
    });

    write_unicode_pst(&path_body, vec![body_only], &[], &WritePstOpts::default()).expect("b");
    write_unicode_pst(&path_att, vec![with_att], &[], &WritePstOpts::default()).expect("a");

    let body_size = {
        let mut pst = pst_reader::PstFile::open(&path_body).expect("open");
        let folders = pst.folders().expect("folders");
        let unique = find_folder(&folders, "Unique Mail");
        pst.read_message_properties(unique.message_nids[0])
            .expect("p")
            .message_size
            .expect("s")
    };
    let att_size = {
        let mut pst = pst_reader::PstFile::open(&path_att).expect("open");
        let folders = pst.folders().expect("folders");
        let unique = find_folder(&folders, "Unique Mail");
        pst.read_message_properties(unique.message_nids[0])
            .expect("p")
            .message_size
            .expect("s")
    };
    assert!(
        att_size > body_size,
        "attach MessageSize ({att_size}) must exceed body-only ({body_size}) using real table heap size"
    );

    cleanup(&path_body);
    cleanup(&path_att);
}

/// MessageSize must count attachment-table matrix bytes (`extra_content_bytes`).
///
/// 328 short-name rows → RowsPerBlock 327 → matrix payload 8176 + 25 = 8201.
/// Subtract on-disk PC/heap contributions; the residual must cover that matrix.
/// Dropping `built.extra_content_bytes` from MessageSize would leave residual ≪ 8201.
#[test]
fn message_size_counts_attachment_table_matrix_bytes() {
    let path = scratch_path("msz_att_matrix");
    cleanup(&path);

    const N: usize = 328;
    const ROW_WIDTH: usize = 25;
    const ROWS_PER_BLOCK: usize = 8176 / ROW_WIDTH; // 327
    let matrix_bytes = 8176 + (N - ROWS_PER_BLOCK) * ROW_WIDTH; // 8201

    let mut msg = base_msg("<msz-matrix@ex.com>", "Matrix size");
    msg.recipients.clear();
    for i in 0..N {
        msg.attachments.push(WriteAttachment {
            filename: format!("a{i}.txt"),
            size: 1,
            attach_method: Some(1),
            data: Some(b"x".to_vec()),
            ..Default::default()
        });
    }
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let msg_nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let message_size = pst
        .read_message_properties(msg_nid)
        .expect("props")
        .message_size
        .expect("MessageSize") as u64;

    let msg_pc_len = pst.read_node_data(msg_nid).expect("msg pc").len() as u64;
    let attaches = pst.list_attachments(msg_nid).expect("list");
    assert_eq!(attaches.len(), N);
    let mut attach_pc_sum = 0u64;
    for a in &attaches {
        attach_pc_sum += pst
            .read_subnode_data(msg_nid, a.nid)
            .expect("attach pc")
            .len() as u64;
    }

    let (table_heap, table_bid_sub) = attachment_table_subnode(&path, msg_nid);
    assert!(!table_bid_sub.is_null());
    let table_heap_len = table_heap.len() as u64;

    let (recip_heap, recip_bid_sub) = recipient_table_subnode(&path, msg_nid);
    assert!(
        recip_bid_sub.is_null(),
        "empty recip TC has no matrix/cells"
    );
    let recip_heap_len = recip_heap.len() as u64;

    let accounted_without_matrix = msg_pc_len + attach_pc_sum + table_heap_len + recip_heap_len;
    let residual = message_size.saturating_sub(accounted_without_matrix);
    assert!(
        residual >= matrix_bytes as u64,
        "MessageSize residual after PC/heaps ({residual}) must cover attach-table matrix \
         ({matrix_bytes}); got message_size={message_size} accounted_without_matrix=\
         {accounted_without_matrix} (msg_pc={msg_pc_len} attaches={attach_pc_sum} \
         table_heap={table_heap_len} recip_heap={recip_heap_len})"
    );

    cleanup(&path);
}

// ── Degraded folder path counter ─────────────────────────────────────────────

#[test]
fn folder_path_dotdot_and_overdepth_degraded_residual() {
    let path = scratch_path("path_degraded");
    cleanup(&path);

    let mut m_dotdot = base_msg("<pd1@ex.com>", "DotDot");
    m_dotdot.source_folder_path = Some("Inbox/../Secret".into());

    let deep: String = (0..33)
        .map(|i| format!("S{i}"))
        .collect::<Vec<_>>()
        .join("/");
    let mut m_deep = base_msg("<pd2@ex.com>", "Deep");
    m_deep.source_folder_path = Some(deep);

    let report = write_unicode_pst(&path, vec![m_dotdot, m_deep], &[], &WritePstOpts::default())
        .expect("write");

    assert!(
        report.folder_paths_degraded >= 1,
        "degraded count must be >= 1; got {}",
        report.folder_paths_degraded
    );
    assert!(
        report.folder_paths_residual >= 2,
        "both paths should residual; residual={}",
        report.folder_paths_residual
    );

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = find_folder(&folders, "Unique Mail");
    assert_eq!(
        unique.message_nids.len(),
        2,
        "both messages land in residual Unique Mail"
    );

    cleanup(&path);
}

// ── zero-byte by-value attach is valid ───────────────────────────────────────

#[test]
fn zero_byte_by_value_attach_is_written() {
    let path = scratch_path("zero_byte");
    cleanup(&path);

    let mut msg = base_msg("<zb@ex.com>", "Empty file");
    msg.attachments.push(WriteAttachment {
        filename: "empty.bin".into(),
        data: Some(Vec::new()),
        attach_method: Some(1),
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_written, 1);
    assert_eq!(report.attachments_failed, 0);

    let nid = first_message_nid(&path, "Unique Mail");
    assert!(read_has_attachments(&path, nid));
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let atts = pst.list_attachments(nid).expect("list");
    assert_eq!(atts.len(), 1);
    assert_eq!(atts[0].filename, "empty.bin");
    assert_eq!(atts[0].size, 0);
    let mut reader = pst
        .open_attachment_data(nid, atts[0].nid)
        .expect("open empty");
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).expect("read");
    assert!(buf.is_empty());

    cleanup(&path);
}

// ── 0094: nest with by-value child attach streams via message node ───────────

/// Stream source that opens nested child attaches via message-node API only.
struct NestedChildStreamSource {
    pst: pst_reader::PstFile,
    nodes: std::collections::HashMap<u64, pst_reader::MessageNodeRef>,
    opened_via_message_node: bool,
}

impl AttachStreamSource for NestedChildStreamSource {
    fn open_attach(
        &mut self,
        source_path: Option<&str>,
        parent_nid: Option<u64>,
        attach_nid: Option<u64>,
        filename: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self.open_attach_stream(source_path, parent_nid, attach_nid, filename)? {
            Some(mut reader) => {
                let mut buf = Vec::new();
                reader
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("read nested child: {e}"))?;
                Ok(Some(buf))
            }
            None => Ok(None),
        }
    }

    fn open_attach_stream(
        &mut self,
        _source_path: Option<&str>,
        parent_nid: Option<u64>,
        attach_nid: Option<u64>,
        _filename: &str,
    ) -> Result<Option<AttachRead>, String> {
        let parent = parent_nid.ok_or_else(|| "missing parent_nid".to_string())?;
        let attach = attach_nid.ok_or_else(|| "missing attach_nid".to_string())?;
        let root = self
            .nodes
            .get(&parent)
            .copied()
            .ok_or_else(|| format!("no MessageNodeRef for parent {parent:#x}"))?;
        // Must use message-node path — NBT open_attachment_data would NodeNotFound.
        let reader = self
            .pst
            .open_attach_data_from_message_node(&root, pst_reader::NodeId(attach))
            .map_err(|e| format!("open_attach_data_from_message_node: {e}"))?;
        self.opened_via_message_node = true;
        Ok(Some(AttachRead::from_reader(Box::new(reader))))
    }
}

#[test]
fn embedded_nest_with_by_value_child_streams() {
    // 1) Write a source PST with nest+child using buffered data (fixture on disk).
    let src_path = scratch_path("embed_child_bin_src");
    let dst_path = scratch_path("embed_child_bin_dst");
    cleanup(&src_path);
    cleanup(&dst_path);

    let mut nested = base_msg("<emb-child@ex.com>", "Nested with file");
    nested.body_plain = Some("nested body".into());
    nested.attachments.push(WriteAttachment {
        filename: "child.bin".into(),
        mime: Some("application/octet-stream".into()),
        size: 4,
        attach_method: Some(1),
        data: Some(b"ABCD".to_vec()),
        stream_available: true,
        ..Default::default()
    });
    let mut msg = base_msg("<parent-child@ex.com>", "Parent");
    msg.attachments.push(WriteAttachment {
        filename: "message.msg".into(),
        attach_method: Some(5),
        embedded_message: Some(Box::new(nested)),
        ..Default::default()
    });

    let report =
        write_unicode_pst(&src_path, vec![msg], &[], &WritePstOpts::default()).expect("write src");
    assert_eq!(report.embedded_messages_written, 1);
    assert_eq!(report.embedded_unparsed, 0);
    assert!(report.attachments_written >= 2);

    // 2) Reopen source; register nested MessageNodeRef; write dest with data:None stream.
    let nid = first_message_nid(&src_path, "Unique Mail");
    let mut src_pst = pst_reader::PstFile::open(&src_path).expect("open src");
    let attaches = src_pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    let parent = src_pst.message_node_from_nbt(nid).expect("parent");
    let nested_root = src_pst
        .resolve_embedded_root(&parent, attaches[0].nid)
        .expect("resolve nest");
    let (children, soft_skipped) = src_pst
        .list_attachments_from_message_node(&nested_root, true)
        .expect("list nested attaches");
    assert_eq!(soft_skipped, 0);
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].filename, "child.bin");
    let child_attach_nid = children[0].nid.0;

    let mut stream_src = NestedChildStreamSource {
        pst: src_pst,
        nodes: std::collections::HashMap::from([(nested_root.nid.0, nested_root)]),
        opened_via_message_node: false,
    };

    let mut nested_stream = base_msg("<emb-child@ex.com>", "Nested with file");
    nested_stream.body_plain = Some("nested body".into());
    nested_stream.source_msg_nid = Some(nested_root.nid.0);
    nested_stream.attachments.push(WriteAttachment {
        filename: "child.bin".into(),
        mime: Some("application/octet-stream".into()),
        size: 4,
        attach_method: Some(1),
        data: None,
        stream_available: true,
        attach_nid: Some(child_attach_nid),
        parent_nid: Some(nested_root.nid.0),
        source_path: Some(src_path.to_string_lossy().into_owned()),
        ..Default::default()
    });
    let mut parent_msg = base_msg("<parent-child@ex.com>", "Parent");
    parent_msg.attachments.push(WriteAttachment {
        filename: "message.msg".into(),
        attach_method: Some(5),
        embedded_message: Some(Box::new(nested_stream)),
        ..Default::default()
    });

    let dst_report = write_unicode_pst_with_streams(
        &dst_path,
        vec![parent_msg],
        &[],
        &WritePstOpts::default(),
        Some(&mut stream_src),
    )
    .expect("write dst via stream");
    assert!(
        stream_src.opened_via_message_node,
        "WRITE path must open nested child via open_attach_data_from_message_node"
    );
    assert_eq!(dst_report.embedded_messages_written, 1);
    assert_eq!(dst_report.embedded_unparsed, 0);
    assert!(dst_report.attachments_written >= 2);

    // 3) Verify dest child bytes.
    let dst_nid = first_message_nid(&dst_path, "Unique Mail");
    let mut dst = pst_reader::PstFile::open(&dst_path).expect("open dst");
    let dst_att = dst.list_attachments(dst_nid).expect("list dst");
    let dst_parent = dst.message_node_from_nbt(dst_nid).expect("dst parent");
    let dst_nested = dst
        .resolve_embedded_root(&dst_parent, dst_att[0].nid)
        .expect("dst nest");
    let (dst_children, _) = dst
        .list_attachments_from_message_node(&dst_nested, true)
        .expect("dst children");
    assert_eq!(dst_children.len(), 1);
    let mut reader = dst
        .open_attach_data_from_message_node(&dst_nested, dst_children[0].nid)
        .expect("stream dest child");
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).expect("read dest child");
    assert_eq!(buf, b"ABCD");

    cleanup(&src_path);
    cleanup(&dst_path);
}

// ── 0094: extract-side depth limit flag → ATTACH_DEPTH_LIMIT ─────────────────

#[test]
fn embedded_depth_limited_flag_maps_to_depth_limit() {
    let path = scratch_path("emb_depth_flag");
    cleanup(&path);

    let mut msg = base_msg("<dl@ex.com>", "Depth flag");
    msg.attachments.push(WriteAttachment {
        filename: "deep.msg".into(),
        attach_method: Some(5),
        embedded_message: None,
        embedded_depth_limited: true,
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert!(report.embedded_depth_limit_hits >= 1);
    assert_eq!(report.embedded_unparsed, 0);
    assert!(report
        .attachment_fidelity_events
        .iter()
        .any(|e| e.kind == AttachmentFidelityKind::DepthLimit));

    cleanup(&path);
}

// ── 0094: NestedCanonicalMessage maps through from_canonical_message ─────────

#[test]
fn from_canonical_maps_nested_embedded_message() {
    use dedup_engine::integrity::RecoverableIntegrity;
    use dedup_engine::keepset::{
        CanonicalAttachment, CanonicalMessage, MessageLocus, NestedCanonicalMessage,
    };
    use pst_writer::from_canonical_message;

    let nested = NestedCanonicalMessage {
        subject: Some("Nest".into()),
        sender: Some("n@ex.com".into()),
        body_plain: Some("nb".into()),
        message_class: Some("IPM.Note".into()),
        message_flags: Some(0x0000_0009), // READ | UNSENT sample bits
        source_msg_nid: Some(0x2004),
        ..Default::default()
    };
    let canonical = CanonicalMessage {
        locus: MessageLocus {
            source_path: "C:/fake/source.pst".into(),
            source_pst: "source.pst".into(),
            folder_path: "Inbox".into(),
            nid: 1,
            is_orphaned: false,
        },
        message_id: Some("<p@ex.com>".into()),
        subject: Some("Parent".into()),
        sender: Some("a@ex.com".into()),
        display_to: None,
        display_cc: None,
        display_bcc: None,
        recipients: Vec::new(),
        message_flags: None,
        submit_time: None,
        size: None,
        message_class: None,
        body_plain: Some("pb".into()),
        body_html: None,
        attachments: vec![CanonicalAttachment {
            filename: "e.msg".into(),
            size: 0,
            mime: None,
            data: None,
            stream_available: false,
            attach_nid: Some(0x25),
            attach_method: Some(5),
            is_cloud_link: false,
            cloud_provider: None,
            cloud_url: None,
            cloud_permission_type: None,
            embedded_message: Some(Box::new(nested)),
            embedded_extract_limit: false,
        }],
        fidelity: RecoverableIntegrity::clean(),
        message_id_norm: None,
        content_hash: [0u8; 32],
        edrm_mih_hex: None,
        body_incomplete: false,
        body_unavailable: false,
    };
    let (write_msg, _) = from_canonical_message(&canonical);
    let emb = write_msg.attachments[0]
        .embedded_message
        .as_ref()
        .expect("nested mapped");
    assert_eq!(emb.subject, "Nest");
    assert_eq!(emb.sender.as_deref(), Some("n@ex.com"));
    assert_eq!(emb.message_flags, Some(0x0000_0009));
    assert!(!write_msg.attachments[0].embedded_depth_limited);
}

// ── embedded_unparsed when method 5 without nested ───────────────────────────

#[test]
fn embedded_unparsed_method_5_without_nested() {
    let path = scratch_path("emb_unparsed");
    cleanup(&path);

    let mut msg = base_msg("<eu@ex.com>", "No nested");
    msg.attachments.push(WriteAttachment {
        filename: "missing.msg".into(),
        attach_method: Some(5),
        data: None,
        embedded_message: None,
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert!(
        report.embedded_unparsed >= 1,
        "embedded_unparsed must count method-5 without nested; got {}",
        report.embedded_unparsed
    );
    assert!(report.attachments_failed >= 1);
    assert_eq!(report.attachments_written, 0);
    assert_eq!(report.messages_written, 1);
    let ev = report
        .attachment_fidelity_events
        .iter()
        .find(|e| e.kind == AttachmentFidelityKind::EmbeddedUnparsed)
        .expect("per-attach embedded_unparsed event");
    assert_eq!(ev.message_subject, "No nested");
    assert_eq!(ev.attach_filename, "missing.msg");
    assert_eq!(ev.kind.as_code(), "ATTACH_EMBEDDED_UNPARSED");
    assert_eq!(ev.severity, pst_writer::AttachEventSeverity::Fail);
    assert_eq!(ev.attach_method, 5);

    cleanup(&path);
}

// ── 0073: reason taxonomy + locus events ─────────────────────────────────────

/// 0096: canonical → from_canonical_message → write → reader must preserve PermissionType.
#[test]
fn from_canonical_cloud_permission_type_round_trips() {
    use dedup_engine::integrity::RecoverableIntegrity;
    use dedup_engine::keepset::{CanonicalAttachment, CanonicalMessage, MessageLocus};
    use pst_writer::{from_canonical_message, AllowlistedNamedProp, NamedPropWritePlan};

    let path = scratch_path("from_canonical_perm");
    cleanup(&path);

    let canonical = CanonicalMessage {
        locus: MessageLocus {
            source_path: r"C:\src\cloud.pst".into(),
            source_pst: "cloud.pst".into(),
            folder_path: "Inbox".into(),
            nid: 0x100,
            is_orphaned: false,
        },
        message_id: Some("<cloudpermcanon@ex.com>".into()),
        subject: Some("CloudPermCanon".into()),
        sender: Some("alice@ex.com".into()),
        display_to: None,
        display_cc: None,
        display_bcc: None,
        recipients: Vec::new(),
        message_flags: None,
        submit_time: None,
        size: None,
        message_class: None,
        body_plain: Some("body".into()),
        body_html: None,
        attachments: vec![CanonicalAttachment {
            filename: "report.xlsx".into(),
            size: 0,
            mime: None,
            data: None,
            stream_available: false,
            attach_nid: Some(0x200),
            attach_method: Some(7),
            is_cloud_link: true,
            cloud_provider: Some("OneDrivePro".into()),
            cloud_url: Some("https://contoso.sharepoint.com/sites/x/report.xlsx".into()),
            cloud_permission_type: Some(1),
            embedded_message: None,
            embedded_extract_limit: false,
        }],
        fidelity: RecoverableIntegrity::clean(),
        message_id_norm: Some("cloudpermcanon@ex.com".into()),
        content_hash: [0u8; 32],
        edrm_mih_hex: None,
        body_incomplete: false,
        body_unavailable: false,
    };

    let (write_msg, _) = from_canonical_message(&canonical);
    assert_eq!(
        write_msg.attachments[0].cloud_permission_type,
        Some(1),
        "from_canonical must copy PermissionType"
    );
    let plan = NamedPropWritePlan::scan_messages(std::slice::from_ref(&write_msg));
    assert!(
        plan.contains(AllowlistedNamedProp::AttachmentPermissionType),
        "canonical permission must plan NPMAP PermissionType"
    );
    let opts = WritePstOpts {
        named_prop_plan: plan,
        ..WritePstOpts::default()
    };
    write_unicode_pst(&path, vec![write_msg], &[], &opts).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    assert!(
        pst.name_id_map()
            .attachment_permission_type_npid()
            .is_some(),
        "NPMAP must resolve AttachmentPermissionType"
    );
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("msg nid");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    assert_eq!(
        attaches[0].cloud_permission_type,
        Some(1),
        "reader must extract PermissionType after from_canonical write: {:?}",
        attaches[0]
    );

    cleanup(&path);
}

#[test]
fn cloud_link_writes_pointer_row_and_ledger() {
    let path = scratch_path("cloud_link_ptr");
    cleanup(&path);

    let mut msg = base_msg("<cloud@ex.com>", "Cloud attach");
    msg.source_path = Some(r"C:\src\cloud.pst".into());
    msg.source_folder_path = Some("Inbox".into());
    msg.attachments.push(WriteAttachment {
        filename: "report.xlsx".into(),
        size: 0,
        attach_method: Some(7), // ATTACH_BY_WEB_REFERENCE
        data: None,
        parent_nid: Some(0x100),
        attach_nid: Some(0x200),
        source_path: Some(r"C:\src\cloud.pst".into()),
        is_cloud_link: true,
        cloud_provider: Some("OneDrivePro".into()),
        cloud_url: Some("https://contoso.sharepoint.com/sites/x/report.xlsx".into()),
        cloud_permission_type: Some(1), // View (0096)
        ..Default::default()
    });

    let plan = NamedPropWritePlan::scan_messages(std::slice::from_ref(&msg));
    let opts = WritePstOpts {
        named_prop_plan: plan,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![msg], &[], &opts).expect("write");
    // Pointer row written (anti-ghost) AND fail ledger for missing payload.
    assert_eq!(
        report.attachments_written, 1,
        "CloudLink must write pointer row"
    );
    assert_eq!(
        report.attachments_failed, 1,
        "CloudLink still fail-severity"
    );
    let ev = report
        .attachment_fidelity_events
        .iter()
        .find(|e| e.kind == AttachmentFidelityKind::CloudLink)
        .expect("ATTACH_CLOUD_LINK event");
    assert_eq!(ev.kind.as_code(), "ATTACH_CLOUD_LINK");
    assert_eq!(ev.severity, pst_writer::AttachEventSeverity::Fail);
    assert_eq!(ev.cloud_provider, "OneDrivePro");
    assert!(ev.cloud_url.contains("sharepoint"));

    // Reader sees attachment-table row (not silent omit).
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    // 0092/0096: allowlisted NPMAP must resolve ProviderType + PermissionType.
    // Sorted name order: Permission < Provider → Permission=0x8000, Provider=0x8001
    // (Url also present → 0x8002).
    assert!(
        pst.name_id_map()
            .attachment_permission_type_npid()
            .is_some(),
        "0096 NPMAP must map AttachmentPermissionType"
    );
    assert!(
        pst.name_id_map().attachment_provider_type_npid().is_some(),
        "0092 NPMAP must map AttachmentProviderType"
    );
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("msg nid");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(
        attaches.len(),
        1,
        "unique-PST must keep attach row for CloudLink"
    );
    assert_eq!(attaches[0].filename, "report.xlsx");
    // Method/web-ref + URL path should re-classify as cloud on read.
    assert!(
        attaches[0].is_cloud_link || attaches[0].attach_method == Some(7),
        "reader should see cloud method or classify cloud: {:?}",
        attaches[0]
    );
    assert_eq!(
        attaches[0].cloud_provider.as_deref(),
        Some("OneDrivePro"),
        "0092 ProviderType must round-trip on attach PC: {:?}",
        attaches[0]
    );
    assert_eq!(
        attaches[0].cloud_permission_type,
        Some(1),
        "0096 PermissionType View (PcValue::I32) must round-trip: {:?}",
        attaches[0]
    );
    let perm_npid = pst
        .name_id_map()
        .attachment_permission_type_npid()
        .expect("permission npid");
    assert!(
        perm_npid >= 0x8000,
        "PermissionType NPID must be named-prop range, got {perm_npid:#x}"
    );

    cleanup(&path);
}

#[test]
fn cloud_link_named_provider_by_value_writes_web_ref_method() {
    // Named-provider CloudLink with original method=1 (BY_VALUE) but no payload
    // must not advertise BY_VALUE without binary — force ATTACH_BY_WEB_REFERENCE (7).
    let path = scratch_path("cloud_link_by_value_method");
    cleanup(&path);

    let mut msg = base_msg("<cloud-bv@ex.com>", "Cloud by-value method");
    msg.attachments.push(WriteAttachment {
        filename: "doc.docx".into(),
        size: 0,
        attach_method: Some(1), // ATTACH_BY_VALUE — dishonest without binary
        data: None,
        is_cloud_link: true,
        cloud_provider: Some("OneDrivePro".into()),
        cloud_url: Some("https://1drv.ms/x/s!xyz".into()),
        ..Default::default()
    });

    let plan = NamedPropWritePlan::scan_messages(std::slice::from_ref(&msg));
    let opts = WritePstOpts {
        named_prop_plan: plan,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![msg], &[], &opts).expect("write");
    assert_eq!(report.attachments_written, 1);
    assert_eq!(report.attachments_failed, 1);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("msg nid");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    assert_eq!(
        attaches[0].attach_method,
        Some(7),
        "CloudLink without payload must not write ATTACH_BY_VALUE: {:?}",
        attaches[0]
    );

    cleanup(&path);
}

#[test]
fn cloud_free_export_keeps_empty_npmapping() {
    // 0092 DoD-3: cloud-free exports must not grow a populated NPMAP.
    let path = scratch_path("cloud_free_empty_npmap");
    cleanup(&path);
    let msg = base_msg("<plain@ex.com>", "No cloud");
    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_written, 0);
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    assert!(
        pst.name_id_map().attachment_provider_type_npid().is_none(),
        "cloud-free unique-PST must not emit AttachmentProviderType"
    );
    assert!(
        pst.name_id_map()
            .attachment_permission_type_npid()
            .is_none(),
        "cloud-free unique-PST must not emit AttachmentPermissionType"
    );
    cleanup(&path);
}

#[test]
fn cloud_link_preserves_empty_filename() {
    let path = scratch_path("cloud_link_empty_name");
    cleanup(&path);

    let mut msg = base_msg("<cloud-empty@ex.com>", "Cloud empty name");
    msg.attachments.push(WriteAttachment {
        filename: String::new(),
        size: 0,
        attach_method: Some(7),
        data: None,
        is_cloud_link: true,
        cloud_provider: Some("OneDriveConsumer".into()),
        cloud_url: Some("https://1drv.ms/x/s!abc".into()),
        ..Default::default()
    });

    let plan = NamedPropWritePlan::scan_messages(std::slice::from_ref(&msg));
    let opts = WritePstOpts {
        named_prop_plan: plan,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![msg], &[], &opts).expect("write");
    assert_eq!(report.attachments_written, 1);
    assert_eq!(report.attachments_failed, 1);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let nid = folders
        .iter()
        .flat_map(|f| f.message_nids.iter().copied())
        .next()
        .expect("msg nid");
    let attaches = pst.list_attachments(nid).expect("list");
    assert_eq!(attaches.len(), 1);
    assert_eq!(
        attaches[0].filename, "",
        "must not invent 'cloud-link' when source filename empty"
    );
    assert!(
        attaches[0].is_cloud_link
            || attaches[0]
                .cloud_url
                .as_deref()
                .is_some_and(|u| u.contains("1drv")),
        "URL pointer should still be preserved: {:?}",
        attaches[0]
    );

    cleanup(&path);
}

#[test]
fn method_unsupported_emits_fail_event() {
    let path = scratch_path("method_unsup");
    cleanup(&path);

    let mut msg = base_msg("<mu@ex.com>", "Bad method");
    msg.source_path = Some(r"C:\src\a.pst".into());
    msg.source_folder_path = Some("Inbox".into());
    msg.attachments.push(WriteAttachment {
        filename: "cloud.ref".into(),
        size: 0,
        attach_method: Some(4), // not by-value / not embedded
        data: None,
        parent_nid: Some(0x100),
        attach_nid: Some(0x200),
        source_path: Some(r"C:\src\a.pst".into()),
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_failed, 1);
    assert_eq!(report.attachments_written, 0);
    let fail_events: Vec<_> = report
        .attachment_fidelity_events
        .iter()
        .filter(|e| e.severity == pst_writer::AttachEventSeverity::Fail)
        .collect();
    assert_eq!(fail_events.len() as u64, report.attachments_failed);
    let ev = fail_events
        .iter()
        .find(|e| e.kind == AttachmentFidelityKind::MethodUnsupported)
        .expect("METHOD_UNSUPPORTED event");
    assert_eq!(ev.kind.as_code(), "ATTACH_METHOD_UNSUPPORTED");
    assert_eq!(ev.attach_method, 4);
    assert_eq!(ev.msg_nid, 0x100);
    assert_eq!(ev.attach_index, 0);
    assert_eq!(ev.source_path, r"C:\src\a.pst");

    cleanup(&path);
}

#[test]
fn stream_open_fail_emits_stream_open_failed() {
    let path = scratch_path("stream_open_evt");
    cleanup(&path);

    let mut msg = base_msg("<so@ex.com>", "Open fail");
    msg.attachments.push(WriteAttachment {
        filename: "missing.txt".into(),
        size: 10,
        attach_method: Some(1),
        data: None,
        stream_available: true,
        parent_nid: Some(9),
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_failed, 1);
    let ev = report
        .attachment_fidelity_events
        .iter()
        .find(|e| e.kind == AttachmentFidelityKind::StreamOpenFailed)
        .expect("STREAM_OPEN_FAILED");
    assert_eq!(ev.kind.as_code(), "ATTACH_STREAM_OPEN_FAILED");
    assert_eq!(ev.severity, pst_writer::AttachEventSeverity::Fail);
    assert_eq!(
        report
            .attachment_fidelity_events
            .iter()
            .filter(|e| e.severity == pst_writer::AttachEventSeverity::Fail)
            .count() as u64,
        report.attachments_failed
    );

    cleanup(&path);
}

#[test]
fn parents_only_omit_info_events_do_not_fail() {
    let path = scratch_path("parents_only_evt");
    cleanup(&path);

    let mut msg = base_msg("<po@ex.com>", "Parents only events");
    msg.attachments.push(WriteAttachment {
        filename: "x.txt".into(),
        size: 3,
        attach_method: Some(1),
        data: Some(b"xyz".to_vec()),
        parent_nid: Some(1),
        ..Default::default()
    });
    msg.attachments.push(WriteAttachment {
        filename: "y.txt".into(),
        size: 1,
        attach_method: Some(1),
        data: Some(b"z".to_vec()),
        parent_nid: Some(1),
        ..Default::default()
    });

    let opts = WritePstOpts {
        parents_only: true,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![msg], &[], &opts).expect("write");
    assert_eq!(report.attachments_failed, 0);
    assert_eq!(report.attachments_omitted_by_policy, 2);
    let omit: Vec<_> = report
        .attachment_fidelity_events
        .iter()
        .filter(|e| e.kind == AttachmentFidelityKind::OmittedByPolicy)
        .collect();
    assert_eq!(omit.len(), 2);
    assert!(omit
        .iter()
        .all(|e| e.severity == pst_writer::AttachEventSeverity::Info
            && e.kind.as_code() == "ATTACH_OMITTED_BY_POLICY"));
    assert!(
        report
            .attachment_fidelity_events
            .iter()
            .filter(|e| e.severity == pst_writer::AttachEventSeverity::Fail)
            .count()
            == 0
    );

    cleanup(&path);
}

/// 0073: `attach_list_failed` emits one ATTACH_META_FAILED and increments attachments_failed.
#[test]
fn attach_list_failed_emits_meta_failed() {
    let path = scratch_path("attach_list_meta");
    cleanup(&path);

    let mut msg = base_msg("<meta@ex.com>", "Meta failed list");
    msg.source_path = Some(r"C:\src\mail.pst".into());
    msg.source_folder_path = Some("Inbox".into());
    msg.source_msg_nid = Some(0x2004);
    msg.attach_list_failed = true;
    // Empty attach list (list_attachments failed) — still one fail event.

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_failed, 1);
    let meta: Vec<_> = report
        .attachment_fidelity_events
        .iter()
        .filter(|e| e.kind == AttachmentFidelityKind::MetaFailed)
        .collect();
    assert_eq!(meta.len(), 1);
    assert_eq!(meta[0].severity, pst_writer::AttachEventSeverity::Fail);
    assert_eq!(meta[0].msg_nid, 0x2004);
    assert_eq!(meta[0].attach_method, -1);
    assert_eq!(meta[0].kind.as_code(), "ATTACH_META_FAILED");
    assert_eq!(
        report
            .attachment_fidelity_events
            .iter()
            .filter(|e| e.severity == pst_writer::AttachEventSeverity::Fail)
            .count() as u64,
        report.attachments_failed
    );

    cleanup(&path);
}

/// 0073: MetaFailed + parents_only omits both appear; omit does not add to failed.
#[test]
fn attach_list_failed_with_parents_only_still_fails() {
    let path = scratch_path("meta_parents_only");
    cleanup(&path);

    let mut msg = base_msg("<both@ex.com>", "Meta + parents");
    msg.source_msg_nid = Some(7);
    msg.attach_list_failed = true;
    msg.attachments.push(WriteAttachment {
        filename: "ghost.txt".into(),
        size: 1,
        attach_method: Some(1),
        data: Some(b"x".to_vec()),
        parent_nid: Some(7),
        ..Default::default()
    });

    let opts = WritePstOpts {
        parents_only: true,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![msg], &[], &opts).expect("write");
    assert_eq!(report.attachments_failed, 1);
    assert_eq!(report.attachments_omitted_by_policy, 1);
    assert!(report
        .attachment_fidelity_events
        .iter()
        .any(|e| e.kind == AttachmentFidelityKind::MetaFailed));
    assert!(report
        .attachment_fidelity_events
        .iter()
        .any(|e| e.kind == AttachmentFidelityKind::OmittedByPolicy));

    cleanup(&path);
}

#[test]
fn zero_byte_by_value_still_succeeds() {
    let path = scratch_path("zero_byte_0073");
    cleanup(&path);

    let mut msg = base_msg("<zb@ex.com>", "Zero byte");
    msg.attachments.push(WriteAttachment {
        filename: "empty.bin".into(),
        size: 0,
        attach_method: Some(1),
        data: Some(Vec::new()),
        ..Default::default()
    });

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.attachments_written, 1);
    assert_eq!(report.attachments_failed, 0);
    assert!(report.attachment_fidelity_events.is_empty());

    cleanup(&path);
}

#[test]
fn stream_read_fail_emits_stream_read_failed() {
    use std::io::{self, Read};

    struct FailAfterChunk {
        sent: bool,
    }
    impl Read for FailAfterChunk {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.sent {
                self.sent = true;
                let n = buf.len().min(100);
                buf[..n].fill(0xAB);
                return Ok(n);
            }
            Err(io::Error::other("mid-stream fail"))
        }
    }
    struct FailSrc;
    impl AttachStreamSource for FailSrc {
        fn open_attach(
            &mut self,
            _: Option<&str>,
            _: Option<u64>,
            _: Option<u64>,
            _: &str,
        ) -> Result<Option<Vec<u8>>, String> {
            Ok(None)
        }
        fn open_attach_stream(
            &mut self,
            _: Option<&str>,
            _: Option<u64>,
            _: Option<u64>,
            _: &str,
        ) -> Result<Option<AttachRead>, String> {
            Ok(Some(AttachRead::from_reader(Box::new(FailAfterChunk {
                sent: false,
            }))))
        }
    }

    let path = scratch_path("stream_read_evt");
    cleanup(&path);
    let mut msg = base_msg("<sr@ex.com>", "Stream read fail");
    msg.attachments.push(WriteAttachment {
        filename: "x.bin".into(),
        size: 100_000,
        attach_method: Some(1),
        data: None,
        stream_available: true,
        parent_nid: Some(3),
        ..Default::default()
    });
    let mut src = FailSrc;
    let report = write_unicode_pst_with_streams(
        &path,
        vec![msg],
        &[],
        &WritePstOpts::default(),
        Some(&mut src),
    )
    .expect("write");
    assert_eq!(report.attachments_failed, 1);
    assert!(report
        .attachment_fidelity_events
        .iter()
        .any(|e| e.kind == AttachmentFidelityKind::StreamReadFailed
            && e.kind.as_code() == "ATTACH_STREAM_READ_FAILED"));
    assert_eq!(
        report
            .attachment_fidelity_events
            .iter()
            .filter(|e| e.severity == pst_writer::AttachEventSeverity::Fail)
            .count() as u64,
        report.attachments_failed
    );

    cleanup(&path);
}

// ── 15: 0068 regression smoke (large body still works) ───────────────────────

#[test]
fn regression_large_body_still_round_trips() {
    let path = scratch_path("reg_body");
    cleanup(&path);

    let long_body: String = "The quick brown fox jumps over the lazy dog. ".repeat(300);
    let mut msg = base_msg("<reg@ex.com>", "Big body");
    msg.body_plain = Some(long_body.clone());

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let extracted = pst.read_message_extract(nid).expect("extract");
    assert_eq!(extracted.body_text.as_deref(), Some(long_body.as_str()));
    assert_eq!(extracted.has_attachments, Some(false));
    let flags = read_message_flags(&path, nid);
    assert_eq!(flags & 0x10, 0);

    cleanup(&path);
}

// ── 0093: cumulative helper-string diversion (multiple 1.5–2 KiB strings) ─────

#[test]
fn cumulative_helper_strings_divert_without_heap_overflow() {
    let path = scratch_path("heap_multi_helpers");
    cleanup(&path);

    // Spec DoD-1: multiple helpers in the 1.5–2 KiB *UTF-16 encoded* band
    // (under per-value 2048 so only cumulative escalate+reprobe saves the page).
    // ASCII: N chars → 2N UTF-16 bytes; use ~950 chars → ~1900 bytes each.
    let band = |seed: &str| -> String {
        let mut s = String::new();
        while s.len() < 950 {
            s.push_str(seed);
        }
        s.truncate(950);
        s
    };
    let subject = band("SUBJ-");
    let sender = band("SENDER@ex.com;");
    let display_to = band("To Person <to@ex.com>; ");
    let display_cc = band("Cc Person <cc@ex.com>; ");
    let message_class = band("IPM.Note.Custom.");

    assert!(
        (1500..2048).contains(&(subject.len() * 2)),
        "subject UTF-16 bytes should be in 1.5–2KiB band, got {}",
        subject.len() * 2
    );

    let mut msg = base_msg("<heap-multi@ex.com>", &subject);
    msg.sender = Some(sender.clone());
    msg.display_to = Some(display_to.clone());
    msg.display_cc = Some(display_cc.clone());
    msg.message_class = Some(message_class.clone());
    msg.body_plain = Some("short body".into());

    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default())
        .expect("write must not heap-overflow with multiple ~2KiB helpers");
    assert_eq!(report.messages_written, 1);

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let extract = pst.read_message_extract(nid).expect("extract");
    assert_eq!(extract.subject.as_deref(), Some(subject.as_str()));
    assert_eq!(extract.sender_email.as_deref(), Some(sender.as_str()));
    assert_eq!(extract.display_to.as_deref(), Some(display_to.as_str()));
    assert_eq!(extract.display_cc.as_deref(), Some(display_cc.as_str()));
    assert_eq!(
        extract.message_class.as_deref(),
        Some(message_class.as_str()),
        "message_class must round-trip via diversion helper"
    );

    cleanup(&path);
}

// ── 0100: Strategy A recipient TC — all included rows, multi-page HN ─────────

fn recip_row(ty: WriteRecipientType, label: &str, i: usize) -> WriteRecipient {
    WriteRecipient {
        recipient_type: ty,
        display_name: Some(format!("{label}{i}")),
        address_type: Some("SMTP".into()),
        email_address: Some(format!("{}{i}@ex.com", label.to_ascii_lowercase())),
        smtp_address: Some(format!("{}{i}@ex.com", label.to_ascii_lowercase())),
    }
}

/// Inspect the per-message recipient-table subnode (NID type 0x12).
fn recipient_table_subnode(
    path: &Path,
    msg_nid: pst_reader::NodeId,
) -> (Vec<u8>, pst_reader::BlockId) {
    let pst = pst_reader::PstFile::open(path).expect("open");
    let msg_entry = pst.nbt().get(msg_nid).cloned().expect("message nbt");
    let mut file = std::fs::File::open(path).expect("file");
    let subs =
        pst_reader::ndb::block::list_subnode_entries(&mut file, pst.bbt(), msg_entry.bid_sub)
            .expect("message subnodes");
    let recip = subs
        .iter()
        .find(|e| {
            matches!(
                e.nid.nid_type(),
                pst_reader::ndb::nid::NidType::RecipientTable
            )
        })
        .expect("recipient table subnode");
    let data = pst_reader::ndb::block::read_block_data(
        &mut file,
        pst.bbt(),
        recip.bid_data,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("recip heap");
    (data, recip.bid_sub)
}

#[test]
fn recipient_tc_writes_all_140_included_rows_and_keeps_display() {
    let path = scratch_path("recip_tc_all_rows");
    cleanup(&path);

    let mut recipients = Vec::new();
    for i in 0..40 {
        recipients.push(recip_row(WriteRecipientType::Bcc, "Bcc", i));
    }
    for i in 0..50 {
        recipients.push(recip_row(WriteRecipientType::Cc, "Cc", i));
    }
    for i in 0..50 {
        recipients.push(recip_row(WriteRecipientType::To, "To", i));
    }
    assert_eq!(recipients.len(), 140);

    let display_to = (0..50)
        .map(|i| format!("To{i} <to{i}@ex.com>"))
        .collect::<Vec<_>>()
        .join("; ");
    let display_cc = (0..50)
        .map(|i| format!("Cc{i} <cc{i}@ex.com>"))
        .collect::<Vec<_>>()
        .join("; ");
    let display_bcc = (0..40)
        .map(|i| format!("Bcc{i} <bcc{i}@ex.com>"))
        .collect::<Vec<_>>()
        .join("; ");

    let mut msg = base_msg("<recip-all@ex.com>", "Many recipients");
    msg.source_path = Some(r"C:\src\budget.pst".into());
    msg.source_msg_nid = Some(0x2044);
    msg.display_to = Some(display_to.clone());
    msg.display_cc = Some(display_cc.clone());
    msg.display_bcc = Some(display_bcc.clone());
    msg.recipients = recipients;

    let opts = WritePstOpts {
        include_bcc_recipients: true,
        ..WritePstOpts::default()
    };
    let report = write_unicode_pst(&path, vec![msg], &[], &opts).expect("write completes");
    assert_eq!(report.messages_written, 1);
    assert_eq!(report.recipient_tc_truncated_messages, 0);
    assert_eq!(report.recipient_rows_truncated, 0);
    assert!(report.recipient_tc_truncated_events.is_empty());

    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(nid).expect("list");
    assert_eq!(recips.len(), 140);
    let n_to = recips
        .iter()
        .filter(|r| r.recipient_type == pst_reader::RecipientType::To)
        .count();
    let n_cc = recips
        .iter()
        .filter(|r| r.recipient_type == pst_reader::RecipientType::Cc)
        .count();
    let n_bcc = recips
        .iter()
        .filter(|r| r.recipient_type == pst_reader::RecipientType::Bcc)
        .count();
    assert_eq!((n_to, n_cc, n_bcc), (50, 50, 40));
    // Continuation-page HN strings must round-trip (opt_row_string would
    // hide InvalidHid as None).
    assert_eq!(recips[0].display_name.as_deref(), Some("To0"));
    assert_eq!(recips[49].display_name.as_deref(), Some("To49"));
    assert_eq!(recips[139].display_name.as_deref(), Some("Bcc39"));
    // To→Cc→Bcc order preserved.
    assert!(recips[..50]
        .iter()
        .all(|r| r.recipient_type == pst_reader::RecipientType::To));
    assert!(recips[50..100]
        .iter()
        .all(|r| r.recipient_type == pst_reader::RecipientType::Cc));
    assert!(recips[100..]
        .iter()
        .all(|r| r.recipient_type == pst_reader::RecipientType::Bcc));

    let extract = pst.read_message_extract(nid).expect("extract");
    assert_eq!(extract.display_to.as_deref(), Some(display_to.as_str()));
    assert_eq!(extract.display_cc.as_deref(), Some(display_cc.as_str()));
    assert_eq!(extract.display_bcc.as_deref(), Some(display_bcc.as_str()));

    let (heap, bid_sub) = recipient_table_subnode(&path, nid);
    assert!(
        !bid_sub.is_null(),
        "non-empty recipient TC must have bid_sub"
    );
    assert!(
        heap.len() > 8176,
        "140-row TC must exercise multi-page HN (heap {} bytes)",
        heap.len()
    );
    let mut file = std::fs::File::open(&path).expect("file");
    let pst2 = pst_reader::PstFile::open(&path).expect("open2");
    let table = pst_reader::ltp::tc::load_from_table_bids(
        heap,
        &mut file,
        pst2.bbt(),
        bid_sub,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("load recip TC");
    assert_ne!(
        table.info().hnid_rows & 0x1F,
        0,
        "hnidRows must be a NID (nidType != 0), got 0x{:08X}",
        table.info().hnid_rows
    );
    assert_eq!(table.row_count(), 140);

    cleanup(&path);
}

/// Default BCC omit: BCC rows absent; To+Cc complete; DisplayBcc still written
/// only when include_bcc (this test uses default omit so DisplayBcc is omitted
/// from the PC — reader extract may be None). QC known_gap is a CLI test.
#[test]
fn recipient_tc_default_bcc_omit_writes_to_cc_only() {
    let path = scratch_path("recip_tc_bcc_omit");
    cleanup(&path);
    let mut recipients = Vec::new();
    for i in 0..5 {
        recipients.push(recip_row(WriteRecipientType::To, "To", i));
        recipients.push(recip_row(WriteRecipientType::Cc, "Cc", i));
        recipients.push(recip_row(WriteRecipientType::Bcc, "Bcc", i));
    }
    let mut msg = base_msg("<recip-bcc@ex.com>", "Bcc omit");
    msg.display_to = Some("to".into());
    msg.display_cc = Some("cc".into());
    msg.display_bcc = Some("bcc".into());
    msg.recipients = recipients;
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(nid).expect("list");
    assert_eq!(recips.len(), 10);
    assert!(recips
        .iter()
        .all(|r| r.recipient_type != pst_reader::RecipientType::Bcc));
    cleanup(&path);
}

/// >RowsPerBlock (live width 56 → Floor(8176/56)=146) so the matrix spans leaves.
#[test]
fn recipient_tc_matrix_spans_rows_per_block() {
    let path = scratch_path("recip_tc_span");
    cleanup(&path);
    const N: usize = 160;
    let mut recipients = Vec::with_capacity(N);
    for i in 0..N {
        recipients.push(recip_row(WriteRecipientType::To, "To", i));
    }
    let mut msg = base_msg("<recip-span@ex.com>", "Span matrix");
    msg.recipients = recipients;
    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.recipient_tc_truncated_messages, 0);
    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(nid).expect("list");
    assert_eq!(
        recips.len(),
        N,
        "reader must ignore dead space; exact count"
    );
    assert_eq!(recips[0].display_name.as_deref(), Some("To0"));
    assert_eq!(
        recips[145].display_name.as_deref(),
        Some("To145"),
        "last row of first matrix leaf"
    );
    assert_eq!(
        recips[146].display_name.as_deref(),
        Some("To146"),
        "first row of second matrix leaf"
    );
    assert_eq!(recips[159].display_name.as_deref(), Some("To159"));
    let (heap, bid_sub) = recipient_table_subnode(&path, nid);
    assert!(!bid_sub.is_null());
    let mut file = std::fs::File::open(&path).expect("file");
    let pst2 = pst_reader::PstFile::open(&path).expect("open2");
    let table = pst_reader::ltp::tc::load_from_table_bids(
        heap,
        &mut file,
        pst2.bbt(),
        bid_sub,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("load");
    assert_eq!(table.row_count(), N);
    cleanup(&path);
}

#[test]
fn recipient_tc_empty_hnid_rows_and_bid_sub_zero() {
    let path = scratch_path("recip_tc_empty");
    cleanup(&path);
    let msg = base_msg("<recip-empty@ex.com>", "No recips");
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(nid).expect("list");
    assert!(recips.is_empty());
    let (heap, bid_sub) = recipient_table_subnode(&path, nid);
    assert!(bid_sub.is_null(), "empty TC bid_sub must be 0");
    let table = pst_reader::ltp::tc::TableContext::load(heap).expect("load empty");
    assert_eq!(table.info().hnid_rows, 0);
    assert!(
        table.info().hid_row_index.is_null(),
        "empty TC hidRowIndex must be 0"
    );
    assert_eq!(table.row_count(), 0);
    cleanup(&path);
}

#[test]
fn recipient_tc_long_string_cell_nid_round_trips() {
    let path = scratch_path("recip_tc_cell_nid");
    cleanup(&path);
    // UTF-16 bytes = 2 * chars; MAX_HEAP_VALUE_SIZE = 2048 → 1025 chars diverts.
    let long = "N".repeat(1025);
    let mut msg = base_msg("<recip-longcell@ex.com>", "Long cell");
    msg.recipients = vec![WriteRecipient {
        recipient_type: WriteRecipientType::To,
        display_name: Some(long.clone()),
        address_type: Some("SMTP".into()),
        email_address: Some("long@ex.com".into()),
        smtp_address: Some("long@ex.com".into()),
    }];
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(nid).expect("list");
    assert_eq!(recips.len(), 1);
    assert_eq!(recips[0].display_name.as_deref(), Some(long.as_str()));

    // seven_bit mirrors display → 2 cell NIDs + matrix = 3 SLENTRYs, NID-ascending.
    let (heap, bid_sub) = recipient_table_subnode(&path, nid);
    assert!(!bid_sub.is_null(), "cell-NID TC must have bid_sub");
    let mut file = std::fs::File::open(&path).expect("file");
    let pst2 = pst_reader::PstFile::open(&path).expect("open2");
    let subs = pst_reader::ndb::block::list_subnode_entries(&mut file, pst2.bbt(), bid_sub)
        .expect("recip SLBLOCK");
    assert_eq!(subs.len(), 3, "display + seven_bit + matrix");
    assert!(
        subs.windows(2).all(|w| w[0].nid.0 < w[1].nid.0),
        "SLBLOCK NIDs must be strictly increasing: {:?}",
        subs.iter().map(|e| e.nid.0).collect::<Vec<_>>()
    );
    let table = pst_reader::ltp::tc::load_from_table_bids(
        heap,
        &mut file,
        pst2.bbt(),
        bid_sub,
        pst_reader::crypto::CryptMethod::None,
    )
    .expect("load recip TC");
    let hnid_rows = table.info().hnid_rows as u64;
    assert!(
        subs.iter().any(|e| e.nid.0 == hnid_rows),
        "hnidRows 0x{hnid_rows:X} must appear in SLBLOCK"
    );

    cleanup(&path);
}

/// Long display + long email (smtp short/None): three cell NIDs + matrix = 4.
#[test]
fn recipient_tc_two_cell_nids_slblock_sorted() {
    let path = scratch_path("recip_tc_two_cell");
    cleanup(&path);
    let long = "N".repeat(1025);
    let mut msg = base_msg("<recip-twocell@ex.com>", "Two long cells");
    msg.recipients = vec![WriteRecipient {
        recipient_type: WriteRecipientType::To,
        display_name: Some(long.clone()),
        address_type: Some("SMTP".into()),
        email_address: Some(long.clone()),
        smtp_address: None,
    }];
    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    let nid = first_message_nid(&path, "Unique Mail");
    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let recips = pst.list_recipients(nid).expect("list");
    assert_eq!(recips.len(), 1);
    assert_eq!(recips[0].display_name.as_deref(), Some(long.as_str()));
    assert_eq!(recips[0].email_address.as_deref(), Some(long.as_str()));

    let (_heap, bid_sub) = recipient_table_subnode(&path, nid);
    assert!(!bid_sub.is_null());
    let mut file = std::fs::File::open(&path).expect("file");
    let pst2 = pst_reader::PstFile::open(&path).expect("open2");
    let subs = pst_reader::ndb::block::list_subnode_entries(&mut file, pst2.bbt(), bid_sub)
        .expect("recip SLBLOCK");
    // display + seven_bit (mirrors display) + email + matrix
    assert_eq!(subs.len(), 4);
    assert!(
        subs.windows(2).all(|w| w[0].nid.0 < w[1].nid.0),
        "SLBLOCK NIDs must be strictly increasing: {:?}",
        subs.iter().map(|e| e.nid.0).collect::<Vec<_>>()
    );
    cleanup(&path);
}
