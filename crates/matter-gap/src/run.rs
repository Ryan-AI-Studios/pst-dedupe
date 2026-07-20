//! Core gap job: collection roster + opposing set-diff + report + audit.
//!
//! Option C: no `create_job` inside the engine; accept runner-created `job_id`.

use std::path::Path;
use std::time::Instant;

use camino::Utf8PathBuf;
use chrono::Utc;
use matter_core::{AuditEventInput, InsertGapImportInput, InsertGapRunInput, Matter};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::column_map::DatColumnMap;
use crate::compare::compare_import;
use crate::dat_parse::{parse_dat_file, DatCaps};
use crate::date_coverage::{analyze_date_coverage, DateBucketRow, DateFinding};
use crate::error::{GapError, Result};
use crate::params::{
    CollectionGapParams, GapParams, OpposingGapParams, KIND_BOTH, KIND_COLLECTION, KIND_OPPOSING,
};
use crate::report::{count_severities, default_gap_report_dir, write_gap_report, GapReportMeta};
use crate::roster::{run_roster_analysis, CollectionGapAnalysis, RosterFinding};

/// Job kind string for process-runner.
pub const JOB_KIND_GAP: &str = "gap";
/// Checkpoint stage name.
pub const GAP_STAGE: &str = "gap";

/// Summary after a gap run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapSummary {
    pub kind: String,
    pub error_count: u64,
    pub warn_count: u64,
    pub finding_count: u64,
    pub report_path: String,
    pub gap_run_id: String,
    pub completed_count: u64,
}

/// Full in-memory report returned to callers.
#[derive(Debug, Clone)]
pub struct GapReport {
    pub generated_at: String,
    pub kind: String,
    pub error_count: u64,
    pub warn_count: u64,
    pub finding_count: u64,
    pub report_path: String,
    pub gap_run_id: String,
    pub roster: CollectionGapAnalysis,
    pub date_findings: Vec<DateFinding>,
    pub date_buckets: Vec<DateBucketRow>,
    pub matched_count: u64,
    pub expected_not_in_matter_count: u64,
    pub expected_doc_count: u64,
}

/// Outcome of [`run_gap`].
#[derive(Debug)]
pub enum GapOutcome {
    Succeeded(GapReport),
    Paused(GapSummary),
    Failed {
        message: String,
        summary: GapSummary,
    },
}

