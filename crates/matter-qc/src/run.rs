//! Core production QC job: select → evaluate → report → persist qc_runs.
//!
//! Resumable via checkpoint stage [`QC_STAGE`] (process-runner Option C).

use std::collections::HashSet;
use std::time::Instant;

use camino::Utf8PathBuf;
use chrono::Utc;
use matter_core::{selection_fingerprint_with_pack, AuditEventInput, InsertQcRunInput, Matter};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{QcError, Result};
use crate::params::{QcParams, QcSeverity};
use crate::report::{count_severities, default_qc_report_dir, write_qc_report, QcReportMeta};
use crate::rules::{
    empty_selection_finding, evaluate_one_item, only_withheld_finding, resolve_rules_for_pack,
    QcFinding,
};
use crate::select::select_item_ids;

/// Job kind string for process-runner.
pub const JOB_KIND_QC: &str = "qc";
/// Checkpoint stage name.
pub const QC_STAGE: &str = "qc";

/// Summary after a QC run (or partial pause).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QcSummary {
    pub completed_count: u64,
    pub candidate_count: u64,
    pub error_count: u64,
    pub warn_count: u64,
    pub passed: bool,
    pub selection_fingerprint: String,
    pub scope: String,
    pub profile: String,
    pub report_path: String,
    pub qc_run_id: String,
}

/// Full in-memory report returned to callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QcReport {
    pub generated_at: String,
    pub passed: bool,
    pub error_count: u64,
    pub warn_count: u64,
    pub findings: Vec<QcFinding>,
    pub candidate_count: u64,
    pub selection_fingerprint: String,
    pub scope: String,
    pub profile: String,
    pub report_path: String,
    pub qc_run_id: String,
}

/// Outcome of [`run_production_qc`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QcOutcome {
    Succeeded(QcReport),
    Paused(QcSummary),
    Failed { message: String, summary: QcSummary },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckpointCursor {
    /// `eval` | `done`
    #[serde(default = "default_phase_eval")]
    phase: String,
    cursor_index: u64,
    completed_count: u64,
    candidate_count: u64,
    params: serde_json::Value,
    #[serde(default)]
    ordered_ids: Vec<String>,
    #[serde(default)]
    findings: Vec<QcFinding>,
    #[serde(default)]
    error_count: u64,
    #[serde(default)]
    warn_count: u64,
    #[serde(default)]
    withheld_count: u64,
    #[serde(default)]
    selection_fingerprint: String,
    #[serde(default)]
    profile: String,
    #[serde(default)]
    scope: String,
}

fn default_phase_eval() -> String {
    "eval".into()
}

