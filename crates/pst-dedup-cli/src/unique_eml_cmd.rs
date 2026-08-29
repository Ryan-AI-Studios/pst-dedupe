//! `pst-dedup unique-eml` — keep_set_v1 → volume-batched EML pack (track 0067).
//!
//! No re-dedupe: winners come only from `finalize_with_materialize`. Source PSTs
//! are read-only. Pack layout is always volume-batched (`VOL001`…).
//!
//! Export order matches `KeepSet.winners` (path+nid sort): finalize promotes first,
//! then each winner is re-materialized once and written in keep-set order.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::grouping_cli::{format_grouping_stats_human, grouping_context_from_cli};
use crate::keep_set_cmd::rank_context_from_cli;
use dedup_engine::integrity::{IntegrityThresholds, ScanMode, SCAN_INTEGRITY_SCHEMA};
use dedup_engine::keepset::{
    finalize_with_materialize_opts, recoverable_items_hint, resolve_groups_with_grouping,
    sort_input_paths, write_keep_set_json, CanonicalMessage, DecisionCsvWriter, FamilyPolicy,
    KeepPolicy, KeepSetProvenance, MaterializeFinalizeOpts, MessageMaterializer,
};
use dedup_engine::{
    clamp_files_per_volume, merge_pack_degraded, validate_volume_prefix, write_canonical_eml,
    write_eml_pack_manifest, EmlPackManifest, EmlPackMessageRow, EmlWriteOpts, VolumePackWriter,
    EML_PACK_SCHEMA,
};
use serde::Serialize;

use crate::error::{CliError, Result};
use crate::paths::{is_same_or_under, paths_equal, resolve_cli_path_maybe_missing};
use crate::pst_materializer::{
    materialize_nested_for_winner, PstAttachStreamSource, PstMaterializer,
};
use crate::scan::{evaluate_exit_policy, resolve_pst_paths, run_scan, ScanOptions, ScanSummary};
use crate::unique_export_report::{
    format_ledger_source_path, resolve_input_source_id, AttachLedgerFinish, AttachLedgerMode,
    AttachLedgerRow, AttachLedgerSink, LedgerPathMode, EXPORT_ATTACHMENTS_CSV_NAME,
};
use dedup_engine::EmlAttachEvent;
use std::collections::BTreeMap;

/// CLI options for `unique-eml`.
pub struct UniqueEmlCliArgs {
    pub paths: Vec<PathBuf>,
    pub out: PathBuf,
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
    pub manifest_json: Option<PathBuf>,
    pub overwrite: bool,
    pub files_per_volume: u32,
    pub volume_prefix: String,
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
    pub strong_content_hash: String,
    /// Max attaches full-stream digested under body-recip-attach (0086).
    pub strong_hash_attach_max_attaches: u64,
    /// Max digest bytes per run under body-recip-attach (0086).
    pub strong_hash_attach_max_bytes: u64,
    /// Per-attach max digest bytes under body-recip-attach (0086).
    pub strong_hash_attach_per_attach_max_bytes: u64,
    pub dedupe_scope: String,
    pub tier1_verify: String,
    pub tier1_backfill: bool,
    pub identity_ignore_inline_attachments: bool,
    pub allow_cross_mid_tier2: bool,
    pub allow_degenerate_tier2: bool,
    pub allow_crc_suspect_tier2: bool,
    pub crc_log_limit: u64,
    pub crc_log_interval_secs: u64,
    /// Fail (exit 64) on partial fidelity (default true; 0078).
    pub fail_on_partial_fidelity: bool,
    /// Allow partial → exit 0 (default false; 0078).
    pub allow_partial_fidelity: bool,
    /// Opt-in risk gate level (0078).
    pub fail_on_export_risk: Option<String>,
    /// Mode A pre-write promote-on-attach-fail (0083). Default false.
    pub promote_on_attach_fail: bool,
    /// Attachment failure ledger: `full` (default CSV+histogram), `summary-only`, or `off` (0089).
    pub attach_ledger: AttachLedgerMode,
    /// Max rows written to `export_attachments.csv` (default 500000).
    pub attach_ledger_max_rows: u64,
    /// How `source_path` columns are written: `full` (default) or `basename` (0081/0089).
    pub ledger_path_mode: LedgerPathMode,
    /// Nested ATTACH_EMBEDDED_MSG extract/write depth (0106). Clamped [1, 8] at runtime.
    pub max_embedded_depth: u32,
}

#[derive(Debug, Serialize)]
struct UniqueEmlSummaryOut {
    schema: String,
    eml_pack_schema: String,
    policy: String,
    family_policy: String,
    keep_set: dedup_engine::KeepSet,
    scan: ScanSummary,
    out: String,
    manifest_json: String,
    decision_csv: Option<String>,
    keep_set_json: Option<String>,
    eml_written: u64,
    unique: u64,
    volumes: u64,
    attach_parts_written: u64,
    embedded_messages_written: u64,
    /// Data-path attach fail counter for fidelity (0078); classify source of truth.
    attach_parts_failed: u64,
    /// Effective nested extract/write depth (always serialized; 0106).
    max_embedded_depth: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachment_ledger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachment_ledger_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachment_ledger_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachment_ledger_rows_written: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attachments_failed_by_reason: Option<BTreeMap<String, u64>>,
    #[serde(default)]
    fidelity: crate::export_outcome::ExportFidelity,
    #[serde(default)]
    exit_code: u8,
    #[serde(default)]
    exit_reason: Vec<String>,
    #[serde(default)]
    artifact_state: crate::export_outcome::ArtifactState,
    #[serde(default)]
    summary_path: String,
}

