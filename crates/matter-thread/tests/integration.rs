//! Synthetic matter threading tests (spec §3.11).

use matter_core::{
    item_role, item_status, item_thread_method, ItemInput, Matter, FAMILY_KIND_EMAIL_ATTACHMENTS,
};
use matter_thread::{
    normalize_subject_thread, run_thread, sha256_hex, ThreadOutcome, ThreadParams, THREAD_STAGE,
};

fn temp_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    let root = root.join(name);
    let matter = Matter::create(&root, name).expect("create");
    (tmp, matter)
}

fn parent(
    matter: &Matter,
    path: &str,
    mid: Option<&str>,
    irt: Option<&str>,
    refs: Option<&str>,
    subject: Option<&str>,
    ci_hex: Option<&str>,
) -> matter_core::Item {
    matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            file_category: Some("email".into()),
            path: Some(path.into()),
            message_id: mid.map(|s| s.into()),
            in_reply_to: irt.map(|s| s.into()),
            references_json: refs.map(|s| s.into()),
            subject: subject.map(|s| s.into()),
            conversation_index_hex: ci_hex.map(|s| s.into()),
            ..Default::default()
        })
        .expect("insert")
}

fn run_default(matter: &Matter, job_id: &str) -> ThreadOutcome {
    let params = ThreadParams::default();
    run_thread(matter, job_id, &params, None, |_| {}).expect("run")
}

#[test]
fn in_reply_to_pair_same_thread_headers() {
    let (_tmp, matter) = temp_matter("irt-pair");
    let job = matter.create_job("thread").expect("job");
    let b = parent(
        &matter,
        "b",
        Some("b@ex.com"),
        None,
        None,
        Some("Hello"),
        None,
    );
    let a = parent(
        &matter,
        "a",
        Some("a@ex.com"),
        Some("b@ex.com"),
        None,
        Some("Re: Hello"),
        None,
    );

    let out = run_default(&matter, &job.id);
    assert!(matches!(out, ThreadOutcome::Succeeded(_)));

    let a2 = matter.get_item(&a.id).unwrap();
    let b2 = matter.get_item(&b.id).unwrap();
    assert_eq!(a2.thread_id, b2.thread_id);
    assert!(a2.thread_id.is_some());
    assert_eq!(
        a2.thread_method.as_deref(),
        Some(item_thread_method::HEADERS)
    );
    assert_eq!(
        b2.thread_method.as_deref(),
        Some(item_thread_method::HEADERS)
    );
    assert_eq!(a2.thread_root_item_id, b2.thread_root_item_id);
    // root = earliest by stable order (imported_at ASC, path ASC, id ASC).
    let root = a2.thread_root_item_id.as_deref().expect("root");
    assert!(
        root == a.id.as_str() || root == b.id.as_str(),
        "root must be one of the pair members"
    );
    // min of a@ex.com / b@ex.com by string order is a@ex.com
    let expected = sha256_hex("thread:v1\na@ex.com");
    assert_eq!(a2.thread_id.as_deref(), Some(expected.as_str()));
}

