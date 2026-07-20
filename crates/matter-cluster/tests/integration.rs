//! Integration tests for concept_cluster (track 0048).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_cluster::{
    run_concept_cluster, ConceptClusterOutcome, ConceptClusterParams, ConceptClusterReport,
    JOB_KIND_CONCEPT_CLUSTER, METHOD_TFIDF_KMEANS_V1,
};
use matter_core::{
    item_status, FilterCondition, FilterSpec, ItemInput, Matter, SCOPE_ENTIRE_MATTER,
};

fn temp_matter(name: &str) -> (tempfile::TempDir, Matter) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8");
    let matter = Matter::create(&root, name).expect("create");
    (tmp, matter)
}

fn insert_text(matter: &Matter, path: &str, body: &str) -> String {
    let digest = matter.put_bytes(body.as_bytes()).expect("cas");
    let item = matter
        .insert_item(ItemInput {
            path: Some(path.into()),
            status: item_status::EXTRACTED.into(),
            text_sha256: Some(digest),
            subject: Some(path.into()),
            ..Default::default()
        })
        .expect("insert");
    item.id
}

fn test_params(k: u32) -> ConceptClusterParams {
    ConceptClusterParams {
        k,
        seed: 42,
        min_df: 1, // small corpora
        max_df_ratio: 1.0,
        max_vocab: 5000,
        label_terms: 5,
        max_docs: 50_000,
        reset: true,
        ..ConceptClusterParams::default()
    }
}

fn run_full(matter: &Matter, params: &ConceptClusterParams) -> ConceptClusterReport {
    let job = matter.create_job(JOB_KIND_CONCEPT_CLUSTER).expect("job");
    let outcome = run_concept_cluster(matter, &job.id, params, None, |_| {}).expect("run");
    match outcome {
        ConceptClusterOutcome::Succeeded(r) => r,
        other => panic!("unexpected {other:?}"),
    }
}

/// Multi-topic synthetic bodies.
fn invoice_body(i: usize) -> String {
    format!(
        "Invoice number {i} payment overdue vendor accounts payable remittance wire transfer billing cycle. \
         The vendor invoice requires payment before the deadline. Accounts payable processes vendor remittance."
    )
}

fn clinical_body(i: usize) -> String {
    format!(
        "Patient clinical dosage protocol trial {i} pharmaceutical regimen laboratory results. \
         Clinical trial dosage titration patient outcomes laboratory biomarkers. Pharmaceutical protocol."
    )
}

fn sports_body(i: usize) -> String {
    format!(
        "Championship tournament soccer football league standings {i} goalkeeper striker midfielder. \
         League standings tournament championship match referee stadium attendance. Football soccer goals."
    )
}

fn item_cluster_for_path(matter: &Matter, path: &str) -> Option<String> {
    let id: String = matter
        .connection()
        .query_row(
            "SELECT id FROM items WHERE matter_id = ?1 AND path = ?2",
            [matter.id(), path],
            |row| row.get(0),
        )
        .ok()?;
    matter.get_item(&id).ok()?.concept_cluster_id
}

