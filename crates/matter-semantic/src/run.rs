//! Resumable `semantic_index` job: digest/fingerprint skip, reset, cancel.

use std::time::Instant;

use matter_core::{
    AuditEventInput, Matter, UpdateSemanticMatterMetaInput, UpsertSemanticChunkInput,
    WriteItemSemanticInput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::chunk::chunk_text;
use crate::embedder::{embedder_for_model_id, Embedder};
use crate::error::{Result, SemanticError};
use crate::params::SemanticIndexParams;
use crate::store::{ItemVectorFile, SemanticStore, StoreMeta, StoredChunk, STORE_FORMAT_VERSION};

/// Job kind string for process-runner.
pub const JOB_KIND_SEMANTIC_INDEX: &str = "semantic_index";
/// Checkpoint stage name.
pub const SEMANTIC_STAGE: &str = "semantic_index";

/// Summary after a semantic index run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticSummary {
    pub completed_count: u64,
    pub embedded_count: u64,
    pub skipped_count: u64,
    pub cleared_count: u64,
    pub error_count: u64,
    pub dropped_chunks: u64,
}

/// Full success payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticReport {
    pub completed_count: u64,
    pub embedded_count: u64,
    pub skipped_count: u64,
    pub cleared_count: u64,
    pub error_count: u64,
    pub dropped_chunks: u64,
    pub model_id: String,
    pub dims: usize,
    pub fingerprint: String,
    pub total_chunks: u64,
}

