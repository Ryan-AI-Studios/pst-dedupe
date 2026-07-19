//! Core production QC job: select → evaluate → report → persist qc_runs.

use std::time::Instant;

use camino::Utf8PathBuf;
use chrono::Utc;
use matter_core::{selection_fingerprint, AuditEventInput, InsertQcRunInput, Matter};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{QcError, Result};
use crate::params::{QcParams, QcSeverity};
use crate::report::{count_severities, default_qc_report_dir, write_qc_report, QcReportMeta};
use crate::rules::{evaluate_candidates, resolve_rules, QcFinding};
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
    cursor_index: u64,
    completed_count: u64,
    candidate_count: u64,
    params: serde_json::Value,
    #[serde(default)]
    ordered_ids: Vec<String>,
}

/// Run production QC on `matter` for the runner-created `job_id`.
///
/// Does **not** call `create_job` (Option C). Honors `cancel` between items.
/// Calls `progress(completed_count)` during evaluation.
pub fn run_production_qc(
    matter: &Matter,
    job_id: &str,
    params: &QcParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<QcOutcome> {
    let started = Instant::now();
    let effective = params.clone();
    effective.validate_shape()?;
    let params_json = serde_json::to_value(&effective).unwrap_or_else(|_| json!({}));
    let rules = resolve_rules(&effective.rules);
    // Prefer explicit profile from params; pack name is fallback identity.
    let profile = if effective.profile.trim().is_empty() {
        rules.profile.clone()
    } else {
        effective.profile.clone()
    };
    let rules_json = serde_json::to_string(&rules.to_configs()).unwrap_or_else(|_| "[]".into());

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "qc.start".into(),
        entity: format!("job:{job_id}"),
        params_json: json!({
            "params": params_json,
            "profile": profile,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let result = run_qc_inner(
        matter,
        job_id,
        &effective,
        &profile,
        &rules_json,
        cancel,
        &progress,
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

fn run_qc_inner(
    matter: &Matter,
    job_id: &str,
    params: &QcParams,
    profile: &str,
    rules_json: &str,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<QcOutcome> {
    if cancel.map(|c| c()).unwrap_or(false) {
        return Ok(QcOutcome::Paused(QcSummary::default()));
    }

    let ordered = select_item_ids(matter, params)?;
    let candidate_count = ordered.len() as u64;
    let fingerprint = selection_fingerprint(&ordered);
    let scope = params.scope.clone();

    // Cooperative cancel between items during load/eval (P0: single-pass evaluate).
    if cancel.map(|c| c()).unwrap_or(false) {
        return Ok(QcOutcome::Paused(QcSummary {
            candidate_count,
            selection_fingerprint: fingerprint,
            scope,
            profile: profile.into(),
            ..Default::default()
        }));
    }

    progress(0);
    let rules = resolve_rules(&params.rules);
    let findings = evaluate_candidates(matter, &ordered, &rules)?;
    progress(candidate_count);

    if cancel.map(|c| c()).unwrap_or(false) {
        return Ok(QcOutcome::Paused(QcSummary {
            completed_count: candidate_count,
            candidate_count,
            selection_fingerprint: fingerprint,
            scope,
            profile: profile.into(),
            ..Default::default()
        }));
    }

    let (error_count, warn_count) = count_severities(&findings);
    let passed = error_count == 0;

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

    // Lightweight checkpoint for resume visibility
    let cursor = CheckpointCursor {
        cursor_index: candidate_count,
        completed_count: candidate_count,
        candidate_count,
        params: serde_json::to_value(params).unwrap_or_else(|_| json!({})),
        ordered_ids: ordered,
    };
    if let Ok(cursor_json) = serde_json::to_string(&cursor) {
        let _ = matter.put_checkpoint(job_id, QC_STAGE, &cursor_json, candidate_count as i64);
    }

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
