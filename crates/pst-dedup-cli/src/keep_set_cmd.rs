//! `pst-dedup keep-set` — keep_set_v1 orchestration (track 0066).
//!
//! Phases: sort paths → integrity scan (collect candidates) → resolve →
//! optional materialize+promote → stream decision CSV + keep-set JSON.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use dedup_engine::integrity::{IntegrityThresholds, ScanMode, SCAN_INTEGRITY_SCHEMA};
use dedup_engine::keepset::{
    finalize_with_materialize, resolve_groups, sort_input_paths, write_keep_set_json,
    CanonicalAttachment, CanonicalMessage, DecisionCsvWriter, FamilyPolicy, KeepPolicy,
    KeepSetProvenance, MaterializeError, MessageLocus, MessageMaterializer,
};
use dedup_engine::reason_from_pst_error;
use pst_reader::{NodeId, PstFile};
use serde::Serialize;

use crate::error::{CliError, Result};
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

/// Materializer holding open PST handles (source PSTs remain read-only).
struct PstMaterializer {
    /// Absolute path string → open file.
    psts: HashMap<String, PstFile>,
    /// When false / parents_only, skip loading attach bytes (metadata list may still be empty).
    load_attach_payloads: bool,
    /// parents_only: do not list attaches at all for payload purposes.
    parents_only: bool,
}

impl PstMaterializer {
    fn new(family: FamilyPolicy) -> Self {
        Self {
            psts: HashMap::new(),
            load_attach_payloads: family == FamilyPolicy::KeepAttachmentsWithParent,
            parents_only: family == FamilyPolicy::ParentsOnly,
        }
    }

    fn open_pst(&mut self, path: &str) -> std::result::Result<&mut PstFile, MaterializeError> {
        if !self.psts.contains_key(path) {
            let pst = PstFile::open(Path::new(path))
                .map_err(|e| MaterializeError::Hard(format!("open {}: {e}", path)))?;
            self.psts.insert(path.to_string(), pst);
        }
        self.psts
            .get_mut(path)
            .ok_or_else(|| MaterializeError::Hard(format!("pst missing after open: {path}")))
    }
}

/// True hard failures that must promote peers. Everything else may soft-recover
/// via `read_message_properties` (scan already classified many of these as recoverable).
fn is_hard_structural_reason(reason: dedup_engine::IntegrityReason) -> bool {
    use dedup_engine::IntegrityReason::*;
    matches!(
        reason,
        OpenFailed
            | AnsiUnsupported
            | UnsupportedCrypt
            | FolderWalkFailed
            | NodeNotFound
            | BlockNotFound
            | PathNotFound
            | NotPst
            | ReadError
    )
}

