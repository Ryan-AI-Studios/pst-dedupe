//! `pst-dedup unique-pst` — keep_set_v1 → streaming unique PST volume(s) + report pack (track 0071).
//!
//! Pipeline (no re-dedupe):
//! integrity scan → resolve_groups → finalize_with_materialize → write_unicode_pst_streaming
//! (multi-volume optional) → report pack → verify completed volumes.
//!
//! Locks: source PSTs read-only; incomplete current volume deleted on fatal write fail;
//! completed volumes retained; export_messages.csv mandatory; default verify is open+count+sample
//! (full rehash only with `--verify-hash`).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Args;
use dedup_engine::integrity::{IntegrityThresholds, ScanMode, SCAN_INTEGRITY_SCHEMA};
use dedup_engine::keepset::{
    finalize_with_materialize, resolve_groups, sort_input_paths, write_keep_set_json,
    DecisionCsvWriter, FamilyPolicy, KeepEntry, KeepPolicy, KeepSetProvenance, MessageMaterializer,
};
use pst_reader::PstFile;
use pst_writer::{
    from_canonical_message, temp_sibling_path, write_unicode_pst_streaming, AttachRead,
    AttachStreamSource, FolderLayoutPolicy, WriteMessage, WriteProgress, WriteProgressSink,
    WritePstOpts, WriteStage,
};
use sha2::{Digest, Sha256};

use crate::error::{CliError, Result};
use crate::paths::{is_same_or_under, paths_equal, resolve_cli_path_maybe_missing};
use crate::pst_materializer::{PstAttachStreamSource, PstMaterializer};
use crate::scan::{evaluate_exit_policy, resolve_pst_paths, run_scan, ScanOptions};
use crate::unique_export_report::{
    default_report_dir, volume_path_for, write_export_messages_csv, write_summary_json,
    write_volumes_csv, ExportMessageRow, ExportSection, SummaryError, UniqueExportSummary,
    VerificationReport, VolumeReportRow, VolumeVerification, UNIQUE_EXPORT_REPORT_SCHEMA,
};

/// Clap surface for `unique-pst` (tuple-variant keeps `Commands` smaller on stack).
#[derive(Debug, Args)]
pub struct UniquePstClapArgs {
    /// PST path(s) as positional arguments (same style as `scan` / `unique-eml`).
    #[arg(required = false)]
    pub paths: Vec<PathBuf>,
    /// PST path(s) via repeated `--input` (merge with positionals).
    #[arg(long = "input", action = clap::ArgAction::Append)]
    pub input: Vec<PathBuf>,
    /// Primary output PST path (volume 1). Multi-volume: `{stem}_vol002.pst`, …
    #[arg(long)]
    pub out: PathBuf,
    /// Report pack directory (default: sibling of `--out` stem + `_report`).
    #[arg(long)]
    pub report_dir: Option<PathBuf>,
    /// Winner policy after fidelity: first_seen (default), keep_largest, prefer_path.
    #[arg(long, default_value = "first_seen", value_parser = parse_keep_policy_arg)]
    pub policy: KeepPolicy,
    /// Parent+attach family: keep_attachments_with_parent (default) or parents_only.
    #[arg(long, default_value = "keep_attachments_with_parent", value_parser = parse_family_policy_arg)]
    pub family_policy: FamilyPolicy,
    /// Path/folder substring preferred under prefer_path (repeatable).
    #[arg(long = "prefer-path-contains")]
    pub prefer_path_contains: Vec<String>,
    /// Streaming decision CSV (default: `{report-dir}/decisions.csv`).
    #[arg(long)]
    pub decision_csv: Option<PathBuf>,
    /// Keep-set JSON (default: `{report-dir}/keepset.json`).
    #[arg(long)]
    pub keep_set_json: Option<PathBuf>,
    /// Folder layout: `preserve` (default) or `flat`.
    #[arg(long, default_value = "preserve", value_parser = parse_folder_layout_arg)]
    pub folder_layout: FolderLayoutArg,
    /// Soft max physical size per volume (bytes). Off = single volume.
    /// Oversized single family may exceed this limit (never severed).
    #[arg(long)]
    pub max_volume_bytes: Option<u64>,
    /// Allow replacing existing `--out` / report-dir contents.
    #[arg(long)]
    pub overwrite: bool,
    /// Full-file rehash of completed volumes vs report digests (default off).
    #[arg(long)]
    pub verify_hash: bool,
    /// Optional co-export unique-eml pack directory (soft residual; may be ignored).
    #[arg(long)]
    pub also_eml: Option<PathBuf>,
    #[arg(long)]
    pub no_tier2: bool,
    #[arg(long)]
    pub no_attachments: bool,
    #[arg(long)]
    pub json: bool,
    #[arg(long, default_value = "best-effort", value_parser = parse_scan_mode_arg)]
    pub mode: ScanMode,
    #[arg(long, default_value_t = 0.05, value_parser = parse_rate_threshold_arg)]
    pub max_skip_rate: f64,
    #[arg(long, default_value_t = 0.01, value_parser = parse_rate_threshold_arg)]
    pub max_crc_skip_rate: f64,
    #[arg(long, default_value_t = 0.0, value_parser = parse_rate_threshold_arg)]
    pub max_failed_file_rate: f64,
    #[arg(long)]
    pub allow_failed_files: bool,
    #[arg(long)]
    pub integrity_csv: Option<PathBuf>,
    #[arg(long, default_value_t = 10_000)]
    pub skip_limit: usize,
}

