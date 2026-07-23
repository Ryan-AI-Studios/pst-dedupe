//! Production Unicode PST writer v1 (track 0068) — synthetic round-trip tests.
//!
//! All fixtures are generated in-memory; nothing here depends on an external
//! operator path (unlike `tests/integration.rs`, kept as-is for the fixture
//! path). Round-trips are verified by opening the written file with
//! `pst_reader::PstFile` and reading back folders/messages/properties.

use std::path::{Path, PathBuf};

use pst_writer::{temp_sibling_path, write_unicode_pst, WriteMessage, WritePstOpts, WriterError};

/// Unique scratch path per test so parallel `cargo test` runs never collide.
fn scratch_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("pst_writer_v1_tests");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!(
        "{name}_{}_{}.pst",
        std::process::id(),
        name.len() * 2654435761usize.wrapping_add(name.as_ptr() as usize)
    ))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn short_message(mid: &str, subject: &str) -> WriteMessage {
    WriteMessage {
        message_id: Some(mid.to_string()),
        subject: subject.to_string(),
        sender: Some("alice@example.com".to_string()),
        display_to: Some("bob@example.com".to_string()),
        submit_time: Some(0x01D5B035EDA780_i64),
        body_plain: Some("Hello, world!".to_string()),
        body_html: None,
        message_class: None,
        body_incomplete: false,
        body_unavailable: false,
    }
}

// ── Test 1: empty message list ───────────────────────────────────────────────

#[test]
fn empty_message_list_produces_valid_openable_pst() {
    let path = scratch_path("empty");
    cleanup(&path);

    let report =
        write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.messages_written, 0);
    assert_eq!(report.messages_skipped, 0);
    assert!(report.bytes > 0);

    let mut pst = pst_reader::PstFile::open(&path).expect("open written PST");
    let folders = pst.folders().expect("folders");
    let total_messages: u32 = folders.iter().map(|f| f.message_count).sum();
    assert_eq!(total_messages, 0);

    cleanup(&path);
}

// ── Test 2: one short message ────────────────────────────────────────────────

#[test]
fn one_short_message_round_trips_props() {
    let path = scratch_path("one_short");
    cleanup(&path);

    let msg = short_message("<abc123@example.com>", "Hello subject");
    let report = write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.messages_written, 1);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("Unique Mail folder present");
    assert_eq!(unique.message_count, 1);

    let nid = unique.message_nids[0];
    let extracted = pst.read_message_extract(nid).expect("extract");
    assert_eq!(extracted.subject.as_deref(), Some("Hello subject"));
    assert_eq!(extracted.sender_email.as_deref(), Some("alice@example.com"));
    assert_eq!(
        extracted.message_id.as_deref(),
        Some("<abc123@example.com>")
    );
    assert_eq!(extracted.body_text.as_deref(), Some("Hello, world!"));
    assert_eq!(extracted.has_attachments, Some(false));

    cleanup(&path);
}

// ── Test 3 / 13: body > 8KB forces XBLOCK; no silent 2000-char truncate ─────

#[test]
fn body_over_8kb_round_trips_full_length_no_truncate() {
    let path = scratch_path("body_8kb");
    cleanup(&path);

    let long_body: String = "The quick brown fox jumps over the lazy dog. ".repeat(300); // ~13,800 chars
    assert!(long_body.len() > 8176, "fixture must exceed one block");
    assert!(
        long_body.len() > 2000,
        "fixture must exceed the old fixture truncate too"
    );

    let mut msg = short_message("<big1@example.com>", "Big body");
    msg.body_plain = Some(long_body.clone());

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];
    let extracted = pst.read_message_extract(nid).expect("extract");
    let got = extracted.body_text.expect("body present");
    assert_eq!(got.chars().count(), long_body.chars().count());
    assert_eq!(got, long_body, "full body must round-trip byte-for-byte");
    assert_ne!(
        got.chars().count(),
        2000,
        "must not silently truncate to the old fixture limit"
    );

    cleanup(&path);
}

// ── Test 4: body > 100KB round trips full length ────────────────────────────

#[test]
fn body_over_100kb_round_trips_full_length() {
    let path = scratch_path("body_100kb");
    cleanup(&path);

    let long_body: String = "0123456789".repeat(15_000); // 150,000 chars
    assert!(long_body.len() > 100_000);

    let mut msg = short_message("<big2@example.com>", "Very big body");
    msg.body_plain = Some(long_body.clone());

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];
    let extracted = pst.read_message_extract(nid).expect("extract");
    let got = extracted.body_text.expect("body present");
    assert_eq!(got.len(), long_body.len());
    assert_eq!(got, long_body);

    cleanup(&path);
}

// ── XXBLOCK: body large enough to exceed one XBLOCK's child-BID capacity ───

#[test]
fn body_forcing_xxblock_round_trips_full_length() {
    let path = scratch_path("body_xxblock");
    cleanup(&path);

    // One XBLOCK holds at most (8176-8)/8 = 1021 external blocks of 8176 bytes
    // each (~8,347,696 bytes). Exceed that in UTF-16 byte terms to force an
    // XXBLOCK: use enough ASCII chars that 2 bytes/char pushes us over.
    let char_count = 4_200_000usize; // ~8.4M UTF-16 bytes
    let long_body: String = "x".repeat(char_count);

    let mut msg = short_message("<xx1@example.com>", "XXBLOCK body");
    msg.body_plain = Some(long_body.clone());

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];
    let extracted = pst.read_message_extract(nid).expect("extract");
    let got = extracted.body_text.expect("body present");
    assert_eq!(got.len(), long_body.len());
    assert_eq!(got, long_body);

    cleanup(&path);
}