/// Run unique-eml orchestration end-to-end.
pub fn run_unique_eml(args: UniqueEmlCliArgs) -> Result<crate::error::CliExit> {
    // Phase 0: resolve + deterministic sort.
    let mut paths = resolve_pst_paths(&args.paths)?;
    sort_input_paths(&mut paths);

    // CLI clamp only; VolumePackWriter accepts any ≥1 for tests.
    let files_per_volume = clamp_files_per_volume(args.files_per_volume);
    let volume_prefix = if args.volume_prefix.is_empty() {
        "VOL".to_string()
    } else {
        args.volume_prefix.clone()
    };
    validate_volume_prefix(&volume_prefix)
        .map_err(|e| CliError::Usage(format!("invalid --volume-prefix {volume_prefix:?}: {e}")))?;

    // Resolve --out (may not exist yet) before any create/clear.
    let out = resolve_cli_path_maybe_missing(&args.out)?.into_std_path_buf();
    let manifest_path = match &args.manifest_json {
        Some(p) => resolve_cli_path_maybe_missing(p)?.into_std_path_buf(),
        None => out.join("manifest.json"),
    };
    let decision_csv = match &args.decision_csv {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => None,
    };
    let keep_set_json = match &args.keep_set_json {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => None,
    };
    let integrity_csv = match &args.integrity_csv {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => None,
    };

    // Refuse layouts that would delete or overwrite source PSTs (especially --overwrite).
    guard_unique_eml_paths(
        &paths,
        &out,
        decision_csv.as_deref(),
        keep_set_json.as_deref(),
        &manifest_path,
        integrity_csv.as_deref(),
    )?;

    // Prepare out dir: create if missing; refuse non-empty unless --overwrite.
    prepare_out_dir(&out, args.overwrite)?;

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
        args.no_attachments,
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
        integrity_csv: integrity_csv.clone(),
        csv: None,
        skip_limit: args.skip_limit,
        retain_rows: false,
        retain_candidates: true,
        cancel: None,
        grouping: grouping.clone(),
        strong_hash_attach_max_attaches: args.strong_hash_attach_max_attaches,
        strong_hash_attach_max_bytes: args.strong_hash_attach_max_bytes,
        strong_hash_attach_per_attach_max_bytes: args.strong_hash_attach_per_attach_max_bytes,
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

    // Phase 2b: promote winners only (no EML write). Bodies are streamed one-at-a-time
    // and dropped; export order is applied in phase 2c so counters match keep_set.winners.
    let nested_depth = args.max_embedded_depth.clamp(1, 8);
    let mut mat = PstMaterializer::new(args.family_policy);
    let mut attach_src = PstAttachStreamSource::new();
    let write_opts = EmlWriteOpts {
        family_policy: args.family_policy,
        max_embedded_depth: nested_depth,
    };

    let mat_opts = MaterializeFinalizeOpts {
        promote_on_attach_fail: args.promote_on_attach_fail,
    };
    let materialized_count =
        finalize_with_materialize_opts(&mut resolved, &mut mat, &mat_opts, &mut |_msg| Ok(()))
            .map_err(|e| CliError::Msg(format!("materialize/promote: {e}")))?;

    let keep_set = resolved.to_keep_set();
    if let Some(hint) = recoverable_items_hint(keep_set.stats.winners_from_recoverable_items) {
        if !args.json {
            eprintln!("note: {hint}");
        }
    }

    // 0089: attach ledger at pack root (`{out}/export_attachments.csv`).
    let input_path_strings: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    let mut ledger_init_error: Option<String> = None;
    let mut attach_ledger = match AttachLedgerSink::new(
        args.attach_ledger,
        args.attach_ledger_max_rows,
        &out,
        &input_path_strings,
        args.ledger_path_mode,
    ) {
        Ok(s) => Some(s),
        Err(e) => {
            let msg = format!("attach ledger init failed: {e}");
            tracing::warn!("{msg}");
            if args.attach_ledger != AttachLedgerMode::Off {
                // Operator requested ledger — fail closed (do not continue with None).
                ledger_init_error = Some(msg);
            }
            None
        }
    };

    // 0083 Mode A honesty: mark promoted winners + emit soft-skip incomplete rows.
    // Drain soft_skip records before any consume/drop of resolved attach-skip data.
    if let Some(ledger) = attach_ledger.as_mut() {
        for w in &keep_set.winners {
            if w.promoted_from_failure {
                ledger.mark_promoted_winner(&w.locus.source_path, w.locus.nid);
            }
        }
        for rec in &resolved.soft_skip_attach_records {
            let source_id = resolve_input_source_id(&rec.source_path, &input_path_strings);
            let peer_source_id =
                resolve_input_source_id(&rec.peer_source_path, &input_path_strings)
                    .map(|id| id.to_string())
                    .unwrap_or_default();
            let row = AttachLedgerRow {
                source_id: source_id.map(|id| id.to_string()).unwrap_or_default(),
                source_path: format_ledger_source_path(&rec.source_path, args.ledger_path_mode),
                folder_path: rec.folder_path.clone(),
                msg_nid: rec.msg_nid,
                attach_nid: rec.attach_nid.map(|n| n.to_string()).unwrap_or_default(),
                attach_index: rec.attach_index,
                filename: rec.filename.clone(),
                size: if rec.size == 0 {
                    String::new()
                } else {
                    rec.size.to_string()
                },
                attach_method: rec.attach_method,
                reason_code: rec.reason_code.clone(),
                severity: "fail".into(),
                volume_path: String::new(),
                volume_index: String::new(),
                winner_promoted: true,
                peer_source_id,
                peer_msg_nid: rec.peer_msg_nid.to_string(),
                message_subject: String::new(),
                cloud_provider: rec.cloud_provider.clone(),
                cloud_url: rec.cloud_url.clone(),
            };
            ledger.enqueue_soft_skip_row(row);
        }
    }

    // Phase 2c: write EMLs in keep_set.winners order (path+nid), re-materializing each
    // winner once so export counters match keep_set.json stability without holding all bodies.
    let mut pack = VolumePackWriter::new(out.clone(), files_per_volume, volume_prefix)
        .map_err(|e| CliError::Msg(format!("volume pack: {e}")))?;
    let mut manifest = EmlPackManifest::new(
        args.policy.as_str(),
        args.family_policy.as_str(),
        files_per_volume,
        paths.iter().map(|p| p.display().to_string()).collect(),
    );
    let mut write_errors: Vec<String> = Vec::new();

    // Export only post-promotion keep_set.winners (no losers / non-exportable peers).
    for entry in &keep_set.winners {
        let mut msg = match mat.materialize(&entry.locus) {
            Ok(m) => m,
            Err(e) => {
                // Winner already promoted; re-materialize should succeed. Soft-continue.
                write_errors.push(format!("nid={:#x} re-materialize: {e}", entry.locus.nid));
                continue;
            }
        };
        // Carry scan keys + post-promotion fidelity from keep entry (same as finalize fill).
        msg.message_id_norm = entry.message_id_norm.clone();
        msg.content_hash = entry.content_hash;
        msg.edrm_mih_hex = entry.edrm_mih_hex.clone();
        msg.fidelity = entry.integrity.clone();

        if let Err(e) = materialize_nested_for_winner(&mut attach_src, &mut msg, nested_depth) {
            tracing::warn!("nested extract nid={:#x}: {e}", msg.locus.nid);
        }

        let (abs_path, relpath) = match pack.next_eml_path(&msg) {
            Ok(v) => v,
            Err(e) => {
                write_errors.push(format!("nid={:#x} path: {e}", msg.locus.nid));
                continue;
            }
        };

        match write_canonical_eml(&abs_path, &msg, &mut attach_src, &write_opts) {
            Ok(wres) => {
                let fidelity_reasons = msg
                    .fidelity
                    .degraded_reasons
                    .iter()
                    .map(|r| r.as_str().to_string())
                    .collect();
                // M4: attach soft-skips must surface as degraded on the manifest row.
                let (degraded, degraded_reasons) =
                    merge_pack_degraded(msg.fidelity.degraded, fidelity_reasons, &wres);
                if degraded {
                    manifest.stats.degraded_messages += 1;
                }
                manifest.stats.eml_written += 1;
                manifest.stats.attach_parts_written += wres.attachments_file_written;
                manifest.stats.embedded_messages_written += wres.embedded_messages_written;
                manifest.stats.attach_parts_failed += wres.attachments_failed;

                if let Some(ledger) = attach_ledger.as_mut() {
                    let vol_path = abs_path
                        .parent()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    ledger.set_volume(&vol_path, pack.current_volume);
                    for ev in &wres.attachment_events {
                        let row = attach_ledger_row_from_eml_event(
                            ev,
                            &msg,
                            &input_path_strings,
                            args.ledger_path_mode,
                            &vol_path,
                            pack.current_volume,
                            ledger
                                .promoted_winner_loci
                                .contains(&(msg.locus.source_path.clone(), msg.locus.nid)),
                        );
                        ledger.enqueue_soft_skip_row(row);
                    }
                }

                let content_hash_hex = msg
                    .content_hash
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();

                manifest.messages.push(EmlPackMessageRow {
                    eml_relpath: relpath,
                    source_path: msg.locus.source_path.clone(),
                    folder: msg.locus.folder_path.clone(),
                    nid: msg.locus.nid,
                    message_id_norm: msg.message_id_norm.clone(),
                    edrm_mih: msg.edrm_mih_hex.clone(),
                    content_hash_hex,
                    degraded,
                    degraded_reasons,
                    body_incomplete: msg.body_incomplete,
                    body_unavailable: msg.body_unavailable,
                    attachment_count: msg.attachments.len() as u64,
                    attachments_file_written: wres.attachments_file_written,
                    embedded_messages_written: wres.embedded_messages_written,
                    attachments_failed: wres.attachments_failed,
                    embedded_message_unparsed: wres.embedded_message_unparsed,
                });
            }
            Err(e) => {
                write_errors.push(format!("{}: {e}", abs_path.display()));
                let _ = fs::remove_file(&abs_path);
            }
        }
    }

    manifest.stats.unique = keep_set.stats.unique;
    manifest.stats.materialize_failed = keep_set.stats.materialize_failed;
    manifest.stats.volumes = pack.volumes_created;

    // Phase 3: flush decision CSV + keep-set JSON + pack manifest before exit.
    let mut decision_csv_out: Option<String> = None;
    if let Some(path) = &decision_csv {
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
    if let Some(path) = &keep_set_json {
        write_keep_set_json(path, &keep_set).map_err(|e| CliError::Msg(e.to_string()))?;
        keep_set_json_out = Some(path.display().to_string());
    }

    write_eml_pack_manifest(&manifest_path, &manifest)
        .map_err(|e| CliError::Msg(format!("manifest: {e}")))?;

    let exit_err = evaluate_exit_policy(&outcome.summary, &opts).err();

    // Success invariant: eml_written == unique (exportable post-promotion).
    let count_mismatch = manifest.stats.eml_written != keep_set.stats.unique;
    let mut pack_err = if count_mismatch {
        Some(format!(
            "eml_written ({}) != unique ({}); write_errors={:?}",
            manifest.stats.eml_written, keep_set.stats.unique, write_errors
        ))
    } else if !write_errors.is_empty() {
        Some(format!("partial eml write errors: {write_errors:?}"))
    } else {
        None
    };

    // Flush attach ledger before classify; init/flush errors are report hard-fail.
    let mut report_ok = true;
    if let Some(msg) = ledger_init_error.take() {
        report_ok = false;
        if pack_err.is_none() {
            pack_err = Some(msg);
        }
    }
    let attach_ledger_finish = match attach_ledger.take() {
        Some(ledger) => match ledger.finish() {
            Ok(f) => Some(f),
            Err(e) => {
                let msg = format!("attach ledger flush failed: {e}");
                tracing::warn!("{msg}");
                report_ok = false;
                if pack_err.is_none() {
                    pack_err = Some(msg);
                }
                None
            }
        },
        None => None,
    };

    // 0078/0089: attach_parts_failed counters remain classify ground truth (ledger additive).
    let attach_failed = manifest.stats.attach_parts_failed;
    // Pack hard fail only when count mismatch or write errors; attach soft alone is partial.
    let export_ok_input = crate::export_outcome::ExportOkInput {
        scan_ok: exit_err.is_none(),
        verify_ok: true,
        export_err_absent: write_errors.is_empty(),
        export_partial: count_mismatch || !write_errors.is_empty(),
        messages_written_total: manifest.stats.eml_written,
        unique: keep_set.stats.unique,
        attach_failed_total: attach_failed,
        body_soft_fail_total: 0,
        report_ok,
    };
    let risk_gate = args
        .fail_on_export_risk
        .as_deref()
        .and_then(crate::export_outcome::RiskGate::parse)
        .unwrap_or(crate::export_outcome::RiskGate::Off);
    let fail_on_partial = args.fail_on_partial_fidelity && !args.allow_partial_fidelity;
    // unique-eml has no export_risk yet — use Ok; risk gate only fires if set to ok.
    let mut classified = crate::export_outcome::classify_export(
        export_ok_input,
        dedup_engine::integrity::PreflightRecommendation::Ok,
        risk_gate,
        fail_on_partial,
        false,
    );
    let ok = classified.fidelity == crate::export_outcome::ExportFidelity::Complete;
    // Same disposition helper as unique-pst (0078): PartialRetained only for soft-fail;
    // hard-fail with bytes → InvalidInPlace (not shippable).
    let artifact_state = crate::export_outcome::artifact_state_for(
        &classified,
        manifest.stats.eml_written > 0,
        crate::export_outcome::QuarantineResult::NotAttempted,
    );

    let (
        attachment_ledger,
        attachment_ledger_mode,
        attachment_ledger_truncated,
        attachment_ledger_rows_written,
        attachments_failed_by_reason,
    ) = ledger_summary_fields(args.attach_ledger, attach_ledger_finish.as_ref());

    // DoD-22: write a dedicated summary.json containing fidelity/exit fields (not
    // manifest.json, which lacks them). Path is self-locating.
    let summary_path = out.join("summary.json");
    let summary_abs = std::path::absolute(&summary_path).unwrap_or_else(|_| summary_path.clone());
    let summary_path_str = summary_abs.display().to_string();

    let payload = UniqueEmlSummaryOut {
        schema: keep_set.schema.clone(),
        eml_pack_schema: EML_PACK_SCHEMA.to_string(),
        policy: args.policy.as_str().to_string(),
        family_policy: args.family_policy.as_str().to_string(),
        keep_set,
        scan: outcome.summary.clone(),
        out: out.display().to_string(),
        manifest_json: manifest_path.display().to_string(),
        decision_csv: decision_csv_out.clone(),
        keep_set_json: keep_set_json_out.clone(),
        eml_written: manifest.stats.eml_written,
        unique: manifest.stats.unique,
        volumes: manifest.stats.volumes,
        attach_parts_written: manifest.stats.attach_parts_written,
        embedded_messages_written: manifest.stats.embedded_messages_written,
        attach_parts_failed: attach_failed,
        max_embedded_depth: nested_depth,
        attachment_ledger,
        attachment_ledger_mode,
        attachment_ledger_truncated,
        attachment_ledger_rows_written,
        attachments_failed_by_reason,
        fidelity: classified.fidelity,
        exit_code: classified.exit.as_u8(),
        exit_reason: classified
            .reasons
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        artifact_state,
        summary_path: summary_path_str.clone(),
    };
    let mut v = serde_json::to_value(&payload)?;
    if let Some(obj) = v.as_object_mut() {
        obj.insert("ok".into(), serde_json::Value::Bool(ok));
        if let Some(msg) = exit_err.as_ref().or(pack_err.as_ref()) {
            obj.insert(
                "error".into(),
                serde_json::json!({
                    "code": if pack_err.is_some() { "eml_pack" } else { "scan_integrity" },
                    "message": msg,
                }),
            );
        }
        obj.insert(
            "materialized".into(),
            serde_json::Value::from(materialized_count),
        );
    }

    // Always write summary.json so summary_path is self-consistent with on-disk fields.
    // Fail-closed: write failure reclassifies as report hard-fail (DoD-22).
    let summary_write_err = (|| -> std::result::Result<(), String> {
        fs::create_dir_all(&out).map_err(|e| format!("summary parent create failed: {e}"))?;
        let body = serde_json::to_string_pretty(&v)
            .map_err(|e| format!("summary.json serialize failed: {e}"))?;
        fs::write(&summary_path, body).map_err(|e| format!("summary.json write failed: {e}"))?;
        Ok(())
    })();
    if let Err(msg) = summary_write_err {
        tracing::warn!(path = %summary_path.display(), "{msg}");
        pack_err = Some(msg.clone());
        // Rebuild export input with report_ok=false and reclassify.
        let mut hard_input = export_ok_input;
        hard_input.report_ok = false;
        classified = crate::export_outcome::classify_export(
            hard_input,
            dedup_engine::integrity::PreflightRecommendation::Ok,
            risk_gate,
            fail_on_partial,
            false,
        );
        if let Some(obj) = v.as_object_mut() {
            obj.insert(
                "ok".into(),
                serde_json::Value::Bool(
                    classified.fidelity == crate::export_outcome::ExportFidelity::Complete,
                ),
            );
            obj.insert(
                "fidelity".into(),
                serde_json::Value::String(classified.fidelity.as_str().to_string()),
            );
            obj.insert(
                "exit_code".into(),
                serde_json::Value::from(classified.exit.as_u8()),
            );
            obj.insert(
                "exit_reason".into(),
                serde_json::Value::Array(
                    classified
                        .reasons
                        .iter()
                        .map(|s| serde_json::Value::String((*s).to_string()))
                        .collect(),
                ),
            );
            obj.insert(
                "error".into(),
                serde_json::json!({ "code": "report", "message": msg }),
            );
        }
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        if classified.exit != crate::error::CliExit::Success {
            let msg = pack_err
                .or(exit_err)
                .unwrap_or_else(|| "unique-eml incomplete".into());
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: classified.exit,
            });
        }
        return Ok(crate::error::CliExit::Success);
    }

    // Human summary.
    println!(
        "=== Unique EML pack ({EML_PACK_SCHEMA}) policy={} family={} ===",
        args.policy.as_str(),
        args.family_policy.as_str()
    );
    println!("  out:           {}", out.display());
    println!(
        "  eml_written:   {}  unique: {}  volumes: {}",
        manifest.stats.eml_written, manifest.stats.unique, manifest.stats.volumes
    );
    println!(
        "  attach file:   {}  embedded: {}  attach failed: {}",
        manifest.stats.attach_parts_written,
        manifest.stats.embedded_messages_written,
        manifest.stats.attach_parts_failed
    );
    println!("  max_embedded_depth: {nested_depth}");
    println!(
        "  recoverable:   {}  duplicates: {}  materialize_failed: {}",
        payload.keep_set.stats.recoverable,
        payload.keep_set.stats.duplicates,
        payload.keep_set.stats.materialize_failed
    );
    println!(
        "  degraded winners: {}  files_per_volume: {files_per_volume}",
        payload.keep_set.stats.degraded_winners
    );
    // 0075 honesty counters (always printed, including when 0).
    println!(
        "  winners_from_recoverable_items: {}",
        payload.keep_set.stats.winners_from_recoverable_items
    );
    println!(
        "  winners_without_bcc_peer_had_bcc: {}",
        payload.keep_set.stats.winners_without_bcc_peer_had_bcc
    );
    println!(
        "  groups_date_source_mixed: {}",
        payload.keep_set.stats.groups_date_source_mixed
    );
    for line in format_grouping_stats_human(&payload.keep_set.stats.grouping) {
        println!("{line}");
    }
    println!("  manifest:      {}", manifest_path.display());
    println!("  summary:       {summary_path_str}");
    if payload.attachment_ledger.is_some() {
        println!(
            "  attach_ledger: {}",
            out.join(EXPORT_ATTACHMENTS_CSV_NAME).display()
        );
    }
    if let Some(p) = &decision_csv_out {
        println!("  decision_csv:  {p}");
    }
    if let Some(p) = &keep_set_json_out {
        println!("  keep_set_json: {p}");
    }
    if let Some(ic) = &outcome.summary.integrity_csv {
        println!("  integrity_csv: {ic}");
    }

    if classified.exit != crate::error::CliExit::Success {
        let _ = writeln!(std::io::stderr(), "summary: {summary_path_str}");
    }
    Ok(classified.exit)
}

