//! `pst-dedup keep-set` — keep_set_v1 orchestration (track 0066).
//!
//! Phases: sort paths → integrity scan (collect candidates) → resolve →
//! optional materialize+promote → stream decision CSV + keep-set JSON.

use std::io::Write;
use std::path::{Path, PathBuf};

use dedup_engine::integrity::{IntegrityThresholds, ScanMode, SCAN_INTEGRITY_SCHEMA};
use dedup_engine::keepset::{
    finalize_with_materialize, recoverable_items_hint, resolve_groups_with_grouping,
    sort_input_paths, write_keep_set_json, DecisionCsvWriter, FamilyPolicy, FidelityMode,
    FolderRankMode, KeepPolicy, KeepSetProvenance, RankContext,
};
use serde::Serialize;

use crate::error::{CliError, CliExit, Result};
use crate::grouping_cli::{format_grouping_stats_human, grouping_context_from_cli};
use crate::pst_materializer::PstMaterializer;
use crate::scan::{evaluate_exit_policy, resolve_pst_paths, run_scan, ScanOptions, ScanSummary};

/// CLI options for `keep-set`.
pub struct KeepSetCliArgs {
    pub paths: Vec<PathBuf>,
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path_contains: Vec<String>,
    pub prefer_bcc_copy: bool,
    pub prefer_folder_class: bool,
    pub folder_rank: Vec<String>,
    pub source_rank: Vec<String>,
    pub rank_folder_class_first: bool,
    pub fidelity_rank: String,
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
    // 0076 identity binding
    pub strong_content_hash: String,
    pub dedupe_scope: String,
    pub tier1_verify: String,
    pub tier1_backfill: bool,
    pub identity_ignore_inline_attachments: bool,
    pub allow_cross_mid_tier2: bool,
    pub allow_degenerate_tier2: bool,
    pub allow_crc_suspect_tier2: bool,
    pub crc_log_limit: u64,
    pub crc_log_interval_secs: u64,
}

/// Resolve on-disk path for the self-locating keep-set exit-contract summary (0078 DoD-22).
///
/// Prefers sibling of `--keep-set-json`, then `--decision-csv`, then `--integrity-csv`.
/// When none of those are set (stdout-only JSON), still writes next to the first input
/// path so every run has an absolute, self-locating `summary_path`.
fn resolve_keep_set_summary_path(args: &KeepSetCliArgs, resolved_paths: &[PathBuf]) -> PathBuf {
    if let Some(anchor) = args
        .keep_set_json
        .as_ref()
        .or(args.decision_csv.as_ref())
        .or(args.integrity_csv.as_ref())
    {
        let parent = anchor.parent().unwrap_or_else(|| Path::new("."));
        return parent.join("keep_set_summary.json");
    }
    // Stdout-only: anchor beside the first input PST (always known after resolve).
    if let Some(first) = resolved_paths.first() {
        let parent = first.parent().unwrap_or_else(|| Path::new("."));
        return parent.join("keep_set_summary.json");
    }
    PathBuf::from("keep_set_summary.json")
}

fn write_keep_set_summary_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::Msg(format!(
                "create keep_set_summary.json parent {}: {e}",
                parent.display()
            ))
        })?;
    }
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(path, json).map_err(|e| {
        CliError::Msg(format!(
            "write keep_set_summary.json {}: {e}",
            path.display()
        ))
    })?;
    Ok(())
}

/// Build [`RankContext`] from CLI keep-set / unique-* flags (0075).
#[allow(clippy::too_many_arguments)]
pub fn rank_context_from_cli(
    policy: KeepPolicy,
    prefer_path_contains: &[String],
    prefer_bcc_copy: bool,
    prefer_folder_class: bool,
    folder_rank: &[String],
    source_rank: &[String],
    rank_folder_class_first: bool,
    fidelity_rank: &str,
) -> RankContext {
    let folder_rank_mode = if !folder_rank.is_empty() {
        FolderRankMode::Custom(folder_rank.to_vec())
    } else if prefer_folder_class {
        FolderRankMode::Builtin
    } else {
        FolderRankMode::Off
    };
    let fidelity_mode = FidelityMode::parse(fidelity_rank).unwrap_or(FidelityMode::Binary);
    RankContext {
        policy,
        prefer_path: prefer_path_contains.to_vec(),
        prefer_bcc_copy,
        source_rank_patterns: source_rank.to_vec(),
        folder_rank: folder_rank_mode,
        folder_class_first: rank_folder_class_first,
        fidelity_mode,
    }
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
    ok: bool,
    #[serde(default)]
    fidelity: crate::export_outcome::ExportFidelity,
    #[serde(default)]
    exit_code: u8,
    #[serde(default)]
    exit_reason: Vec<String>,
    #[serde(default)]
    artifact_state: crate::export_outcome::ArtifactState,
    /// Absolute path of this summary file (always written; self-locating).
    #[serde(default)]
    summary_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
}

