//! Resumable `entity_scan` job: fingerprint-aware skip, mask/hash hits only.

use std::time::Instant;

use matter_core::{
    sha256_hex, AuditEventInput, EntityScanCandidate, Matter, ReplaceEntityHitsInput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{EntityError, Result};
use crate::mask;
use crate::packs;
use crate::params::EntityScanParams;
use crate::scan::{flags_from_hits, scan_text, RawHit};

/// Job kind string for process-runner.
pub const JOB_KIND_ENTITY_SCAN: &str = "entity_scan";
/// Checkpoint stage name.
pub const ENTITY_SCAN_STAGE: &str = "entity_scan";

/// Fingerprint schema version prefix (stored in `entity_scanned_text_sha256`).
pub const ESCAN_FP_VERSION: &str = "escan_v1";

/// Summary after an entity scan (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityScanSummary {
    pub completed_count: u64,
    pub scanned_count: u64,
    pub skipped_count: u64,
    pub hit_count: u64,
    pub error_count: u64,
    pub truncated_count: u64,
}

/// Full success payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityScanReport {
    pub completed_count: u64,
    pub scanned_count: u64,
    pub skipped_count: u64,
    pub hit_count: u64,
    pub error_count: u64,
    pub truncated_count: u64,
    pub packs: Vec<String>,
}

/// Outcome of [`run_entity_scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityScanOutcome {
    Succeeded(EntityScanReport),
    Paused(EntityScanSummary),
    Failed {
        message: String,
        summary: EntityScanSummary,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    cursor_index: u64,
    #[serde(default)]
    last_item_id: Option<String>,
    completed_count: u64,
    scanned_count: u64,
    skipped_count: u64,
    hit_count: u64,
    error_count: u64,
    #[serde(default)]
    truncated_count: u64,
    /// When true, matter-wide reset wipe already applied.
    #[serde(default)]
    reset_done: bool,
    params: serde_json::Value,
}

/// Body scan outcome component of the scan fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyScanOutcome {
    /// Full body read (or no body present).
    Full,
    /// Body truncated at `max_text_bytes`.
    Truncated(u64),
    /// CAS load failed for the declared body digest.
    CasError,
}

/// Run entity / PII pack scan on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between items.
///
/// **Idempotency (`reset: false`):**
/// - Scan when `entity_scanned_text_sha256` is NULL or does not equal the
///   current **full-success** fingerprint (packs + body digest + `trunc=full`
///   + subject/from content hashes).
/// - Skip only when the stored fingerprint matches that full-success form.
/// - Incomplete scans (`trunc=N`, `body=err:…`) never match → always retry.
/// - Pack set/version or subject/from content changes force rescan.
pub fn run_entity_scan(
    matter: &Matter,
    job_id: &str,
    params: &EntityScanParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<EntityScanOutcome> {
    let started = Instant::now();
    let result = run_body(matter, job_id, params, cancel, &progress);

    match &result {
        Ok(EntityScanOutcome::Succeeded(r)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "entity_scan.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "scanned_count": r.scanned_count,
                    "skipped_count": r.skipped_count,
                    "hit_count": r.hit_count,
                    "error_count": r.error_count,
                    "truncated_count": r.truncated_count,
                    "completed_count": r.completed_count,
                    "packs": packs::pack_audit_entries(&r.packs),
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                let message = format!("audit complete failed: {e}");
                let summary = summary_from_report(r);
                let _ = matter.append_audit(AuditEventInput {
                    actor: "system".into(),
                    action: "entity_scan.fail".into(),
                    entity: format!("job:{job_id}"),
                    params_json: fail_audit_params(&message, &summary).to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                });
                return Ok(EntityScanOutcome::Failed { message, summary });
            }
        }
        Ok(EntityScanOutcome::Paused(_)) => {}
        Ok(EntityScanOutcome::Failed { message, summary }) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "entity_scan.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(message, summary).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
        Err(e) => {
            let empty = EntityScanSummary::default();
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "entity_scan.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: fail_audit_params(&e.to_string(), &empty).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
    }

    result
}

fn summary_from_report(r: &EntityScanReport) -> EntityScanSummary {
    EntityScanSummary {
        completed_count: r.completed_count,
        scanned_count: r.scanned_count,
        skipped_count: r.skipped_count,
        hit_count: r.hit_count,
        error_count: r.error_count,
        truncated_count: r.truncated_count,
    }
}

fn fail_audit_params(message: &str, summary: &EntityScanSummary) -> serde_json::Value {
    json!({
        "error": message,
        "completed_count": summary.completed_count,
        "scanned_count": summary.scanned_count,
        "skipped_count": summary.skipped_count,
        "hit_count": summary.hit_count,
        "error_count": summary.error_count,
        "truncated_count": summary.truncated_count,
    })
}

fn run_body(
    matter: &Matter,
    job_id: &str,
    params: &EntityScanParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<EntityScanOutcome> {
    params.validate()?;

    let prior = load_prior_checkpoint(matter, job_id)?;
    let effective = effective_params(params, prior.as_ref())?;
    effective.validate()?;
    let params_json = serde_json::to_value(&effective)
        .map_err(|e| EntityError::other(format!("serialize entity_scan params: {e}")))?;

    let resuming = prior.as_ref().is_some_and(|p| p.completed_count > 0);
    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "entity_scan.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "resume": resuming,
            "packs": packs::pack_audit_entries(&effective.packs),
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
    )
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, ENTITY_SCAN_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(EntityError::other(format!("corrupt checkpoint: {e}"))),
    }
}