/// Run production QC on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between items.
/// Calls `progress(completed_count)` during evaluation.
///
/// Resumable: loads prior checkpoint for `job_id` when params match and continues
/// from `cursor_index`. Partial cancel writes a checkpoint and returns
/// [`QcOutcome::Paused`] without inserting `qc_runs` (only full success authorizes produce).
pub fn run_production_qc(
    matter: &Matter,
    job_id: &str,
    params: &QcParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<QcOutcome> {
    let started = Instant::now();
    let call_params = params.clone();
    call_params.validate_shape()?;
    let params_json = serde_json::to_value(&call_params).unwrap_or_else(|_| json!({}));

    let prior = load_prior_checkpoint(matter, job_id)?;
    let resuming = prior
        .as_ref()
        .is_some_and(|p| params_match(p, &params_json) && p.phase != "done");

    let pack_id = call_params.resolved_pack_id();
    let rules = crate::rules::resolve_rules_for_pack(&pack_id, &call_params.rules);
    let rules_json = serde_json::to_value(rules.to_configs()).unwrap_or_else(|_| json!([]));
    let config_hash = {
        use sha2::{Digest, Sha256};
        let payload = json!({
            "pack_id": pack_id,
            "rules": rules_json,
        });
        let s = payload.to_string();
        let digest = Sha256::digest(s.as_bytes());
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "qc.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "pack_id": pack_id,
            "config_hash": config_hash,
            "resume": resuming,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_qc_inner(
        matter,
        job_id,
        &call_params,
        &params_json,
        cancel,
        &progress,
        prior,
    );

    match &result {
        Ok(QcOutcome::Succeeded(r)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "qc.complete".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "passed": r.passed,
                    "error_count": r.error_count,
                    "warn_count": r.warn_count,
                    "candidate_count": r.candidate_count,
                    "selection_fingerprint": r.selection_fingerprint,
                    "report_path": r.report_path,
                    "qc_run_id": r.qc_run_id,
                    "profile": r.profile,
                    "pack_id": r.profile,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(QcOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: summary_from_report(r),
                });
            }
        }
        Ok(QcOutcome::Paused(_)) => {}
        Ok(QcOutcome::Failed { message, summary }) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "qc.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "candidate_count": summary.candidate_count,
                    "error_count": summary.error_count,
                    "warn_count": summary.warn_count,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(QcError::Other(format!(
                    "audit fail write failed after run failure ({message}): {e}"
                )));
            }
        }
        Err(e) => {
            if let Err(ae) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "qc.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Err(QcError::Other(format!(
                    "{e}; audit fail write also failed: {ae}"
                )));
            }
        }
    }

    result
}

fn summary_from_report(r: &QcReport) -> QcSummary {
    QcSummary {
        completed_count: r.candidate_count,
        candidate_count: r.candidate_count,
        error_count: r.error_count,
        warn_count: r.warn_count,
        passed: r.passed,
        selection_fingerprint: r.selection_fingerprint.clone(),
        scope: r.scope.clone(),
        profile: r.profile.clone(),
        report_path: r.report_path.clone(),
        qc_run_id: r.qc_run_id.clone(),
    }
}

fn summary_from_cursor(c: &CheckpointCursor) -> QcSummary {
    QcSummary {
        completed_count: c.completed_count,
        candidate_count: c.candidate_count,
        error_count: c.error_count,
        warn_count: c.warn_count,
        passed: false,
        selection_fingerprint: c.selection_fingerprint.clone(),
        scope: c.scope.clone(),
        profile: c.profile.clone(),
        report_path: String::new(),
        qc_run_id: String::new(),
    }
}

fn load_prior_checkpoint(matter: &Matter, job_id: &str) -> Result<Option<CheckpointCursor>> {
    let Some(cp) = matter.get_checkpoint(job_id, QC_STAGE)? else {
        return Ok(None);
    };
    if cp.cursor_json.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<CheckpointCursor>(&cp.cursor_json) {
        Ok(c) => Ok(Some(c)),
        Err(e) => Err(QcError::Other(format!("corrupt checkpoint: {e}"))),
    }
}

fn params_match(prior: &CheckpointCursor, call_params_json: &serde_json::Value) -> bool {
    &prior.params == call_params_json
}

fn write_checkpoint(matter: &Matter, job_id: &str, cursor: &CheckpointCursor) -> Result<()> {
    let cursor_json = serde_json::to_string(cursor)
        .map_err(|e| QcError::Other(format!("checkpoint serialize: {e}")))?;
    matter.put_checkpoint(
        job_id,
        QC_STAGE,
        &cursor_json,
        cursor.completed_count as i64,
    )?;
    Ok(())
}

fn recompute_severity_counts(findings: &[QcFinding]) -> (u64, u64) {
    count_severities(findings)
}

