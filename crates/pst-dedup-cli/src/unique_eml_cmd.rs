//! `pst-dedup unique-eml` — keep_set_v1 → volume-batched EML pack (track 0067).
//!
//! No re-dedupe: winners come only from `finalize_with_materialize`. Source PSTs
//! are read-only. Pack layout is always volume-batched (`VOL001`…).
//!
//! Export order matches `KeepSet.winners` (path+nid sort): finalize promotes first,
//! then each winner is re-materialized once and written in keep-set order.

use std::fs;
use std::path::{Path, PathBuf};

use dedup_engine::integrity::{IntegrityThresholds, ScanMode, SCAN_INTEGRITY_SCHEMA};
use dedup_engine::keepset::{
    finalize_with_materialize, resolve_groups, sort_input_paths, write_keep_set_json,
    DecisionCsvWriter, FamilyPolicy, KeepPolicy, KeepSetProvenance, MessageMaterializer,
};
use dedup_engine::{
    clamp_files_per_volume, merge_pack_degraded, validate_volume_prefix, write_canonical_eml,
    write_eml_pack_manifest, EmlPackManifest, EmlPackMessageRow, EmlWriteOpts, VolumePackWriter,
    EML_PACK_SCHEMA,
};
use serde::Serialize;

use crate::error::{CliError, Result};
use crate::paths::{is_same_or_under, paths_equal, resolve_cli_path_maybe_missing};
use crate::pst_materializer::{PstAttachStreamSource, PstMaterializer};
use crate::scan::{evaluate_exit_policy, resolve_pst_paths, run_scan, ScanOptions, ScanSummary};

/// CLI options for `unique-eml`.
pub struct UniqueEmlCliArgs {
    pub paths: Vec<PathBuf>,
    pub out: PathBuf,
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path_contains: Vec<String>,
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
    attach_parts_failed: u64,
}

/// Run unique-eml orchestration end-to-end.
pub fn run_unique_eml(args: UniqueEmlCliArgs) -> Result<()> {
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
        integrity_csv: integrity_csv.clone(),
        csv: None,
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

    // Phase 2b: promote winners only (no EML write). Bodies are streamed one-at-a-time
    // and dropped; export order is applied in phase 2c so counters match keep_set.winners.
    let mut mat = PstMaterializer::new(args.family_policy);
    let mut attach_src = PstAttachStreamSource::new();
    let write_opts = EmlWriteOpts {
        family_policy: args.family_policy,
        ..EmlWriteOpts::default()
    };

    let materialized_count = finalize_with_materialize(&mut resolved, &mut mat, &mut |_msg| Ok(()))
        .map_err(|e| CliError::Msg(format!("materialize/promote: {e}")))?;

    let keep_set = resolved.to_keep_set();

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
    let pack_err = if count_mismatch {
        Some(format!(
            "eml_written ({}) != unique ({}); write_errors={:?}",
            manifest.stats.eml_written, keep_set.stats.unique, write_errors
        ))
    } else if !write_errors.is_empty() {
        Some(format!("partial eml write errors: {write_errors:?}"))
    } else {
        None
    };

    let ok = exit_err.is_none() && pack_err.is_none();

    if args.json {
        let payload = UniqueEmlSummaryOut {
            schema: keep_set.schema.clone(),
            eml_pack_schema: EML_PACK_SCHEMA.to_string(),
            policy: args.policy.as_str().to_string(),
            family_policy: args.family_policy.as_str().to_string(),
            keep_set,
            scan: outcome.summary,
            out: out.display().to_string(),
            manifest_json: manifest_path.display().to_string(),
            decision_csv: decision_csv_out,
            keep_set_json: keep_set_json_out,
            eml_written: manifest.stats.eml_written,
            unique: manifest.stats.unique,
            volumes: manifest.stats.volumes,
            attach_parts_written: manifest.stats.attach_parts_written,
            embedded_messages_written: manifest.stats.embedded_messages_written,
            attach_parts_failed: manifest.stats.attach_parts_failed,
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
        println!("{}", serde_json::to_string_pretty(&v)?);
        if let Some(msg) = pack_err.or(exit_err) {
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: crate::error::CliExit::Generic,
            });
        }
        return Ok(());
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
    println!(
        "  recoverable:   {}  duplicates: {}  materialize_failed: {}",
        keep_set.stats.recoverable, keep_set.stats.duplicates, keep_set.stats.materialize_failed
    );
    println!(
        "  degraded winners: {}  files_per_volume: {files_per_volume}",
        keep_set.stats.degraded_winners
    );
    println!("  manifest:      {}", manifest_path.display());
    if let Some(p) = &decision_csv_out {
        println!("  decision_csv:  {p}");
    }
    if let Some(p) = &keep_set_json_out {
        println!("  keep_set_json: {p}");
    }
    if let Some(ic) = &outcome.summary.integrity_csv {
        println!("  integrity_csv: {ic}");
    }

    if let Some(msg) = pack_err.or(exit_err) {
        return Err(CliError::Msg(msg));
    }
    Ok(())
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
        };
        let ks = KeepSet {
            schema: "keep_set_v1".into(),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            created_from: None,
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
            },
        };
        // Export loop is `for entry in &keep_set.winners` — count matches unique.
        assert_eq!(ks.winners.len() as u64, ks.stats.unique);
        assert_eq!(ks.stats.unique, 1);
        assert!(ks.stats.duplicates > 0, "dup peers are not winners");
    }
}