/// Runtime options for `unique-pst` orchestration.
pub struct UniquePstCliArgs {
    pub paths: Vec<PathBuf>,
    pub out: PathBuf,
    pub report_dir: Option<PathBuf>,
    pub policy: KeepPolicy,
    pub family_policy: FamilyPolicy,
    pub prefer_path_contains: Vec<String>,
    pub decision_csv: Option<PathBuf>,
    pub keep_set_json: Option<PathBuf>,
    pub folder_layout: FolderLayoutArg,
    pub max_volume_bytes: Option<u64>,
    pub overwrite: bool,
    pub verify_hash: bool,
    /// Soft: optional co-export unique-eml pack (residual if unused).
    pub also_eml: Option<PathBuf>,
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

impl UniquePstClapArgs {
    /// Merge positionals + `--input` into orchestration args.
    pub fn into_cli_args(self) -> std::result::Result<UniquePstCliArgs, CliError> {
        let mut paths = self.paths;
        paths.extend(self.input);
        if paths.is_empty() {
            return Err(CliError::Usage(
                "unique-pst requires at least one PST path (positional or --input)".into(),
            ));
        }
        Ok(UniquePstCliArgs {
            paths,
            out: self.out,
            report_dir: self.report_dir,
            policy: self.policy,
            family_policy: self.family_policy,
            prefer_path_contains: self.prefer_path_contains,
            decision_csv: self.decision_csv,
            keep_set_json: self.keep_set_json,
            folder_layout: self.folder_layout,
            max_volume_bytes: self.max_volume_bytes,
            overwrite: self.overwrite,
            verify_hash: self.verify_hash,
            also_eml: self.also_eml,
            no_tier2: self.no_tier2,
            no_attachments: self.no_attachments,
            json: self.json,
            mode: self.mode,
            max_skip_rate: self.max_skip_rate,
            max_crc_skip_rate: self.max_crc_skip_rate,
            max_failed_file_rate: self.max_failed_file_rate,
            allow_failed_files: self.allow_failed_files,
            integrity_csv: self.integrity_csv,
            skip_limit: self.skip_limit,
        })
    }
}

/// Folder layout CLI choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderLayoutArg {
    Preserve,
    Flat,
}

impl FolderLayoutArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Flat => "flat",
        }
    }
}

fn parse_folder_layout_arg(s: &str) -> std::result::Result<FolderLayoutArg, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "preserve" => Ok(FolderLayoutArg::Preserve),
        "flat" => Ok(FolderLayoutArg::Flat),
        other => Err(format!(
            "invalid folder-layout '{other}': expected preserve or flat"
        )),
    }
}

fn parse_keep_policy_arg(s: &str) -> std::result::Result<KeepPolicy, String> {
    KeepPolicy::parse(s).ok_or_else(|| {
        format!("invalid policy '{s}': expected first_seen, keep_largest, or prefer_path")
    })
}

fn parse_family_policy_arg(s: &str) -> std::result::Result<FamilyPolicy, String> {
    FamilyPolicy::parse(s).ok_or_else(|| {
        format!(
            "invalid family-policy '{s}': expected keep_attachments_with_parent or parents_only"
        )
    })
}

fn parse_scan_mode_arg(s: &str) -> std::result::Result<ScanMode, String> {
    ScanMode::parse(s).ok_or_else(|| format!("invalid mode '{s}': expected best-effort or strict"))
}

fn parse_rate_threshold_arg(s: &str) -> std::result::Result<f64, String> {
    let v: f64 = s
        .parse()
        .map_err(|_| format!("invalid rate threshold '{s}'"))?;
    if !(0.0..=1.0).contains(&v) {
        return Err(format!("rate threshold must be in [0,1], got {v}"));
    }
    Ok(v)
}

/// Winner prepared for streaming write (meta retained for export_messages).
struct PreparedWinner {
    source_path: String,
    folder_path: String,
    nid: u64,
    message_id_norm: String,
    edrm_mih: String,
    content_hash_hex: String,
    write_msg: WriteMessage,
}

/// Adapter: `PstAttachStreamSource` → `pst_writer::AttachStreamSource`.
struct WriterAttachAdapter<'a> {
    inner: &'a mut PstAttachStreamSource,
}