/// Map an engine soft-fail event into a 0073 attach-ledger CSV row (0089).
fn attach_ledger_row_from_eml_event(
    ev: &EmlAttachEvent,
    msg: &CanonicalMessage,
    input_paths: &[String],
    path_mode: LedgerPathMode,
    volume_path: &str,
    volume_index: u32,
    winner_promoted: bool,
) -> AttachLedgerRow {
    let source_id = resolve_input_source_id(&msg.locus.source_path, input_paths);
    AttachLedgerRow {
        source_id: source_id.map(|id| id.to_string()).unwrap_or_default(),
        source_path: format_ledger_source_path(&msg.locus.source_path, path_mode),
        folder_path: msg.locus.folder_path.clone(),
        msg_nid: msg.locus.nid,
        attach_nid: ev.attach_nid.map(|n| n.to_string()).unwrap_or_default(),
        attach_index: ev.attach_index,
        filename: ev.filename.clone(),
        size: ev.size.map(|n| n.to_string()).unwrap_or_default(),
        attach_method: ev.attach_method,
        reason_code: ev.reason_code.clone(),
        severity: ev.severity.clone(),
        volume_path: volume_path.to_string(),
        volume_index: if volume_index == 0 {
            String::new()
        } else {
            volume_index.to_string()
        },
        winner_promoted,
        peer_source_id: String::new(),
        peer_msg_nid: String::new(),
        message_subject: ev
            .message_subject
            .clone()
            .or_else(|| msg.subject.clone())
            .unwrap_or_default(),
        cloud_provider: ev.cloud_provider.clone(),
        cloud_url: ev.cloud_url.clone(),
    }
}