/// Run keep-set orchestration end-to-end.
pub fn run_keep_set(args: KeepSetCliArgs) -> Result<()> {
    // Phase 0: resolve + deterministic sort.
    let mut paths = resolve_pst_paths(&args.paths)?;
    sort_input_paths(&mut paths);

    pst_reader::integrity_telemetry::set_log_limit(
        args.crc_log_limit,
        std::time::Duration::from_secs(args.crc_log_interval_secs),
    );

    let grouping = grouping_context_from_cli(
        args.no_tier2,
        &args.strong_content_hash,
        &args.dedupe_scope,
        &args.tier1_verify,
        args.allow_cross_mid_tier2,
        args.allow_degenerate_tier2,
        args.allow_crc_suspect_tier2,
        args.tier1_backfill,
        args.identity_ignore_inline_attachments,
    )
    .map_err(CliError::Usage)?;

    let opts = ScanOptions {
        enable_tier2: !args.no_tier2,
        include_attachments: !args.no_attachments,
        mode: args.mode,
        thresholds: IntegrityThresholds {
            max_skip_rate: args.max_skip_rate,
            max_crc_skip_rate: args.max_crc_skip_rate,
            max_failed_file_rate: args.max_failed_file_rate,
            max_attach_fail_rate: 0.05,
        },
        allow_failed_files: args.allow_failed_files,
        integrity_csv: args.integrity_csv.clone(),
        csv: None, // keep-set decision CSV is Phase 3 only (not first-seen mid-scan)
        skip_limit: args.skip_limit,
        retain_rows: false,
        retain_candidates: true,
        cancel: None,
        grouping: grouping.clone(),
        ..Default::default()
    };

    // Phase 1: integrity-aware scan collecting candidates.
    // Dual-rate poly sources reclassify (clear) false-positive CRC_SUSPECT in
    // run_scan so keep-set sees clean identity without Tier-2 auto-allow.
    let outcome = run_scan(&paths, &opts)?;

    let provenance = KeepSetProvenance {
        scan_integrity_schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: args.mode.as_str().to_string(),
        input_files: paths.iter().map(|p| p.display().to_string()).collect(),
    };

    // Phase 2: resolve (fidelity → evidence rungs → policy → path/nid).
    let rank_ctx = rank_context_from_cli(
        args.policy,
        &args.prefer_path_contains,
        args.prefer_bcc_copy,
        args.prefer_folder_class,
        &args.folder_rank,
        &args.source_rank,
        args.rank_folder_class_first,
        &args.fidelity_rank,
    );
    let mut resolved = resolve_groups_with_grouping(
        outcome.candidates,
        args.family_policy,
        &rank_ctx,
        &grouping,
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
    if let Some(hint) = recoverable_items_hint(keep_set.stats.winners_from_recoverable_items) {
        if !args.json {
            eprintln!("note: {hint}");
        }
    }

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

    // DoD-22: always establish a self-locating summary path (even pure --json).
    let summary_disk = resolve_keep_set_summary_path(&args, &paths);
    let summary_path_str = std::path::absolute(&summary_disk)
        .unwrap_or_else(|_| summary_disk.clone())
        .display()
        .to_string();

    // 0078: keep-set is not an export artifact write, but shares classify_export for
    // scan integrity → fidelity/exit (no attach soft-fail path here).
    let mut report_ok = true;
    let mut classified = crate::export_outcome::classify_export(
        crate::export_outcome::ExportOkInput {
            scan_ok: exit_err.is_none(),
            verify_ok: true,
            export_err_absent: true,
            export_partial: false,
            messages_written_total: keep_set.stats.unique,
            unique: keep_set.stats.unique,
            attach_failed_total: 0,
            body_soft_fail_total: 0,
            report_ok,
        },
        outcome.summary.preflight.recommendation,
        crate::export_outcome::RiskGate::Off,
        true,
        false,
    );

    let mut error_obj = exit_err.as_ref().map(|msg| {
        serde_json::json!({
            "code": "scan_integrity",
            "message": msg,
        })
    });

    let build_summary = |classified: &crate::export_outcome::ExportOutcome,
                         error: &Option<serde_json::Value>|
     -> Result<serde_json::Value> {
        let ok = classified.fidelity == crate::export_outcome::ExportFidelity::Complete;
        let payload = KeepSetSummaryOut {
            schema: keep_set.schema.clone(),
            policy: args.policy.as_str().to_string(),
            family_policy: args.family_policy.as_str().to_string(),
            keep_set: keep_set.clone(),
            scan: outcome.summary.clone(),
            decision_csv: decision_csv_out.clone(),
            keep_set_json: keep_set_json_out.clone(),
            materialized: materialized_count,
            ok,
            fidelity: classified.fidelity,
            exit_code: classified.exit.as_u8(),
            exit_reason: classified
                .reasons
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            artifact_state: crate::export_outcome::ArtifactState::Absent,
            summary_path: summary_path_str.clone(),
            error: error.clone(),
        };
        Ok(serde_json::to_value(&payload)?)
    };

    let mut summary_value = build_summary(&classified, &error_obj)?;

    // Fail-closed: summary write failure is a report failure (DoD-22).
    if let Err(e) = write_keep_set_summary_json(&summary_disk, &summary_value) {
        report_ok = false;
        let msg = format!("keep_set_summary.json write failed: {e}");
        tracing::warn!(path = %summary_disk.display(), "{msg}");
        error_obj = Some(serde_json::json!({
            "code": "report",
            "message": msg,
        }));
        classified = crate::export_outcome::classify_export(
            crate::export_outcome::ExportOkInput {
                scan_ok: exit_err.is_none(),
                verify_ok: true,
                export_err_absent: true,
                export_partial: false,
                messages_written_total: keep_set.stats.unique,
                unique: keep_set.stats.unique,
                attach_failed_total: 0,
                body_soft_fail_total: 0,
                report_ok,
            },
            outcome.summary.preflight.recommendation,
            crate::export_outcome::RiskGate::Off,
            true,
            false,
        );
        summary_value = build_summary(&classified, &error_obj)?;
        // Best-effort rewrite of corrected summary (may still fail).
        let _ = write_keep_set_summary_json(&summary_disk, &summary_value);
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary_value)?);
        if classified.exit != CliExit::Success {
            let msg = exit_err
                .or_else(|| {
                    error_obj
                        .as_ref()
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "keep-set failed".into());
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: classified.exit,
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
    for line in format_grouping_stats_human(&keep_set.stats.grouping) {
        println!("{line}");
    }
    println!("  degraded winners: {}", keep_set.stats.degraded_winners);
    println!(
        "  materialize_failed: {}  promoted: {}  groups_dropped_materialize: {}",
        keep_set.stats.materialize_failed,
        keep_set.stats.promoted_from_failure,
        keep_set.stats.groups_dropped_materialize
    );
    // 0075 honesty counters (always printed, including when 0).
    println!(
        "  winners_from_recoverable_items: {}",
        keep_set.stats.winners_from_recoverable_items
    );
    println!(
        "  winners_without_bcc_peer_had_bcc: {}",
        keep_set.stats.winners_without_bcc_peer_had_bcc
    );
    println!(
        "  groups_date_source_mixed: {}",
        keep_set.stats.groups_date_source_mixed
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
    if !summary_path_str.is_empty() {
        println!("  summary:       {summary_path_str}");
    }
    if args.materialize {
        println!("  materialized:  {materialized_count}");
    }
    if let Some(ic) = &outcome.summary.integrity_csv {
        println!("  integrity_csv: {ic}");
    }

    if classified.exit != CliExit::Success {
        if !summary_path_str.is_empty() {
            let _ = writeln!(std::io::stderr(), "summary: {summary_path_str}");
        }
        let msg = exit_err.unwrap_or_else(|| "keep-set failed".into());
        return Err(CliError::AlreadyEmitted {
            message: msg,
            exit: classified.exit,
        });
    }
    Ok(())
}