// ── Test 5: HTML + plain → NativeBody = HTML ────────────────────────────────

#[test]
fn html_and_plain_message_sets_native_body_html() {
    let path = scratch_path("html_plain");
    cleanup(&path);

    let mut msg = short_message("<html1@example.com>", "HTML message");
    msg.body_plain = Some("plain fallback".to_string());
    msg.body_html = Some(b"<html><body>Hi <b>there</b></body></html>".to_vec());

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];
    let extracted = pst.read_message_extract(nid).expect("extract");

    let html = extracted.body_html.expect("html body present");
    assert_eq!(html, b"<html><body>Hi <b>there</b></body></html>".to_vec());

    // NativeBody / editor format aren't exposed by ExtractedMessage — read the
    // raw PC directly (public API: `read_node_data` + `PropContext::load`).
    let raw = pst.read_node_data(nid).expect("raw node data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("pc");
    let native_body = pc
        .get_i32(0x1016)
        .expect("get")
        .expect("native body present");
    assert_eq!(
        native_body, 3,
        "NativeBody must be HTML (3) when HTML is written"
    );

    cleanup(&path);
}

// ── Test 6: plain-only → NativeBody = Plain, no stale HTML ──────────────────

#[test]
fn plain_only_message_sets_native_body_plain() {
    let path = scratch_path("plain_only");
    cleanup(&path);

    let mut msg = short_message("<plain1@example.com>", "Plain message");
    msg.body_html = None;

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];
    let extracted = pst.read_message_extract(nid).expect("extract");
    assert!(
        extracted.body_html.is_none(),
        "no stale HTML when none was written"
    );

    let raw = pst.read_node_data(nid).expect("raw node data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("pc");
    let native_body = pc
        .get_i32(0x1016)
        .expect("get")
        .expect("native body present");
    assert_eq!(
        native_body, 1,
        "NativeBody must be Plain (1) when only plain is written"
    );

    cleanup(&path);
}

// ── Test 7: computed MessageSize ignores an inflated source size ───────────