/// Import opposing DAT into gap_expected_docs; returns import id.
pub fn import_opposing_dat(
    matter: &Matter,
    path: &Path,
    column_map: Option<&DatColumnMap>,
    caps: DatCaps,
) -> Result<String> {
    let map = column_map
        .cloned()
        .unwrap_or_else(DatColumnMap::default_produce_v1);
    let parsed = parse_dat_file(path, &map, caps)?;
    let path_str = path.to_string_lossy().to_string();
    let map_json = serde_json::to_string(
        &map.map
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().to_string()))
            .collect::<std::collections::HashMap<_, _>>(),
    )
    .unwrap_or_else(|_| "{}".into());

    let imp = matter.insert_gap_import(InsertGapImportInput {
        kind: "opposing_dat".into(),
        path: path_str.clone(),
        row_count: parsed.rows.len() as u64,
        column_map_json: Some(map_json),
        error_count: Some(0),
    })?;
    matter.insert_gap_expected_docs(&imp.id, &parsed.rows)?;

    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "gap.opposing_import".into(),
        entity: format!("import:{}", imp.id),
        params_json: json!({
            "path": path_str,
            "row_count": parsed.rows.len(),
            "format": format!("{:?}", parsed.format),
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    Ok(imp.id)
}

/// Import expected custodians CSV and audit.
pub fn import_roster_csv(
    matter: &Matter,
    path: &Path,
) -> Result<matter_core::ImportExpectedCustodiansResult> {
    let result = matter.import_expected_custodians_csv_path(path)?;
    matter.append_audit(AuditEventInput {
        actor: "system".into(),
        action: "gap.roster_import".into(),
        entity: format!("matter:{}", matter.id()),
        params_json: json!({
            "path": path.to_string_lossy(),
            "inserted": result.inserted,
            "updated": result.updated,
            "total_rows": result.total_rows,
        })
        .to_string(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
    })?;
    Ok(result)
}

/// Run collection gap only (roster + optional date window).
pub fn run_collection_gap(
    matter: &Matter,
    params: &CollectionGapParams,
    job_id: Option<&str>,
) -> Result<GapReport> {
    params.validate_shape()?;
    let gp = GapParams {
        kind: KIND_COLLECTION.into(),
        window_start: params.window_start.clone(),
        window_end: params.window_end.clone(),
        bucket: params.bucket.clone(),
        flag_unexpected_custodian: params.flag_unexpected_custodian,
        report_dir: params.report_dir.clone(),
        ..Default::default()
    };
    match run_gap(matter, job_id.unwrap_or(""), &gp, None, |_| {})? {
        GapOutcome::Succeeded(r) => Ok(r),
        GapOutcome::Paused(s) => Err(GapError::Other(format!("paused: {}", s.report_path))),
        GapOutcome::Failed { message, .. } => Err(GapError::Other(message)),
    }
}

/// Run opposing set-diff only.
pub fn run_opposing_gap(
    matter: &Matter,
    params: &OpposingGapParams,
    job_id: Option<&str>,
) -> Result<GapReport> {
    params.validate_shape()?;
    let gp = GapParams {
        kind: KIND_OPPOSING.into(),
        import_id: Some(params.import_id.clone()),
        matter_scope: params.matter_scope.clone(),
        production_set_id: params.production_set_id.clone(),
        flag_matter_not_in_expected: params.flag_matter_not_in_expected,
        report_dir: params.report_dir.clone(),
        ..Default::default()
    };
    match run_gap(matter, job_id.unwrap_or(""), &gp, None, |_| {})? {
        GapOutcome::Succeeded(r) => Ok(r),
        GapOutcome::Paused(s) => Err(GapError::Other(format!("paused: {}", s.report_path))),
        GapOutcome::Failed { message, .. } => Err(GapError::Other(message)),
    }
}

/// Unified gap run for job kind `gap` (Option C: job_id from runner).
pub fn run_gap(
    matter: &Matter,
    job_id: &str,
    params: &GapParams,
    cancel: Option<&dyn Fn() -> bool>,
    progress: impl Fn(u64),
) -> Result<GapOutcome> {
    let started = Instant::now();
    params.validate_shape()?;
    let params_json = serde_json::to_value(params).unwrap_or_else(|_| json!({}));
    let started_at = Utc::now().to_rfc3339();

    if !job_id.is_empty() {
        matter.append_audit(AuditEventInput {
            actor: "system".into(),
            action: "gap.run.start".into(),
            entity: format!("job:{job_id}"),
            params_json: params_json.to_string(),
            tool_version: env!("CARGO_PKG_VERSION").into(),
        })?;
    }

    let result = run_gap_inner(matter, job_id, params, &started_at, cancel, &progress);

    match &result {
        Ok(GapOutcome::Succeeded(r)) => {
            if let Err(e) = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "gap.run.complete".into(),
                entity: if job_id.is_empty() {
                    format!("gap_run:{}", r.gap_run_id)
                } else {
                    format!("job:{job_id}")
                },
                params_json: json!({
                    "kind": r.kind,
                    "error_count": r.error_count,
                    "warn_count": r.warn_count,
                    "finding_count": r.finding_count,
                    "report_path": r.report_path,
                    "gap_run_id": r.gap_run_id,
                    "matched_count": r.matched_count,
                    "expected_not_in_matter_count": r.expected_not_in_matter_count,
                    "duration_ms": started.elapsed().as_millis() as u64,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            }) {
                return Ok(GapOutcome::Failed {
                    message: format!("audit complete failed: {e}"),
                    summary: summary_from_report(r),
                });
            }
        }
        Ok(GapOutcome::Paused(_)) => {}
        Ok(GapOutcome::Failed { message, summary }) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "gap.run.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({
                    "error": message,
                    "error_count": summary.error_count,
                    "warn_count": summary.warn_count,
                })
                .to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
        Err(e) => {
            let _ = matter.append_audit(AuditEventInput {
                actor: "system".into(),
                action: "gap.run.fail".into(),
                entity: format!("job:{job_id}"),
                params_json: json!({ "error": e.to_string() }).to_string(),
                tool_version: env!("CARGO_PKG_VERSION").into(),
            });
        }
    }

    result
}

fn summary_from_report(r: &GapReport) -> GapSummary {
    GapSummary {
        kind: r.kind.clone(),
        error_count: r.error_count,
        warn_count: r.warn_count,
        finding_count: r.finding_count,
        report_path: r.report_path.clone(),
        gap_run_id: r.gap_run_id.clone(),
        completed_count: r.expected_doc_count.max(r.finding_count),
    }
}