fn run_qc_inner(
    matter: &Matter,
    job_id: &str,
    params: &QcParams,
    params_json: &serde_json::Value,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
    prior: Option<CheckpointCursor>,
) -> Result<QcOutcome> {
    let pack_id = params.resolved_pack_id();
    let rules = resolve_rules_for_pack(&pack_id, &params.rules);
    // Store canonical pack id on qc_runs.profile (pack-bound fingerprint identity).
    let profile = rules.profile.clone();
    let rules_json = serde_json::to_string(&rules.to_configs()).unwrap_or_else(|_| "[]".into());
    let scope = params.scope.clone();

    // Resume only when params match and phase is still eval.
    let mut cursor = if let Some(prior) = prior {
        if params_match(&prior, params_json) && prior.phase != "done" {
            prior
        } else {
            // Params changed or already done — start fresh selection.
            CheckpointCursor {
                phase: "eval".into(),
                cursor_index: 0,
                completed_count: 0,
                candidate_count: 0,
                params: params_json.clone(),
                ordered_ids: Vec::new(),
                findings: Vec::new(),
                error_count: 0,
                warn_count: 0,
                withheld_count: 0,
                selection_fingerprint: String::new(),
                profile: profile.clone(),
                scope: scope.clone(),
            }
        }
    } else {
        CheckpointCursor {
            phase: "eval".into(),
            cursor_index: 0,
            completed_count: 0,
            candidate_count: 0,
            params: params_json.clone(),
            ordered_ids: Vec::new(),
            findings: Vec::new(),
            error_count: 0,
            warn_count: 0,
            withheld_count: 0,
            selection_fingerprint: String::new(),
            profile: profile.clone(),
            scope: scope.clone(),
        }
    };

    // Keep identity fields aligned with this call.
    cursor.params = params_json.clone();
    cursor.profile = profile.clone();
    cursor.scope = scope.clone();

    // Freeze selection on first entry (or when restarting without ids).
    if cursor.ordered_ids.is_empty() && cursor.cursor_index == 0 && cursor.completed_count == 0 {
        if cancel.map(|c| c()).unwrap_or(false) {
            return Ok(QcOutcome::Paused(summary_from_cursor(&cursor)));
        }
        let ordered = select_item_ids(matter, params)?;
        cursor.ordered_ids = ordered;
        cursor.candidate_count = cursor.ordered_ids.len() as u64;
        // Fingerprint includes pack id so produce gate cannot reuse a pass
        // under a different severity pack (track 0060).
        cursor.selection_fingerprint =
            selection_fingerprint_with_pack(&cursor.ordered_ids, &profile);
        // Persist selection freeze early so a cancel mid-eval resumes same set.
        write_checkpoint(matter, job_id, &cursor)?;
    }

    let candidate_count = cursor.candidate_count;
    progress(cursor.completed_count);

    // Empty selection: set-level only, then finish.
    if candidate_count == 0 {
        if cursor.findings.is_empty() {
            if let Some(f) = empty_selection_finding(&rules) {
                cursor.findings.push(f);
            }
        }
        let (error_count, warn_count) = recompute_severity_counts(&cursor.findings);
        cursor.error_count = error_count;
        cursor.warn_count = warn_count;
        cursor.completed_count = 0;
        cursor.cursor_index = 0;
        return finalize_success(
            matter,
            job_id,
            params,
            &rules_json,
            &profile,
            &rules,
            &mut cursor,
        );
    }

    let ordered = cursor.ordered_ids.clone();
    let candidate_set: HashSet<&str> = ordered.iter().map(String::as_str).collect();
    let start = cursor.cursor_index as usize;
    if start > ordered.len() {
        return Err(QcError::Other(format!(
            "checkpoint cursor_index {start} exceeds candidate count {}",
            ordered.len()
        )));
    }

    for (i, id) in ordered.iter().enumerate().skip(start) {
        if cancel.map(|c| c()).unwrap_or(false) {
            // Persist mid-scan progress so resume continues past completed items.
            write_checkpoint(matter, job_id, &cursor)?;
            return Ok(QcOutcome::Paused(summary_from_cursor(&cursor)));
        }

        let item = matter.get_item(id)?;
        let is_withheld = matter.item_is_withheld(id)?;
        if is_withheld {
            cursor.withheld_count += 1;
        }
        let item_findings = evaluate_one_item(matter, &item, is_withheld, &candidate_set, &rules)?;
        cursor.findings.extend(item_findings);

        cursor.cursor_index = (i as u64) + 1;
        cursor.completed_count = cursor.cursor_index;
        let (error_count, warn_count) = recompute_severity_counts(&cursor.findings);
        cursor.error_count = error_count;
        cursor.warn_count = warn_count;

        // Persist every item so cancel mid-scan is exact and resume skips done ids.
        write_checkpoint(matter, job_id, &cursor)?;
        progress(cursor.completed_count);
    }

    if cancel.map(|c| c()).unwrap_or(false) {
        write_checkpoint(matter, job_id, &cursor)?;
        return Ok(QcOutcome::Paused(summary_from_cursor(&cursor)));
    }

    finalize_success(
        matter,
        job_id,
        params,
        &rules_json,
        &profile,
        &rules,
        &mut cursor,
    )
}