impl AttachStreamSource for WriterAttachAdapter<'_> {
    fn open_attach(
        &mut self,
        source_path: Option<&str>,
        parent_nid: Option<u64>,
        attach_nid: Option<u64>,
        _filename: &str,
    ) -> std::result::Result<Option<Vec<u8>>, String> {
        // Prefer stream path; this full-Vec fallback only for trait completeness.
        match self.open_attach_stream(source_path, parent_nid, attach_nid, _filename)? {
            Some(mut reader) => {
                let mut buf = Vec::new();
                reader
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("read attach: {e}"))?;
                Ok(Some(buf))
            }
            None => Ok(None),
        }
    }

    fn open_attach_stream(
        &mut self,
        source_path: Option<&str>,
        parent_nid: Option<u64>,
        attach_nid: Option<u64>,
        _filename: &str,
    ) -> std::result::Result<Option<AttachRead>, String> {
        let source = source_path.ok_or_else(|| "attach stream missing source_path".to_string())?;
        let parent = parent_nid.ok_or_else(|| "attach stream missing parent_nid".to_string())?;
        let attach = attach_nid.ok_or_else(|| "attach stream missing attach_nid".to_string())?;
        let locus = dedup_engine::keepset::MessageLocus {
            source_path: source.to_string(),
            source_pst: Path::new(source)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            folder_path: String::new(),
            nid: parent,
            is_orphaned: false,
        };
        match dedup_engine::AttachStreamSource::open_attach(self.inner, &locus, attach) {
            Ok(reader) => Ok(Some(AttachRead::from_reader(reader))),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Progress + soft max-volume stop (between messages only).
struct VolumeProgressSink {
    max_volume_bytes: Option<u64>,
    volume_index: u32,
    stderr: bool,
}

impl WriteProgressSink for VolumeProgressSink {
    fn on_progress(&mut self, p: &WriteProgress) {
        if !self.stderr {
            return;
        }
        if p.stage == WriteStage::WritingMessages {
            let _ = writeln!(
                std::io::stderr(),
                "unique-pst: volume {} stage={:?} messages={} physical_bytes={}",
                self.volume_index,
                p.stage,
                p.messages_written,
                p.current_physical_size
            );
        }
    }

    fn should_stop_and_finalize(&self, p: &WriteProgress) -> bool {
        let Some(max) = self.max_volume_bytes else {
            return false;
        };
        p.stage == WriteStage::WritingMessages && p.current_physical_size >= max
    }
}

/// Iterator that moves `WriteMessage`s out of a prepared slice (for early finalize).
struct TakeWriteMsgs<'a> {
    slice: &'a mut [PreparedWinner],
    pos: usize,
}

impl Iterator for TakeWriteMsgs<'_> {
    type Item = WriteMessage;

    fn next(&mut self) -> Option<WriteMessage> {
        if self.pos >= self.slice.len() {
            return None;
        }
        let msg = std::mem::take(&mut self.slice[self.pos].write_msg);
        self.pos += 1;
        Some(msg)
    }
}

