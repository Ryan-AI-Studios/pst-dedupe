//! Resumable `concept_cluster` job: Phase A features + Phase B atomic replace.

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Instant;

use matter_core::{
    sha256_hex, AuditEventInput, ConceptClusterWrite, ConceptMembershipWrite, Matter,
    ReplaceConceptClusterSetInput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ctfidf::cluster_labels;
use crate::error::{ClusterError, Result};
use crate::kmeans::kmeans;
use crate::params::ConceptClusterParams;
use crate::prep::{prep_fingerprint_token, strip_headers_and_disclaimers};
use crate::stopwords::STOPWORDS_VERSION;
use crate::tfidf::{build_matrix, build_vocabulary};
use crate::tokenize::{term_counts, tokenize};

/// Job kind string for process-runner.
pub const JOB_KIND_CONCEPT_CLUSTER: &str = "concept_cluster";
/// Checkpoint stage name.
pub const CONCEPT_CLUSTER_STAGE: &str = "concept_cluster";
/// Engine version token embedded in fingerprint.
pub const CONCEPT_CLUSTER_ENGINE_VERSION: &str = "concept_cluster_v1";
/// Frozen method string written to every set row.
pub const METHOD_TFIDF_KMEANS_V1: &str = "tfidf_kmeans_v1";

/// Summary after run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptClusterSummary {
    pub completed_count: u64,
    pub candidate_count: u64,
    pub clustered_count: u64,
    pub cluster_count: u64,
    pub k_requested: u64,
    pub skipped_empty_text: u64,
    pub phase_a_done: bool,
    pub phase_b_done: bool,
}

/// Full success payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConceptClusterReport {
    pub candidate_count: u64,
    pub clustered_count: u64,
    pub cluster_count: u64,
    pub k_requested: u64,
    pub set_id: String,
    pub set_name: String,
    pub method: String,
    pub fingerprint: String,
    pub built_at: String,
}

/// Outcome of [`run_concept_cluster`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConceptClusterOutcome {
    Succeeded(ConceptClusterReport),
    Paused(ConceptClusterSummary),
    Failed {
        message: String,
        summary: ConceptClusterSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    #[serde(default)]
    phase: String,
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    candidate_count: u64,
    skipped_empty_text: u64,
    #[serde(default)]
    phase_a_done: bool,
    /// Serialized term counts: Vec<(item_id, BTreeMap term→count)>
    #[serde(default)]
    docs_json: Option<String>,
    params: serde_json::Value,
}

/// Params portion of the fingerprint (excludes inventory).
pub fn concept_cluster_params_fingerprint_input(
    params: &ConceptClusterParams,
    candidate_count: u64,
) -> String {
    format!(
        "{METHOD_TFIDF_KMEANS_V1}|k={}|seed={}|scope={}|min_df={}|max_df_ratio={}|max_vocab={}|\
         label_terms={}|max_docs={}|max_text_bytes={}|drop_digits={}|candidates={}|engine={}|\
         prep={}|stop={}",
        params.k,
        params.seed,
        params.scope,
        params.min_df,
        params.max_df_ratio,
        params.max_vocab,
        params.label_terms,
        params.max_docs,
        params.max_text_bytes,
        params.drop_digits,
        candidate_count,
        CONCEPT_CLUSTER_ENGINE_VERSION,
        prep_fingerprint_token(),
        STOPWORDS_VERSION,
    )
}

/// Full fingerprint: `sha256(params_input)|inventory_digest` so content changes invalidate skip.
///
/// `inventory_digest` is sha256 of ordered `item_id\\0text_sha256\\n` lines (candidate set).
pub fn concept_cluster_fingerprint(
    params: &ConceptClusterParams,
    candidate_count: u64,
    inventory_digest: &str,
) -> Result<String> {
    let params_hex =
        sha256_hex(concept_cluster_params_fingerprint_input(params, candidate_count).as_bytes());
    Ok(format!("{params_hex}:{inventory_digest}"))
}

/// Build inventory digest from candidate id + text digests (order-sensitive; callers must sort by id).
pub fn inventory_digest_from_candidates<'a>(
    candidates: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut buf = String::new();
    for (id, text_sha) in candidates {
        buf.push_str(id);
        buf.push('\0');
        buf.push_str(text_sha);
        buf.push('\n');
    }
    sha256_hex(buf.as_bytes())
}

/// Extract the inventory digest portion from a stored fingerprint (`params_hex:inv_hex`).
pub fn inventory_digest_from_fingerprint(fingerprint: &str) -> Option<&str> {
    fingerprint.rsplit_once(':').map(|(_, inv)| inv)
}