type LedgerSummaryFields = (
    Option<String>,
    Option<String>,
    Option<bool>,
    Option<u64>,
    Option<BTreeMap<String, u64>>,
);

fn ledger_summary_fields(
    mode: AttachLedgerMode,
    finish: Option<&AttachLedgerFinish>,
) -> LedgerSummaryFields {
    match mode {
        AttachLedgerMode::Off => (None, None, None, None, None),
        AttachLedgerMode::SummaryOnly => {
            let hist = finish.map(|f| f.failed_by_reason.clone());
            (
                None,
                Some(mode.as_str().to_string()),
                Some(false),
                Some(0),
                hist,
            )
        }
        AttachLedgerMode::Full => match finish {
            Some(f) => (
                Some(EXPORT_ATTACHMENTS_CSV_NAME.to_string()),
                Some(mode.as_str().to_string()),
                Some(f.truncated),
                Some(f.rows_written),
                Some(f.failed_by_reason.clone()),
            ),
            None => (None, Some(mode.as_str().to_string()), None, None, None),
        },
    }
}

/// Refuse path layouts that would delete or overwrite source PSTs.
///
/// Checks (absolute/normalized compare):
/// 1. No input PST is equal to `--out` or contained under `--out` (recursive clear).
/// 2. `--out` is not equal to an input PST, and not nested under an input PST path.
/// 3. decision_csv / keep_set_json / manifest_json / integrity_csv do not equal any input PST.
fn guard_unique_eml_paths(
    inputs: &[PathBuf],
    out: &Path,
    decision_csv: Option<&Path>,
    keep_set_json: Option<&Path>,
    manifest_json: &Path,
    integrity_csv: Option<&Path>,
) -> Result<()> {
    for input in inputs {
        // Input equal to out, or input lives under out → overwrite clear would delete it.
        if is_same_or_under(input, out) {
            return Err(CliError::Usage(format!(
                "refusing --out that contains or equals an input PST (would delete source): \
                 out={} input={}",
                out.display(),
                input.display()
            )));
        }
        // out equal to input file, or path-string "under" a file (nonsense layout).
        if is_same_or_under(out, input) {
            return Err(CliError::Usage(format!(
                "refusing --out equal to or nested under an input PST: out={} input={}",
                out.display(),
                input.display()
            )));
        }
        if let Some(p) = decision_csv {
            if paths_equal(p, input) {
                return Err(CliError::Usage(format!(
                    "refusing --decision-csv that equals an input PST: {}",
                    p.display()
                )));
            }
        }
        if let Some(p) = keep_set_json {
            if paths_equal(p, input) {
                return Err(CliError::Usage(format!(
                    "refusing --keep-set-json that equals an input PST: {}",
                    p.display()
                )));
            }
        }
        if paths_equal(manifest_json, input) {
            return Err(CliError::Usage(format!(
                "refusing --manifest-json that equals an input PST: {}",
                manifest_json.display()
            )));
        }
        if let Some(p) = integrity_csv {
            if paths_equal(p, input) {
                return Err(CliError::Usage(format!(
                    "refusing --integrity-csv that equals an input PST: {}",
                    p.display()
                )));
            }
        }
    }
    Ok(())
}