/// Run unique-pst orchestration end-to-end.
pub fn run_unique_pst(args: UniquePstCliArgs) -> Result<()> {
    let started = Instant::now();

    // ── Phase 0: resolve paths, guards, prepare report-dir ──────────────────
    let mut paths = resolve_pst_paths(&args.paths)?;
    sort_input_paths(&mut paths);

    let out = resolve_cli_path_maybe_missing(&args.out)?.into_std_path_buf();
    if out
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| !e.eq_ignore_ascii_case("pst"))
        .unwrap_or(true)
    {
        // Soft warn only — allow any extension if caller insists.
        tracing::warn!(path = %out.display(), "unique-pst --out does not end in .pst");
    }

    let report_dir = match &args.report_dir {
        Some(p) => resolve_cli_path_maybe_missing(p)?.into_std_path_buf(),
        None => default_report_dir(&out),
    };

    let decision_csv = match &args.decision_csv {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => Some(report_dir.join("decisions.csv")),
    };
    let keep_set_json = match &args.keep_set_json {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => Some(report_dir.join("keepset.json")),
    };
    let integrity_csv = match &args.integrity_csv {
        Some(p) => Some(resolve_cli_path_maybe_missing(p)?.into_std_path_buf()),
        None => None,
    };

    if let Some(eml) = &args.also_eml {
        let _ = resolve_cli_path_maybe_missing(eml)?;
        // Soft residual: co-export not implemented in this track.
        tracing::warn!(
            path = %eml.display(),
            "--also-eml is accepted but not implemented (D-0071-also-eml residual); ignoring"
        );
    }

    guard_unique_pst_paths(
        &paths,
        &out,
        &report_dir,
        decision_csv.as_deref(),
        keep_set_json.as_deref(),
        integrity_csv.as_deref(),
    )?;

    // Refuse existing primary out without --overwrite.
    if out.exists() && !args.overwrite {
        return Err(CliError::Usage(format!(
            "--out already exists (pass --overwrite to replace): {}",
            out.display()
        )));
    }
    // Pre-check multi-volume siblings when overwriting is required later.
    if !args.overwrite {
        // volume 2+ existence is checked before each write with opts.overwrite
    }

    prepare_report_dir(&report_dir, args.overwrite)?;

    // Remove stale primary out if overwrite.
    if out.exists() && args.overwrite {
        if out.is_file() {
            fs::remove_file(&out).map_err(|e| {
                CliError::Msg(format!("remove existing --out {}: {e}", out.display()))
            })?;
        } else {
            return Err(CliError::Usage(format!(
                "--out exists and is not a file: {}",
                out.display()
            )));
        }
    }
    // Clear stale multi-volume siblings on overwrite so prior runs don't linger.
    if args.overwrite {
        clear_stale_volume_siblings(&out)?;
    }

    let eprint = |msg: &str| {
        let _ = writeln!(std::io::stderr(), "unique-pst: {msg}");
    };
    eprint("stage=scan");

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
    };

    // ── Phase 1: integrity scan ─────────────────────────────────────────────
    let outcome = run_scan(&paths, &opts)?;

    let provenance = KeepSetProvenance {
        scan_integrity_schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: args.mode.as_str().to_string(),
        input_files: paths.iter().map(|p| p.display().to_string()).collect(),
    };

    // ── Phase 2 / 2b: resolve + promote ─────────────────────────────────────
    eprint("stage=resolve");
    let mut resolved = resolve_groups(
        outcome.candidates,
        args.policy,
        args.family_policy,
        &args.prefer_path_contains,
        !args.no_tier2,
        Some(provenance),
    );

    eprint("stage=materialize");
    let mut mat = PstMaterializer::new(args.family_policy);
    let mut attach_src = PstAttachStreamSource::new();
    let _materialized_count =
        finalize_with_materialize(&mut resolved, &mut mat, &mut |_msg| Ok(()))
            .map_err(|e| CliError::Msg(format!("materialize/promote: {e}")))?;

    let keep_set = resolved.to_keep_set();

    // Prepare winners for write (keep_set order).
    eprint("stage=prepare_winners");
    let mut prepared: Vec<PreparedWinner> = Vec::with_capacity(keep_set.winners.len());
    let mut prepare_errors: Vec<String> = Vec::new();
    for entry in &keep_set.winners {
        match prepare_winner(&mut mat, entry) {
            Ok(p) => prepared.push(p),
            Err(e) => prepare_errors.push(format!("nid={:#x}: {e}", entry.locus.nid)),
        }
    }

    let folder_layout = match args.folder_layout {
        FolderLayoutArg::Preserve => FolderLayoutPolicy::PreservePaths {
            multi_source_prefix: true,
        },
        FolderLayoutArg::Flat => FolderLayoutPolicy::Flat {
            folder_display_name: "Unique Mail".to_string(),
        },
    };
    let parents_only = args.family_policy == FamilyPolicy::ParentsOnly;

    let write_opts_base = WritePstOpts {
        folder_display_name: "Unique Mail".to_string(),
        folder_layout,
        overwrite: args.overwrite,
        max_embedded_depth: 3,
        parents_only,
    };

    // ── Phase 3: multi-volume streaming write ───────────────────────────────
    eprint("stage=write");
    let mut volumes: Vec<VolumeReportRow> = Vec::new();
    let mut export_rows: Vec<ExportMessageRow> = Vec::new();
    let mut export_message_index: u64 = 0;
    let mut attach_written_total: u64 = 0;
    let mut attach_failed_total: u64 = 0;
    let mut export_partial = false;
    let mut export_error: Option<String> = None;
    let mut failed_volume_index: Option<u32> = None;
    let mut cursor = 0usize;
    let mut volume_index: u32 = 0;

    let protected: Vec<PathBuf> = paths.clone();

    while cursor < prepared.len() {
        volume_index += 1;
        let vol_path = volume_path_for(&out, volume_index);

        // Refuse existing secondary volumes without overwrite.
        if vol_path.exists() && !args.overwrite {
            export_partial = true;
            export_error = Some(format!(
                "volume path already exists (pass --overwrite): {}",
                vol_path.display()
            ));
            failed_volume_index = Some(volume_index);
            break;
        }
        if vol_path.exists() && args.overwrite {
            if vol_path.is_file() {
                if let Err(e) = fs::remove_file(&vol_path) {
                    export_partial = true;
                    export_error = Some(format!(
                        "cannot remove existing volume {}: {e}",
                        vol_path.display()
                    ));
                    failed_volume_index = Some(volume_index);
                    break;
                }
            } else {
                // Directory or other — will fail create; useful for fail-atomicity tests.
            }
        }

        // Ensure parent exists.
        if let Some(parent) = vol_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                export_partial = true;
                export_error = Some(format!("create volume parent {}: {e}", parent.display()));
                failed_volume_index = Some(volume_index);
                break;
            }
        }

        eprint(&format!(
            "stage=write_volume volume={volume_index} path={} remaining={}",
            vol_path.display(),
            prepared.len() - cursor
        ));

        let mut sink = VolumeProgressSink {
            max_volume_bytes: args.max_volume_bytes,
            volume_index,
            stderr: true,
        };
        let mut adapter = WriterAttachAdapter {
            inner: &mut attach_src,
        };

        // Per-volume overwrite: primary may already be cleared; secondary needs true
        // when we just deleted, false when fresh. Writer refuses existing unless overwrite.
        let mut vol_opts = write_opts_base.clone();
        vol_opts.overwrite = true; // we already enforced / deleted

        let remaining = &mut prepared[cursor..];
        let start_cursor = cursor;
        let iter = TakeWriteMsgs {
            slice: remaining,
            pos: 0,
        };

        let write_result = write_unicode_pst_streaming(
            &vol_path,
            iter,
            &protected,
            &vol_opts,
            Some(&mut adapter),
            Some(&mut sink),
        );

        match write_result {
            Ok(report) => {
                let written = report.messages_written as usize;
                let exceeded = args
                    .max_volume_bytes
                    .map(|max| report.bytes > max)
                    .unwrap_or(false);

                // Export rows for written messages (meta still on prepared[start..]).
                for i in 0..written {
                    let p = &prepared[start_cursor + i];
                    export_message_index += 1;
                    export_rows.push(ExportMessageRow {
                        source_path: p.source_path.clone(),
                        folder_path: p.folder_path.clone(),
                        nid: p.nid,
                        message_id_norm: p.message_id_norm.clone(),
                        edrm_mih: p.edrm_mih.clone(),
                        content_hash_hex: p.content_hash_hex.clone(),
                        volume_path: vol_path.display().to_string(),
                        volume_index,
                        export_message_index,
                    });
                }

                volumes.push(VolumeReportRow {
                    volume_index,
                    path: vol_path.display().to_string(),
                    bytes: report.bytes,
                    sha256_hex: report.sha256_hex,
                    md5_hex: report.md5_hex,
                    messages_written: report.messages_written,
                    finalized_early: report.finalized_early,
                    volume_exceeded_soft_limit: exceeded,
                });
                attach_written_total =
                    attach_written_total.saturating_add(report.attachments_written);
                attach_failed_total = attach_failed_total.saturating_add(report.attachments_failed);

                cursor = start_cursor + written;

                if !report.finalized_early {
                    // Consumed all remaining (or empty).
                    break;
                }
                // Early finalize: continue remaining winners on next volume.
                if written == 0 {
                    export_partial = true;
                    export_error = Some(format!(
                        "volume {volume_index} finalized with 0 messages written"
                    ));
                    failed_volume_index = Some(volume_index);
                    break;
                }
            }
            Err(e) => {
                // §3.3.1: delete incomplete current volume (and temp sibling); keep prior.
                delete_incomplete_volume(&vol_path);
                export_partial = true;
                export_error = Some(format!("volume {volume_index} write failed: {e}"));
                failed_volume_index = Some(volume_index);
                break;
            }
        }
    }

    // Prepare-errors mean some winners never written.
    if !prepare_errors.is_empty() && export_error.is_none() {
        export_partial = true;
        export_error = Some(format!(
            "prepare/materialize errors ({}): {:?}",
            prepare_errors.len(),
            prepare_errors
        ));
    }

    let messages_written_total: u64 = volumes.iter().map(|v| v.messages_written).sum();
    let count_mismatch = messages_written_total != keep_set.stats.unique && !export_partial;
    if count_mismatch {
        export_partial = true;
        export_error = Some(format!(
            "messages_written_total ({messages_written_total}) != unique ({})",
            keep_set.stats.unique
        ));
    }

    // ── Phase 4: report pack (always flush before exit) ─────────────────────
    eprint("stage=report");
    let mut decision_csv_out: Option<String> = None;
    if let Some(path) = &decision_csv {
        match DecisionCsvWriter::create(path) {
            Ok(mut wtr) => {
                if let Err(e) = resolved.write_decisions_csv(&mut wtr) {
                    tracing::warn!(error = %e, "decision csv write failed");
                } else if let Err(e) = wtr.flush() {
                    tracing::warn!(error = %e, "decision csv flush failed");
                } else {
                    decision_csv_out = Some(path.display().to_string());
                }
            }
            Err(e) => tracing::warn!(error = %e, "decision csv create failed"),
        }
    }

    let mut keep_set_json_out: Option<String> = None;
    if let Some(path) = &keep_set_json {
        match write_keep_set_json(path, &keep_set) {
            Ok(()) => keep_set_json_out = Some(path.display().to_string()),
            Err(e) => tracing::warn!(error = %e, "keepset.json write failed"),
        }
    }

    let volumes_csv_path = report_dir.join("volumes.csv");
    if let Err(e) = write_volumes_csv(&volumes_csv_path, &volumes) {
        tracing::warn!(error = %e, "volumes.csv write failed");
    }

    // export_messages.csv mandatory when ≥1 message written.
    let export_messages_path = report_dir.join("export_messages.csv");
    if messages_written_total > 0 || !export_rows.is_empty() {
        if let Err(e) = write_export_messages_csv(&export_messages_path, &export_rows) {
            tracing::warn!(error = %e, "export_messages.csv write failed");
        }
    } else {
        // Still write empty file with header for pack completeness when zero winners.
        if let Err(e) = write_export_messages_csv(&export_messages_path, &[]) {
            tracing::warn!(error = %e, "export_messages.csv write failed");
        }
    }

    // ── Phase 5: verify completed volumes ───────────────────────────────────
    eprint("stage=verify");
    let verification = verify_volumes(&volumes, &export_rows, args.verify_hash);

    let duration_ms = started.elapsed().as_millis() as u64;
    let exit_err = evaluate_exit_policy(&outcome.summary, &opts).err();
    let verify_err = if verification.ok {
        None
    } else {
        Some("verification failed".to_string())
    };
    let export_err = export_error.clone();

    let ok = exit_err.is_none()
        && verify_err.is_none()
        && export_err.is_none()
        && !export_partial
        && messages_written_total == keep_set.stats.unique;

    let summary_error = if !ok {
        let (code, message) = if let Some(msg) = export_err.as_ref() {
            ("export", msg.clone())
        } else if let Some(msg) = verify_err.as_ref() {
            ("verification", msg.clone())
        } else if let Some(msg) = exit_err.as_ref() {
            ("scan_integrity", msg.clone())
        } else {
            ("export", "unique-pst incomplete".to_string())
        };
        Some(SummaryError {
            code: code.to_string(),
            message,
        })
    } else {
        None
    };

    let summary = UniqueExportSummary {
        schema: UNIQUE_EXPORT_REPORT_SCHEMA.to_string(),
        ok,
        inputs: paths.iter().map(|p| p.display().to_string()).collect(),
        policy: args.policy.as_str().to_string(),
        family_policy: args.family_policy.as_str().to_string(),
        mode: args.mode.as_str().to_string(),
        folder_layout: args.folder_layout.as_str().to_string(),
        out: out.display().to_string(),
        report_dir: report_dir.display().to_string(),
        keep_set: keep_set.clone(),
        scan: outcome.summary,
        export: ExportSection {
            volumes: volumes.clone(),
            partial: export_partial || !ok && messages_written_total < keep_set.stats.unique,
            messages_written_total,
            attachments_written: attach_written_total,
            attachments_failed: attach_failed_total,
            error: export_error.clone(),
            failed_volume_index,
        },
        verification,
        duration_ms,
        max_volume_bytes: args.max_volume_bytes,
        decision_csv: decision_csv_out.clone(),
        keep_set_json: keep_set_json_out.clone(),
        error: summary_error.clone(),
    };

    let summary_path = report_dir.join("summary.json");
    if let Err(e) = write_summary_json(&summary_path, &summary) {
        tracing::warn!(error = %e, "summary.json write failed");
    }

    // ── Phase 6: exit ───────────────────────────────────────────────────────
    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        if !ok {
            let msg = summary_error
                .map(|e| e.message)
                .unwrap_or_else(|| "unique-pst failed".into());
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: crate::error::CliExit::Generic,
            });
        }
        return Ok(());
    }

    println!(
        "=== Unique PST export ({UNIQUE_EXPORT_REPORT_SCHEMA}) policy={} family={} ===",
        args.policy.as_str(),
        args.family_policy.as_str()
    );
    println!("  out:              {}", out.display());
    println!("  report_dir:       {}", report_dir.display());
    println!(
        "  messages_written: {}  unique: {}  volumes: {}",
        messages_written_total,
        keep_set.stats.unique,
        volumes.len()
    );
    println!(
        "  attach written:   {}  attach failed: {}",
        attach_written_total, attach_failed_total
    );
    println!("  partial:          {}  ok: {ok}", summary.export.partial);
    for v in &volumes {
        println!(
            "  volume {}: {} ({} msgs, {} bytes)",
            v.volume_index, v.path, v.messages_written, v.bytes
        );
    }
    if let Some(p) = &decision_csv_out {
        println!("  decision_csv:     {p}");
    }
    if let Some(p) = &keep_set_json_out {
        println!("  keep_set_json:    {p}");
    }
    println!("  summary:          {}", summary_path.display());

    if !ok {
        let msg = summary_error
            .map(|e| e.message)
            .unwrap_or_else(|| "unique-pst failed".into());
        return Err(CliError::Msg(msg));
    }
    Ok(())
}