/// Run concept clustering for the runner-created `job_id`.
///
/// Does **not** call `create_job`. Honors `cancel` during Phase A.
/// `built_at` is set only after Phase B commits.
pub fn run_concept_cluster(
    matter: &Matter,
    job_id: &str,
    params: &ConceptClusterParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<ConceptClusterOutcome> {
    let started = Instant::now();
    let result = run_body(matter, job_id, params, cancel, &progress);

    match &result {
        Ok(ConceptClusterOutcome::Succeeded(r)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "concept_cluster.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "candidate_count": r.candidate_count,
                    "clustered_count": r.clustered_count,
                    "cluster_count": r.cluster_count,
                    "k": r.k_requested,
                    "set_id": r.set_id,
                    "set_name": r.set_name,
                    "method": r.method,
                    "fingerprint": r.fingerprint,
                    "built_at": r.built_at,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                let message = format!("audit complete failed: {e}");
                let summary = summary_from_report(r);
                let _ = matter.append_audit(AuditEventInput {
                    actor: "system".into(),
                    action: "concept_cluster.fail".into(),
                    entity: format!("job:{job_id}"),
                    params_json: fail_audit_params(&message, &summary).to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                });
                return Ok(ConceptClusterOutcome::Failed { message, summary });
            }
        }
        Ok(ConceptClusterOutcome::Paused(_)) => {}
        Ok(ConceptClusterOutcome::Failed { message, summary }) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "concept_cluster.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(message, summary).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
        Err(e) => {
            let empty = ConceptClusterSummary::default();
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "concept_cluster.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(&e.to_string(), &empty).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
    }

    result
}

fn summary_from_report(r: &ConceptClusterReport) -> ConceptClusterSummary {
    ConceptClusterSummary {
        completed_count: r.clustered_count,
        candidate_count: r.candidate_count,
        clustered_count: r.clustered_count,
        cluster_count: r.cluster_count,
        k_requested: r.k_requested,
        skipped_empty_text: 0,
        phase_a_done: true,
        phase_b_done: true,
    }
}

fn fail_audit_params(message: &str, summary: &ConceptClusterSummary) -> serde_json::Value {
    json!({
        "error": message,
        "candidate_count": summary.candidate_count,
        "clustered_count": summary.clustered_count,
        "cluster_count": summary.cluster_count,
        "k": summary.k_requested,
    })
}

fn run_body(
    matter: &Matter,
    job_id: &str,
    params: &ConceptClusterParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<ConceptClusterOutcome> {
    params.validate()?;

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate()?;
    let params_json = serde_json::to_value(&effective)
        .map_err(|e| ClusterError::other(format!("serialize params: {e}")))?;

    let candidate_count = matter.count_concept_cluster_candidates()?;
    if candidate_count > effective.max_docs {
        return Err(ClusterError::MaxDocsExceeded {
            candidate_count,
            max_docs: effective.max_docs,
        });
    }

    // Content-aware inventory (ordered by item id) so same-count text changes rebuild.
    let inventory_digest = {
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = matter.list_concept_cluster_candidates(after.as_deref(), 500)?;
            if page.is_empty() {
                break;
            }
            for c in &page {
                pairs.push((c.id.clone(), c.text_sha256.clone()));
            }
            after = page.last().map(|c| c.id.clone());
        }
        inventory_digest_from_candidates(pairs.iter().map(|(a, b)| (a.as_str(), b.as_str())))
    };
    let fingerprint = concept_cluster_fingerprint(&effective, candidate_count, &inventory_digest)?;
    let resuming = prior
        .as_ref()
        .is_some_and(|p| p.completed_count > 0 || p.phase_a_done);

    // Soft skip when reset:false and fingerprint matches complete set.
    if !effective.reset {
        if let Some(set) = matter.get_concept_cluster_set(&effective.set_name)? {
            if set.built_at.is_some() && set.fingerprint.as_deref() == Some(fingerprint.as_str()) {
                matter.append_audit(AuditEventInput {
                    actor: "system".into(),
                    action: "concept_cluster.start".into(),
                    entity: format!("job:{job_id}"),
                    params_json: json!({
                        "params": params_json,
                        "skip": "fingerprint_match",
                        "fingerprint": fingerprint,
                        "inventory_digest": inventory_digest,
                    })
                    .to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                })?;
                return Ok(ConceptClusterOutcome::Succeeded(ConceptClusterReport {
                    candidate_count,
                    clustered_count: set.item_count as u64,
                    cluster_count: set.cluster_count as u64,
                    k_requested: set.k as u64,
                    set_id: set.id,
                    set_name: set.name,
                    method: set.method,
                    fingerprint,
                    built_at: set.built_at.unwrap_or_default(),
                }));
            }
        }
    }

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "concept_cluster.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "resume": resuming,
            "reset": effective.reset,
            "fingerprint": fingerprint,
            "method": METHOD_TFIDF_KMEANS_V1,
            "engine_version": CONCEPT_CLUSTER_ENGINE_VERSION,
            "candidate_count": candidate_count,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    run_inner(
        matter,
        job_id,
        &effective,
        cancel,
        progress,
        &params_json,
        prior,
        candidate_count,
        fingerprint,
    )
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, CONCEPT_CLUSTER_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(ClusterError::other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &ConceptClusterParams,
    prior: Option<&CheckpointCursor>,
) -> Result<ConceptClusterParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<ConceptClusterParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(ClusterError::other(format!(
                        "checkpoint params unreadable: {e}"
                    )));
                }
            }
        }
    }
    Ok(call_site.clone())
}

