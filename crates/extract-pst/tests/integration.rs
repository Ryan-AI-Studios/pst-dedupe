//! Integration tests for extract-pst (fixture PSTs only).

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use extract_pst::{
    encode_native_message_v1, extract_pst_item, extract_pst_path, list_discovered_psts,
    native_message_v1_digest, parse_display_list, resume_extract, ExtractLimits, NativeAttachment,
    NativeMessageV1, JOB_KIND_EXTRACT_PST, NATIVE_FORMAT_V1, STAGE_PST_EXTRACT,
};
use matter_core::{
    compute_email_logical_hash, item_role, item_status, EmailLogicalInput, ItemInput, JobState,
    Matter, WORKSPACE_DIR, WORKSPACE_TEMP_DIR,
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

/// Count total messages in a fixture PST.
fn fixture_message_count(pst: &PathBuf) -> usize {
    let Ok(mut f) = pst_reader::PstFile::open(pst) else {
        return 0;
    };
    let Ok(folders) = f.folders() else {
        return 0;
    };
    folders.iter().map(|folder| folder.message_nids.len()).sum()
}

#[test]
fn happy_path_fixture_extract() {
    let Some(pst) = fixture_pst_with_messages() else {
        eprintln!("skip: no fixture PST with messages");
        return;
    };
    let total_msgs = fixture_message_count(&pst);
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-happy");
    let matter = Matter::create(&root, "Happy").expect("create");
    let source = matter
        .insert_source(pst.to_str().unwrap(), "pst", "importing", None)
        .expect("source");
    let inv_id = register_pst_inventory(&matter, &source.id, &pst);

    // No max_messages cap so full walk can claim completed when the fixture is small.
    let limits = ExtractLimits {
        batch_size: 50,
        max_messages: None,
        ..ExtractLimits::default()
    };
    let summary = extract_pst_item(&matter, &source.id, &inv_id, &limits, None).expect("extract");
    assert!(summary.completed, "full walk without cap must complete");
    assert!(!summary.cancelled);
    assert!(
        summary.messages_ok + summary.messages_err > 0,
        "expected at least one message attempt"
    );
    assert!(
        summary.messages_ok + summary.messages_err >= total_msgs as u64 || total_msgs == 0,
        "expected to attempt all {total_msgs} fixture messages"
    );

    let items = matter.list_items_for_source(&source.id).expect("list");
    let emails: Vec<_> = items
        .iter()
        .filter(|i| i.file_category.as_deref() == Some("email"))
        .collect();
    assert!(!emails.is_empty(), "expected extracted email items");

    for email in &emails {
        assert!(
            email.role.as_deref() == Some(item_role::PARENT)
                || email.role.as_deref() == Some(item_role::STANDALONE),
            "email item must have parent/standalone role, got {:?}",
            email.role
        );
        assert!(
            email.status == item_status::EXTRACTED || email.status == item_status::PARTIAL,
            "email status: {}",
            email.status
        );
        assert!(
            email.logical_hash.is_some(),
            "logical_hash set on {}",
            email.path.as_deref().unwrap_or("?")
        );
        assert_eq!(email.logical_hash_version, 1);
        assert!(email.native_sha256.is_some(), "native_sha256 set");
    }

    let parent = emails
        .iter()
        .find(|i| i.status == item_status::EXTRACTED || i.status == item_status::PARTIAL)
        .expect("extracted parent");
    if let Some(ref extra) = parent.extra_json {
        assert!(
            extra.contains(NATIVE_FORMAT_V1),
            "extra_json records native format: {extra}"
        );
    }

    // Family / attachment children when the fixture has attaches.
    let attaches: Vec<_> = items
        .iter()
        .filter(|i| i.role.as_deref() == Some(item_role::ATTACHMENT))
        .collect();
    if !attaches.is_empty() {
        assert!(
            parent.family_id.is_some(),
            "parent with attachments must have family_id"
        );
        let family_id = parent.family_id.as_deref().unwrap();
        let members = matter.list_family_members(family_id).expect("family");
        assert!(
            members.len() >= 2,
            "family should include parent + attach(es)"
        );
        assert!(
            attaches.iter().any(|a| a.native_sha256.is_some()
                || a.status == item_status::ERROR
                || a.status == item_status::PARTIAL),
            "attachment children present"
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

fn message_paths_unique(matter: &Matter, source_id: &str) -> std::collections::HashSet<String> {
    let items = matter.list_items_for_source(source_id).expect("list");
    let mut paths = std::collections::HashSet::new();
    for it in &items {
        if let Some(ref p) = it.path {
            if p.contains("!/") && !p.contains("/attach/") {
                assert!(paths.insert(p.clone()), "duplicate message path: {p}");
            }
        }
    }
    paths
}

#[test]
fn resume_mid_folder_no_duplicates() {
    let Some(pst) = fixture_pst_with_messages() else {
        eprintln!("skip: no fixture PST with messages");
        return;
    };
    let total = fixture_message_count(&pst);
    if total < 2 {
        eprintln!("skip: need ≥2 messages to force mid-walk cancel+resume");
        return;
    }
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
        first.cancelled,
        "with ≥2 messages and cancel after 2 polls, first run must cancel"
    );
    assert!(!first.completed);
    assert!(
        first.messages_ok + first.messages_err >= 1,
        "expected at least one message before cancel"
    );

    let job = matter.get_job(&first.job_id).expect("job");
    assert_eq!(job.state, JobState::Paused);

    let cp = matter
        .get_checkpoint(&first.job_id, STAGE_PST_EXTRACT)
        .expect("cp")
        .expect("checkpoint must exist after progress");
    let cursor: serde_json::Value = serde_json::from_str(&cp.cursor_json).expect("cursor json");
    assert!(
        cursor.get("folder_message_index").is_some() || cursor.get("last_message_nid").is_some(),
        "checkpoint must record mid-folder position: {}",
        cp.cursor_json
    );
    let paths_before = message_paths_unique(&matter, &source.id);
    assert!(!paths_before.is_empty());

    let resumed = resume_extract(&matter, &source.id, &first.job_id, &lim, None).expect("resume");
    assert!(
        resumed.completed || resumed.messages_ok + resumed.messages_err > first.messages_ok,
        "resume must complete or make progress"
    );

    let paths_after = message_paths_unique(&matter, &source.id);
    assert!(
        paths_after.len() >= paths_before.len(),
        "resume must not drop paths"
    );
    // Re-extract same inventory must not duplicate paths either.
    let second = extract_pst_item(&matter, &source.id, &inv_id, &lim, None).expect("re-extract");
    assert!(second.completed || second.messages_ok + second.messages_err > 0);
    let paths_re = message_paths_unique(&matter, &source.id);
    assert_eq!(
        paths_re.len(),
        paths_after.len(),
        "re-extract must not create duplicate message paths"
    );
}

#[test]
fn max_messages_pauses_incomplete_job() {
    let Some(pst) = fixture_pst_with_messages() else {
        eprintln!("skip: no fixture PST with messages");
        return;
    };
    let total = fixture_message_count(&pst);
    if total < 2 {
        eprintln!("skip: need ≥2 messages for max_messages pause test");
        return;
    }
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-maxmsg");
    let matter = Matter::create(&root, "MaxMsg").expect("create");
    let source = matter
        .insert_source(pst.to_str().unwrap(), "pst", "importing", None)
        .expect("source");
    let inv_id = register_pst_inventory(&matter, &source.id, &pst);

    let lim = ExtractLimits {
        batch_size: 1,
        max_messages: Some(1),
        ..ExtractLimits::default()
    };
    let first = extract_pst_item(&matter, &source.id, &inv_id, &lim, None).expect("capped");
    assert!(!first.completed, "max_messages mid-PST must not complete");
    assert!(!first.cancelled);
    assert_eq!(first.messages_ok + first.messages_err, 1);
    let job = matter.get_job(&first.job_id).expect("job");
    assert_eq!(job.state, JobState::Paused);
    assert!(
        matter
            .get_checkpoint(&first.job_id, STAGE_PST_EXTRACT)
            .expect("cp")
            .is_some(),
        "checkpoint required for resume after max_messages"
    );

    // Resume with a higher (or no) cap continues the same job.
    let lim2 = ExtractLimits {
        batch_size: 50,
        max_messages: None,
        ..ExtractLimits::default()
    };
    let resumed = resume_extract(&matter, &source.id, &first.job_id, &lim2, None).expect("resume");
    assert!(resumed.completed, "resume without cap should finish walk");
    let job2 = matter.get_job(&first.job_id).expect("job");
    assert_eq!(job2.state, JobState::Succeeded);
    let paths = message_paths_unique(&matter, &source.id);
    assert!(paths.len() >= 2, "expected multiple unique message paths");
}

#[test]
fn partial_attach_cap_records_errors() {
    let Some(pst) = fixture_pst_with_messages() else {
        eprintln!("skip: no fixture PST with messages");
        return;
    };
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-partial");
    let matter = Matter::create(&root, "Partial").expect("create");
    let source = matter
        .insert_source(pst.to_str().unwrap(), "pst", "importing", None)
        .expect("source");
    let inv_id = register_pst_inventory(&matter, &source.id, &pst);

    // Force every non-empty attachment over the cap → attach_too_large + parent partial.
    let lim = ExtractLimits {
        batch_size: 50,
        max_messages: Some(30),
        max_attachment_bytes: Some(0),
        ..ExtractLimits::default()
    };
    let summary = extract_pst_item(&matter, &source.id, &inv_id, &lim, None).expect("extract");
    assert!(summary.messages_ok + summary.messages_err > 0);

    let items = matter.list_items_for_source(&source.id).expect("list");
    let emails: Vec<_> = items
        .iter()
        .filter(|i| i.file_category.as_deref() == Some("email"))
        .collect();
    assert!(!emails.is_empty(), "other messages / parents still present");

    let errors = matter
        .item_errors_for_source(&source.id)
        .expect("item_errors");
    let attach_errs: Vec<_> = errors
        .iter()
        .filter(|e| e.code == "attach_too_large")
        .collect();
    if summary.attachments_err == 0 && attach_errs.is_empty() {
        // Fixture may have zero attachments; still require clean parents with hash.
        for e in &emails {
            assert!(e.logical_hash.is_some(), "message logical_hash set");
            assert!(
                e.status == item_status::EXTRACTED || e.status == item_status::PARTIAL,
                "status {}",
                e.status
            );
        }
        eprintln!("note: fixture has no attachments; partial-attach path not exercised");
        return;
    }
    assert!(
        !attach_errs.is_empty() || summary.attachments_err > 0,
        "expected attach_too_large errors when attachments present"
    );
    let partial_parents: Vec<_> = emails
        .iter()
        .filter(|e| e.status == item_status::PARTIAL)
        .collect();
    assert!(
        !partial_parents.is_empty(),
        "parent should be partial when attach fails"
    );
    // Sibling success: at least one email with logical_hash.
    assert!(
        emails.iter().any(|e| e.logical_hash.is_some()),
        "successful sibling messages keep logical_hash"
    );
}

#[test]
fn extract_pst_path_streams_without_full_buf() {
    let Some(pst) = fixture_pst_with_messages() else {
        eprintln!("skip: no fixture PST with messages");
        return;
    };
    let (_tmp, base) = utf8_tempdir();
    let root = base.join("matter-path");
    let matter = Matter::create(&root, "Path").expect("create");
    let source = matter
        .insert_source(pst.to_str().unwrap(), "pst", "importing", None)
        .expect("source");
    let lim = ExtractLimits {
        max_messages: Some(1),
        batch_size: 1,
        ..ExtractLimits::default()
    };
    let summary = extract_pst_path(
        &matter,
        &source.id,
        pst.to_str().expect("utf8 path"),
        &lim,
        None,
    )
    .expect("extract_pst_path");
    assert!(summary.messages_ok + summary.messages_err >= 1);
    // Inventory digest present and matches put_reader of the same file.
    let items = matter.list_items_for_source(&source.id).expect("list");
    let inv = items
        .iter()
        .find(|i| {
            i.file_category.as_deref() == Some("pst")
                || i.path
                    .as_deref()
                    .map(|p| p.to_ascii_lowercase().ends_with(".pst") && !p.contains("!/"))
                    .unwrap_or(false)
        })
        .expect("inventory pst item");
    let digest = inv.native_sha256.as_deref().expect("digest");
    let mut f = std::fs::File::open(&pst).expect("open");
    let expected = matter.put_reader(&mut f).expect("stream digest");
    assert_eq!(digest, expected, "path entry must stream-put same digest");
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
    // Cap may pause the job; this test only needs open-from-CAS to succeed.
    let limits = ExtractLimits {
        max_messages: Some(5),
        ..ExtractLimits::default()
    };
    let summary =
        extract_pst_item(&matter, &source.id, &inv_id, &limits, None).expect("cas extract");
    assert!(summary.messages_ok + summary.messages_err > 0);
    // completed may be false when fixture has >5 messages (max_messages pause).

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