fn effective_params(
    call_site: &EntityScanParams,
    prior: Option<&CheckpointCursor>,
) -> Result<EntityScanParams> {
    if let Some(p) = prior {
        if !p.params.is_null() && p.params.as_object().is_some_and(|o| !o.is_empty()) {
            match serde_json::from_value::<EntityScanParams>(p.params.clone()) {
                Ok(frozen) => return Ok(frozen),
                Err(e) => {
                    return Err(EntityError::other(format!(
                        "checkpoint params unreadable: {e}"
                    )));
                }
            }
        }
    }
    Ok(call_site.clone())
}

fn run_inner(
    matter: &Matter,
    job_id: &str,
    params: &EntityScanParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    params_json: &serde_json::Value,
    prior: Option<CheckpointCursor>,
) -> Result<EntityScanOutcome> {
    let mut summary = EntityScanSummary::default();
    let mut cursor_index = 0u64;
    let mut last_item_id: Option<String> = None;
    let mut reset_done = false;

    if let Some(p) = prior {
        cursor_index = p.cursor_index;
        last_item_id = p.last_item_id;
        summary.completed_count = p.completed_count;
        summary.scanned_count = p.scanned_count;
        summary.skipped_count = p.skipped_count;
        summary.hit_count = p.hit_count;
        summary.error_count = p.error_count;
        summary.truncated_count = p.truncated_count;
        reset_done = p.reset_done;
    }

    let fail = |summary: EntityScanSummary, e: EntityError| -> Result<EntityScanOutcome> {
        Ok(EntityScanOutcome::Failed {
            message: e.to_string(),
            summary,
        })
    };

    // reset:true → clear all hits once before scanning (not again on resume).
    if params.reset && !reset_done {
        if let Err(e) = matter.clear_entity_hits_for_matter() {
            return fail(summary, e.into());
        }
        reset_done = true;
        if let Err(e) = write_checkpoint(
            matter,
            job_id,
            cursor_index,
            &summary,
            params_json,
            last_item_id.as_deref(),
            reset_done,
        ) {
            return fail(summary, e);
        }
    }

    let batch = params.batch_size.max(1) as u64;
    loop {
        if cancel.map(|c| c()).unwrap_or(false) {
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                params_json,
                last_item_id.as_deref(),
                reset_done,
            ) {
                return fail(summary, e);
            }
            progress(summary.completed_count);
            return Ok(EntityScanOutcome::Paused(summary));
        }

        let candidates = match matter.list_entity_scan_candidates(last_item_id.as_deref(), batch) {
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
                    params_json,
                    last_item_id.as_deref(),
                    reset_done,
                ) {
                    return fail(summary, e);
                }
                progress(summary.completed_count);
                return Ok(EntityScanOutcome::Paused(summary));
            }

            if let Err(e) = process_one(matter, job_id, &cand, params, &mut summary) {
                return fail(summary, e);
            }
            cursor_index += 1;
            last_item_id = Some(cand.id.clone());
            progress(summary.completed_count);
            if let Err(e) = write_checkpoint(
                matter,
                job_id,
                cursor_index,
                &summary,
                params_json,
                last_item_id.as_deref(),
                reset_done,
            ) {
                return fail(summary, e);
            }
        }
    }

    Ok(EntityScanOutcome::Succeeded(EntityScanReport {
        completed_count: summary.completed_count,
        scanned_count: summary.scanned_count,
        skipped_count: summary.skipped_count,
        hit_count: summary.hit_count,
        error_count: summary.error_count,
        truncated_count: summary.truncated_count,
        packs: params.packs.clone(),
    }))
}

