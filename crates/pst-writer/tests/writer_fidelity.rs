//! Production PST writer fidelity tests (track 0069) — matrix §9.
//!
//! Synthetic tempfile-only fixtures; round-trips verified via `pst-reader`.

use std::io::Read;
use std::path::{Path, PathBuf};

use pst_writer::{
    write_unicode_pst, write_unicode_pst_with_streams, AttachStreamSource, AttachmentFidelityKind,
    FolderLayoutPolicy, WriteAttachment, WriteMessage, WritePstOpts,
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

// ── 8: two sources different basenames ───────────────────────────────────────

#[test]
fn multi_source_distinct_basenames() {
    let path = scratch_path("multi_src_diff");
    cleanup(&path);

    let mut m1 = base_msg("<s8a@ex.com>", "From A");
    m1.source_path = Some(r"C:\data\alice.pst".into());
    m1.source_folder_path = Some("Inbox".into());

    let mut m2 = base_msg("<s8b@ex.com>", "From B");
    m2.source_path = Some(r"C:\data\bob.pst".into());
    m2.source_folder_path = Some("Inbox".into());

    let report =
        write_unicode_pst(&path, vec![m1, m2], &[], &WritePstOpts::default()).expect("write");
    assert!(report.folders_created >= 4); // alice, bob, Inbox×2

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    assert!(folders.iter().any(|f| f.name == "alice"));
    assert!(folders.iter().any(|f| f.name == "bob"));

    cleanup(&path);
}

// ── 9: two sources same basename ─────────────────────────────────────────────

#[test]
fn multi_source_same_basename_unique_prefixes() {
    let path = scratch_path("multi_src_same");
    cleanup(&path);

    let mut m1 = base_msg("<s9a@ex.com>", "Archive 1");
    m1.source_path = Some(r"C:\custodian1\archive.pst".into());
    m1.source_folder_path = Some("Inbox".into());

    let mut m2 = base_msg("<s9b@ex.com>", "Archive 2");
    m2.source_path = Some(r"C:\custodian2\archive.pst".into());
    m2.source_folder_path = Some("Inbox".into());

    write_unicode_pst(&path, vec![m1, m2], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let tops: Vec<_> = folders
        .iter()
        .filter(|f| f.path.matches('/').count() == 2 && f.name.starts_with("archive"))
        .map(|f| f.name.as_str())
        .collect();
    // Expect "archive" and "archive (2)"
    assert!(tops.contains(&"archive"), "tops={tops:?}");
    assert!(tops.iter().any(|n| n.contains("(2)")), "tops={tops:?}");
    // Messages must not share a single Inbox under one prefix only
    let inboxes: Vec<_> = folders.iter().filter(|f| f.name == "Inbox").collect();
    assert_eq!(inboxes.len(), 2);
    assert_eq!(inboxes[0].message_nids.len(), 1);
    assert_eq!(inboxes[1].message_nids.len(), 1);

    cleanup(&path);
}

/// Case-differing basenames must not merge under case-insensitive folder keys.
#[test]
fn multi_source_case_differing_stems_unique_prefixes() {
    let path = scratch_path("multi_src_case");
    cleanup(&path);

    let mut m1 = base_msg("<sc1@ex.com>", "A");
    m1.source_path = Some(r"C:\c1\Archive.pst".into());
    m1.source_folder_path = Some("Inbox".into());

    let mut m2 = base_msg("<sc2@ex.com>", "B");
    m2.source_path = Some(r"C:\c2\archive.pst".into());
    m2.source_folder_path = Some("Inbox".into());

    // Third source already named like a generated suffix must not collide.
    let mut m3 = base_msg("<sc3@ex.com>", "C");
    m3.source_path = Some(r"C:\c3\archive (2).pst".into());
    m3.source_folder_path = Some("Inbox".into());

    write_unicode_pst(&path, vec![m1, m2, m3], &[], &WritePstOpts::default()).expect("write");

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
    let inboxes: Vec<_> = folders.iter().filter(|f| f.name == "Inbox").collect();
    assert_eq!(inboxes.len(), 3);
    for ib in &inboxes {
        assert_eq!(ib.message_nids.len(), 1);
    }

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

    let nested = base_msg("<emb@ex.com>", "Nested subject");
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

    // Attach PC must not carry PidTagAttachDataBinary (by-value file path).
    // Nested content lives under the attach subnode leaf; reader
    // `open_attachment_data` may best-effort stream that subnode as bytes, but
    // that is not a by-value binary property we wrote.
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
    // Nested message object exists under the attach (subnode tree non-empty).
    // Method + size + subnode presence is the reader-honest surface for 0069;
    // PidTagAttachDataObject (PtypObject) remains residual.

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
            .any(|e| e.kind == AttachmentFidelityKind::DepthLimitExceeded),
        "per-attach depth_limit_exceeded event required; events={:?}",
        report.attachment_fidelity_events
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
    let table = pst_reader::ltp::tc::TableContext::load(raw, None).expect("TC load");
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

    let table_raw = pst
        .read_subnode_data(msg_nid, pst_reader::NodeId(0x671))
        .expect("message subnode 0x671 attachment table");
    let table = pst_reader::ltp::tc::TableContext::load(table_raw, None).expect("TC");
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