fn prepare_winner(
    mat: &mut PstMaterializer,
    entry: &KeepEntry,
) -> std::result::Result<PreparedWinner, String> {
    let mut msg = mat
        .materialize(&entry.locus)
        .map_err(|e| format!("re-materialize: {e}"))?;
    msg.message_id_norm = entry.message_id_norm.clone();
    msg.content_hash = entry.content_hash;
    msg.edrm_mih_hex = entry.edrm_mih_hex.clone();
    msg.fidelity = entry.integrity.clone();

    let (write_msg, _dropped) = from_canonical_message(&msg);
    let content_hash_hex = entry
        .content_hash
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    Ok(PreparedWinner {
        source_path: entry.locus.source_path.clone(),
        folder_path: entry.locus.folder_path.clone(),
        nid: entry.locus.nid,
        message_id_norm: entry.message_id_norm.clone().unwrap_or_default(),
        edrm_mih: entry.edrm_mih_hex.clone().unwrap_or_default(),
        content_hash_hex,
        write_msg,
    })
}

/// Delete incomplete volume file and same-dir temp sibling (writer cleanup best-effort).
fn delete_incomplete_volume(vol_path: &Path) {
    let tmp = temp_sibling_path(vol_path);
    if tmp.exists() {
        let _ = fs::remove_file(&tmp);
    }
    if vol_path.exists() {
        let _ = fs::remove_file(vol_path);
    }
    // If vol_path is a directory (fail-injection), leave it — we didn't create a PST there.
}