fn process_one(
    matter: &Matter,
    job_id: &str,
    cand: &EntityScanCandidate,
    params: &EntityScanParams,
    summary: &mut EntityScanSummary,
) -> Result<()> {
    // Skip only when prior scan was a full-success fingerprint for current params.
    if !params.reset {
        let expected = full_success_fingerprint(cand, params);
        if cand.entity_scanned_text_sha256.as_deref() == Some(expected.as_str()) {
            summary.skipped_count += 1;
            summary.completed_count += 1;
            return Ok(());
        }
    }

    let mut hits: Vec<RawHit> = Vec::new();
    let mut truncated = false;
    let mut body_cas_failed = false;

    // Body text from CAS.
    if let Some(digest) = cand
        .text_sha256
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        match load_text_capped(matter, digest, params.max_text_bytes) {
            Ok((text, was_trunc)) => {
                if was_trunc {
                    truncated = true;
                    summary.truncated_count += 1;
                }
                hits.extend(scan_text(&text, "text", &params.packs));
            }
            Err(e) => {
                // CAS missing / unreadable — record error but still try subject/from.
                // Fingerprint will use body=err:digest so skip never claims full success.
                body_cas_failed = true;
                let _ = matter.record_item_error(matter_core::ItemErrorInput {
                    item_id: Some(cand.id.clone()),
                    source_id: None,
                    job_id: Some(job_id.to_string()),
                    stage: ENTITY_SCAN_STAGE.into(),
                    code: "entity_text_load".into(),
                    message: e.to_string(),
                    detail: None,
                });
                summary.error_count += 1;
            }
        }
    }

    if let Some(subj) = cand
        .subject
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        hits.extend(scan_text(subj, "subject", &params.packs));
    }

    // Optional from_addr field (email-only signal).
    if let Some(from) = cand
        .from_addr
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if params.packs.iter().any(|p| p == "email") {
            hits.extend(scan_text(from, "from", &["email".to_string()]));
        }
    }

    let body_outcome = if body_cas_failed {
        BodyScanOutcome::CasError
    } else if truncated {
        BodyScanOutcome::Truncated(params.max_text_bytes)
    } else {
        BodyScanOutcome::Full
    };

    let fingerprint = build_scan_fingerprint(
        &params.packs,
        cand.text_sha256.as_deref(),
        body_outcome,
        cand.subject.as_deref(),
        cand.from_addr.as_deref(),
    );

    let flags = flags_from_hits(&hits);
    let hit_count = hits.len() as i64;
    let creates: Vec<_> = hits.into_iter().map(RawHit::into_create).collect();
    let scan_at = Matter::entity_scan_now();

    matter.replace_entity_hits_for_item(ReplaceEntityHitsInput {
        item_id: &cand.id,
        hits: &creates,
        flags,
        hit_count,
        scanned_text_sha256: Some(&fingerprint),
        job_id: Some(job_id),
        scan_at: &scan_at,
    })?;

    summary.scanned_count += 1;
    summary.hit_count = summary.hit_count.saturating_add(hit_count as u64);
    summary.completed_count += 1;
    Ok(())
}

/// Full-success fingerprint used for skip comparison (`trunc=full`, clean body digest).
pub fn full_success_fingerprint(cand: &EntityScanCandidate, params: &EntityScanParams) -> String {
    build_scan_fingerprint(
        &params.packs,
        cand.text_sha256.as_deref(),
        BodyScanOutcome::Full,
        cand.subject.as_deref(),
        cand.from_addr.as_deref(),
    )
}

/// Build composite scan fingerprint stored in `entity_scanned_text_sha256`.
///
/// Format:
/// `escan_v1|packs=<id@ver sorted>|body=<hex|-|err:hex>|trunc=<full|N>|subj=<hex|->|from=<hex|->`
pub fn build_scan_fingerprint(
    packs: &[String],
    body_digest: Option<&str>,
    body_outcome: BodyScanOutcome,
    subject: Option<&str>,
    from_addr: Option<&str>,
) -> String {
    let packs_part = packs_fingerprint_token(packs);
    let body_part = body_fingerprint_token(body_digest, body_outcome);
    let trunc_part = match body_outcome {
        BodyScanOutcome::Truncated(n) => format!("trunc={n}"),
        BodyScanOutcome::Full | BodyScanOutcome::CasError => "trunc=full".to_string(),
    };
    let subj = content_hash_token(subject);
    let from = content_hash_token(from_addr);
    format!(
        "{ESCAN_FP_VERSION}|packs={packs_part}|{body_part}|{trunc_part}|subj={subj}|from={from}"
    )
}

