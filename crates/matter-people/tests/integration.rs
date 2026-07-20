//! Integration tests for people_graph (track 0047).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_core::{
    item_status, person_id_for, FilterCondition, FilterSpec, ItemInput, Matter, SCOPE_ENTIRE_MATTER,
};
use matter_people::{
    normalize_participant, run_people_graph, PeopleGraphOutcome, PeopleGraphParams,
    PeopleGraphReport, JOB_KIND_PEOPLE_GRAPH,
};

fn temp_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    let matter = Matter::create(&root, name).expect("create");
    (tmp, matter)
}

fn insert_mail(
    matter: &Matter,
    from: &str,
    to: &[&str],
    cc: &[&str],
    bcc: &[&str],
    sent_at: &str,
) -> String {
    let to_json = serde_json::to_string(to).unwrap();
    let cc_json = serde_json::to_string(cc).unwrap();
    let bcc_json = serde_json::to_string(bcc).unwrap();
    let item = matter
        .insert_item(ItemInput {
            path: Some("msg.eml".into()),
            status: item_status::EXTRACTED.into(),
            from_addr: Some(from.into()),
            to_addrs_json: Some(to_json),
            cc_addrs_json: Some(cc_json),
            bcc_addrs_json: Some(bcc_json),
            sent_at: Some(sent_at.into()),
            ..Default::default()
        })
        .expect("insert");
    item.id
}