/// Outcome of [`run_semantic_index`].
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticOutcome {
    Succeeded(SemanticReport),
    Paused(SemanticSummary),
    Failed {
        message: String,
        summary: SemanticSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    embedded_count: u64,
    skipped_count: u64,
    #[serde(default)]
    cleared_count: u64,
    error_count: u64,
    #[serde(default)]
    dropped_chunks: u64,
    #[serde(default)]
    total_chunks: u64,
    /// When true, matter-wide reset wipe already applied.
    #[serde(default)]
    reset_done: bool,
    /// Frozen for the job lifetime: whether the **prior** build fingerprint
    /// matched the current params fingerprint at job start (or resume).
    ///
    /// Must be checkpointed: matter/store meta is rewritten with the new
    /// fingerprint early for partial queryability; re-reading meta on resume
    /// would otherwise make every remaining item look already up-to-date (F-09).
    #[serde(default)]
    fingerprint_matches: Option<bool>,
    params: serde_json::Value,
}

/// Run offline semantic indexing on `matter` for the runner-created `job_id`.
///
/// Uses `embedder` when provided; otherwise resolves from `params.model_id`
/// (default mock). Does **not** call `create_job` (Option C). Honors `cancel`
/// between items.
pub fn run_semantic_index(
    matter: &Matter,
    job_id: &str,
    params: &SemanticIndexParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<SemanticOutcome> {
    let embedder = embedder_for_model_id(&params.model_id)?;
    run_semantic_index_with_embedder(matter, job_id, params, embedder.as_ref(), cancel, progress)
}

/// Same as [`run_semantic_index`] with an explicit embedder (tests).
pub fn run_semantic_index_with_embedder(
    matter: &Matter,
    job_id: &str,
    params: &SemanticIndexParams,
    embedder: &dyn Embedder,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<SemanticOutcome> {
    let started = Instant::now();
    let result = run_body(matter, job_id, params, embedder, cancel, &progress);

    match &result {
        Ok(SemanticOutcome::Succeeded(r)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "semantic_index.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "embedded_count": r.embedded_count,
                    "skipped_count": r.skipped_count,
                    "cleared_count": r.cleared_count,
                    "error_count": r.error_count,
                    "completed_count": r.completed_count,
                    "dropped_chunks": r.dropped_chunks,
                    "total_chunks": r.total_chunks,
                    "model_id": r.model_id,
                    "dims": r.dims,
                    "fingerprint": r.fingerprint,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                let message = format!("audit complete failed: {e}");
                let summary = summary_from_report(r);
                let _ = matter.append_audit(AuditEventInput {
                    actor: "system".into(),
                    action: "semantic_index.fail".into(),
                    entity: format!("job:{job_id}"),
                    params_json: fail_audit_params(&message, &summary).to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                });
                return Ok(SemanticOutcome::Failed { message, summary });
            }
        }
        Ok(SemanticOutcome::Paused(_)) => {}
        Ok(SemanticOutcome::Failed { message, summary }) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "semantic_index.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(message, summary).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
        Err(e) => {
            let empty = SemanticSummary::default();
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "semantic_index.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(&e.to_string(), &empty).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
    }

    result
}

fn summary_from_report(r: &SemanticReport) -> SemanticSummary {
    SemanticSummary {
        completed_count: r.completed_count,
        embedded_count: r.embedded_count,
        skipped_count: r.skipped_count,
        cleared_count: r.cleared_count,
        error_count: r.error_count,
        dropped_chunks: r.dropped_chunks,
    }
}

fn fail_audit_params(message: &str, summary: &SemanticSummary) -> serde_json::Value {
    json!({
        "error": message,
        "completed_count": summary.completed_count,
        "embedded_count": summary.embedded_count,
        "skipped_count": summary.skipped_count,
        "cleared_count": summary.cleared_count,
        "error_count": summary.error_count,
        "dropped_chunks": summary.dropped_chunks,
    })
}

fn run_body(
    matter: &Matter,
    job_id: &str,
    params: &SemanticIndexParams,
    embedder: &dyn Embedder,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<SemanticOutcome> {
    params.validate()?;

    if embedder.model_id() != params.model_id.trim() {
        return Ok(SemanticOutcome::Failed {
            message: format!(
                "params.model_id '{}' does not match embedder.model_id '{}'",
                params.model_id,
                embedder.model_id()
            ),
            summary: SemanticSummary::default(),
        });
    }

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate()?;
    let params_json = serde_json::to_value(&effective)
        .map_err(|e| SemanticError::other(format!("serialize semantic params: {e}")))?;

    let resuming = prior.as_ref().is_some_and(|p| p.completed_count > 0);
    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "semantic_index.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "resume": resuming,
            "model_id": effective.model_id,
            "dims": embedder.dimensions(),
            "engine_tag": embedder.engine_tag(),
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    run_inner(
        matter,
        job_id,
        &effective,
        embedder,
        cancel,
        progress,
        &params_json,
        prior,
    )
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, SEMANTIC_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(SemanticError::other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &SemanticIndexParams,
    prior: Option<&CheckpointCursor>,
) -> Result<SemanticIndexParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<SemanticIndexParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(SemanticError::other(format!(
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
    params: &SemanticIndexParams,
    embedder: &dyn Embedder,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<SemanticOutcome> {
    let mut summary = SemanticSummary::default();
    let mut cursor_index = 0u64;
    let mut last_item_id: Option<String> = None;
    let mut reset_done = false;
    let mut total_chunks = 0u64;
    // Frozen from checkpoint when resuming a mid-run fingerprint-change job.
    let mut checkpoint_fingerprint_matches: Option<bool> = None;

    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        last_item_id = p.last_item_id;
        summary.completed_count = p.completed_count;
        summary.embedded_count = p.embedded_count;
        summary.skipped_count = p.skipped_count;
        summary.cleared_count = p.cleared_count;
        summary.error_count = p.error_count;
        summary.dropped_chunks = p.dropped_chunks;
        total_chunks = p.total_chunks;
        reset_done = p.reset_done;
        checkpoint_fingerprint_matches = p.fingerprint_matches;
    }

    let fail = |summary: SemanticSummary, e: SemanticError| -> Result<SemanticOutcome> {
        Ok(SemanticOutcome::Failed {
            message: e.to_string(),
            summary,
        })
    };

    let matter_root = matter.root().to_path_buf();
    let dims = embedder.dimensions();
    let fingerprint = params.fingerprint(dims, embedder.engine_tag());
    let chunk_params_json = match params.chunk_params_json() {
        Ok(s) => s,
        Err(e) => return fail(summary, e),
    };

    if params.reset && !reset_done {
        if let Err(e) = SemanticStore::reset_namespace(&matter_root, &params.model_id) {
            return fail(summary, e);
        }
        if let Err(e) = matter.clear_all_semantic(Some(&params.model_id)) {
            return fail(summary, e.into());
        }
        if let Err(e) = matter.clear_semantic_matter_meta() {
            return fail(summary, e.into());
        }
        reset_done = true;
        if let Err(e) = write_checkpoint(
            matter,
            job_id,
            cursor_index,
            &summary,
            total_chunks,
            params_json,
            last_item_id.as_deref(),
            reset_done,
            // Reset invalidates prior fingerprint match.
            Some(false),
        ) {
            return fail(summary, e);
        }
    }

    // Fail-closed max_docs: count candidates up front when not resuming mid-run.
    if summary.completed_count == 0 {
        let mut count = 0u64;
        let mut after: Option<String> = None;
        loop {
            let page = match matter.list_semantic_candidates(after.as_deref(), 500) {
                Ok(p) => p,
                Err(e) => return fail(summary, e.into()),
            };
            if page.is_empty() {
                break;
            }
            count += page.len() as u64;
            after = page.last().map(|c| c.id.clone());
            if count > params.max_docs {
                return fail(
                    summary,
                    SemanticError::other(format!(
                        "semantic_index max_docs exceeded: candidates={count} max_docs={} (fail-closed)",
                        params.max_docs
                    )),
                );
            }
        }
    }

    let store = match SemanticStore::open(&matter_root, &params.model_id, dims) {
        Ok(s) => s,
        Err(e) => return fail(summary, e),
    };

    // Capture PRIOR fingerprint before overwriting matter/store meta (F-01).
    // On resume, prefer the value frozen in the checkpoint so early meta write
    // does not make remaining items look already up-to-date (F-09).
    let fingerprint_matches = if let Some(frozen) = checkpoint_fingerprint_matches {
        frozen
    } else {
        let prior_matter_fp = match matter.get_semantic_meta() {
            Ok(m) => m.semantic_fingerprint,
            Err(e) => return fail(summary, e.into()),
        };
        let prior_store_fp = match store.read_meta() {
            Ok(Some(m)) => Some(m.fingerprint),
            Ok(None) => None,
            Err(e) => return fail(summary, e),
        };
        let prior_fingerprint = prior_matter_fp.or(prior_store_fp);
        prior_fingerprint.as_deref() == Some(fingerprint.as_str())
    };
    // After a reset wipe, prior fingerprint is gone — force re-embed.
    let fingerprint_matches = if params.reset && reset_done {
        false
    } else {
        fingerprint_matches
    };

    if let Err(e) = store.write_meta(&StoreMeta {
        format_version: STORE_FORMAT_VERSION,
        model_id: params.model_id.clone(),
        dims,
        chunk_chars: params.chunk_chars,
        chunk_overlap: params.chunk_overlap,
        max_chunks_per_item: params.max_chunks_per_item,
        engine_tag: embedder.engine_tag().to_string(),
        fingerprint: fingerprint.clone(),
    }) {
        return fail(summary, e);
    }

    // Enable meta early so partial runs are queryable for completed items.
    // Skip decisions use `fingerprint_matches` (prior), not this new value.
    if let Err(e) = matter.update_semantic_matter_meta(UpdateSemanticMatterMetaInput {
        enabled: true,
        model_id: Some(&params.model_id),
        dims: Some(dims as i64),
        chunk_params_json: Some(&chunk_params_json),
        fingerprint: Some(&fingerprint),
        built_at: Some(&Matter::semantic_now()),
        job_id: Some(job_id),
        chunk_count: total_chunks as i64,
    }) {
        return fail(summary, e.into());
    }

    let batch = params.batch_size.max(1) as u64;
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                total_chunks,
                params_json,
                last_item_id.as_deref(),
                reset_done,
                Some(fingerprint_matches),
            ) {
                return fail(summary, e);
            }
            progress(summary.completed_count);
            return Ok(SemanticOutcome::Paused(summary));
        }

        let candidates = match matter.list_semantic_candidates(last_item_id.as_deref(), batch) {
            Ok(c) => c,
            Err(e) => return fail(summary, e.into()),
        };
        if candidates.is_empty() {
            break;
        }

        for cand in candidates {
            if cancel.map(|c| c()).unwrap_or(false) {
                if let Err(e) = write_checkpoint(
                    matter,
                    job_id,
                    cursor_index,
                    &summary,
                    total_chunks,
                    params_json,
                    last_item_id.as_deref(),
                    reset_done,
                    Some(fingerprint_matches),
                ) {
                    return fail(summary, e);
                }
                progress(summary.completed_count);
                return Ok(SemanticOutcome::Paused(summary));
            }

            match process_one(
                matter,
                &store,
                job_id,
                &cand,
                params,
                embedder,
                &fingerprint,
                fingerprint_matches,
                &mut summary,
                &mut total_chunks,
            ) {
                Ok(()) => {}
                Err(e) => return fail(summary, e),
            }
            cursor_index += 1;
            last_item_id = Some(cand.id.clone());
            progress(summary.completed_count);
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                total_chunks,
                params_json,
                last_item_id.as_deref(),
                reset_done,
                Some(fingerprint_matches),
            ) {
                return fail(summary, e);
            }
        }
    }

    // Final meta with total chunk count.
    if let Err(e) = matter.update_semantic_matter_meta(UpdateSemanticMatterMetaInput {
        enabled: true,
        model_id: Some(&params.model_id),
        dims: Some(dims as i64),
        chunk_params_json: Some(&chunk_params_json),
        fingerprint: Some(&fingerprint),
        built_at: Some(&Matter::semantic_now()),
        job_id: Some(job_id),
        chunk_count: total_chunks as i64,
    }) {
        return fail(summary, e.into());
    }

    Ok(SemanticOutcome::Succeeded(SemanticReport {
        completed_count: summary.completed_count,
        embedded_count: summary.embedded_count,
        skipped_count: summary.skipped_count,
        cleared_count: summary.cleared_count,
        error_count: summary.error_count,
        dropped_chunks: summary.dropped_chunks,
        model_id: params.model_id.clone(),
        dims,
        fingerprint,
        total_chunks,
    }))
}

#[allow(clippy::too_many_arguments)]
fn process_one(
    matter: &Matter,
    store: &SemanticStore,
    job_id: &str,
    cand: &matter_core::SemanticCandidate,
    params: &SemanticIndexParams,
    embedder: &dyn Embedder,
    // Current job fingerprint written onto each item vector file.
    fingerprint: &str,
    // True when prior matter/store fingerprint equals this run's fingerprint.
    // Must be computed before overwriting meta (F-01).
    fingerprint_matches: bool,
    summary: &mut SemanticSummary,
    total_chunks: &mut u64,
) -> Result<()> {
    let text_digest = cand
        .text_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Skip only when digests match AND fingerprint matches prior build.
    // Fingerprint change (chunk params, model, dims, engine) forces re-embed
    // even when text digests are unchanged.
    if !params.reset {
        if let (Some(text), Some(emb)) = (
            text_digest,
            cand.semantic_embedded_text_sha256
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        ) {
            if text == emb && fingerprint_matches {
                summary.skipped_count += 1;
                summary.completed_count += 1;
                // Count existing chunks toward total if known.
                if let Some(n) = cand.semantic_chunk_count {
                    *total_chunks = total_chunks.saturating_add(n as u64);
                }
                return Ok(());
            }
        }
    }

    // No body — clear semantic meta + store vectors.
    let Some(digest) = text_digest else {
        let _ = store.delete_item(&cand.id);
        matter.clear_item_semantic(&cand.id, Some(&params.model_id))?;
        summary.cleared_count += 1;
        summary.completed_count += 1;
        return Ok(());
    };

    let text = match load_text_capped(matter, digest, params.max_text_bytes) {
        Ok(t) => t,
        Err(e) => {
            let _ = matter.record_item_error(matter_core::ItemErrorInput {
                item_id: Some(cand.id.clone()),
                source_id: None,
                job_id: Some(job_id.to_string()),
                stage: SEMANTIC_STAGE.into(),
                code: "semantic_text_load".into(),
                message: e.to_string(),
                detail: None,
            });
            let _ = store.delete_item(&cand.id);
            matter.clear_item_semantic(&cand.id, Some(&params.model_id))?;
            summary.error_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    };

    if text.trim().is_empty() {
        let _ = store.delete_item(&cand.id);
        matter.clear_item_semantic(&cand.id, Some(&params.model_id))?;
        summary.cleared_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    let chunked = chunk_text(
        &text,
        params.chunk_chars,
        params.chunk_overlap,
        params.max_chunks_per_item,
    );
    summary.dropped_chunks += u64::from(chunked.dropped_chunks);

    if chunked.chunks.is_empty() {
        let _ = store.delete_item(&cand.id);
        matter.clear_item_semantic(&cand.id, Some(&params.model_id))?;
        summary.cleared_count += 1;
        summary.completed_count += 1;
        return Ok(());
    }

    let refs: Vec<&str> = chunked.chunks.iter().map(|c| c.text.as_str()).collect();
    let vectors = embedder.embed_passages(&refs)?;
    if vectors.len() != chunked.chunks.len() {
        return Err(SemanticError::embedder(format!(
            "embedder returned {} vectors for {} chunks",
            vectors.len(),
            chunked.chunks.len()
        )));
    }

    let mut stored = Vec::with_capacity(chunked.chunks.len());
    let mut catalog = Vec::with_capacity(chunked.chunks.len());
    for (ch, vec) in chunked.chunks.iter().zip(vectors) {
        if vec.len() != embedder.dimensions() {
            return Err(SemanticError::embedder(format!(
                "vector len {} != dims {}",
                vec.len(),
                embedder.dimensions()
            )));
        }
        stored.push(StoredChunk {
            ordinal: ch.ordinal,
            start_offset: ch.start,
            end_offset: ch.end,
            vector: vec,
        });
        catalog.push(UpsertSemanticChunkInput {
            item_id: &cand.id,
            ordinal: i64::from(ch.ordinal),
            start_offset: Some(ch.start as i64),
            end_offset: Some(ch.end as i64),
            text_sha256: digest,
            model_id: &params.model_id,
        });
    }

    // Delete-before-write on disk + catalog.
    store.delete_item(&cand.id)?;
    store.write_item(&ItemVectorFile {
        format_version: STORE_FORMAT_VERSION,
        item_id: cand.id.clone(),
        text_sha256: digest.to_string(),
        model_id: params.model_id.clone(),
        dims: embedder.dimensions(),
        fingerprint: fingerprint.to_string(),
        chunks: stored,
    })?;
    matter.replace_item_semantic_chunks(&cand.id, &params.model_id, &catalog)?;

    let n_chunks = catalog.len() as i64;
    let embedded_at = Matter::semantic_now();
    matter.write_item_semantic_meta(WriteItemSemanticInput {
        item_id: &cand.id,
        embedded_text_sha256: digest,
        chunk_count: n_chunks,
        embedded_at: &embedded_at,
    })?;

    *total_chunks = total_chunks.saturating_add(n_chunks as u64);
    summary.embedded_count += 1;
    summary.completed_count += 1;
    Ok(())
}

fn load_text_capped(matter: &Matter, digest: &str, max_bytes: u64) -> Result<String> {
    match matter.get_bytes_capped(digest, max_bytes) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(matter_core::Error::Other(msg)) if msg.contains("exceeds cap") => {
            let mut file = matter.cas().open_read(digest)?;
            let mut buf = vec![0u8; max_bytes as usize];
            use std::io::Read;
            let n = file.read(&mut buf).map_err(matter_core::Error::from)?;
            buf.truncate(n);
            Ok(String::from_utf8_lossy(&buf).into_owned())
        }
        Err(e) => Err(e.into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn write_checkpoint(
    matter: &Matter,
    job_id: &str,
    cursor_index: u64,
    summary: &SemanticSummary,
    total_chunks: u64,
    params_json: &serde_json::Value,
    last_item_id: Option<&str>,
    reset_done: bool,
    fingerprint_matches: Option<bool>,
) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index,
        last_item_id: last_item_id.map(|s| s.to_string()),
        completed_count: summary.completed_count,
        embedded_count: summary.embedded_count,
        skipped_count: summary.skipped_count,
        cleared_count: summary.cleared_count,
        error_count: summary.error_count,
        dropped_chunks: summary.dropped_chunks,
        total_chunks,
        reset_done,
        fingerprint_matches,
        params: params_json.clone(),
    };
    let json = serde_json::to_string(&cursor).map_err(|e| SemanticError::other(e.to_string()))?;
    matter.put_checkpoint(
        job_id,
        SEMANTIC_STAGE,
        &json,
        summary.completed_count as i64,
    )?;
    Ok(())
}