fn prepare_out_dir(out: &Path, overwrite: bool) -> Result<()> {
    if out.exists() {
        if !out.is_dir() {
            return Err(CliError::Usage(format!(
                "--out exists and is not a directory: {}",
                out.display()
            )));
        }
        let non_empty = fs::read_dir(out)
            .map_err(|e| CliError::Msg(format!("read --out {}: {e}", out.display())))?
            .next()
            .is_some();
        if non_empty && !overwrite {
            return Err(CliError::Usage(format!(
                "--out is not empty (pass --overwrite to replace contents): {}",
                out.display()
            )));
        }
        if non_empty && overwrite {
            // Clear contents so volume dirs and manifest are fresh.
            for entry in fs::read_dir(out)
                .map_err(|e| CliError::Msg(format!("read --out {}: {e}", out.display())))?
            {
                let entry = entry.map_err(|e| CliError::Msg(format!("read_dir entry: {e}")))?;
                let p = entry.path();
                if p.is_dir() {
                    fs::remove_dir_all(&p)
                        .map_err(|e| CliError::Msg(format!("remove {}: {e}", p.display())))?;
                } else {
                    fs::remove_file(&p)
                        .map_err(|e| CliError::Msg(format!("remove {}: {e}", p.display())))?;
                }
            }
        }
    } else {
        fs::create_dir_all(out)
            .map_err(|e| CliError::Msg(format!("create --out {}: {e}", out.display())))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_rejects_input_under_out() {
        let inputs = vec![PathBuf::from(r"C:\pack\mail.pst")];
        let out = PathBuf::from(r"C:\pack");
        let man = PathBuf::from(r"C:\pack\manifest.json");
        let err = guard_unique_eml_paths(&inputs, &out, None, None, &man, None).unwrap_err();
        let msg = err.to_string().to_ascii_lowercase();
        assert!(
            msg.contains("out") || msg.contains("input") || msg.contains("delete"),
            "{msg}"
        );
    }

    #[test]
    fn guard_rejects_out_equal_input_pst() {
        let inputs = vec![PathBuf::from(r"C:\data\mail.pst")];
        let out = PathBuf::from(r"C:\data\mail.pst");
        let man = PathBuf::from(r"C:\data\manifest.json");
        assert!(guard_unique_eml_paths(&inputs, &out, None, None, &man, None).is_err());
    }

    #[test]
    fn guard_rejects_artifact_equal_input() {
        let inputs = vec![PathBuf::from(r"C:\data\mail.pst")];
        let out = PathBuf::from(r"C:\data\pack");
        let man = PathBuf::from(r"C:\data\manifest.json");
        let dec = PathBuf::from(r"C:\data\mail.pst");
        assert!(guard_unique_eml_paths(&inputs, &out, Some(&dec), None, &man, None).is_err());
    }

    #[test]
    fn guard_rejects_integrity_csv_equal_input() {
        let inputs = vec![PathBuf::from(r"C:\data\mail.pst")];
        let out = PathBuf::from(r"C:\data\pack");
        let man = PathBuf::from(r"C:\data\pack\manifest.json");
        let ic = PathBuf::from(r"C:\data\mail.pst");
        let err = guard_unique_eml_paths(&inputs, &out, None, None, &man, Some(&ic)).unwrap_err();
        let msg = err.to_string().to_ascii_lowercase();
        assert!(msg.contains("integrity") || msg.contains("input"), "{msg}");
    }

    #[test]
    fn guard_accepts_disjoint_layout() {
        let inputs = vec![PathBuf::from(r"C:\data\mail.pst")];
        let out = PathBuf::from(r"C:\data\pack");
        let man = PathBuf::from(r"C:\data\pack\manifest.json");
        let dec = PathBuf::from(r"C:\data\decisions.csv");
        let ks = PathBuf::from(r"C:\data\keepset.json");
        let ic = PathBuf::from(r"C:\data\integrity.csv");
        guard_unique_eml_paths(&inputs, &out, Some(&dec), Some(&ks), &man, Some(&ic)).expect("ok");
    }

    /// Pure invariant: unique-eml export targets are keep_set.winners only.
    /// (Integration covers real PST; keepset tests cover promote → winners.)
    #[test]
    fn export_targets_are_winners_only() {
        use dedup_engine::integrity::RecoverableIntegrity;
        use dedup_engine::keepset::{
            FamilyPolicy, KeepEntry, KeepPolicy, KeepSet, KeepSetStats, MessageLocus,
        };

        let winner = KeepEntry {
            locus: MessageLocus {
                source_path: r"C:\a.pst".into(),
                source_pst: "a.pst".into(),
                folder_path: "/Inbox".into(),
                nid: 0x21,
                is_orphaned: false,
            },
            message_id_norm: Some("<w@x>".into()),
            content_hash: [1u8; 32],
            edrm_mih_hex: None,
            integrity: RecoverableIntegrity::clean(),
            size: 100,
            promoted_from_failure: false,
            folder_class: None,
            decided_by: None,
            duplicate_source_count: 0,
            duplicate_sources: vec![],
            duplicate_sources_truncated: false,
        };
        let ks = KeepSet {
            schema: "keep_set_v1".into(),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            created_from: None,
            identity_level: None,
            dedupe_scope: None,
            winners: vec![winner],
            stats: KeepSetStats {
                recoverable: 3,
                unique: 1,
                duplicates: 2,
                tier1_dups: 2,
                tier2_dups: 0,
                degraded_winners: 0,
                materialize_failed: 0,
                promoted_from_failure: 0,
                groups_dropped_materialize: 0,
                groups: 1,
                ..KeepSetStats::default()
            },
        };
        // Export loop is `for entry in &keep_set.winners` — count matches unique.
        assert_eq!(ks.winners.len() as u64, ks.stats.unique);
        assert_eq!(ks.stats.unique, 1);
        assert!(ks.stats.duplicates > 0, "dup peers are not winners");
    }

    /// 0089: soft-fail EmlAttachEvent → CSV at pack root with identical header.
    #[test]
    fn soft_fail_eml_event_writes_export_attachments_csv_header() {
        use crate::unique_export_report::EXPORT_ATTACHMENTS_CSV_HEADER;
        use dedup_engine::eml_pack::{write_canonical_eml, EmlWriteOpts, NullAttachStreamSource};
        use dedup_engine::integrity::RecoverableIntegrity;
        use dedup_engine::keepset::{CanonicalAttachment, MessageLocus};

        let msg = CanonicalMessage {
            locus: MessageLocus {
                source_path: r"C:\in\a.pst".into(),
                source_pst: "a.pst".into(),
                folder_path: "Inbox".into(),
                nid: 0x100,
                is_orphaned: false,
            },
            message_id: Some("<0089@test>".into()),
            subject: Some("soft fail attach".into()),
            sender: Some("a@b.c".into()),
            display_to: None,
            display_cc: None,
            display_bcc: None,
            recipients: Vec::new(),
            message_flags: None,
            submit_time: None,
            size: Some(10),
            message_class: None,
            body_plain: Some("plain body".into()),
            body_html: None,
            attachments: vec![CanonicalAttachment {
                filename: "missing.bin".into(),
                size: 10,
                mime: Some("application/octet-stream".into()),
                data: None,
                stream_available: true,
                attach_nid: Some(999),
                attach_method: Some(1),
                is_cloud_link: false,
                cloud_provider: None,
                cloud_url: None,
                cloud_permission_type: None,
                embedded_message: None,
                embedded_extract_limit: false,
            }],
            fidelity: RecoverableIntegrity::clean(),
            message_id_norm: Some("0089@test".into()),
            content_hash: [0u8; 32],
            edrm_mih_hex: None,
            body_incomplete: false,
            body_unavailable: false,
        };
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("pack");
        fs::create_dir_all(&out).expect("mkdir");
        let eml_path = out.join("msg.eml");
        let mut src = NullAttachStreamSource;
        let wres = write_canonical_eml(&eml_path, &msg, &mut src, &EmlWriteOpts::default())
            .expect("write");
        assert_eq!(wres.attachments_failed, 1);
        assert_eq!(wres.attachment_events.len(), 1);

        let inputs = vec![r"C:\in\a.pst".to_string()];
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            500_000,
            &out,
            &inputs,
            LedgerPathMode::Full,
        )
        .expect("sink");
        sink.set_volume(&out.join("VOL001").display().to_string(), 1);
        for ev in &wres.attachment_events {
            let row = attach_ledger_row_from_eml_event(
                ev,
                &msg,
                &inputs,
                LedgerPathMode::Full,
                &out.join("VOL001").display().to_string(),
                1,
                false,
            );
            sink.enqueue_soft_skip_row(row);
        }
        let finish = sink.finish().expect("finish");
        assert_eq!(finish.rows_written, 1);

        let csv_path = out.join(EXPORT_ATTACHMENTS_CSV_NAME);
        assert!(csv_path.is_file(), "export_attachments.csv at pack root");
        let csv = fs::read_to_string(&csv_path).expect("csv");
        let first = csv.lines().next().expect("header");
        assert_eq!(first, EXPORT_ATTACHMENTS_CSV_HEADER);
        assert!(csv.contains("ATTACH_STREAM_OPEN_FAILED"));
        assert!(csv.contains("missing.bin"));
    }

    /// 0089 Mode A: soft-skip loser rows carry winner_promoted; promoted write-fail does too.
    #[test]
    fn mode_a_soft_skip_and_promoted_winner_rows() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string(), r"C:\in\b.pst".to_string()];
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            500_000,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
        .expect("sink");
        sink.enqueue_soft_skip_row(AttachLedgerRow {
            source_id: "0".into(),
            source_path: r"C:\in\a.pst".into(),
            folder_path: "Inbox".into(),
            msg_nid: 10,
            attach_nid: "99".into(),
            attach_index: 0,
            filename: "missing.bin".into(),
            size: "100".into(),
            attach_method: 1,
            reason_code: "ATTACH_STREAM_OPEN_FAILED".into(),
            severity: "fail".into(),
            volume_path: String::new(),
            volume_index: String::new(),
            winner_promoted: true,
            peer_source_id: "1".into(),
            peer_msg_nid: "20".into(),
            message_subject: String::new(),
            cloud_provider: String::new(),
            cloud_url: String::new(),
        });
        sink.mark_promoted_winner(r"C:\in\b.pst", 20);
        let msg = CanonicalMessage {
            locus: dedup_engine::keepset::MessageLocus {
                source_path: r"C:\in\b.pst".into(),
                source_pst: "b.pst".into(),
                folder_path: "Inbox".into(),
                nid: 20,
                is_orphaned: false,
            },
            message_id: None,
            subject: Some("winner".into()),
            sender: None,
            display_to: None,
            display_cc: None,
            display_bcc: None,
            recipients: Vec::new(),
            message_flags: None,
            submit_time: None,
            size: None,
            message_class: None,
            body_plain: Some("x".into()),
            body_html: None,
            attachments: Vec::new(),
            fidelity: dedup_engine::integrity::RecoverableIntegrity::clean(),
            message_id_norm: None,
            content_hash: [0u8; 32],
            edrm_mih_hex: None,
            body_incomplete: false,
            body_unavailable: false,
        };
        let ev = EmlAttachEvent {
            attach_index: 0,
            filename: "still-bad.bin".into(),
            size: Some(1),
            attach_method: 1,
            attach_nid: Some(7),
            reason_code: "ATTACH_STREAM_OPEN_FAILED".into(),
            severity: "fail".into(),
            error_detail: "stream not available".into(),
            cloud_provider: String::new(),
            cloud_url: String::new(),
            message_subject: Some("winner".into()),
        };
        let promoted = sink
            .promoted_winner_loci
            .contains(&(msg.locus.source_path.clone(), msg.locus.nid));
        assert!(promoted);
        let row = attach_ledger_row_from_eml_event(
            &ev,
            &msg,
            &inputs,
            LedgerPathMode::Full,
            "",
            0,
            promoted,
        );
        sink.enqueue_soft_skip_row(row);
        let _ = sink.finish().expect("finish");
        let csv = fs::read_to_string(dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
        assert!(
            csv.lines()
                .any(|l| l.contains("missing.bin") && l.contains(",true,")),
            "soft-skip loser must have winner_promoted: {csv}"
        );
        assert!(
            csv.lines()
                .any(|l| l.contains("still-bad.bin") && l.contains(",true,")),
            "promoted winner write-fail must have winner_promoted: {csv}"
        );
    }

    /// 0089: row-cap emits ATTACH_LEDGER_TRUNCATED marker.
    #[test]
    fn attach_ledger_row_cap_truncated_marker() {
        let dir = tempfile::tempdir().expect("tmp");
        let inputs = vec![r"C:\in\a.pst".to_string()];
        let mut sink = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            2, // leave one slot for marker after first data row
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        )
        .expect("sink");
        for i in 0..3u32 {
            sink.enqueue_soft_skip_row(AttachLedgerRow {
                source_id: "0".into(),
                source_path: r"C:\in\a.pst".into(),
                folder_path: "Inbox".into(),
                msg_nid: 1,
                attach_nid: i.to_string(),
                attach_index: i,
                filename: format!("f{i}.bin"),
                size: "1".into(),
                attach_method: 1,
                reason_code: "ATTACH_STREAM_OPEN_FAILED".into(),
                severity: "fail".into(),
                volume_path: String::new(),
                volume_index: String::new(),
                winner_promoted: false,
                peer_source_id: String::new(),
                peer_msg_nid: String::new(),
                message_subject: String::new(),
                cloud_provider: String::new(),
                cloud_url: String::new(),
            });
        }
        let finish = sink.finish().expect("finish");
        assert!(finish.truncated);
        let csv = fs::read_to_string(dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME)).expect("csv");
        assert!(
            csv.contains("ATTACH_LEDGER_TRUNCATED"),
            "cap must emit truncated marker: {csv}"
        );
    }

    /// 0089: ledger init fail with mode=full must fail closed (report_ok=false → non-success).
    #[test]
    fn attach_ledger_init_fail_full_fail_closed() {
        use crate::export_outcome::{classify_export, ExportFidelity, ExportOkInput, RiskGate};

        let dir = tempfile::tempdir().expect("tmp");
        // Plant a directory where the CSV file should be created → File::create fails.
        let blocker = dir.path().join(EXPORT_ATTACHMENTS_CSV_NAME);
        fs::create_dir_all(&blocker).expect("blocker dir");
        let inputs = vec![r"C:\in\a.pst".to_string()];
        let init = AttachLedgerSink::new(
            AttachLedgerMode::Full,
            500_000,
            dir.path(),
            &inputs,
            LedgerPathMode::Full,
        );
        assert!(init.is_err(), "init must fail when CSV path is a directory");

        // Mirror unique_eml_cmd: mode != Off + init Err → report_ok=false.
        let input = ExportOkInput {
            scan_ok: true,
            verify_ok: true,
            export_err_absent: true,
            export_partial: false,
            messages_written_total: 1,
            unique: 1,
            attach_failed_total: 0,
            body_soft_fail_total: 0,
            report_ok: false,
        };
        let o = classify_export(
            input,
            dedup_engine::integrity::PreflightRecommendation::Ok,
            RiskGate::Off,
            true,
            false,
        );
        assert_ne!(o.exit.as_u8(), 0, "ledger init fail must be non-success");
        assert_ne!(o.fidelity, ExportFidelity::Complete);
        assert!(o.reasons.contains(&"REPORT_WRITE_FAILED"));
    }
}