impl MessageMaterializer for PstMaterializer {
    fn materialize(
        &mut self,
        locus: &MessageLocus,
    ) -> std::result::Result<CanonicalMessage, MaterializeError> {
        // Full attach-byte streaming / EML pack is track 0067. Keep-set materialize
        // (0066) validates extract + attachment *metadata* for promotion honesty.
        // Large attach payloads are never loaded into Vecs; `stream_available` marks
        // that open_attachment_data can be used by downstream exporters (0067).
        let parents_only = self.parents_only;
        let load_payloads = self.load_attach_payloads;
        let pst = self.open_pst(&locus.source_path)?;
        let nid = NodeId(locus.nid);

        let mut soft_reasons: Vec<dedup_engine::IntegrityReason> = Vec::new();

        // Prefer full extract; on soft body/property errors fall back to properties
        // so sole degraded winners are not ghost-dropped (§3.7 rule 3 / D-0065-soft-body).
        let (
            message_id,
            subject,
            sender,
            display_to,
            display_cc,
            display_bcc,
            submit_time,
            size,
            message_class,
            body_plain,
            body_html,
            body_incomplete,
            body_unavailable,
        ) = match pst.read_message_extract(nid) {
            Ok(extracted) => {
                let body_unavailable =
                    extracted.body_text.is_none() && extracted.body_html.is_none();
                if body_unavailable {
                    soft_reasons.push(dedup_engine::IntegrityReason::BodyUnavailable);
                }
                (
                    extracted.message_id,
                    extracted.subject,
                    extracted.sender_email,
                    extracted.display_to,
                    extracted.display_cc,
                    extracted.display_bcc,
                    extracted.submit_time,
                    extracted.message_size.map(|s| s as u32),
                    extracted.message_class,
                    extracted.body_text,
                    extracted.body_html,
                    false,
                    body_unavailable,
                )
            }
            Err(e) => {
                let reason = reason_from_pst_error(&e);
                // Hard-structural only: node/block missing, open fail, etc.
                // Invalid HID / structure on body props is recoverable via properties
                // (scan already surfaces BODY_UNAVAILABLE for these aspose fixtures).
                if is_hard_structural_reason(reason) {
                    return Err(MaterializeError::Hard(format!(
                        "extract nid={:#x} {}: {e}",
                        locus.nid,
                        reason.as_str()
                    )));
                }
                match pst.read_message_properties(nid) {
                    Ok(props) => {
                        let body_incomplete = props.body_incomplete;
                        // Extract failed → full body not exportable; never use 4KB preview.
                        soft_reasons.push(dedup_engine::IntegrityReason::BodyUnavailable);
                        if body_incomplete
                            && !soft_reasons.contains(&dedup_engine::IntegrityReason::BodyTruncated)
                        {
                            soft_reasons.push(dedup_engine::IntegrityReason::BodyTruncated);
                        }
                        if !soft_reasons.contains(&reason) {
                            soft_reasons.push(reason);
                        }
                        (
                            props.message_id,
                            props.subject,
                            props.sender_email,
                            props.display_to,
                            None,
                            None,
                            props.submit_time,
                            props.message_size.map(|s| s as u32),
                            None,
                            None,
                            None,
                            body_incomplete,
                            true,
                        )
                    }
                    Err(e2) => {
                        let r2 = reason_from_pst_error(&e2);
                        if is_hard_structural_reason(r2) {
                            return Err(MaterializeError::Hard(format!(
                                "extract+props nid={:#x} {}: {e2}",
                                locus.nid,
                                r2.as_str()
                            )));
                        }
                        soft_reasons.push(dedup_engine::IntegrityReason::BodyUnavailable);
                        if !soft_reasons.contains(&r2) {
                            soft_reasons.push(r2);
                        }
                        (
                            None, None, None, None, None, None, None, None, None, None, None,
                            false, true,
                        )
                    }
                }
            }
        };

        let mut attachments = Vec::new();
        // parents_only: empty attachments list (family policy).
        // KeepAttachmentsWithParent: always list metadata; payloads optional / size-capped.
        if !parents_only {
            match pst.list_attachments(nid) {
                Ok(list) => {
                    // Cap optional small-payload probe so we never materialize multi-GB Vecs.
                    // Metadata + stream_available is always recorded for 0067 consumers.
                    const SMALL_ATTACH_CAP: u32 = 64 * 1024;
                    for att in list {
                        let mut data = None;
                        // stream_available: list succeeded and size known — open_attachment_data
                        // is the streaming handle for downstream (0067). Not a lost attach.
                        let stream_available = att.size > 0 || !att.filename.is_empty();
                        if load_payloads && att.size > 0 && att.size <= SMALL_ATTACH_CAP {
                            match pst.open_attachment_data(nid, att.nid) {
                                Ok(mut reader) => {
                                    let mut buf = Vec::new();
                                    match reader.read_to_end(&mut buf) {
                                        Ok(_) => data = Some(buf),
                                        Err(e) => {
                                            tracing::warn!(
                                                nid = locus.nid,
                                                attach_nid = att.nid.0,
                                                err = %e,
                                                "open/read attachment payload failed (soft ATTACH_META_FAILED)"
                                            );
                                            if !soft_reasons.contains(
                                                &dedup_engine::IntegrityReason::AttachMetaFailed,
                                            ) {
                                                soft_reasons.push(
                                                    dedup_engine::IntegrityReason::AttachMetaFailed,
                                                );
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        nid = locus.nid,
                                        attach_nid = att.nid.0,
                                        err = %e,
                                        "open_attachment_data failed (soft ATTACH_META_FAILED)"
                                    );
                                    if !soft_reasons
                                        .contains(&dedup_engine::IntegrityReason::AttachMetaFailed)
                                    {
                                        soft_reasons
                                            .push(dedup_engine::IntegrityReason::AttachMetaFailed);
                                    }
                                }
                            }
                        }
                        attachments.push(CanonicalAttachment {
                            filename: att.filename,
                            size: att.size,
                            mime: att.mime_tag,
                            data,
                            stream_available,
                            attach_nid: Some(att.nid.0),
                        });
                    }
                }
                Err(e) => {
                    // Soft attach list failure — do not hard-fail materialize.
                    tracing::warn!(
                        nid = locus.nid,
                        err = %e,
                        "list_attachments failed during materialize (soft ATTACH_META_FAILED)"
                    );
                    soft_reasons.push(dedup_engine::IntegrityReason::AttachMetaFailed);
                }
            }
        }

        let fidelity = if soft_reasons.is_empty() {
            dedup_engine::integrity::RecoverableIntegrity::clean()
        } else {
            dedup_engine::integrity::RecoverableIntegrity::with_degraded(
                soft_reasons,
                locus.is_orphaned,
            )
        };

        Ok(CanonicalMessage {
            locus: locus.clone(),
            message_id,
            subject,
            sender,
            display_to,
            display_cc,
            display_bcc,
            submit_time,
            size,
            message_class,
            body_plain,
            body_html,
            attachments,
            fidelity,
            message_id_norm: None,
            content_hash: [0; 32],
            edrm_mih_hex: None,
            body_incomplete,
            body_unavailable,
        })
    }
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

    // Phase 2b: materialize + promote when requested (or when producing export-ready set).
    // Default: materialize when family needs attach decisions OR always when materialize flag.
    // Spec: pure keep-set JSON may list provisional winners without materialize.
    let mut materialized_count = 0u64;
    if args.materialize {
        let mut mat = PstMaterializer::new(args.family_policy);
        // O(1) body memory: callback receives one winner at a time and drops it.
        // Full EML pack is 0067; here we only finalize roles + fidelity honesty.
        materialized_count = finalize_with_materialize(&mut resolved, &mut mat, &mut |_msg| Ok(()))
            .map_err(|e| CliError::Msg(format!("materialize: {e}")))?;
    }

    // Phase 3: stream decision CSV + keep-set JSON from finalized roles.
    // Decisions stream O(1) row buffer (no all-rows Vec). Keep-set winners are O(unique).
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