fn clear_stale_volume_siblings(out: &Path) -> Result<()> {
    // Remove out_vol002.pst, out_vol003.pst, ... that exist as files.
    for i in 2u32..=999 {
        let p = volume_path_for(out, i);
        if !p.exists() {
            break;
        }
        if p.is_file() {
            fs::remove_file(&p)
                .map_err(|e| CliError::Msg(format!("remove stale volume {}: {e}", p.display())))?;
        }
    }
    Ok(())
}

fn prepare_report_dir(report_dir: &Path, overwrite: bool) -> Result<()> {
    if report_dir.exists() {
        if !report_dir.is_dir() {
            return Err(CliError::Usage(format!(
                "--report-dir exists and is not a directory: {}",
                report_dir.display()
            )));
        }
        let non_empty = fs::read_dir(report_dir)
            .map_err(|e| CliError::Msg(format!("read report-dir {}: {e}", report_dir.display())))?
            .next()
            .is_some();
        if non_empty && !overwrite {
            return Err(CliError::Usage(format!(
                "--report-dir is not empty (pass --overwrite to replace contents): {}",
                report_dir.display()
            )));
        }
        if non_empty && overwrite {
            for entry in fs::read_dir(report_dir).map_err(|e| {
                CliError::Msg(format!("read report-dir {}: {e}", report_dir.display()))
            })? {
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
        fs::create_dir_all(report_dir).map_err(|e| {
            CliError::Msg(format!("create --report-dir {}: {e}", report_dir.display()))
        })?;
    }
    Ok(())
}

/// Path guards: refuse layouts that would overwrite or nest under source PSTs.
fn guard_unique_pst_paths(
    inputs: &[PathBuf],
    out: &Path,
    report_dir: &Path,
    decision_csv: Option<&Path>,
    keep_set_json: Option<&Path>,
    integrity_csv: Option<&Path>,
) -> Result<()> {
    for input in inputs {
        if paths_equal(out, input) {
            return Err(CliError::Usage(format!(
                "refusing --out equal to an input PST: {}",
                out.display()
            )));
        }
        if is_same_or_under(out, input) {
            return Err(CliError::Usage(format!(
                "refusing --out nested under an input PST: out={} input={}",
                out.display(),
                input.display()
            )));
        }
        // Multi-volume siblings next to out — check primary parent vs inputs.
        if is_same_or_under(report_dir, input) && paths_equal(report_dir, input) {
            return Err(CliError::Usage(format!(
                "refusing --report-dir equal to an input PST: {}",
                report_dir.display()
            )));
        }
        if paths_equal(report_dir, input) {
            return Err(CliError::Usage(format!(
                "refusing --report-dir equal to an input PST: {}",
                report_dir.display()
            )));
        }
        // Report-dir must not contain an input (recursive clear on overwrite).
        if is_same_or_under(input, report_dir) {
            return Err(CliError::Usage(format!(
                "refusing --report-dir that contains an input PST: report_dir={} input={}",
                report_dir.display(),
                input.display()
            )));
        }
        // Out must not land inside report-dir clear? out is a file; if out is under report-dir
        // and we clear report-dir first we're fine (out not yet written). Still refuse out==input.
        for art in [decision_csv, keep_set_json, integrity_csv]
            .into_iter()
            .flatten()
        {
            if paths_equal(art, input) {
                return Err(CliError::Usage(format!(
                    "refusing report artifact that equals an input PST: {}",
                    art.display()
                )));
            }
        }
        // Volume 2+ paths next to out: if out's directory is an input file path nonsense — already covered.
        let vol2 = volume_path_for(out, 2);
        if paths_equal(&vol2, input) {
            return Err(CliError::Usage(format!(
                "refusing multi-volume path equal to an input PST: {}",
                vol2.display()
            )));
        }
    }
    Ok(())
}

fn verify_volumes(
    volumes: &[VolumeReportRow],
    export_rows: &[ExportMessageRow],
    verify_hash: bool,
) -> VerificationReport {
    let mut vol_results = Vec::new();
    let mut all_ok = true;

    for vol in volumes {
        let path = PathBuf::from(&vol.path);
        let mut open_ok = false;
        let mut message_count_match = false;
        let mut messages_found = 0u64;
        let mut sample_mid_ok = true;
        let mut hash_match: Option<bool> = None;
        let mut error: Option<String> = None;

        match PstFile::open(&path) {
            Ok(mut pst) => {
                open_ok = true;
                match pst.folders() {
                    Ok(folders) => {
                        messages_found = folders.iter().map(|f| f.message_nids.len() as u64).sum();
                        message_count_match = messages_found == vol.messages_written;

                        // Sample min(5, N) Message-IDs or subjects vs export_messages for volume.
                        let vol_exports: Vec<&ExportMessageRow> = export_rows
                            .iter()
                            .filter(|r| r.volume_index == vol.volume_index)
                            .collect();
                        let sample_n = (vol_exports.len()).min(5);
                        if sample_n > 0 {
                            // Collect message NIDs from written PST with properties.
                            let mut written_mids: Vec<String> = Vec::new();
                            let mut written_subjects: Vec<String> = Vec::new();
                            for folder in &folders {
                                for &nid in &folder.message_nids {
                                    if let Ok(props) = pst.read_message_properties(nid) {
                                        if let Some(mid) = props.message_id {
                                            written_mids.push(normalize_mid_loose(&mid));
                                        }
                                        if let Some(sub) = props.subject {
                                            written_subjects.push(sub);
                                        }
                                    }
                                    if written_mids.len() + written_subjects.len() >= 64 {
                                        break;
                                    }
                                }
                            }
                            for r in vol_exports.iter().take(sample_n) {
                                let ok_one = if !r.message_id_norm.is_empty() {
                                    let want = normalize_mid_loose(&r.message_id_norm);
                                    written_mids.iter().any(|m| m == &want)
                                        || written_mids.iter().any(|m| {
                                            m.contains(want.trim_matches(|c| c == '<' || c == '>'))
                                        })
                                } else {
                                    // No MID on export row — structural count is enough for this sample.
                                    true
                                };
                                if !ok_one {
                                    sample_mid_ok = false;
                                    error = Some(format!(
                                        "sample MID not found in volume: {}",
                                        r.message_id_norm
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error = Some(format!("folders: {e}"));
                        sample_mid_ok = false;
                    }
                }
            }
            Err(e) => {
                error = Some(format!("open: {e}"));
            }
        }

        if verify_hash {
            match sha256_file(&path) {
                Ok(hex) => {
                    let m = hex.eq_ignore_ascii_case(&vol.sha256_hex);
                    hash_match = Some(m);
                    if !m {
                        error = Some(format!(
                            "sha256 mismatch: report={} rehash={}",
                            vol.sha256_hex, hex
                        ));
                    }
                }
                Err(e) => {
                    hash_match = Some(false);
                    error = Some(format!("rehash: {e}"));
                }
            }
        }

        let vol_ok = open_ok && message_count_match && sample_mid_ok && hash_match.unwrap_or(true);
        if !vol_ok {
            all_ok = false;
        }
        vol_results.push(VolumeVerification {
            volume_index: vol.volume_index,
            path: vol.path.clone(),
            open_ok,
            message_count_match,
            messages_found,
            messages_expected: vol.messages_written,
            sample_mid_ok,
            hash_match,
            error,
        });
    }

    // No volumes completed and we expected work → not ok (caller may still set ok=false).
    if volumes.is_empty() {
        all_ok = false;
    }

    VerificationReport {
        ok: all_ok,
        volumes: vol_results,
        rehash_ran: verify_hash,
    }
}

fn normalize_mid_loose(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_ascii_lowercase()
}

fn sha256_file(path: &Path) -> std::result::Result<String, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_rejects_out_equal_input() {
        let inputs = vec![PathBuf::from(r"C:\data\mail.pst")];
        let out = PathBuf::from(r"C:\data\mail.pst");
        let report = PathBuf::from(r"C:\data\mail_report");
        assert!(guard_unique_pst_paths(&inputs, &out, &report, None, None, None).is_err());
    }

    #[test]
    fn guard_rejects_report_dir_contains_input() {
        let inputs = vec![PathBuf::from(r"C:\data\pack\mail.pst")];
        let out = PathBuf::from(r"C:\data\unique.pst");
        let report = PathBuf::from(r"C:\data\pack");
        assert!(guard_unique_pst_paths(&inputs, &out, &report, None, None, None).is_err());
    }

    #[test]
    fn guard_accepts_disjoint() {
        let inputs = vec![PathBuf::from(r"C:\data\mail.pst")];
        let out = PathBuf::from(r"C:\export\unique.pst");
        let report = PathBuf::from(r"C:\export\unique_report");
        let dec = PathBuf::from(r"C:\export\unique_report\decisions.csv");
        guard_unique_pst_paths(&inputs, &out, &report, Some(&dec), None, None).expect("ok");
    }
}