fn run_gap_inner(
    matter: &Matter,
    job_id: &str,
    params: &GapParams,
    started_at: &str,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &impl Fn(u64),
) -> Result<GapOutcome> {
    if cancel.map(|c| c()).unwrap_or(false) {
        return Ok(GapOutcome::Paused(GapSummary {
            kind: params.kind.clone(),
            ..Default::default()
        }));
    }

    let do_collection = matches!(params.kind.as_str(), KIND_COLLECTION | KIND_BOTH);
    let do_opposing = matches!(params.kind.as_str(), KIND_OPPOSING | KIND_BOTH);

    let mut roster = CollectionGapAnalysis::default();
    let mut date_findings = Vec::new();
    let mut date_buckets = Vec::new();
    let mut roster_findings: Vec<RosterFinding> = Vec::new();

    if do_collection {
        roster = run_roster_analysis(matter, params.flag_unexpected_custodian)?;
        roster_findings = roster.findings.clone();

        let item_dates = matter.list_item_best_dates()?;
        let (df, db) = analyze_date_coverage(
            &item_dates,
            params.window_start.as_deref(),
            params.window_end.as_deref(),
            &params.bucket,
        )?;
        date_findings = df;
        date_buckets = db;
    }

    let mut matched = Vec::new();
    let mut unmatched_expected = Vec::new();
    let mut expected_doc_count = 0u64;

    if do_opposing {
        let opp = params.to_opposing()?;
        let docs = matter.list_gap_expected_docs(&opp.import_id)?;
        expected_doc_count = docs.len() as u64;
        let scope = if opp.matter_scope == "production_set_id" {
            "production_set"
        } else {
            opp.matter_scope.as_str()
        };
        let matter_ids =
            matter.list_item_ids_for_gap_scope(scope, opp.production_set_id.as_deref())?;

        // Optional lightweight checkpoint for large compares
        if !job_id.is_empty() {
            let cursor = json!({
                "phase": "compare",
                "params": params,
                "expected_doc_count": expected_doc_count,
            });
            let _ = matter.put_checkpoint(job_id, GAP_STAGE, &cursor.to_string(), 0);
        }

        let cmp = compare_import(
            matter,
            &docs,
            &matter_ids,
            opp.flag_matter_not_in_expected,
            cancel,
            |n| {
                progress(n);
                if !job_id.is_empty() && n % 500 == 0 {
                    let cursor = json!({
                        "phase": "compare",
                        "cursor": n,
                        "expected_doc_count": expected_doc_count,
                    });
                    let _ = matter.put_checkpoint(job_id, GAP_STAGE, &cursor.to_string(), n as i64);
                }
            },
        );
        match cmp {
            Ok(c) => {
                matched = c.matched;
                unmatched_expected = c.unmatched_expected;
            }
            Err(GapError::Cancelled) => {
                return Ok(GapOutcome::Paused(GapSummary {
                    kind: params.kind.clone(),
                    completed_count: 0,
                    ..Default::default()
                }));
            }
            Err(e) => return Err(e),
        }
    }

    progress(expected_doc_count.max(roster_findings.len() as u64));

    let (error_count, warn_count) = count_severities(
        &roster_findings,
        &date_findings,
        unmatched_expected.len() as u64,
    );
    let finding_count =
        roster_findings.len() as u64 + date_findings.len() as u64 + unmatched_expected.len() as u64;

    let report_dir = if let Some(ref dir) = params.report_dir {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            default_gap_report_dir(matter.root())
        } else {
            Utf8PathBuf::from(trimmed)
        }
    } else {
        default_gap_report_dir(matter.root())
    };

    let expected_custodian_count = matter.list_expected_custodians(true)?.len() as u64;
    let meta = GapReportMeta {
        matter_id: matter.id(),
        kind: &params.kind,
        error_count,
        warn_count,
        finding_count,
        expected_custodian_count,
        missing_custodian_count: roster.missing.len() as u64,
        unexpected_custodian_count: roster.unexpected.len() as u64,
        expected_doc_count,
        matched_count: matched.len() as u64,
        expected_not_in_matter_count: unmatched_expected.len() as u64,
    };

    let report_path = write_gap_report(
        &report_dir,
        &meta,
        &roster_findings,
        &roster.inventory,
        &date_findings,
        &date_buckets,
        &unmatched_expected,
        &matched,
    )?;

    let finished_at = Utc::now().to_rfc3339();
    let summary_json = json!({
        "missing_custodian_count": roster.missing.len(),
        "unexpected_custodian_count": roster.unexpected.len(),
        "matched_count": matched.len(),
        "expected_not_in_matter_count": unmatched_expected.len(),
        "expected_doc_count": expected_doc_count,
    })
    .to_string();

    let record = matter.insert_gap_run(InsertGapRunInput {
        kind: params.kind.clone(),
        params_json: Some(serde_json::to_string(params).unwrap_or_else(|_| "{}".into())),
        started_at: started_at.into(),
        finished_at: Some(finished_at),
        error_count,
        warn_count,
        finding_count,
        report_path: Some(report_path.clone()),
        job_id: if job_id.is_empty() {
            None
        } else {
            Some(job_id.into())
        },
        summary_json: Some(summary_json),
    })?;

    if !job_id.is_empty() {
        let cursor = json!({ "phase": "done", "gap_run_id": record.id });
        let _ = matter.put_checkpoint(
            job_id,
            GAP_STAGE,
            &cursor.to_string(),
            expected_doc_count as i64,
        );
    }

    Ok(GapOutcome::Succeeded(GapReport {
        generated_at: Utc::now().to_rfc3339(),
        kind: params.kind.clone(),
        error_count,
        warn_count,
        finding_count,
        report_path,
        gap_run_id: record.id,
        roster,
        date_findings,
        date_buckets,
        matched_count: matched.len() as u64,
        expected_not_in_matter_count: unmatched_expected.len() as u64,
        expected_doc_count,
    }))
}