fn run_full(matter: &Matter) -> PeopleGraphReport {
    let job = matter.create_job(JOB_KIND_PEOPLE_GRAPH).expect("job");
    let outcome = run_people_graph(matter, &job.id, &PeopleGraphParams::default(), None, |_| {})
        .expect("run");
    match outcome {
        PeopleGraphOutcome::Succeeded(r) => r,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn normalize_units() {
    let d = normalize_participant("John Doe").expect("display");
    assert_eq!(d.identity_kind, "display");
    let x = normalize_participant("/o=Exch/ou=AG/cn=Recipients/cn=jdoe").expect("x500");
    assert_eq!(x.identity_kind, "x500");
    let a = normalize_participant("bob@example.com,").expect("smtp");
    let b = normalize_participant("bob@example.com").expect("smtp");
    assert_eq!(a.normalized_key, b.normalized_key);
}

#[test]
fn a_to_b_c_two_edges_visible() {
    let (_tmp, matter) = temp_matter("edges");
    insert_mail(
        &matter,
        "a@example.com",
        &["b@example.com", "c@example.com"],
        &[],
        &[],
        "2024-03-01T10:00:00Z",
    );
    let report = run_full(&matter);
    assert!(report.people_count >= 3);
    assert_eq!(report.edge_count, 2);

    let edges = matter.list_people_edges(10).expect("edges");
    assert_eq!(edges.len(), 2);
    for e in &edges {
        assert!(e.visible_count >= 1);
        assert_eq!(e.bcc_count, 0);
        assert_ne!(e.from_person_id, e.to_person_id);
    }
}

#[test]
fn self_mail_no_edge() {
    let (_tmp, matter) = temp_matter("self");
    insert_mail(
        &matter,
        "solo@example.com",
        &["solo@example.com"],
        &[],
        &[],
        "2024-03-02T10:00:00Z",
    );
    let report = run_full(&matter);
    assert_eq!(report.edge_count, 0);
    let people = matter.list_people(10).expect("people");
    let solo = people
        .iter()
        .find(|p| p.normalized_key == "solo@example.com")
        .expect("solo");
    assert!(solo.self_mail_count >= 1);
}

#[test]
fn bcc_not_in_visible_count() {
    let (_tmp, matter) = temp_matter("bcc");
    insert_mail(
        &matter,
        "a@example.com",
        &["b@example.com"],
        &[],
        &["hidden@example.com"],
        "2024-03-03T10:00:00Z",
    );
    let _ = run_full(&matter);
    let edges = matter.list_people_edges(10).expect("edges");
    let ab = edges
        .iter()
        .find(|e| {
            e.from_key.as_deref() == Some("a@example.com")
                && e.to_key.as_deref() == Some("b@example.com")
        })
        .expect("a→b");
    assert_eq!(ab.to_count, 1);
    assert_eq!(ab.bcc_count, 0);
    assert_eq!(ab.visible_count, 1);

    let ah = edges
        .iter()
        .find(|e| {
            e.from_key.as_deref() == Some("a@example.com")
                && e.to_key.as_deref() == Some("hidden@example.com")
        })
        .expect("a→hidden bcc edge stored");
    assert_eq!(ah.bcc_count, 1);
    assert_eq!(ah.to_count, 0);
    assert_eq!(ah.visible_count, 0); // BCC excluded from visible
}

#[test]
fn cancel_mid_pass1_resume_matches_clean() {
    const TOTAL_ITEMS: u64 = 6;
    let (_tmp, matter) = temp_matter("resume");
    for i in 0..TOTAL_ITEMS {
        insert_mail(
            &matter,
            "from@example.com",
            &[&format!("to{i}@example.com")],
            &[],
            &[],
            "2024-04-01T00:00:00Z",
        );
    }

    // Clean full run on a copy-like second matter for expected counts.
    let (_tmp2, clean) = temp_matter("resume-clean");
    for i in 0..TOTAL_ITEMS {
        insert_mail(
            &clean,
            "from@example.com",
            &[&format!("to{i}@example.com")],
            &[],
            &[],
            "2024-04-01T00:00:00Z",
        );
    }
    let clean_report = run_full(&clean);

    // Cancel only after at least one Pass-1 item has actually been processed
    // (progress callback fires post-upsert). Pre-item cancel polls stay false.
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag_for_fn = cancel_flag.clone();
    let cancel_flag_for_progress = cancel_flag.clone();
    let job = matter.create_job(JOB_KIND_PEOPLE_GRAPH).expect("job");
    let cancel_fn = move || cancel_flag_for_fn.load(Ordering::SeqCst);
    let cancel: Option<&dyn Fn() -> bool> = Some(&cancel_fn);
    // Small batch so mid-pass pause is deterministic with multi-item fixture.
    let partial_params = PeopleGraphParams {
        batch_size: 1,
        ..PeopleGraphParams::default()
    };
    let o1 = run_people_graph(
        &matter,
        &job.id,
        &partial_params,
        cancel,
        move |completed| {
            if completed >= 1 {
                cancel_flag_for_progress.store(true, Ordering::SeqCst);
            }
        },
    )
    .expect("partial");
    match o1 {
        PeopleGraphOutcome::Paused(s) => {
            assert!(
                s.items_processed > 0,
                "cancel must fire after real Pass1 work, got items_processed=0"
            );
            assert!(
                s.items_processed < TOTAL_ITEMS,
                "expected mid-pass cancel, got items_processed={} (pass1_done={})",
                s.items_processed,
                s.pass1_done
            );
            assert!(!s.pass2_done);
            assert!(!s.pass1_done);
        }
        other => panic!("expected Paused mid Pass1, got {other:?}"),
    }

    // Resume same job: unique upserts + Pass2 rebuild; no double-count.
    let o2 = run_people_graph(
        &matter,
        &job.id,
        &PeopleGraphParams::default(),
        None,
        |_| {},
    )
    .expect("resume");
    let report = match o2 {
        PeopleGraphOutcome::Succeeded(r) => r,
        other => panic!("expected success after resume: {other:?}"),
    };

    assert_eq!(report.people_count, clean_report.people_count);
    assert_eq!(report.edge_count, clean_report.edge_count);
    assert_eq!(
        report.participants_written, clean_report.participants_written,
        "resume must not double-count participants vs clean full run"
    );
    assert_eq!(
        report.items_processed, clean_report.items_processed,
        "resume totals should match clean full run (no double-count)"
    );
    let st = matter.people_graph_status().expect("status");
    assert!(st.is_complete);
    assert_eq!(st.people_count as u64, clean_report.people_count);
    assert_eq!(st.edge_count as u64, clean_report.edge_count);
    assert_eq!(
        st.participant_count as u64,
        clean_report.participants_written
    );
}

#[test]
fn filter_by_person_and_display() {
    let (_tmp, matter) = temp_matter("filter");
    let id = insert_mail(
        &matter,
        "John Doe",
        &["alice@example.com"],
        &[],
        &[],
        "2024-05-01T00:00:00Z",
    );
    let _ = run_full(&matter);

    let john = normalize_participant("John Doe").unwrap();
    let pid = person_id_for(&john.identity_kind, &john.normalized_key);

    let mut spec = FilterSpec {
        conditions: vec![FilterCondition {
            field: "person_id".into(),
            op: "eq".into(),
            value: Some(serde_json::Value::String(pid)),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    spec.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter
        .list_items_filtered_thin(&spec, 50, 0)
        .expect("filter");
    assert!(rows.iter().any(|r| r.id == id));

    let mut key_spec = FilterSpec {
        conditions: vec![FilterCondition {
            field: "participant_key".into(),
            op: "eq".into(),
            value: Some(serde_json::Value::String("alice@example.com".into())),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    key_spec.scope = SCOPE_ENTIRE_MATTER.into();
    let rows2 = matter
        .list_items_filtered_thin(&key_spec, 50, 0)
        .expect("key");
    assert!(rows2.iter().any(|r| r.id == id));
}

#[test]
fn audit_start_complete() {
    let (_tmp, matter) = temp_matter("audit");
    insert_mail(
        &matter,
        "a@example.com",
        &["b@example.com"],
        &[],
        &[],
        "2024-06-01T00:00:00Z",
    );
    let _ = run_full(&matter);
    let mut stmt = matter
        .connection()
        .prepare(
            "SELECT action, params_json FROM audit_events \
             WHERE action IN ('people_graph.start', 'people_graph.complete') \
             ORDER BY seq ASC",
        )
        .expect("prep");
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("q")
        .map(|r| r.expect("row"))
        .collect();
    assert!(rows.iter().any(|(a, _)| a == "people_graph.start"));
    assert!(rows.iter().any(|(a, _)| a == "people_graph.complete"));

    let start_payload = rows
        .iter()
        .find(|(a, _)| a == "people_graph.start")
        .map(|(_, p)| p.as_str())
        .expect("start payload");
    let start_json: serde_json::Value =
        serde_json::from_str(start_payload).expect("start params_json");
    assert!(
        start_json
            .get("fingerprint")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty()),
        "people_graph.start must include fingerprint: {start_json}"
    );
    assert!(start_json.get("params").is_some());
    assert!(start_json.get("resume").is_some());
    assert!(start_json.get("reset").is_some());
}

#[test]
fn include_entity_emails_true_fails_closed_at_run() {
    let (_tmp, matter) = temp_matter("entity-emails-fail");
    insert_mail(
        &matter,
        "a@example.com",
        &["b@example.com"],
        &[],
        &[],
        "2024-06-01T00:00:00Z",
    );
    let job = matter.create_job(JOB_KIND_PEOPLE_GRAPH).expect("job");
    let params = PeopleGraphParams {
        include_entity_emails: true,
        ..PeopleGraphParams::default()
    };
    let err = run_people_graph(&matter, &job.id, &params, None, |_| {}).expect_err("must fail");
    assert!(
        err.to_string().contains("include_entity_emails"),
        "got: {err}"
    );
}

#[test]
fn include_entity_emails_false_succeeds() {
    let (_tmp, matter) = temp_matter("entity-emails-ok");
    insert_mail(
        &matter,
        "a@example.com",
        &["b@example.com"],
        &[],
        &[],
        "2024-06-01T00:00:00Z",
    );
    let job = matter.create_job(JOB_KIND_PEOPLE_GRAPH).expect("job");
    let params = PeopleGraphParams {
        include_entity_emails: false,
        ..PeopleGraphParams::default()
    };
    let outcome = run_people_graph(&matter, &job.id, &params, None, |_| {}).expect("run");
    match outcome {
        PeopleGraphOutcome::Succeeded(r) => {
            assert!(r.people_count >= 2);
            assert_eq!(r.edge_count, 1);
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
}