fn finalize_success(
    matter: &Matter,
    job_id: &str,
    params: &QcParams,
    rules_json: &str,
    profile: &str,
    rules: &crate::rules::ResolvedRules,
    cursor: &mut CheckpointCursor,
) -> Result<QcOutcome> {
    // Set-level only_withheld once, after a full item scan (idempotent on resume).
    if cursor.candidate_count > 0
        && !cursor
            .findings
            .iter()
            .any(|f| f.rule_id == crate::rules::RULE_ONLY_WITHHELD)
    {
        if let Some(f) = only_withheld_finding(rules, cursor.candidate_count, cursor.withheld_count)
        {
            cursor.findings.push(f);
        }
    }

    let (error_count, warn_count) = recompute_severity_counts(&cursor.findings);
    cursor.error_count = error_count;
    cursor.warn_count = warn_count;
    let passed = error_count == 0;
    let fingerprint = cursor.selection_fingerprint.clone();
    let scope = cursor.scope.clone();
    let candidate_count = cursor.candidate_count;
    let findings = cursor.findings.clone();

    let report_dir = if let Some(ref dir) = params.report_dir {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            default_qc_report_dir(matter.root())
        } else {
            Utf8PathBuf::from(trimmed)
        }
    } else {
        default_qc_report_dir(matter.root())
    };

    let report_path = write_qc_report(
        &report_dir,
        &QcReportMeta {
            matter_id: matter.id(),
            profile,
            scope: &scope,
            passed,
            error_count,
            warn_count,
            candidate_count,
            selection_fingerprint: &fingerprint,
        },
        &findings,
    )?;

    let scope_json = serde_json::to_string(&json!({
        "scope": scope,
        "expand_family_for_scan": params.expand_family_for_scan,
        "item_ids_len": params.item_ids.len(),
    }))
    .ok();

    let record = matter.insert_qc_run(InsertQcRunInput {
        profile: profile.into(),
        passed,
        error_count,
        warn_count,
        candidate_count,
        selection_fingerprint: fingerprint.clone(),
        scope: scope.clone(),
        scope_json,
        report_path: Some(report_path.clone()),
        job_id: Some(job_id.into()),
        rules_json: Some(rules_json.into()),
    })?;

    cursor.phase = "done".into();
    write_checkpoint(matter, job_id, cursor)?;

    let report = QcReport {
        generated_at: Utc::now().to_rfc3339(),
        passed,
        error_count,
        warn_count,
        findings,
        candidate_count,
        selection_fingerprint: fingerprint,
        scope,
        profile: profile.into(),
        report_path,
        qc_run_id: record.id,
    };

    Ok(QcOutcome::Succeeded(report))
}

/// Convenience: severity of a finding for tests.
pub fn finding_is_error(f: &QcFinding) -> bool {
    f.severity == QcSeverity::Error
}