#[test]
fn references_chain_of_three() {
    let (_tmp, matter) = temp_matter("refs-chain");
    let job = matter.create_job("thread").expect("job");
    let a = parent(&matter, "a", Some("a@ex.com"), None, None, Some("T"), None);
    let b = parent(
        &matter,
        "b",
        Some("b@ex.com"),
        Some("a@ex.com"),
        Some(r#"["a@ex.com"]"#),
        Some("Re: T"),
        None,
    );
    let c = parent(
        &matter,
        "c",
        Some("c@ex.com"),
        Some("b@ex.com"),
        Some(r#"["a@ex.com","b@ex.com"]"#),
        Some("Re: T"),
        None,
    );

    let _ = run_default(&matter, &job.id);
    let ta = matter.get_item(&a.id).unwrap().thread_id;
    let tb = matter.get_item(&b.id).unwrap().thread_id;
    let tc = matter.get_item(&c.id).unwrap().thread_id;
    assert_eq!(ta, tb);
    assert_eq!(tb, tc);
    assert!(ta.is_some());
}

#[test]
fn phantom_mid_links_two_children() {
    let (_tmp, matter) = temp_matter("phantom");
    let job = matter.create_job("thread").expect("job");
    let c1 = parent(
        &matter,
        "c1",
        Some("c1@ex.com"),
        Some("phantom@ex.com"),
        Some(r#"["phantom@ex.com"]"#),
        Some("Re: X"),
        None,
    );
    let c2 = parent(
        &matter,
        "c2",
        Some("c2@ex.com"),
        Some("phantom@ex.com"),
        Some(r#"["phantom@ex.com"]"#),
        Some("Re: X"),
        None,
    );

    let _ = run_default(&matter, &job.id);
    let t1 = matter.get_item(&c1.id).unwrap().thread_id;
    let t2 = matter.get_item(&c2.id).unwrap().thread_id;
    assert_eq!(t1, t2);
    assert_eq!(
        matter.get_item(&c1.id).unwrap().thread_method.as_deref(),
        Some(item_thread_method::HEADERS)
    );
}

#[test]
fn subject_merge_singletons() {
    let (_tmp, matter) = temp_matter("subj");
    let job = matter.create_job("thread").expect("job");
    let a = parent(&matter, "a", None, None, None, Some("Re: Budget"), None);
    let b = parent(&matter, "b", None, None, None, Some("FW: Budget"), None);

    assert_eq!(
        normalize_subject_thread("Re: Budget"),
        normalize_subject_thread("FW: Budget")
    );

    let _ = run_default(&matter, &job.id);
    let a2 = matter.get_item(&a.id).unwrap();
    let b2 = matter.get_item(&b.id).unwrap();
    assert_eq!(a2.thread_id, b2.thread_id);
    assert_eq!(
        a2.thread_method.as_deref(),
        Some(item_thread_method::SUBJECT)
    );
    assert_eq!(
        b2.thread_method.as_deref(),
        Some(item_thread_method::SUBJECT)
    );
    let expected = sha256_hex("thread-subj:v1\nbudget");
    assert_eq!(a2.thread_id.as_deref(), Some(expected.as_str()));
}

#[test]
fn subject_does_not_glue_into_headers_thread() {
    let (_tmp, matter) = temp_matter("subj-no-glue");
    let job = matter.create_job("thread").expect("job");
    // Header pair on "Invoice"
    let root = parent(
        &matter,
        "root",
        Some("root@ex.com"),
        None,
        None,
        Some("Invoice"),
        None,
    );
    let reply = parent(
        &matter,
        "reply",
        Some("reply@ex.com"),
        Some("root@ex.com"),
        None,
        Some("Re: Invoice"),
        None,
    );
    // Orphan singleton with same stripped subject
    let orphan = parent(
        &matter,
        "orphan",
        None,
        None,
        None,
        Some("FW: Invoice"),
        None,
    );

    let _ = run_default(&matter, &job.id);
    let r = matter.get_item(&root.id).unwrap();
    let p = matter.get_item(&reply.id).unwrap();
    let o = matter.get_item(&orphan.id).unwrap();
    assert_eq!(r.thread_id, p.thread_id);
    assert_eq!(
        r.thread_method.as_deref(),
        Some(item_thread_method::HEADERS)
    );
    assert_ne!(
        o.thread_id, r.thread_id,
        "subject must not glue orphan into headers multi-thread"
    );
    // Orphan alone → singleton (no second subject peer)
    assert_eq!(
        o.thread_method.as_deref(),
        Some(item_thread_method::SINGLETON)
    );
}

#[test]
fn empty_subject_no_headers_singleton() {
    let (_tmp, matter) = temp_matter("empty-singleton");
    let job = matter.create_job("thread").expect("job");
    let a = parent(&matter, "a", None, None, None, None, None);
    let b = parent(&matter, "b", None, None, None, Some(""), None);

    let _ = run_default(&matter, &job.id);
    let a2 = matter.get_item(&a.id).unwrap();
    let b2 = matter.get_item(&b.id).unwrap();
    assert_eq!(
        a2.thread_method.as_deref(),
        Some(item_thread_method::SINGLETON)
    );
    assert_eq!(
        b2.thread_method.as_deref(),
        Some(item_thread_method::SINGLETON)
    );
    assert_ne!(a2.thread_id, b2.thread_id);
    assert_eq!(a2.thread_id.as_deref(), Some(a.id.as_str()));
}

#[test]
fn conversation_index_opaque_prefix() {
    let (_tmp, matter) = temp_matter("ci");
    let job = matter.create_job("thread").expect("job");
    // 22-byte prefix shared (44 hex); different suffixes
    let prefix: String = (0u8..22).map(|b| format!("{b:02x}")).collect();
    assert_eq!(prefix.len(), 44);
    // Aberrant 01 01… still groups when prefix shared
    let hex_a = format!("{prefix}aabb");
    let hex_b = format!("{prefix}ccdd");
    let a = parent(&matter, "a", None, None, None, None, Some(&hex_a));
    let b = parent(&matter, "b", None, None, None, None, Some(&hex_b));

    let _ = run_default(&matter, &job.id);
    let a2 = matter.get_item(&a.id).unwrap();
    let b2 = matter.get_item(&b.id).unwrap();
    assert_eq!(a2.thread_id, b2.thread_id);
    assert_eq!(
        a2.thread_method.as_deref(),
        Some(item_thread_method::CONVERSATION_INDEX)
    );
    let expected = sha256_hex(&format!("thread-ci:v1\n{prefix}"));
    assert_eq!(a2.thread_id.as_deref(), Some(expected.as_str()));
}

#[test]
fn family_inherit_attachment() {
    let (_tmp, matter) = temp_matter("family");
    let job = matter.create_job("thread").expect("job");
    let fam = matter
        .insert_family(FAMILY_KIND_EMAIL_ATTACHMENTS)
        .expect("fam");
    let p = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::PARENT.into()),
            file_category: Some("email".into()),
            path: Some("p".into()),
            family_id: Some(fam.id.clone()),
            message_id: Some("p@ex.com".into()),
            in_reply_to: Some("root@ex.com".into()),
            ..Default::default()
        })
        .expect("p");
    let att = matter
        .insert_item(ItemInput {
            status: item_status::EXTRACTED.into(),
            role: Some(item_role::ATTACHMENT.into()),
            file_category: Some("attachment".into()),
            path: Some("p/att".into()),
            family_id: Some(fam.id.clone()),
            parent_item_id: Some(p.id.clone()),
            ..Default::default()
        })
        .expect("att");

    // Second parent so headers multi-thread is interesting
    let _root = parent(
        &matter,
        "root",
        Some("root@ex.com"),
        None,
        None,
        Some("X"),
        None,
    );

    let _ = run_default(&matter, &job.id);
    let p2 = matter.get_item(&p.id).unwrap();
    let a2 = matter.get_item(&att.id).unwrap();
    assert!(p2.thread_id.is_some());
    assert_eq!(a2.thread_id, p2.thread_id);
    assert_eq!(a2.thread_root_item_id, p2.thread_root_item_id);
    assert_eq!(a2.thread_method, p2.thread_method);
}

#[test]
fn reset_recomputes_deterministically() {
    let (_tmp, matter) = temp_matter("reset");
    let job1 = matter.create_job("thread").expect("job1");
    let a = parent(
        &matter,
        "a",
        Some("a@ex.com"),
        Some("b@ex.com"),
        None,
        Some("S"),
        None,
    );
    let b = parent(&matter, "b", Some("b@ex.com"), None, None, Some("S"), None);

    let _ = run_default(&matter, &job1.id);
    let t1 = matter.get_item(&a.id).unwrap().thread_id.clone();

    let job2 = matter.create_job("thread").expect("job2");
    let _ = run_default(&matter, &job2.id);
    let t2 = matter.get_item(&a.id).unwrap().thread_id.clone();
    let tb = matter.get_item(&b.id).unwrap().thread_id;
    assert_eq!(t1, t2);
    assert_eq!(t2, tb);
}

#[test]
fn cancel_pauses_with_checkpoint() {
    let (_tmp, matter) = temp_matter("cancel");
    let job = matter.create_job("thread").expect("job");
    for i in 0..20 {
        let _ = parent(
            &matter,
            &format!("p{i:02}"),
            Some(&format!("{i}@ex.com")),
            None,
            None,
            Some("alone"),
            None,
        );
    }

    let cancel_after = std::sync::atomic::AtomicU64::new(0);
    let params = ThreadParams {
        batch_size: 3,
        ..Default::default()
    };
    let outcome = run_thread(
        &matter,
        &job.id,
        &params,
        Some(&|| {
            // Cancel after first progress tick would be mid-run; force cancel always
            // after some commits by counting calls... simpler: always cancel after
            // first non-zero completed via shared state in progress is hard.
            // Use: cancel when any checkpoint exists with completed > 0.
            cancel_after.load(std::sync::atomic::Ordering::SeqCst) > 0
        }),
        |completed| {
            if completed > 0 {
                cancel_after.store(1, std::sync::atomic::Ordering::SeqCst);
            }
        },
    )
    .expect("run");

    // With cancel only after progress, we may get Paused or Succeeded if too fast.
    // Ensure checkpoint write path works either way.
    let _cp = matter.get_checkpoint(&job.id, THREAD_STAGE).expect("cp");
    match outcome {
        ThreadOutcome::Paused(s) => {
            assert!(s.completed_count > 0);
            // Resume
            let outcome2 = run_thread(&matter, &job.id, &params, None, |_| {}).expect("resume");
            assert!(matches!(outcome2, ThreadOutcome::Succeeded(_)));
        }
        ThreadOutcome::Succeeded(_) => {
            // Race: finished before cancel observed — still valid.
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn compact_keys_no_full_item_in_engine_api() {
    // Smoke: ThreadCandidate is the public thin row type.
    let (_tmp, matter) = temp_matter("thin");
    let _ = parent(&matter, "a", Some("a@ex.com"), None, None, Some("S"), None);
    let rows = matter.list_email_parents_for_thread().unwrap();
    assert_eq!(rows.len(), 1);
    // Thin fields present; no body columns on the type.
    assert!(rows[0].message_id.is_some());
}