fn packs_fingerprint_token(packs: &[String]) -> String {
    let mut entries: Vec<String> = packs
        .iter()
        .map(|id| format!("{}@{}", id.trim(), packs::pack_version(id.trim())))
        .collect();
    entries.sort();
    entries.dedup();
    entries.join(",")
}

fn body_fingerprint_token(body_digest: Option<&str>, body_outcome: BodyScanOutcome) -> String {
    let dig = body_digest.map(str::trim).filter(|s| !s.is_empty());
    match (dig, body_outcome) {
        (None, _) => "body=-".to_string(),
        (Some(d), BodyScanOutcome::CasError) => format!("body=err:{d}"),
        (Some(d), _) => format!("body={d}"),
    }
}

fn content_hash_token(field: Option<&str>) -> String {
    match field.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => mask::sha256_hex(s.as_bytes()),
        None => "-".to_string(),
    }
}

/// Load CAS text, truncating to `max_bytes` when larger (no full alloc of huge blobs).
fn load_text_capped(matter: &Matter, digest: &str, max_bytes: u64) -> Result<(String, bool)> {
    match matter.get_bytes_capped(digest, max_bytes) {
        Ok(bytes) => Ok((String::from_utf8_lossy(&bytes).into_owned(), false)),
        Err(matter_core::Error::Other(msg)) if msg.contains("exceeds cap") => {
            // Truncate: open and read only max_bytes.
            let mut file = matter.open_read(digest)?;
            let mut buf = vec![0u8; max_bytes as usize];
            use std::io::Read;
            let n = file.read(&mut buf).map_err(matter_core::Error::from)?;
            buf.truncate(n);
            Ok((String::from_utf8_lossy(&buf).into_owned(), true))
        }
        Err(e) => Err(e.into()),
    }
}

fn write_checkpoint(
    matter: &Matter,
    job_id: &str,
    cursor_index: u64,
    summary: &EntityScanSummary,
    params_json: &serde_json::Value,
    last_item_id: Option<&str>,
    reset_done: bool,
) -> Result<()> {
    let cursor = CheckpointCursor {
        cursor_index,
        last_item_id: last_item_id.map(|s| s.to_string()),
        completed_count: summary.completed_count,
        scanned_count: summary.scanned_count,
        skipped_count: summary.skipped_count,
        hit_count: summary.hit_count,
        error_count: summary.error_count,
        truncated_count: summary.truncated_count,
        reset_done,
        params: params_json.clone(),
    };
    let json = serde_json::to_string(&cursor).map_err(|e| EntityError::other(e.to_string()))?;
    matter.put_checkpoint(
        job_id,
        ENTITY_SCAN_STAGE,
        &json,
        summary.completed_count as i64,
    )?;
    Ok(())
}

/// Expose sha256 for tests (subject marker consistency).
#[allow(dead_code)]
pub fn test_sha256_hex(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    #[test]
    fn fingerprint_includes_sorted_packs_and_full_trunc() {
        let fp = build_scan_fingerprint(
            &["ssn_us".into(), "email".into()],
            Some("deadbeef"),
            BodyScanOutcome::Full,
            Some("  Hello  "),
            None,
        );
        assert!(fp.starts_with("escan_v1|packs=email@1,ssn_us@1|"));
        assert!(fp.contains("|body=deadbeef|trunc=full|"));
        assert!(fp.contains("|subj="));
        assert!(fp.ends_with("|from=-"));
        // subject is trimmed before hash
        let subj_hash = mask::sha256_hex(b"Hello");
        assert!(fp.contains(&format!("subj={subj_hash}")));
    }

    #[test]
    fn cas_error_body_token_differs_from_full() {
        let full = build_scan_fingerprint(
            &["email".into()],
            Some("abc"),
            BodyScanOutcome::Full,
            None,
            None,
        );
        let err = build_scan_fingerprint(
            &["email".into()],
            Some("abc"),
            BodyScanOutcome::CasError,
            None,
            None,
        );
        assert_ne!(full, err);
        assert!(err.contains("body=err:abc"));
        assert!(!full.contains("body=err:"));
    }

    #[test]
    fn truncated_never_equals_full_success() {
        let full = build_scan_fingerprint(
            &["email".into()],
            Some("abc"),
            BodyScanOutcome::Full,
            None,
            None,
        );
        let trunc = build_scan_fingerprint(
            &["email".into()],
            Some("abc"),
            BodyScanOutcome::Truncated(100),
            None,
            None,
        );
        assert_ne!(full, trunc);
        assert!(trunc.contains("trunc=100"));
    }
}