/// Majority cluster id among paths with a given prefix (e.g. "inv").
fn majority_cluster(matter: &Matter, prefix: &str, n: usize) -> String {
    let mut counts = std::collections::HashMap::new();
    for i in 0..n {
        let path = format!("{prefix}{i}.txt");
        if let Some(cid) = item_cluster_for_path(matter, &path) {
            *counts.entry(cid).or_insert(0usize) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(id, _)| id)
        .expect("theme must have majority cluster")
}

#[test]
fn multi_topic_separation() {
    let (_tmp, matter) = temp_matter("topics");
    for i in 0..6 {
        insert_text(&matter, &format!("inv{i}.txt"), &invoice_body(i));
    }
    for i in 0..6 {
        insert_text(&matter, &format!("clin{i}.txt"), &clinical_body(i));
    }
    for i in 0..6 {
        insert_text(&matter, &format!("sport{i}.txt"), &sports_body(i));
    }

    let report = run_full(&matter, &test_params(3));
    assert_eq!(report.method, METHOD_TFIDF_KMEANS_V1);
    assert!(report.cluster_count >= 2, "expected multiple clusters");
    assert!(report.built_at.len() > 10);
    assert_eq!(report.k_requested, 3);

    let clusters = matter.list_concept_clusters(&report.set_id).expect("list");
    assert_eq!(clusters.len() as u64, report.cluster_count);
    assert!(clusters.iter().all(|c| c.item_count > 0));

    // Labels should not be identical across clusters for distinct themes.
    if clusters.len() >= 2 {
        let t0: Vec<String> =
            serde_json::from_str(&clusters[0].label_terms_json).unwrap_or_default();
        let t1: Vec<String> =
            serde_json::from_str(&clusters[1].label_terms_json).unwrap_or_default();
        let top_same = t0.first() == t1.first() && !t0.is_empty();
        let jaccard = {
            use std::collections::BTreeSet;
            let a: BTreeSet<_> = t0.iter().take(5).collect();
            let b: BTreeSet<_> = t1.iter().take(5).collect();
            let inter = a.intersection(&b).count() as f64;
            let union = a.union(&b).count() as f64;
            if union == 0.0 {
                0.0
            } else {
                inter / union
            }
        };
        assert!(
            !top_same || jaccard < 1.0,
            "labels should differ: {:?} vs {:?} j={jaccard}",
            t0,
            t1
        );
    }

    // Theme purity: each topic's majority cluster must be distinct (DoD-6 multi-topic).
    let inv_maj = majority_cluster(&matter, "inv", 6);
    let clin_maj = majority_cluster(&matter, "clin", 6);
    let sport_maj = majority_cluster(&matter, "sport", 6);
    assert_ne!(
        inv_maj, clin_maj,
        "invoice and clinical themes must not share majority cluster"
    );
    assert_ne!(
        inv_maj, sport_maj,
        "invoice and sports themes must not share majority cluster"
    );
    assert_ne!(
        clin_maj, sport_maj,
        "clinical and sports themes must not share majority cluster"
    );
    // Majority purity: at least 4/6 of each theme in its majority cluster.
    for (prefix, maj) in [
        ("inv", &inv_maj),
        ("clin", &clin_maj),
        ("sport", &sport_maj),
    ] {
        let mut same = 0usize;
        for i in 0..6 {
            if item_cluster_for_path(&matter, &format!("{prefix}{i}.txt")).as_ref() == Some(maj) {
                same += 1;
            }
        }
        assert!(
            same >= 4,
            "theme {prefix} purity too low: {same}/6 in majority {maj}"
        );
    }
}

#[test]
fn l2_length_bias_same_topic_co_cluster() {
    let (_tmp, matter) = temp_matter("l2");
    // Identical term support, different magnitude — L2 must keep them co-clustered
    // (without L2, Euclidean k-means splits by length).
    let base = "Invoice payment vendor remittance accounts payable billing wire transfer overdue";
    let short = base.to_string();
    let long = format!("{} ", base).repeat(40);
    for i in 0..5 {
        insert_text(&matter, &format!("s{i}.txt"), &short);
    }
    for i in 0..5 {
        insert_text(&matter, &format!("l{i}.txt"), &long);
    }
    for i in 0..5 {
        insert_text(&matter, &format!("c{i}.txt"), &clinical_body(i));
    }

    let report = run_full(&matter, &test_params(2));
    assert!(report.cluster_count >= 1);

    let clusters = matter.list_concept_clusters(&report.set_id).expect("list");
    let mut inv_cluster_ids = Vec::new();
    for path_prefix in ["s", "l"] {
        for i in 0..5 {
            let path = format!("{path_prefix}{i}.txt");
            let id: String = matter
                .connection()
                .query_row(
                    "SELECT id FROM items WHERE matter_id = ?1 AND path = ?2",
                    [matter.id(), path.as_str()],
                    |row| row.get(0),
                )
                .expect("item");
            let item = matter.get_item(&id).expect("get");
            if let Some(cid) = item.concept_cluster_id {
                inv_cluster_ids.push(cid);
            }
        }
    }
    assert_eq!(inv_cluster_ids.len(), 10);
    let mut counts = std::collections::HashMap::new();
    for c in &inv_cluster_ids {
        *counts.entry(c.clone()).or_insert(0) += 1;
    }
    let max = counts.values().copied().max().unwrap_or(0);
    assert!(
        max >= 8,
        "expected long+short same topic co-cluster (L2), counts={counts:?} clusters={clusters:?}"
    );
}

#[test]
fn header_disclaimer_not_mega_cluster_label() {
    let (_tmp, matter) = temp_matter("headers");
    let disclaimer = "This message is privileged and confidential. If you are not the intended recipient, please delete this email. Unauthorized disclosure is prohibited.";
    for i in 0..5 {
        let body = format!(
            "From: sender{i}@corp.example\nTo: recv@corp.example\nSubject: RE: matter\nSent: Monday\n\n{}\n\n{}",
            invoice_body(i),
            disclaimer
        );
        insert_text(&matter, &format!("mail_inv{i}.eml"), &body);
    }
    for i in 0..5 {
        let body = format!(
            "From: sender{i}@corp.example\nTo: recv@corp.example\nSubject: RE: matter\nSent: Monday\n\n{}\n\n{}",
            clinical_body(i),
            disclaimer
        );
        insert_text(&matter, &format!("mail_clin{i}.eml"), &body);
    }

    let report = run_full(&matter, &test_params(2));
    let clusters = matter.list_concept_clusters(&report.set_id).expect("list");
    assert!(report.cluster_count >= 1);
    for c in &clusters {
        let lower = c.label.to_lowercase();
        // Must not be dominated by header tokens.
        assert!(
            !lower.starts_with("from ")
                && !lower.contains("mailto")
                && !lower.split_whitespace().take(3).all(|t| {
                    matches!(t, "from" | "sent" | "subject" | "to" | "cc" | "privileged")
                }),
            "header-dominated label: {}",
            c.label
        );
    }
}

#[test]
fn empty_cluster_drop_dense_ordinals() {
    let (_tmp, matter) = temp_matter("emptyk");
    // Few docs, high k → empties dropped (k clamped to n_docs but still may drop).
    for i in 0..4 {
        insert_text(&matter, &format!("a{i}.txt"), &invoice_body(i));
    }
    for i in 0..4 {
        insert_text(&matter, &format!("b{i}.txt"), &clinical_body(i));
    }
    let report = run_full(&matter, &test_params(10));
    // Requested k=10 but only 8 docs → cluster_count < requested k.
    assert!(
        report.cluster_count < report.k_requested,
        "expected empty drop / clamp: cluster_count={} k={}",
        report.cluster_count,
        report.k_requested
    );
    assert!(report.cluster_count <= report.clustered_count);
    let clusters = matter.list_concept_clusters(&report.set_id).expect("list");
    assert_eq!(clusters.len() as u64, report.cluster_count);
    assert!(clusters.iter().all(|c| c.item_count > 0));
    let mut ordinals: Vec<i64> = clusters.iter().map(|c| c.ordinal).collect();
    ordinals.sort();
    // Dense: 0..n-1
    for (i, o) in ordinals.iter().enumerate() {
        assert_eq!(*o, i as i64, "ordinals={ordinals:?}");
    }
}

#[test]
fn empty_vocabulary_fails_closed() {
    let (_tmp, matter) = temp_matter("emptyvocab");
    // Single short doc + min_df=2 → zero vocabulary after DF filter.
    insert_text(&matter, "solo.txt", "uniqueone-off-token-xyz");
    let mut p = test_params(2);
    p.min_df = 2;
    let job = matter.create_job(JOB_KIND_CONCEPT_CLUSTER).expect("job");
    let err = run_concept_cluster(&matter, &job.id, &p, None, |_| {}).expect_err("fail closed");
    let msg = err.to_string();
    assert!(
        msg.contains("vocabulary") || msg.contains("usable") || msg.contains("fail closed"),
        "msg={msg}"
    );
    let status = matter.concept_cluster_status("default").expect("status");
    assert!(
        !status.is_complete,
        "must not publish built_at on empty vocab"
    );
}

#[test]
fn fingerprint_detects_content_change() {
    let (_tmp, matter) = temp_matter("fp");
    for i in 0..4 {
        insert_text(&matter, &format!("i{i}.txt"), &invoice_body(i));
        insert_text(&matter, &format!("c{i}.txt"), &clinical_body(i));
    }
    let mut p = test_params(2);
    p.reset = true;
    let r1 = run_full(&matter, &p);
    // Change one document body (same path/count).
    let id: String = matter
        .connection()
        .query_row(
            "SELECT id FROM items WHERE matter_id = ?1 AND path = ?2",
            [matter.id(), "i0.txt"],
            |row| row.get(0),
        )
        .expect("id");
    let new_digest = matter.put_bytes(clinical_body(99).as_bytes()).expect("cas");
    matter
        .connection()
        .execute(
            "UPDATE items SET text_sha256 = ?1 WHERE id = ?2",
            [new_digest.as_str(), id.as_str()],
        )
        .expect("update");
    // reset:false must rebuild because inventory digest changed.
    p.reset = false;
    let job = matter.create_job(JOB_KIND_CONCEPT_CLUSTER).expect("job");
    let outcome = run_concept_cluster(&matter, &job.id, &p, None, |_| {}).expect("run");
    match outcome {
        ConceptClusterOutcome::Succeeded(r2) => {
            assert_ne!(
                r1.fingerprint, r2.fingerprint,
                "content change must change fingerprint"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let status = matter.concept_cluster_status("default").expect("status");
    assert!(status.is_complete);
    assert!(!status.is_stale, "after rebuild status must not be stale");
}

#[test]
fn cas_read_failure_fails_closed() {
    let (_tmp, matter) = temp_matter("casfail");
    insert_text(&matter, "ok.txt", &invoice_body(0));
    // Plant candidate with non-existent CAS digest.
    matter
        .insert_item(ItemInput {
            path: Some("missing.txt".into()),
            status: item_status::EXTRACTED.into(),
            text_sha256: Some(
                "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
            ),
            subject: Some("missing".into()),
            ..Default::default()
        })
        .expect("insert");
    let p = test_params(2);
    let job = matter.create_job(JOB_KIND_CONCEPT_CLUSTER).expect("job");
    let err = run_concept_cluster(&matter, &job.id, &p, None, |_| {}).expect_err("cas fail");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("cas")
            || msg.contains("not found")
            || msg.contains("blob")
            || msg.contains("read"),
        "msg={msg}"
    );
    let status = matter.concept_cluster_status("default").expect("status");
    assert!(!status.is_complete);
}

#[test]
fn determinism_same_seed() {
    let (_tmp, matter) = temp_matter("det");
    for i in 0..5 {
        insert_text(&matter, &format!("i{i}.txt"), &invoice_body(i));
        insert_text(&matter, &format!("c{i}.txt"), &clinical_body(i));
    }
    let p = test_params(3);
    let r1 = run_full(&matter, &p);
    let labels1: Vec<String> = matter
        .list_concept_clusters(&r1.set_id)
        .unwrap()
        .into_iter()
        .map(|c| c.label)
        .collect();

    let r2 = run_full(&matter, &p);
    let labels2: Vec<String> = matter
        .list_concept_clusters(&r2.set_id)
        .unwrap()
        .into_iter()
        .map(|c| c.label)
        .collect();
    assert_eq!(r1.cluster_count, r2.cluster_count);
    assert_eq!(labels1, labels2);
    assert_eq!(r1.fingerprint, r2.fingerprint);
}

#[test]
fn max_docs_fail_closed() {
    let (_tmp, matter) = temp_matter("cap");
    for i in 0..3 {
        insert_text(&matter, &format!("d{i}.txt"), &invoice_body(i));
    }
    let mut p = test_params(2);
    p.max_docs = 2;
    let job = matter.create_job(JOB_KIND_CONCEPT_CLUSTER).expect("job");
    let err = run_concept_cluster(&matter, &job.id, &p, None, |_| {}).expect_err("fail closed");
    let msg = err.to_string();
    assert!(
        msg.contains("max_docs") || msg.contains("fail closed"),
        "msg={msg}"
    );
    // No complete built_at.
    let status = matter.concept_cluster_status("default").expect("status");
    assert!(!status.is_complete);
}

#[test]
fn cancel_phase_a_no_built_at() {
    let (_tmp, matter) = temp_matter("cancel");
    for i in 0..40 {
        insert_text(&matter, &format!("d{i}.txt"), &invoice_body(i));
    }
    // Mid-stream cancel: arm after a few progress callbacks (docs processed).
    let progress_hits = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let flag = Arc::new(AtomicBool::new(false));
    let flag2 = flag.clone();
    let hits2 = progress_hits.clone();
    let cancel = move || flag2.load(Ordering::SeqCst);
    let mut p = test_params(3);
    p.batch_size = 1;
    let job = matter.create_job(JOB_KIND_CONCEPT_CLUSTER).expect("job");
    let outcome = run_concept_cluster(&matter, &job.id, &p, Some(&cancel), |completed| {
        hits2.store(completed, Ordering::SeqCst);
        // Cancel after some Phase A progress so we exercise mid-stream path.
        if completed >= 3 {
            flag.store(true, Ordering::SeqCst);
        }
    })
    .expect("run");
    match outcome {
        ConceptClusterOutcome::Paused(s) => {
            assert!(
                s.completed_count >= 3 || progress_hits.load(Ordering::SeqCst) >= 3,
                "expected mid-stream pause after progress, summary={s:?}"
            );
        }
        ConceptClusterOutcome::Succeeded(_) => {
            panic!("expected Paused when cancel fires mid Phase A");
        }
        ConceptClusterOutcome::Failed { message, .. } => panic!("failed: {message}"),
    }
    let status = matter.concept_cluster_status("default").expect("status");
    assert!(
        !status.is_complete,
        "cancel during A must not publish built_at"
    );
    // No complete membership published for default set.
    let n: i64 = matter
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM item_concept_membership WHERE matter_id = ?1",
            [matter.id()],
            |row| row.get(0),
        )
        .expect("count");
    assert_eq!(n, 0, "cancel during A must not leave membership rows");
}

#[test]
fn filter_returns_membership_items() {
    let (_tmp, matter) = temp_matter("filter");
    for i in 0..5 {
        insert_text(&matter, &format!("i{i}.txt"), &invoice_body(i));
        insert_text(&matter, &format!("c{i}.txt"), &clinical_body(i));
    }
    let report = run_full(&matter, &test_params(2));
    let clusters = matter.list_concept_clusters(&report.set_id).expect("list");
    assert!(!clusters.is_empty());
    let cid = clusters[0].id.clone();

    let mut spec = FilterSpec {
        conditions: vec![FilterCondition {
            field: "concept_cluster_id".into(),
            op: "eq".into(),
            value: Some(serde_json::Value::String(cid.clone())),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    spec.scope = SCOPE_ENTIRE_MATTER.into();
    let rows = matter
        .list_items_filtered_thin(&spec, 100, 0)
        .expect("filter");
    assert_eq!(rows.len() as i64, clusters[0].item_count);
    assert!(!rows.is_empty());

    let mut has = FilterSpec {
        conditions: vec![FilterCondition {
            field: "has_concept_cluster".into(),
            op: "eq".into(),
            value: Some(serde_json::Value::Bool(true)),
            values: None,
            start: None,
            end: None,
        }],
        ..FilterSpec::default()
    };
    has.scope = SCOPE_ENTIRE_MATTER.into();
    let all = matter.list_items_filtered_thin(&has, 100, 0).expect("has");
    assert_eq!(all.len() as u64, report.clustered_count);
}
