//! Integration tests for extract-pst (fixture PSTs only).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use extract_pst::{
    encode_native_message_v1, extract_pst_item, list_discovered_psts, native_message_v1_digest,
    parse_display_list, resume_extract, ExtractLimits, NativeAttachment, NativeMessageV1,
    JOB_KIND_EXTRACT_PST, NATIVE_FORMAT_V1,
};
use matter_core::{
    compute_email_logical_hash, item_status, EmailLogicalInput, ItemInput, Matter, WORKSPACE_DIR,
    WORKSPACE_TEMP_DIR,
};
use tempfile::tempdir;

fn utf8_tempdir() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
    (dir, path)
}

fn workspace_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates
    path.pop(); // workspace
    path
}

/// Prefer fixtures known to contain messages (sample.pst is often empty).
fn fixture_pst_with_messages() -> Option<PathBuf> {
    let root = workspace_root();
    let candidates = [
        root.join("fixtures/aspose_outlook.pst"),
        root.join("fixtures/aspose_sub.pst"),
        root.join("fixtures/aspose_personalstorage.pst"),
        root.join("fixtures/sample.pst"),
    ];
    for p in candidates {
        if !p.is_file() {
            continue;
        }
        if let Ok(mut pst) = pst_reader::PstFile::open(&p) {
            if let Ok(folders) = pst.folders() {
                let n: usize = folders.iter().map(|f| f.message_nids.len()).sum();
                if n > 0 {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn any_fixture_pst() -> Option<PathBuf> {
    fixture_pst_with_messages().or_else(|| {
        let root = workspace_root();
        [
            root.join("fixtures/sample.pst"),
            root.join("fixtures/aspose_outlook.pst"),
        ]
        .into_iter()
        .find(|p| p.is_file())
    })
}

fn register_pst_inventory(matter: &Matter, source_id: &str, pst_path: &PathBuf) -> String {
    let bytes = fs::read(pst_path).expect("read pst");
    let digest = matter.put_bytes(&bytes).expect("cas put pst");
    let name = pst_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("mail.pst")
        .to_string();
    let item = matter
        .insert_item(ItemInput {
            source_id: Some(source_id.to_string()),
            path: Some(name),
            native_sha256: Some(digest),
            status: item_status::DISCOVERED.to_string(),
            size_bytes: Some(bytes.len() as i64),
            file_category: Some("pst".into()),
            ..Default::default()
        })
        .expect("insert inventory");
    item.id
}

#[test]
fn happy_path_fixture_extract() {
    let Some(pst) = fixture_pst_with_messages() else {
        eprintln!("skip: no fixture PST with messages");
        return;
    };
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-happy");
    let matter = Matter::create(&root, "Happy").expect("create");
    let source = matter
        .insert_source(pst.to_str().unwrap(), "pst", "importing", None)
        .expect("source");
    let inv_id = register_pst_inventory(&matter, &source.id, &pst);

    let limits = ExtractLimits {
        batch_size: 50,
        max_messages: Some(20),
        ..ExtractLimits::default()
    };
    let summary = extract_pst_item(&matter, &source.id, &inv_id, &limits, None).expect("extract");
    assert!(summary.completed);
    assert!(!summary.cancelled);
    assert!(
        summary.messages_ok + summary.messages_err > 0,
        "expected at least one message attempt"
    );

    let items = matter.list_items_for_source(&source.id).expect("list");
    let emails: Vec<_> = items
        .iter()
        .filter(|i| i.file_category.as_deref() == Some("email"))
        .collect();
    assert!(!emails.is_empty(), "expected extracted email items");

    let parent = emails
        .iter()
        .find(|i| i.status == item_status::EXTRACTED || i.status == item_status::PARTIAL)
        .expect("extracted parent");
    assert!(parent.logical_hash.is_some(), "logical_hash set");
    assert_eq!(parent.logical_hash_version, 1);
    assert!(parent.native_sha256.is_some(), "native_sha256 set");
    if let Some(ref extra) = parent.extra_json {
        assert!(
            extra.contains(NATIVE_FORMAT_V1),
            "extra_json records native format: {extra}"
        );
    }

    // Logical hash matches direct compute_email_logical_hash for same fields.
    if let Some(ref lh) = parent.logical_hash {
        let to: Vec<String> = parent
            .to_addrs_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();
        let cc: Vec<String> = parent
            .cc_addrs_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();
        let bcc: Vec<String> = parent
            .bcc_addrs_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();
        // Body text is normalized before store; re-derive from CAS when present.
        let body = parent
            .text_sha256
            .as_deref()
            .and_then(|d| matter.get_bytes(d).ok())
            .and_then(|b| String::from_utf8(b).ok());
        let atts = matter.list_attachments(&parent.id).unwrap_or_default();
        let logical_atts: Vec<_> = atts
            .iter()
            .filter_map(|a| {
                Some(matter_core::LogicalAttachment {
                    filename: a.title.clone().unwrap_or_default(),
                    size: a.size_bytes.unwrap_or(0) as u64,
                    native_sha256: a.native_sha256.clone()?,
                })
            })
            .collect();
        let recomputed = compute_email_logical_hash(&EmailLogicalInput {
            message_id: parent.message_id.clone(),
            subject: parent.subject.clone(),
            from: parent.from_addr.clone(),
            to,
            cc,
            bcc,
            sent: parent.sent_at.clone(),
            received: parent.received_at.clone(),
            body,
            attachments: logical_atts,
        });
        assert_eq!(
            recomputed, *lh,
            "logical_hash must match compute_email_logical_hash"
        );
    }

    matter.verify_audit_chain().expect("audit");

    let psts = list_discovered_psts(&matter, &source.id).expect("list psts");
    assert!(!psts.is_empty());
}

#[test]
fn resume_mid_folder_no_duplicates() {
    let Some(pst) = fixture_pst_with_messages() else {
        eprintln!("skip: no fixture PST with messages");
        return;
    };
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-resume");
    let matter = Matter::create(&root, "Resume").expect("create");
    let source = matter
        .insert_source(pst.to_str().unwrap(), "pst", "importing", None)
        .expect("source");
    let inv_id = register_pst_inventory(&matter, &source.id, &pst);

    // Cancel after the first message completes (cancel checked before each message;
    // allow checks 0 and 1 so message 0 runs, then cancel before message 1).
    let calls = Arc::new(AtomicU64::new(0));
    let cancel_fn = {
        let calls = calls.clone();
        move || calls.fetch_add(1, Ordering::SeqCst) >= 2
    };
    let lim = ExtractLimits {
        batch_size: 1,
        max_messages: None,
        ..ExtractLimits::default()
    };
    let first = extract_pst_item(&matter, &source.id, &inv_id, &lim, Some(&cancel_fn))
        .expect("partial extract");
    assert!(
        first.cancelled || first.messages_ok + first.messages_err >= 1,
        "expected cancel or at least one message"
    );

    if first.cancelled {
        let resumed =
            resume_extract(&matter, &source.id, &first.job_id, &lim, None).expect("resume");
        assert!(resumed.completed || resumed.messages_ok > 0);
    } else {
        // If not cancelled (tiny folder), still verify no dups on second extract.
        let _ = extract_pst_item(&matter, &source.id, &inv_id, &lim, None).expect("second");
    }

    let items = matter.list_items_for_source(&source.id).expect("list");
    let mut paths = std::collections::HashSet::new();
    for it in &items {
        if let Some(ref p) = it.path {
            if p.contains("!/") && !p.contains("/attach/") {
                assert!(
                    paths.insert(p.clone()),
                    "duplicate message path on resume: {p}"
                );
            }
        }
    }
    assert!(!paths.is_empty(), "expected extracted message paths");
}

#[test]
fn open_from_cas_only_temp_under_workspace() {
    let Some(pst) = fixture_pst_with_messages() else {
        eprintln!("skip: no fixture PST with messages");
        return;
    };
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-cas-only");
    let matter = Matter::create(&root, "CAS only").expect("create");
    // Source path intentionally does NOT exist on disk.
    let source = matter
        .insert_source("Z:\\nonexistent\\package", "purview", "importing", None)
        .expect("source");
    let inv_id = register_pst_inventory(&matter, &source.id, &pst);

    let os_temp = std::env::temp_dir();
    let limits = ExtractLimits {
        max_messages: Some(5),
        ..ExtractLimits::default()
    };
    let summary =
        extract_pst_item(&matter, &source.id, &inv_id, &limits, None).expect("cas extract");
    assert!(summary.messages_ok + summary.messages_err > 0);

    let ws_temp = root.join(WORKSPACE_DIR).join(WORKSPACE_TEMP_DIR);
    assert!(ws_temp.as_std_path().is_dir());
    let matter_canon = root
        .as_std_path()
        .canonicalize()
        .unwrap_or(root.as_std_path().to_path_buf());
    assert!(
        !matter_canon.starts_with(&os_temp),
        "matter root must not be under OS TEMP for this test"
    );
    // After extract, temp PST should be cleaned by RAII guard.
    let leftovers: Vec<_> = fs::read_dir(ws_temp.as_std_path())
        .map(|rd| rd.filter_map(|e| e.ok()).collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "workspace/temp should be empty after successful extract (RAII), got {leftovers:?}"
    );
}

#[test]
fn orphan_temp_cleaned_on_matter_open() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-orphan");
    {
        let matter = Matter::create(&root, "Orphan").expect("create");
        let orphan = matter.workspace_temp_dir().join("leftover.pst");
        fs::write(orphan.as_std_path(), b"crash residue").expect("write");
    }
    let matter = Matter::open(&root).expect("open");
    assert!(!matter
        .workspace_temp_dir()
        .join("leftover.pst")
        .as_std_path()
        .exists());
}

#[test]
fn put_reader_parity_via_matter() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-putr");
    let matter = Matter::create(&root, "PutR").expect("create");
    let data: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
    let d1 = matter.put_bytes(&data).expect("bytes");
    let d2 = matter
        .put_reader(&mut std::io::Cursor::new(data.as_slice()))
        .expect("reader");
    assert_eq!(d1, d2);
}

#[test]
fn ansi_or_bad_file_structured_fail() {
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-bad");
    let matter = Matter::create(&root, "Bad").expect("create");
    let source = matter
        .insert_source("Z:\\nope", "pst", "importing", None)
        .expect("source");
    let digest = matter.put_bytes(b"not a pst file at all!!!!").expect("put");
    let inv = matter
        .insert_item(ItemInput {
            source_id: Some(source.id.clone()),
            path: Some("bad.pst".into()),
            native_sha256: Some(digest),
            status: item_status::DISCOVERED.to_string(),
            ..Default::default()
        })
        .expect("inv");
    let err = extract_pst_item(
        &matter,
        &source.id,
        &inv.id,
        &ExtractLimits::default(),
        None,
    )
    .expect_err("must fail");
    let code = err.code();
    assert!(
        code == "pst_open_failed" || code == "pst_ansi_rejected",
        "got code {code}: {err}"
    );
}

#[test]
fn bcc_mapping_and_logical_hash_integration() {
    let to = parse_display_list(Some("Alice <a@example.com>"));
    let bcc = parse_display_list(Some("Secret <bcc@example.com>"));
    assert_eq!(to, vec!["a@example.com"]);
    assert_eq!(bcc, vec!["bcc@example.com"]);

    let with_bcc = EmailLogicalInput {
        message_id: Some("<x@y>".into()),
        subject: Some("S".into()),
        from: Some("f@e.com".into()),
        to: to.clone(),
        cc: vec![],
        bcc: bcc.clone(),
        sent: Some("2020-01-02T03:04:05Z".into()),
        received: None,
        body: Some("hi".into()),
        attachments: vec![],
    };
    let without_bcc = EmailLogicalInput {
        bcc: vec![],
        ..with_bcc.clone()
    };
    let h1 = compute_email_logical_hash(&with_bcc);
    let h2 = compute_email_logical_hash(&without_bcc);
    assert_ne!(h1, h2, "BCC must affect logical_hash");
    assert_eq!(h1, compute_email_logical_hash(&with_bcc));
}

#[test]
fn native_v1_golden_stability() {
    let msg = NativeMessageV1 {
        message_nid: 0x2004,
        message_id: "<msg-1@example.com>".into(),
        subject: "Hello".into(),
        from: "a@example.com".into(),
        to: "b@example.com".into(),
        cc: "".into(),
        bcc: "".into(),
        sent: "2020-01-02T03:04:05Z".into(),
        received: "2020-01-02T03:05:00Z".into(),
        body: b"body text".to_vec(),
        attachments: vec![NativeAttachment {
            filename: "a.txt".into(),
            size: 3,
            native_sha256: "aabbccdd".into(),
        }],
    };
    const GOLDEN: &str = "09b8a17797e679fd028aae7b48e05a9e1a1796fb37f6f8c13a5ca548d6ab8160";
    assert_eq!(native_message_v1_digest(&msg), GOLDEN);
    let bytes = encode_native_message_v1(&msg);
    assert_eq!(&bytes[0..4], b"PNM1");
}

#[test]
fn job_kind_constant() {
    assert_eq!(JOB_KIND_EXTRACT_PST, "extract_pst");
}

#[test]
fn any_fixture_available_smoke() {
    // Documents fixture discovery for CI logs.
    if let Some(p) = any_fixture_pst() {
        eprintln!("fixture available: {}", p.display());
    }
}
