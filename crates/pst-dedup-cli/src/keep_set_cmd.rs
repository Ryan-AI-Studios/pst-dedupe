//! `pst-dedup keep-set` — keep_set_v1 orchestration (track 0066).
//!
//! Phases: sort paths → integrity scan (collect candidates) → resolve →
//! optional materialize+promote → stream decision CSV + keep-set JSON.

use std::path::PathBuf;

use dedup_engine::integrity::{IntegrityThresholds, ScanMode, SCAN_INTEGRITY_SCHEMA};
use dedup_engine::keepset::{
    finalize_with_materialize, recoverable_items_hint, resolve_groups_with_grouping,
    sort_input_paths, write_keep_set_json, DecisionCsvWriter, FamilyPolicy, FidelityMode,
    FolderRankMode, KeepPolicy, KeepSetProvenance, RankContext,
};
use serde::Serialize;

use crate::error::{CliError, Result};
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