#[test]
fn message_size_is_computed_not_copied_from_inflated_source() {
    use dedup_engine::integrity::RecoverableIntegrity;
    use dedup_engine::keepset::{CanonicalMessage, MessageLocus};

    let path = scratch_path("size_computed");
    cleanup(&path);

    let canonical = CanonicalMessage {
        locus: MessageLocus {
            source_path: "C:/fake/source.pst".to_string(),
            source_pst: "source.pst".to_string(),
            folder_path: "Inbox".to_string(),
            nid: 1,
            is_orphaned: false,
        },
        message_id: Some("<size1@example.com>".to_string()),
        subject: Some("Small body, big declared size".to_string()),
        sender: Some("alice@example.com".to_string()),
        display_to: None,
        display_cc: None,
        display_bcc: None,
        submit_time: None,
        // Fake, hugely inflated declared size (e.g. included attachments the
        // writer never sees / never writes).
        size: Some(50_000_000),
        message_class: None,
        body_plain: Some("tiny body".to_string()),
        body_html: None,
        attachments: Vec::new(),
        fidelity: RecoverableIntegrity::clean(),
        message_id_norm: None,
        content_hash: [0u8; 32],
        edrm_mih_hex: None,
        body_incomplete: false,
        body_unavailable: false,
    };

    let (write_msg, dropped) = pst_writer::from_canonical_message(&canonical);
    assert_eq!(dropped, 0);

    write_unicode_pst(&path, vec![write_msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];
    let props = pst.read_message_properties(nid).expect("props");
    let stored_size = props.message_size.expect("message size present");

    assert!(
        (stored_size as i64) < 1_000,
        "stored size ({stored_size}) must be small (order of the tiny actual body), \
         not the fake 50,000,000-byte declared source size"
    );

    cleanup(&path);
}

// ── Test 8: hierarchy — IPM_SUBTREE present, Unique Mail is its child ───────

#[test]
fn hierarchy_places_unique_mail_under_ipm_subtree_with_store_entryid() {
    let path = scratch_path("hierarchy");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");

    let root = folders
        .iter()
        .find(|f| f.path == "Root")
        .expect("root folder");
    assert_eq!(
        root.child_folder_nids.len(),
        1,
        "root's only child must be IPM_SUBTREE, not the mail folder directly"
    );

    let ipm_subtree = folders
        .iter()
        .find(|f| f.name == "Top of Personal Folders")
        .expect("IPM_SUBTREE (\"Top of Personal Folders\") folder present");
    assert_eq!(ipm_subtree.path, "Root/Top of Personal Folders");
    assert_eq!(
        ipm_subtree.child_folder_nids.len(),
        2,
        "IPM_SUBTREE's children must be Unique Mail and Deleted Items"
    );

    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("Unique Mail folder present");
    assert_eq!(unique.path, "Root/Top of Personal Folders/Unique Mail");
    assert!(
        ipm_subtree.child_folder_nids.contains(&unique.nid),
        "Unique Mail must be a child of IPM_SUBTREE"
    );

    // Store PidTagIpmSubtreeEntryId (0x35E0): 24 bytes, embeds IPM_SUBTREE's NID.
    let raw = pst
        .read_node_data(pst_reader::NodeId(0x21))
        .expect("store raw data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("store pc");
    let entry_id = pc
        .get_binary(0x35E0)
        .expect("get")
        .expect("IpmSubtreeEntryId present");
    assert_eq!(entry_id.len(), 24);
    let embedded_nid = u32::from_le_bytes([entry_id[20], entry_id[21], entry_id[22], entry_id[23]]);
    assert_eq!(embedded_nid as u64, ipm_subtree.nid.0);

    cleanup(&path);
}

// ── Test 8b: Unique Mail folder carries PidTagContainerClass = IPF.Note ─────

#[test]
fn unique_mail_folder_has_ipf_note_container_class() {
    let path = scratch_path("container_class");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");

    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("Unique Mail folder present");

    // PidTagContainerClass (0x3613): spec.md §3.2's LOCKED hierarchy table
    // requires "standard display name / container class for IPM subtree per
    // MS-PST messaging conventions". Real-world Unicode PSTs leave the
    // IPM_SUBTREE node's own container class absent and instead set
    // `PidTagContainerClass = "IPF.Note"` on the actual mail-containing
    // folder beneath it — see the comments above the IPM_SUBTREE and
    // "Unique Mail" PC builds in `production.rs` and
    // `docs/pst-writer-fidelity-v1.md` for the full reasoning.
    let raw = pst
        .read_node_data(unique.nid)
        .expect("Unique Mail raw node data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("Unique Mail pc");
    let container_class = pc
        .get_string(0x3613)
        .expect("get")
        .expect("PidTagContainerClass present on Unique Mail");
    assert_eq!(container_class, "IPF.Note");

    // And confirm IPM_SUBTREE itself does NOT carry a container class value —
    // the decision documented above is that only the leaf mail folder does.
    let ipm_subtree = folders
        .iter()
        .find(|f| f.name == "Top of Personal Folders")
        .expect("IPM_SUBTREE (\"Top of Personal Folders\") folder present");
    let ipm_raw = pst
        .read_node_data(ipm_subtree.nid)
        .expect("IPM_SUBTREE raw node data");
    let ipm_pc = pst_reader::ltp::pc::PropContext::load(ipm_raw).expect("IPM_SUBTREE pc");
    assert_eq!(
        ipm_pc.get_string(0x3613).expect("get"),
        None,
        "IPM_SUBTREE itself should not carry PidTagContainerClass in v1"
    );

    cleanup(&path);
}

// ── Track 0068 round 9 (verified MS-PST source data): IPM_SUBTREE required
// initialization, Deleted Items / Search Root folder objects, store
// PidTagIpmWastebasketEntryId/PidTagFinderEntryId, and the four fixed
// "template object" tables. Supersedes the prior D-0068-05 decline and the
// round-6 template-objects decline note — see docs/pst-writer-fidelity-v1.md.

// ── New test 1: IPM_SUBTREE required schema-property initialization ────────

#[test]
fn ipm_subtree_has_required_top_of_personal_folders_initialization() {
    let path = scratch_path("ipm_subtree_init");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let ipm_subtree = folders
        .iter()
        .find(|f| f.name == "Top of Personal Folders")
        .expect("IPM_SUBTREE (\"Top of Personal Folders\") folder present");

    let raw = pst
        .read_node_data(ipm_subtree.nid)
        .expect("IPM_SUBTREE raw node data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("IPM_SUBTREE pc");

    let display_name = pc
        .get_string(0x3001)
        .expect("get")
        .expect("PidTagDisplayName present");
    assert_eq!(
        display_name, "Top of Personal Folders",
        "PidTagDisplayName must be the MS-PST-required value, not the prior \
         literal-string-bug value \"IPM_SUBTREE\""
    );

    let content_count = pc
        .get_i32(0x3602)
        .expect("get")
        .expect("PidTagContentCount present");
    assert_eq!(content_count, 1, "PidTagContentCount must be 1 (verified)");

    let content_unread_count = pc.get_i32(0x3603).expect("get");
    assert_eq!(
        content_unread_count,
        Some(0),
        "PidTagContentUnreadCount must be present and 0 (verified)"
    );

    let subfolders = pc.get_bool(0x360A).expect("get");
    assert_eq!(
        subfolders,
        Some(true),
        "PidTagSubfolders must be present and true (verified)"
    );

    cleanup(&path);
}

// ── New test 2: IPM_SUBTREE's hierarchy TC resolves both children by name ──

#[test]
fn ipm_subtree_hierarchy_resolves_unique_mail_and_deleted_items_by_name() {
    let path = scratch_path("ipm_subtree_children_by_name");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");

    let ipm_subtree = folders
        .iter()
        .find(|f| f.name == "Top of Personal Folders")
        .expect("IPM_SUBTREE folder present");

    let child_names: std::collections::HashSet<&str> = ipm_subtree
        .child_folder_nids
        .iter()
        .filter_map(|nid| folders.iter().find(|f| f.nid == *nid))
        .map(|f| f.name.as_str())
        .collect();

    assert_eq!(
        child_names,
        std::collections::HashSet::from(["Unique Mail", "Deleted Items"]),
        "IPM_SUBTREE's hierarchy table must resolve to exactly \"Unique Mail\" \
         and \"Deleted Items\" by name, got: {child_names:?}"
    );

    // Deleted Items itself must be empty (v1 never invents deleted-items
    // content) and must not be the folder any message was written under.
    let deleted_items = folders
        .iter()
        .find(|f| f.name == "Deleted Items")
        .expect("Deleted Items folder present");
    assert_eq!(deleted_items.message_count, 0);
    assert_eq!(
        deleted_items.path,
        "Root/Top of Personal Folders/Deleted Items"
    );

    cleanup(&path);
}

// ── New test 3: store PidTagIpmWastebasketEntryId / PidTagFinderEntryId ─────

#[test]
fn store_has_wastebasket_and_finder_entry_ids_matching_real_folder_nids() {
    let path = scratch_path("wastebasket_finder_entryid");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");

    // Deleted Items' real NID comes from the folder hierarchy (independent of
    // the EntryID we're about to decode).
    let folders = pst.folders().expect("folders");
    let deleted_items = folders
        .iter()
        .find(|f| f.name == "Deleted Items")
        .expect("Deleted Items folder present");

    // Search Root is NOT part of the IPM_SUBTREE hierarchy tree (it is
    // referenced only via PidTagFinderEntryId, per the verified source data),
    // so it cannot be found via `folders()`. Its real NID is instead read
    // independently from the raw NBT — a genuine cross-check, not decoding
    // the EntryID we're about to verify against it.
    let search_root_nid = pst
        .nbt()
        .iter()
        .map(|(raw_nid, _)| pst_reader::NodeId(*raw_nid))
        .find(|nid| nid.nid_type() == pst_reader::ndb::nid::NidType::SearchFolder)
        .expect("a NID_TYPE_SEARCH_FOLDER (0x03) node must exist (Search Root)");

    let raw = pst
        .read_node_data(pst_reader::NodeId(0x21))
        .expect("store raw data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("store pc");

    let wastebasket_entry_id = pc
        .get_binary(0x35E3)
        .expect("get")
        .expect("PidTagIpmWastebasketEntryId present");
    assert_eq!(wastebasket_entry_id.len(), 24);
    assert_ne!(wastebasket_entry_id, vec![0u8; 24], "must not be all-zero");
    let wastebasket_nid = u32::from_le_bytes([
        wastebasket_entry_id[20],
        wastebasket_entry_id[21],
        wastebasket_entry_id[22],
        wastebasket_entry_id[23],
    ]);
    assert_eq!(
        wastebasket_nid as u64, deleted_items.nid.0,
        "PidTagIpmWastebasketEntryId's embedded NID must match Deleted Items' actual NID"
    );

    let finder_entry_id = pc
        .get_binary(0x35E7)
        .expect("get")
        .expect("PidTagFinderEntryId present");
    assert_eq!(finder_entry_id.len(), 24);
    assert_ne!(finder_entry_id, vec![0u8; 24], "must not be all-zero");
    let finder_nid = u32::from_le_bytes([
        finder_entry_id[20],
        finder_entry_id[21],
        finder_entry_id[22],
        finder_entry_id[23],
    ]);
    assert_eq!(
        finder_nid as u64, search_root_nid.0,
        "PidTagFinderEntryId's embedded NID must match Search Root's actual NID"
    );

    cleanup(&path);
}

// ── New test 4: four fixed MS-PST template-object tables are present and
// readable as valid, empty TCs ───────────────────────────────────────────

#[test]
fn fixed_template_object_tables_are_present_and_empty() {
    let path = scratch_path("template_objects");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");

    // (NID, expected column count) — column counts cross-check the verified
    // source data's column lists (§5a-5d), duplicate 0x0E07/0x0E17 in
    // Microsoft's own published Search Folder Contents Table Template page
    // collapsed to one column each, per that table's own documented note.
    let templates: [(u64, usize); 4] = [
        (0x60D, 13), // Hierarchy Table Template
        (0x60E, 27), // Contents Table Template
        (0x60F, 14), // FAI Contents Table Template
        (0x610, 18), // Search Folder Contents Table Template
    ];

    for (nid, expected_cols) in templates {
        let raw = pst
            .read_node_data(pst_reader::NodeId(nid))
            .unwrap_or_else(|e| panic!("template object 0x{nid:X} must be readable, got: {e}"));
        let table = pst_reader::ltp::tc::TableContext::load(raw, None).unwrap_or_else(|e| {
            panic!("template object 0x{nid:X} must parse as a valid TC, got: {e}")
        });
        assert_eq!(
            table.row_count(),
            0,
            "template object 0x{nid:X} must have zero data rows (verified requirement)"
        );
        assert_eq!(
            table.columns().len(),
            expected_cols,
            "template object 0x{nid:X} must have {expected_cols} columns per the verified source data"
        );
    }

    cleanup(&path);
}

// ── Associated-contents (FAI) table present for all three folders ──────────
//
// Codex round-6 P1 finding, Item 2: per MS-PST §2.4.2 a complete Folder object
// is PC + hierarchy TC + contents TC + associated-contents TC, even when the
// latter is empty. This does not create any new folder objects (declined
// separately — see docs/pst-writer-fidelity-v1.md) — it only completes the
// definition of the three folders (Root, IPM_SUBTREE, Unique Mail) this
// writer already creates. NID suffix 0x0F is `pst_reader`'s own canonical
// `NodeId::associated_contents_table()` derivation, used directly below
// rather than re-deriving the bit math by hand.

#[test]
fn all_three_folders_have_readable_empty_associated_contents_table() {
    let path = scratch_path("assoc_contents");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");

    let root = folders
        .iter()
        .find(|f| f.path == "Root")
        .expect("root folder");
    let ipm_subtree = folders
        .iter()
        .find(|f| f.name == "Top of Personal Folders")
        .expect("IPM_SUBTREE (\"Top of Personal Folders\") folder");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("Unique Mail folder");

    for folder in [root, ipm_subtree, unique] {
        let assoc_nid = folder.nid.associated_contents_table();
        let raw = pst
            .read_node_data(assoc_nid)
            .unwrap_or_else(|e| panic!("associated-contents table for {} (nid 0x{:X}) must be readable via pst-reader, got: {e}", folder.name, assoc_nid.0));
        let table = pst_reader::ltp::tc::TableContext::load(raw, None).unwrap_or_else(|e| {
            panic!(
                "associated-contents table for {} must parse as a valid TC, got: {e}",
                folder.name
            )
        });
        assert_eq!(
            table.row_count(),
            0,
            "associated-contents table for {} must be empty in v1 (no FAI items written)",
            folder.name
        );
    }

    cleanup(&path);
}

// ── Test 9: missing MID still written with subject ──────────────────────────

#[test]
fn missing_message_id_still_writes_subject() {
    let path = scratch_path("missing_mid");
    cleanup(&path);

    let mut msg = short_message("<unused@example.com>", "Subject survives");
    msg.message_id = None;

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    assert_eq!(unique.message_count, 1);
    let nid = unique.message_nids[0];
    let props = pst.read_message_properties(nid).expect("props");
    assert!(props.message_id.is_none());
    assert_eq!(props.subject.as_deref(), Some("Subject survives"));

    cleanup(&path);
}

// ── Test 10: body_unavailable → no invented content ─────────────────────────

#[test]
fn body_unavailable_writes_no_invented_body() {
    let path = scratch_path("body_unavailable");
    cleanup(&path);

    let mut msg = short_message("<unavail1@example.com>", "No body here");
    msg.body_plain = Some("this must never appear".to_string());
    msg.body_html = Some(b"<p>nor this</p>".to_vec());
    msg.body_unavailable = true;

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];
    let extracted = pst.read_message_extract(nid).expect("extract");
    assert!(
        extracted.body_text.is_none() || extracted.body_text.as_deref() == Some(""),
        "body_unavailable must never invent body content"
    );
    assert!(extracted.body_html.is_none());

    cleanup(&path);
}

// ── Test 11: N=50 synthetic messages ─────────────────────────────────────────

#[test]
fn fifty_synthetic_messages_round_trip() {
    let path = scratch_path("fifty");
    cleanup(&path);

    let messages: Vec<WriteMessage> = (0..50)
        .map(|i| short_message(&format!("<msg{i}@example.com>"), &format!("Subject {i}")))
        .collect();

    let report = write_unicode_pst(&path, messages, &[], &WritePstOpts::default()).expect("write");
    assert_eq!(report.messages_written, 50);

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    assert_eq!(unique.message_count, 50);
    assert_eq!(unique.message_nids.len(), 50);

    let mut seen_mids = std::collections::HashSet::new();
    for &nid in &unique.message_nids {
        let props = pst.read_message_properties(nid).expect("props");
        let mid = props.message_id.expect("mid present");
        assert!(mid.starts_with("<msg") && mid.ends_with("@example.com>"));
        seen_mids.insert(mid);
    }
    assert_eq!(
        seen_mids.len(),
        50,
        "all 50 MIDs must be distinct and readable"
    );

    cleanup(&path);
}

// ── Test 12: refuse to overwrite an existing destination ───────────────────

#[test]
fn refuses_to_overwrite_existing_destination_by_default() {
    let path = scratch_path("refuse_overwrite");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("first write");

    let second = write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default());
    assert!(
        matches!(second, Err(WriterError::Refused(_))),
        "must refuse to silently overwrite an existing destination"
    );

    // Explicit opt-in succeeds.
    let opts = WritePstOpts {
        overwrite: true,
        ..WritePstOpts::default()
    };
    write_unicode_pst(&path, Vec::new(), &[], &opts).expect("overwrite with explicit opt-in");

    cleanup(&path);
}

// ── Protected source paths: refused even with overwrite: true ──────────────

#[test]
fn refuses_protected_source_path_even_with_overwrite_true() {
    let path = scratch_path("protected_source");
    cleanup(&path);

    // Simulate a caller (e.g. a future 0069/0071 CLI) that knows this path is
    // one of the dedupe *input* PSTs, and also (incorrectly, or via a bad glue
    // script) passes `overwrite: true`. The source-overwrite refusal must be
    // unconditional — `overwrite` must never bypass it (spec §3.7 rule 2 /
    // Core Mandate #3: never mutate PST inputs).
    let opts = WritePstOpts {
        overwrite: true,
        ..WritePstOpts::default()
    };
    let protected_source_paths = vec![path.clone()];

    let result = write_unicode_pst(&path, Vec::new(), &protected_source_paths, &opts);
    assert!(
        matches!(result, Err(WriterError::RefusedSourceOverwrite(_))),
        "writing to a protected source path must be refused even when overwrite is true, got: {result:?}"
    );

    // The file must not have been created/mutated by the refused call.
    assert!(
        !path.exists(),
        "refused write must not touch the destination at all"
    );

    cleanup(&path);
}

#[test]
fn protected_source_check_is_independent_of_overwrite_default() {
    let path = scratch_path("protected_source_default");
    cleanup(&path);

    // Same as above but with the default `overwrite: false`, to prove the
    // protected-source check is a distinct, earlier gate than the generic
    // "destination exists" refusal — not just a side effect of it.
    let opts = WritePstOpts::default();
    let protected_source_paths = vec![path.clone()];

    let result = write_unicode_pst(&path, Vec::new(), &protected_source_paths, &opts);
    assert!(
        matches!(result, Err(WriterError::RefusedSourceOverwrite(_))),
        "expected RefusedSourceOverwrite, got: {result:?}"
    );

    cleanup(&path);
}

// ── Protected source paths: the temp-staging sibling is checked too, not
// just the final destination (P2 fix, review round 8) ──────────────────────

#[test]
fn refuses_when_temp_staging_path_is_a_protected_source() {
    let path = scratch_path("temp_staging_protected");
    cleanup(&path);

    // `write_unicode_pst` first writes the whole file to a computed
    // `.tmp-<pid>-<entropy>` sibling of `path`, then renames it over `path`
    // only on success. Before this fix, `protected_source_paths` was only
    // ever compared against the final `path`, never against that temp
    // sibling — so a protected source that happened to collide with the
    // *temp* name would get silently truncated by `File::create` during
    // staging, bypassing the safety mechanism entirely. Reproduce that exact
    // collision by calling `pst_writer::temp_sibling_path` directly (the
    // same function `write_unicode_pst` calls internally, exported for
    // exactly this purpose) rather than guessing the naming scheme, so this
    // test can never silently drift from the real implementation.
    let tmp_path = temp_sibling_path(&path);
    cleanup(&tmp_path);

    let protected_source_paths = vec![tmp_path.clone()];
    let opts = WritePstOpts::default();

    let result = write_unicode_pst(&path, Vec::new(), &protected_source_paths, &opts);
    assert!(
        matches!(result, Err(WriterError::RefusedSourceOverwrite(_))),
        "must refuse when the temp-staging path collides with a protected source path, got: {result:?}"
    );

    // The refusal must happen BEFORE `File::create(&tmp_path)` — i.e. no
    // bytes ever get written to the protected temp path, and the (unrelated)
    // final destination is untouched either.
    assert!(
        !tmp_path.exists(),
        "refused write must not create the temp-staging path at all"
    );
    assert!(
        !path.exists(),
        "refused write must not touch the destination at all"
    );

    cleanup(&path);
    cleanup(&tmp_path);
}

// ── Test 14: raw header bytes — bSentinel/bCryptMethod/bidNextB at the real
// MS-PST offsets (0x200/0x201/0x204), matching `pst_reader::header` exactly.
// Regression test for the P1 review finding: `write_header_v1` used to pad the
// ROOT trailer to 8 bytes (instead of 4) and rgbFM/rgbFP to 508 bytes (instead
// of 256), shifting every field after that by ~256 bytes. `PstFile::open` is
// too lenient to catch this (it never validates bSentinel and the shifted
// bCryptMethod byte happened to still land on a zero), so this test reads the
// raw file bytes directly instead of going through the reader's structs.
#[test]
fn header_unicode_fields_land_at_correct_raw_offsets() {
    use std::io::Read;

    let path = scratch_path("header_raw_offsets");
    cleanup(&path);

    let messages: Vec<WriteMessage> = (0..5)
        .map(|i| {
            short_message(
                &format!("<hdr{i}@example.com>", i = i),
                &format!("Subject {i}"),
            )
        })
        .collect();
    write_unicode_pst(&path, messages, &[], &WritePstOpts::default()).expect("write");

    let mut file = std::fs::File::open(&path).expect("open raw file");
    let mut header = vec![0u8; 0x200 + 8 + 8];
    file.read_exact(&mut header).expect("read header bytes");

    let b_sentinel = header[0x200];
    assert_eq!(
        b_sentinel, 0x80,
        "bSentinel must be 0x80 at the real MS-PST offset 0x200 \
         (header byte layout regressed to the pre-fix 256-byte-shifted padding)"
    );

    let b_crypt_method = header[0x201];
    assert_eq!(
        b_crypt_method, 0,
        "bCryptMethod must be 0 (none) at offset 0x201"
    );

    let bid_next_b_bytes: [u8; 8] = header[0x204..0x20C].try_into().expect("8 bytes");
    let bid_next_b = u64::from_le_bytes(bid_next_b_bytes);
    assert!(
        bid_next_b > 0x10,
        "bidNextB at offset 0x204 must be a sane, non-zero next-BID counter \
         (got {bid_next_b:#x}); a zero/bogus value here indicates the ROOT \
         trailer / rgbFM+rgbFP padding is still misaligned"
    );

    // Cross-check against the reader's own (already-correct) header parse —
    // this is a secondary confirmation, not a substitute for the raw check
    // above, since `PstHeader::read` doesn't validate the sentinel value.
    let mut file2 = std::fs::File::open(&path).expect("reopen");
    let parsed = pst_reader::header::PstHeader::read(&mut file2).expect("parse header");
    assert_eq!(parsed.bid_next_b, bid_next_b);

    cleanup(&path);
}

// ── Test 15: PidTagMessageSize does not double-count an inline (non-subnode)
// body — regression test for the P2 review finding where `written_content_bytes`
// was added unconditionally even when the body was written inline in the PC
// heap (already captured by `probe_bytes`), inflating the stored size by
// roughly one extra copy of the body.
#[test]
fn message_size_does_not_double_count_inline_body() {
    let path = scratch_path("size_inline_no_double_count");
    cleanup(&path);

    // 1000 ASCII chars => exactly 2000 UTF-16LE bytes, well under
    // MAX_HEAP_VALUE_SIZE (3580) so this body is written inline, not diverted
    // to a subnode.
    let body: String = "A".repeat(1000);
    let mut msg = short_message("<inline_size@example.com>", "Inline size check");
    msg.body_plain = Some(body);
    msg.body_html = None;

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];
    let props = pst.read_message_properties(nid).expect("props");
    let stored_size = props.message_size.expect("message size present") as i64;

    // The inline UTF-16LE body alone is 2000 bytes; the whole PC heap should
    // be a modest amount larger than that (other short props + BTH/heap
    // overhead), but nowhere near double. Under the old double-counting bug
    // this would be >= 2000 (body, once inside probe_bytes) + 2000
    // (body, again via written_content_bytes) = 4000+ plus overhead, so a
    // tight upper bound of 3000 fails under the bug and passes after the fix.
    assert!(
        stored_size > 1900,
        "stored size ({stored_size}) is suspiciously small for a 2000-byte inline body"
    );
    assert!(
        stored_size < 3000,
        "stored size ({stored_size}) indicates the inline body's bytes were \
         counted twice (once in the PC heap probe, once via written_content_bytes) \
         — MAX_HEAP_VALUE_SIZE-bounded inline bodies must not be added again on \
         top of the heap probe"
    );

    cleanup(&path);
}

// ── Test 16: WritePstReport surfaces body_incomplete/body_unavailable counts ─
#[test]
fn report_counts_incomplete_and_unavailable_bodies() {
    let path = scratch_path("report_body_fidelity_counts");
    cleanup(&path);

    let mut normal = short_message("<normal@example.com>", "Normal");

    let mut incomplete_a = short_message("<incomplete_a@example.com>", "Incomplete A");
    incomplete_a.body_incomplete = true;

    let mut incomplete_b = short_message("<incomplete_b@example.com>", "Incomplete B");
    incomplete_b.body_incomplete = true;

    let mut unavailable = short_message("<unavailable@example.com>", "Unavailable");
    unavailable.body_unavailable = true;

    // A message flagged both ways at once must still be counted in both
    // buckets independently (additive counters, not mutually exclusive).
    let mut both = short_message("<both@example.com>", "Both flags");
    both.body_incomplete = true;
    both.body_unavailable = true;

    normal.body_incomplete = false;
    normal.body_unavailable = false;

    let report = write_unicode_pst(
        &path,
        vec![normal, incomplete_a, incomplete_b, unavailable, both],
        &[],
        &WritePstOpts::default(),
    )
    .expect("write");

    assert_eq!(report.messages_written, 5);
    assert_eq!(
        report.messages_with_incomplete_body, 3,
        "incomplete_a, incomplete_b, and both must all be counted"
    );
    assert_eq!(
        report.messages_with_unavailable_body, 2,
        "unavailable and both must all be counted"
    );

    cleanup(&path);
}

// ── MessageFlags / CreationTime / LastModificationTime (codex review fold) ──

#[test]
fn message_with_submit_time_gets_flags_and_creation_modification_times() {
    let path = scratch_path("flags_times_present");
    cleanup(&path);

    let msg = short_message("<flags1@example.com>", "Has submit time");
    let submit_time = msg.submit_time.expect("fixture sets submit_time");

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];

    let raw = pst.read_node_data(nid).expect("raw node data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("pc");

    let flags = pc
        .get_i32(0x0E07)
        .expect("get")
        .expect("PidTagMessageFlags present");
    assert_eq!(flags, 1, "PidTagMessageFlags must be MSGFLAG_READ (1)");

    let creation_time = pc
        .get_time(0x3007)
        .expect("get")
        .expect("PidTagCreationTime present");
    assert_eq!(
        creation_time, submit_time,
        "PidTagCreationTime must use submit_time as a stand-in"
    );

    let modification_time = pc
        .get_time(0x3008)
        .expect("get")
        .expect("PidTagLastModificationTime present");
    assert_eq!(
        modification_time, submit_time,
        "PidTagLastModificationTime must use submit_time as a stand-in"
    );

    cleanup(&path);
}

#[test]
fn message_without_submit_time_omits_creation_and_modification_time() {
    let path = scratch_path("flags_times_absent");
    cleanup(&path);

    let mut msg = short_message("<flags2@example.com>", "No submit time");
    msg.submit_time = None;

    write_unicode_pst(&path, vec![msg], &[], &WritePstOpts::default()).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    let unique = folders
        .iter()
        .find(|f| f.name == "Unique Mail")
        .expect("folder");
    let nid = unique.message_nids[0];

    let raw = pst.read_node_data(nid).expect("raw node data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("pc");

    let flags = pc
        .get_i32(0x0E07)
        .expect("get")
        .expect("PidTagMessageFlags present");
    assert_eq!(
        flags, 1,
        "PidTagMessageFlags must always be present (MSGFLAG_READ), even without submit_time"
    );

    assert!(
        pc.get_time(0x3007).expect("get").is_none(),
        "PidTagCreationTime must be omitted, never invented, when submit_time is None"
    );
    assert!(
        pc.get_time(0x3008).expect("get").is_none(),
        "PidTagLastModificationTime must be omitted, never invented, when submit_time is None"
    );

    cleanup(&path);
}

// ── Custom folder display name ───────────────────────────────────────────────

#[test]
fn custom_folder_display_name_is_honored() {
    let path = scratch_path("custom_folder_name");
    cleanup(&path);

    let opts = WritePstOpts {
        folder_display_name: "My Export".to_string(),
        ..WritePstOpts::default()
    };
    write_unicode_pst(&path, Vec::new(), &[], &opts).expect("write");

    let mut pst = pst_reader::PstFile::open(&path).expect("open");
    let folders = pst.folders().expect("folders");
    assert!(folders.iter().any(|f| f.name == "My Export"));
    assert!(!folders.iter().any(|f| f.name == "Unique Mail"));

    cleanup(&path);
}

// ── Store PidTagRecordKey / EntryID ProviderUID self-consistency ───────────
//
// Round-5 cross-model review finding (Part A): the store previously never
// wrote PidTagRecordKey (0x0FF9) at all, and the EntryID's 16-byte
// ProviderUID was a hardcoded all-zero placeholder unrelated to it. Fixed so
// both carry the same per-write-generated, non-cryptographic 16-byte value.

fn read_store_record_key_and_entry_id_provider_uid(path: &Path) -> (Vec<u8>, Vec<u8>) {
    let mut pst = pst_reader::PstFile::open(path).expect("open");
    let raw = pst
        .read_node_data(pst_reader::NodeId(0x21))
        .expect("store raw data");
    let pc = pst_reader::ltp::pc::PropContext::load(raw).expect("store pc");

    let record_key = pc
        .get_binary(0x0FF9)
        .expect("get")
        .expect("PidTagRecordKey present");

    let entry_id = pc
        .get_binary(0x35E0)
        .expect("get")
        .expect("PidTagIpmSubtreeEntryId present");
    assert_eq!(entry_id.len(), 24, "EntryID must be 24 bytes total");
    // abFlags(4) + ProviderUID(16) + NID(4) = 24; ProviderUID is bytes 4..20.
    let provider_uid = entry_id[4..20].to_vec();

    (record_key, provider_uid)
}

#[test]
fn store_record_key_present_nonzero_and_matches_entry_id_provider_uid() {
    let path = scratch_path("record_key_self_consistent");
    cleanup(&path);

    write_unicode_pst(&path, Vec::new(), &[], &WritePstOpts::default()).expect("write");

    let (record_key, provider_uid) = read_store_record_key_and_entry_id_provider_uid(&path);

    assert_eq!(record_key.len(), 16, "PidTagRecordKey must be 16 bytes");
    assert_ne!(
        record_key,
        vec![0u8; 16],
        "PidTagRecordKey must not be an all-zero placeholder"
    );
    assert_eq!(
        provider_uid, record_key,
        "EntryID's ProviderUID must equal the store's own PidTagRecordKey byte-for-byte \
         (self-consistent same-store EntryID, not an arbitrary placeholder)"
    );

    cleanup(&path);
}

#[test]
fn store_record_key_differs_across_separate_writes() {
    let path_a = scratch_path("record_key_write_a");
    let path_b = scratch_path("record_key_write_b");
    cleanup(&path_a);
    cleanup(&path_b);

    write_unicode_pst(&path_a, Vec::new(), &[], &WritePstOpts::default()).expect("write a");
    // A different message count feeds into record-key generation alongside
    // wall-clock/pid, so this also guards against a key that's a function of
    // the (identical-in-this-test) destination path alone.
    let msg = short_message("<record-key-b@example.com>", "second write");
    write_unicode_pst(&path_b, vec![msg], &[], &WritePstOpts::default()).expect("write b");

    let (record_key_a, _) = read_store_record_key_and_entry_id_provider_uid(&path_a);
    let (record_key_b, _) = read_store_record_key_and_entry_id_provider_uid(&path_b);

    assert_ne!(
        record_key_a, record_key_b,
        "two separate writes must produce different record keys, proving this is \
         genuinely generated per-write and not a hardcoded constant"
    );

    cleanup(&path_a);
    cleanup(&path_b);
}