#[allow(clippy::too_many_arguments)]
fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &ConceptClusterParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
    candidate_count: u64,
    fingerprint: String,
) -> Result<ConceptClusterOutcome> {
    let mut summary = ConceptClusterSummary {
        candidate_count,
        k_requested: u64::from(params.k),
        ..Default::default()
    };

    // Phase A: extract term counts (may recompute from scratch for simplicity).
    // If prior has phase_a_done + docs_json, reuse; else rebuild.
    let mut docs: Vec<(String, BTreeMap<String, u32>)> = Vec::new();
    let mut phase_a_done = false;

    if let Some(ref p) = prior {
        if p.phase_a_done {
            if let Some(ref raw) = p.docs_json {
                match serde_json::from_str::<Vec<(String, BTreeMap<String, u32>)>>(raw) {
                    Ok(d) => {
                        docs = d;
                        phase_a_done = true;
                        summary.completed_count = p.completed_count;
                        summary.skipped_empty_text = p.skipped_empty_text;
                        summary.phase_a_done = true;
                    }
                    Err(_) => {
                        // Fall through to recompute.
                    }
                }
            }
        }
    }

    if !phase_a_done {
        // Prefer simple full recompute of Phase A (spec P0).
        docs.clear();
        summary.skipped_empty_text = 0;
        summary.completed_count = 0;
        let mut after_id: Option<String> = None;
        loop {
            if cancel.map(|c| c()).unwrap_or(false) {
                write_checkpoint(
                    matter,
                    job_id,
                    &CheckpointCursor {
                        phase: "a".into(),
                        cursor_index: summary.completed_count,
                        last_item_id: after_id.clone(),
                        completed_count: summary.completed_count,
                        candidate_count,
                        skipped_empty_text: summary.skipped_empty_text,
                        phase_a_done: false,
                        docs_json: None,
                        params: params_json.clone(),
                    },
                )?;
                return Ok(ConceptClusterOutcome::Paused(summary));
            }

            let page = matter.list_concept_cluster_candidates(
                after_id.as_deref(),
                u64::from(params.batch_size),
            )?;
            if page.is_empty() {
                break;
            }
            for cand in &page {
                if cancel.map(|c| c()).unwrap_or(false) {
                    write_checkpoint(
                        matter,
                        job_id,
                        &CheckpointCursor {
                            phase: "a".into(),
                            cursor_index: summary.completed_count,
                            last_item_id: after_id.clone(),
                            completed_count: summary.completed_count,
                            candidate_count,
                            skipped_empty_text: summary.skipped_empty_text,
                            phase_a_done: false,
                            docs_json: None,
                            params: params_json.clone(),
                        },
                    )?;
                    return Ok(ConceptClusterOutcome::Paused(summary));
                }

                match load_text_capped(matter, &cand.text_sha256, params.max_text_bytes) {
                    Ok(text) => {
                        let cleaned = strip_headers_and_disclaimers(&text);
                        let tokens = tokenize(&cleaned, params.drop_digits);
                        let counts = term_counts(&tokens);
                        if counts.is_empty() {
                            summary.skipped_empty_text += 1;
                        } else {
                            docs.push((cand.id.clone(), counts));
                        }
                    }
                    Err(e) => {
                        // Fail closed on CAS read errors (not silent empty-text skip).
                        return Err(ClusterError::CasReadFailed {
                            item_id: cand.id.clone(),
                            message: e.to_string(),
                        });
                    }
                }
                summary.completed_count += 1;
                after_id = Some(cand.id.clone());
                progress(summary.completed_count);
            }
        }
        summary.phase_a_done = true;

        // Checkpoint after Phase A (no membership yet — not complete).
        let docs_json = serde_json::to_string(&docs)
            .map_err(|e| ClusterError::other(format!("serialize docs: {e}")))?;
        write_checkpoint(
            matter,
            job_id,
            &CheckpointCursor {
                phase: "b".into(),
                cursor_index: summary.completed_count,
                last_item_id: after_id,
                completed_count: summary.completed_count,
                candidate_count,
                skipped_empty_text: summary.skipped_empty_text,
                phase_a_done: true,
                docs_json: Some(docs_json),
                params: params_json.clone(),
            },
        )?;
    }

    if cancel.map(|c| c()).unwrap_or(false) {
        return Ok(ConceptClusterOutcome::Paused(summary));
    }

    // Phase B: TF–IDF → L2 → k-means → drop empty → c-TF-IDF → atomic replace.
    // Fail closed: never publish built_at with zero assignments / empty vocab.
    if docs.is_empty() {
        return Err(ClusterError::NoUsableFeatures {
            candidate_count,
            skipped_empty: summary.skipped_empty_text,
        });
    }

    let count_maps: Vec<BTreeMap<String, u32>> = docs.iter().map(|(_, m)| m.clone()).collect();
    let vocab = build_vocabulary(
        &count_maps,
        params.min_df,
        params.max_df_ratio,
        params.max_vocab,
    );
    if vocab.terms.is_empty() {
        return Err(ClusterError::EmptyVocabulary {
            doc_count: docs.len() as u64,
        });
    }
    let matrix = build_matrix(&count_maps, &vocab);
    let km = kmeans(
        &matrix,
        vocab.terms.len(),
        params.k as usize,
        params.seed,
        params.max_iters,
    );
    if km.cluster_count == 0 || km.assignment.is_empty() {
        return Err(ClusterError::EmptyVocabulary {
            doc_count: docs.len() as u64,
        });
    }

    let labels = cluster_labels(
        &count_maps,
        &km.assignment,
        km.cluster_count,
        params.label_terms as usize,
    );

    let mut cluster_writes: Vec<ConceptClusterWrite> = Vec::with_capacity(km.cluster_count);
    for ordinal in 0..km.cluster_count {
        let (label, terms) = labels.get(ordinal).cloned().unwrap_or_else(|| {
            (
                format!("cluster_{ordinal}"),
                vec![format!("cluster_{ordinal}")],
            )
        });
        let label_terms_json = serde_json::to_string(&terms)
            .map_err(|e| ClusterError::other(format!("label json: {e}")))?;
        let mut members = Vec::new();
        for (di, &c) in km.assignment.iter().enumerate() {
            if c == ordinal {
                members.push(ConceptMembershipWrite {
                    item_id: docs[di].0.clone(),
                    distance: km.distances.get(di).copied(),
                });
            }
        }
        if members.is_empty() {
            continue;
        }
        cluster_writes.push(ConceptClusterWrite {
            ordinal: ordinal as i64,
            label,
            label_terms_json,
            members,
        });
    }

    // Dense re-ordinal if any empty slipped through.
    for (i, c) in cluster_writes.iter_mut().enumerate() {
        c.ordinal = i as i64;
    }

    let clustered_count: u64 = cluster_writes.iter().map(|c| c.members.len() as u64).sum();
    let cluster_count = cluster_writes.len() as u64;

    let set = matter.replace_concept_cluster_set(ReplaceConceptClusterSetInput {
        set_name: params.set_name.clone(),
        method: METHOD_TFIDF_KMEANS_V1.into(),
        k: i64::from(params.k),
        params_json: params_json.to_string(),
        fingerprint: fingerprint.clone(),
        job_id: Some(job_id.into()),
        clusters: cluster_writes,
    })?;

    summary.clustered_count = clustered_count;
    summary.cluster_count = cluster_count;
    summary.phase_b_done = true;
    progress(summary.completed_count.saturating_add(clustered_count));

    Ok(ConceptClusterOutcome::Succeeded(ConceptClusterReport {
        candidate_count,
        clustered_count,
        cluster_count,
        k_requested: u64::from(params.k),
        set_id: set.id,
        set_name: set.name,
        method: METHOD_TFIDF_KMEANS_V1.into(),
        fingerprint,
        built_at: set.built_at.unwrap_or_default(),
    }))
}

fn write_checkpoint(matter: &Matter, job_id: &str, cursor: &CheckpointCursor) -> Result<()> {
    let json = serde_json::to_string(cursor)
        .map_err(|e| ClusterError::other(format!("checkpoint json: {e}")))?;
    matter.put_checkpoint(
        job_id,
        CONCEPT_CLUSTER_STAGE,
        &json,
        cursor.completed_count as i64,
    )?;
    Ok(())
}

fn load_text_capped(matter: &Matter, digest: &str, max_bytes: u64) -> Result<String> {
    match matter.get_bytes_capped(digest, max_bytes) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(matter_core::Error::Other(msg)) if msg.contains("exceeds cap") => {
            let mut file = matter.open_read(digest)?;
            let mut buf = vec![0u8; max_bytes as usize];
            let n = file.read(&mut buf).map_err(matter_core::Error::from)?;
            buf.truncate(n);
            Ok(String::from_utf8_lossy(&buf).into_owned())
        }
        Err(e) => Err(e.into()),
    }
}
