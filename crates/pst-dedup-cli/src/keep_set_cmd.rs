//! `pst-dedup keep-set` — keep_set_v1 orchestration (track 0066).
//!
//! Phases: sort paths → integrity scan (collect candidates) → resolve →
//! optional materialize+promote → stream decision CSV + keep-set JSON.

use std::path::PathBuf;

use dedup_engine::integrity::{IntegrityThresholds, ScanMode, SCAN_INTEGRITY_SCHEMA};
use dedup_engine::keepset::{
    finalize_with_materialize, resolve_groups, sort_input_paths, write_keep_set_json,
    DecisionCsvWriter, FamilyPolicy, KeepPolicy, KeepSetProvenance,
};
use serde::Serialize;

use crate::error::{CliError, Result};
use crate::pst_materializer::PstMaterializer;
use crate::scan::{evaluate_exit_policy, resolve_pst_paths, run_scan, ScanOptions, ScanSummary};

/// CLI options for `keep-set`.
pub struct KeepSetCliArgs {
    pub paths: Vec<PathBuf>,
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path_contains: Vec<String>,
    pub decision_csv: Option<PathBuf>,
    pub keep_set_json: Option<PathBuf>,
    pub materialize: bool,
    pub no_tier2: bool,
    pub no_attachments: bool,
    pub json: bool,
    pub mode: ScanMode,
    pub max_skip_rate: f64,
    pub max_crc_skip_rate: f64,
    pub max_failed_file_rate: f64,
    pub allow_failed_files: bool,
    pub integrity_csv: Option<PathBuf>,
    pub skip_limit: usize,
}

#[derive(Debug, Serialize)]
struct KeepSetSummaryOut {
    schema: String,
    policy: String,
    family_policy: String,
    keep_set: dedup_engine::KeepSet,
    scan: ScanSummary,
    decision_csv: Option<String>,
    keep_set_json: Option<String>,
    materialized: u64,
}

/// Run keep-set orchestration end-to-end.
pub fn run_keep_set(args: KeepSetCliArgs) -> Result<()> {
    // Phase 0: resolve + deterministic sort.
    let mut paths = resolve_pst_paths(&args.paths)?;
    sort_input_paths(&mut paths);

    let opts = ScanOptions {
        enable_tier2: !args.no_tier2,
        include_attachments: !args.no_attachments,
        mode: args.mode,
        thresholds: IntegrityThresholds {
            max_skip_rate: args.max_skip_rate,
            max_crc_skip_rate: args.max_crc_skip_rate,
            max_failed_file_rate: args.max_failed_file_rate,
        },
        allow_failed_files: args.allow_failed_files,
        integrity_csv: args.integrity_csv.clone(),
        csv: None, // keep-set decision CSV is Phase 3 only (not first-seen mid-scan)
        skip_limit: args.skip_limit,
        retain_rows: false,
        retain_candidates: true,
        cancel: None,
    };

    // Phase 1: integrity-aware scan collecting candidates.
    let outcome = run_scan(&paths, &opts)?;

    let provenance = KeepSetProvenance {
        scan_integrity_schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: args.mode.as_str().to_string(),
        input_files: paths.iter().map(|p| p.display().to_string()).collect(),
    };

    // Phase 2: resolve (fidelity → policy → deterministic order).
    let mut resolved = resolve_groups(
        outcome.candidates,
        args.policy,
        args.family_policy,
        &args.prefer_path_contains,
        !args.no_tier2,
        Some(provenance),
    );

    // Phase 2b: materialize + promote when requested.
    let mut materialized_count = 0u64;
    if args.materialize {
        let mut mat = PstMaterializer::new(args.family_policy);
        // O(1) body memory: callback receives one winner at a time and drops it.
        materialized_count = finalize_with_materialize(&mut resolved, &mut mat, &mut |_msg| Ok(()))
            .map_err(|e| CliError::Msg(format!("materialize: {e}")))?;
    }

    // Phase 3: stream decision CSV + keep-set JSON from finalized roles.
    let keep_set = resolved.to_keep_set();

    let mut decision_csv_out: Option<String> = None;
    if let Some(path) = &args.decision_csv {
        let mut wtr = DecisionCsvWriter::create(path).map_err(|e| CliError::CsvWrite {
            path: path.clone(),
            source: Box::new(e),
        })?;
        resolved
            .write_decisions_csv(&mut wtr)
            .map_err(|e| CliError::CsvWrite {
                path: path.clone(),
                source: Box::new(e),
            })?;
        wtr.flush().map_err(|e| CliError::CsvWrite {
            path: path.clone(),
            source: Box::new(e),
        })?;
        decision_csv_out = Some(path.display().to_string());
    }

    let mut keep_set_json_out: Option<String> = None;
    if let Some(path) = &args.keep_set_json {
        write_keep_set_json(path, &keep_set).map_err(|e| CliError::Msg(e.to_string()))?;
        keep_set_json_out = Some(path.display().to_string());
    }

    // Exit policy after artifacts flushed.
    let exit_err = evaluate_exit_policy(&outcome.summary, &opts).err();

    if args.json {
        let ok = exit_err.is_none();
        let payload = KeepSetSummaryOut {
            schema: keep_set.schema.clone(),
            policy: args.policy.as_str().to_string(),
            family_policy: args.family_policy.as_str().to_string(),
            keep_set,
            scan: outcome.summary,
            decision_csv: decision_csv_out,
            keep_set_json: keep_set_json_out,
            materialized: materialized_count,
        };
        let mut v = serde_json::to_value(&payload)?;
        if let Some(obj) = v.as_object_mut() {
            obj.insert("ok".into(), serde_json::Value::Bool(ok));
            if let Some(msg) = &exit_err {
                obj.insert(
                    "error".into(),
                    serde_json::json!({
                        "code": "scan_integrity",
                        "message": msg,
                    }),
                );
            }
        }
        println!("{}", serde_json::to_string_pretty(&v)?);
        if let Some(msg) = exit_err {
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: crate::error::CliExit::Generic,
            });
        }
        return Ok(());
    }

    // Human summary.
    println!(
        "=== Keep-set ({}) policy={} family={} ===",
        keep_set.schema,
        args.policy.as_str(),
        args.family_policy.as_str()
    );
    println!("  recoverable:   {}", keep_set.stats.recoverable);
    println!("  unique:        {}", keep_set.stats.unique);
    println!("  duplicates:    {}", keep_set.stats.duplicates);
    println!(
        "  tier1 dups:    {}  tier2 dups: {}",
        keep_set.stats.tier1_dups, keep_set.stats.tier2_dups
    );
    println!("  degraded winners: {}", keep_set.stats.degraded_winners);
    println!(
        "  materialize_failed: {}  promoted: {}  groups_dropped_materialize: {}",
        keep_set.stats.materialize_failed,
        keep_set.stats.promoted_from_failure,
        keep_set.stats.groups_dropped_materialize
    );
    println!(
        "  scan: skipped={} failed_files={} preflight={}",
        outcome.summary.skipped,
        outcome.summary.failed_files,
        outcome.summary.preflight.recommendation.as_str()
    );
    if let Some(p) = &decision_csv_out {
        println!("  decision_csv:  {p}");
    }
    if let Some(p) = &keep_set_json_out {
        println!("  keep_set_json: {p}");
    }
    if args.materialize {
        println!("  materialized:  {materialized_count}");
    }
    if let Some(ic) = &outcome.summary.integrity_csv {
        println!("  integrity_csv: {ic}");
    }

    if let Some(msg) = exit_err {
        return Err(CliError::Msg(msg));
    }
    Ok(())
}
