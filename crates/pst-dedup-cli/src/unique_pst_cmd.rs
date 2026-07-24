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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::Args;
use dedup_engine::integrity::{
    compute_preflight, IntegrityThresholds, PreflightInputs, ScanMode, SCAN_INTEGRITY_SCHEMA,
};
use dedup_engine::keepset::{
    finalize_with_materialize, resolve_groups, sort_input_paths, write_keep_set_json,
    DecisionCsvWriter, FamilyPolicy, KeepEntry, KeepPolicy, KeepSet, KeepSetProvenance,
    KeepSetStats, MessageMaterializer, KEEP_SET_SCHEMA,
};
use pst_reader::PstFile;
use pst_writer::{
    from_canonical_message, temp_sibling_path, write_unicode_pst_streaming, AttachRead,
    AttachStreamSource, FolderLayoutPolicy, WriteMessage, WriteProgress, WriteProgressSink,
    WritePstOpts, WriteStage,
};
use sha2::{Digest, Sha256};

use crate::error::{CliError, Result};
use crate::paths::{
    is_same_or_under, is_same_or_under_resolved, paths_equal, paths_equal_resolved,
    resolve_cli_path_maybe_missing,
};
use crate::pst_materializer::{PstAttachStreamSource, PstMaterializer};
use crate::scan::{evaluate_exit_policy, resolve_pst_paths, run_scan, ScanOptions};
use crate::unique_export_report::{
    default_report_dir, volume_path_for, write_export_messages_csv, write_summary_json,
    write_volumes_csv, ExportMessageRow, ExportSection, SummaryError, UniqueExportSummary,
    VerificationReport, VolumeReportRow, VolumeVerification, UNIQUE_EXPORT_REPORT_SCHEMA,
};

/// Max volume index considered for stale-sibling cleanup and collision guards.
const MAX_VOLUME_SIBLING_INDEX: u32 = 999;

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
#[derive(Debug, Clone)]
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

/// Run options / hooks for GUI and library callers (0072).
///
/// CLI default: `stderr_progress: true`, no cancel / progress / log callbacks.
pub struct UniquePstRunOptions {
    pub cancel: Option<Arc<AtomicBool>>,
    /// When true (CLI default), mirror stage lines to stderr.
    pub stderr_progress: bool,
    /// Optional progress observer (GUI).
    pub on_progress: Option<Box<dyn FnMut(UniquePstProgress) + Send>>,
    /// Optional log/warning lines (GUI Details panel).
    pub on_log: Option<Box<dyn FnMut(String) + Send>>,
}

impl Default for UniquePstRunOptions {
    fn default() -> Self {
        Self {
            cancel: None,
            stderr_progress: true,
            on_progress: None,
            on_log: None,
        }
    }
}

/// Structured progress tick for GUI / automation.
#[derive(Debug, Clone)]
pub struct UniquePstProgress {
    /// Stage label: `"scan"` | `"resolve"` | `"materialize"` | `"write"` | `"report"` | `"verify"` | …
    pub stage: String,
    pub volume_index: u32,
    /// Messages written on the **current** volume (resets each volume).
    pub messages_written: u64,
    /// Cumulative messages written across completed volumes + current volume.
    pub messages_written_cumulative: u64,
    pub physical_bytes: u64,
    pub winners_total: Option<u64>,
}

/// Per-volume digest surfaced on [`UniquePstOutcome`] (Done UI / library).
#[derive(Debug, Clone)]
pub struct UniqueVolumeDigest {
    pub volume_index: u32,
    pub path: String,
    pub bytes: u64,
    pub messages_written: u64,
    pub sha256_hex: String,
    /// MD5 when available from the writer report (may be empty).
    pub md5_hex: String,
}

impl From<&VolumeReportRow> for UniqueVolumeDigest {
    fn from(v: &VolumeReportRow) -> Self {
        Self {
            volume_index: v.volume_index,
            path: v.path.clone(),
            bytes: v.bytes,
            messages_written: v.messages_written,
            sha256_hex: v.sha256_hex.clone(),
            md5_hex: v.md5_hex.clone(),
        }
    }
}

/// Structured outcome for library / GUI (also used after soft cancel / partial).
#[derive(Debug, Clone)]
pub struct UniquePstOutcome {
    pub ok: bool,
    pub cancelled: bool,
    pub report_dir: PathBuf,
    pub summary_path: PathBuf,
    pub out: PathBuf,
    pub messages_written_total: u64,
    pub unique: u64,
    pub volume_count: usize,
    /// Completed volumes with digests (empty on pre-write cancel).
    pub volumes: Vec<UniqueVolumeDigest>,
    pub error_message: Option<String>,
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
    subject: String,
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

type ProgressCb = Arc<Mutex<Box<dyn FnMut(UniquePstProgress) + Send>>>;
type LogCb = Arc<Mutex<Box<dyn FnMut(String) + Send>>>;

/// Log write-stage progress at most this often (messages) or every
/// [`WRITE_PROGRESS_LOG_INTERVAL`] (whichever comes first after the first tick).
const WRITE_PROGRESS_LOG_EVERY_N: u64 = 50;
const WRITE_PROGRESS_LOG_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Progress + soft max-volume stop + cooperative cancel (between messages only).
struct VolumeProgressSink {
    max_volume_bytes: Option<u64>,
    volume_index: u32,
    /// Messages already written on prior completed volumes (for cumulative progress).
    messages_written_prior: u64,
    stderr: bool,
    cancel: Option<Arc<AtomicBool>>,
    winners_total: Option<u64>,
    on_progress: Option<ProgressCb>,
    on_log: Option<LogCb>,
    last_log_at: Instant,
    last_logged_messages: u64,
    /// True until the first WritingMessages tick (always log first).
    first_write_log: bool,
}

impl WriteProgressSink for VolumeProgressSink {
    fn on_progress(&mut self, p: &WriteProgress) {
        if p.stage == WriteStage::WritingMessages {
            // Progress bar: every tick (GUI needs smooth updates).
            if let Some(cb) = &self.on_progress {
                if let Ok(mut g) = cb.lock() {
                    g(UniquePstProgress {
                        stage: "write".into(),
                        volume_index: self.volume_index,
                        messages_written: p.messages_written,
                        messages_written_cumulative: self
                            .messages_written_prior
                            .saturating_add(p.messages_written),
                        physical_bytes: p.current_physical_size,
                        winners_total: self.winners_total,
                    });
                }
            }
            // Log lines: throttle to avoid Details/stderr spam on large volumes.
            let elapsed = self.last_log_at.elapsed() >= WRITE_PROGRESS_LOG_INTERVAL;
            let every_n = p.messages_written.saturating_sub(self.last_logged_messages)
                >= WRITE_PROGRESS_LOG_EVERY_N;
            if self.first_write_log || elapsed || every_n {
                self.first_write_log = false;
                self.last_log_at = Instant::now();
                self.last_logged_messages = p.messages_written;
                let line = format!(
                    "unique-pst: volume {} stage={:?} messages={} cumulative={} physical_bytes={}",
                    self.volume_index,
                    p.stage,
                    p.messages_written,
                    self.messages_written_prior
                        .saturating_add(p.messages_written),
                    p.current_physical_size
                );
                if self.stderr {
                    let _ = writeln!(std::io::stderr(), "{line}");
                }
                if let Some(log) = &self.on_log {
                    if let Ok(mut g) = log.lock() {
                        g(line);
                    }
                }
            }
        }
    }

    fn should_stop_and_finalize(&self, p: &WriteProgress) -> bool {
        let Some(max) = self.max_volume_bytes else {
            return false;
        };
        p.stage == WriteStage::WritingMessages && p.current_physical_size >= max
    }

    fn should_cancel(&self, _p: &WriteProgress) -> bool {
        self.cancel
            .as_ref()
            .map(|c| c.load(Ordering::SeqCst))
            .unwrap_or(false)
    }
}

fn cancel_requested(cancel: &Option<Arc<AtomicBool>>) -> bool {
    cancel
        .as_ref()
        .map(|c| c.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// Emit a stage/log line to stderr (optional) and `on_log`.
fn emit_log(stderr: bool, on_log: &Option<LogCb>, msg: &str) {
    let line = format!("unique-pst: {msg}");
    if stderr {
        let _ = writeln!(std::io::stderr(), "{line}");
    }
    if let Some(log) = on_log {
        if let Ok(mut g) = log.lock() {
            g(line);
        }
    }
}

fn emit_stage_progress(
    on_progress: &Option<ProgressCb>,
    stage: &str,
    volume_index: u32,
    messages_written: u64,
    messages_written_cumulative: u64,
    physical_bytes: u64,
    winners_total: Option<u64>,
) {
    if let Some(cb) = on_progress {
        if let Ok(mut g) = cb.lock() {
            g(UniquePstProgress {
                stage: stage.into(),
                volume_index,
                messages_written,
                messages_written_cumulative,
                physical_bytes,
                winners_total,
            });
        }
    }
}

/// Paths + policy context for a minimal cancelled `summary.json`.
struct CancelledSummaryCtx<'a> {
    summary_path: &'a Path,
    inputs: &'a [PathBuf],
    out: &'a Path,
    report_dir: &'a Path,
    policy: KeepPolicy,
    family_policy: FamilyPolicy,
    mode: ScanMode,
    folder_layout: FolderLayoutArg,
    max_volume_bytes: Option<u64>,
    duration_ms: u64,
}

/// Minimal cancelled `summary.json` when cancel hits after report-dir prepare but
/// before a full report pack can be built (pre-scan / early exit).
fn write_cancelled_summary_json(ctx: &CancelledSummaryCtx<'_>) {
    let preflight = compute_preflight(&PreflightInputs {
        mode: ctx.mode,
        recoverable: 0,
        skipped: 0,
        crc_skips: 0,
        failed_files: 0,
        input_file_count: ctx.inputs.len() as u64,
        thresholds: IntegrityThresholds::default(),
    });
    let scan = crate::scan::ScanSummary {
        schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: ctx.mode,
        files: vec![],
        total_messages: 0,
        unique: 0,
        duplicates: 0,
        tier1_hits: 0,
        tier2_hits: 0,
        savings_bytes: 0,
        skipped: 0,
        skipped_by_reason: std::collections::BTreeMap::new(),
        recoverable_messages: 0,
        degraded_messages: 0,
        degraded_by_reason: std::collections::BTreeMap::new(),
        orphaned_messages: 0,
        failed_files: 0,
        partial_files: 0,
        opened_files: 0,
        duration_secs: 0.0,
        preflight,
        skips: vec![],
        integrity_csv: None,
    };
    let keep_set = KeepSet {
        schema: KEEP_SET_SCHEMA.to_string(),
        policy: ctx.policy,
        family_policy: ctx.family_policy,
        created_from: None,
        winners: vec![],
        stats: KeepSetStats::default(),
    };
    let summary = UniqueExportSummary {
        schema: UNIQUE_EXPORT_REPORT_SCHEMA.to_string(),
        ok: false,
        inputs: ctx.inputs.iter().map(|p| p.display().to_string()).collect(),
        policy: ctx.policy.as_str().to_string(),
        family_policy: ctx.family_policy.as_str().to_string(),
        mode: ctx.mode.as_str().to_string(),
        folder_layout: ctx.folder_layout.as_str().to_string(),
        out: ctx.out.display().to_string(),
        report_dir: ctx.report_dir.display().to_string(),
        keep_set,
        scan,
        export: ExportSection {
            volumes: vec![],
            partial: true,
            messages_written_total: 0,
            attachments_written: 0,
            attachments_failed: 0,
            error: Some("cancelled".into()),
            failed_volume_index: None,
        },
        verification: VerificationReport {
            ok: false,
            volumes: vec![],
            rehash_ran: false,
        },
        duration_ms: ctx.duration_ms,
        max_volume_bytes: ctx.max_volume_bytes,
        decision_csv: None,
        keep_set_json: None,
        error: Some(SummaryError {
            code: "cancelled".into(),
            message: "cancelled".into(),
        }),
    };
    if let Err(e) = write_summary_json(ctx.summary_path, &summary) {
        tracing::warn!(
            path = %ctx.summary_path.display(),
            "cancelled summary.json write failed: {e}"
        );
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

/// Run unique-pst orchestration end-to-end (CLI entry).
///
/// Defaults to stderr stage lines; maps structured outcome to [`Result<()>`].
pub fn run_unique_pst(args: UniquePstCliArgs) -> Result<()> {
    let outcome = run_unique_pst_with_options(
        args,
        UniquePstRunOptions {
            cancel: None,
            stderr_progress: true,
            on_progress: None,
            on_log: None,
        },
    )?;
    if outcome.ok && !outcome.cancelled {
        Ok(())
    } else {
        Err(CliError::Msg(
            outcome
                .error_message
                .unwrap_or_else(|| "unique-pst failed".into()),
        ))
    }
}

/// Library / GUI entry: same orchestration as CLI with cancel, progress, and log hooks.
///
/// Returns a structured [`UniquePstOutcome`] even on soft cancel/partial when the
/// report pack could be flushed. Hard usage/path errors still return [`Err`].
///
/// When `args.json` is true, the summary is printed to stdout (CLI contract). On
/// JSON failure the function returns [`CliError::AlreadyEmitted`] after printing.
pub fn run_unique_pst_with_options(
    args: UniquePstCliArgs,
    run_opts: UniquePstRunOptions,
) -> Result<UniquePstOutcome> {
    let started = Instant::now();
    let cancel = run_opts.cancel.clone();
    let stderr = run_opts.stderr_progress;
    let on_progress = run_opts.on_progress.map(|f| Arc::new(Mutex::new(f)));
    let on_log = run_opts.on_log.map(|f| Arc::new(Mutex::new(f)));

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
        emit_log(stderr, &on_log, "warning: --out does not end in .pst");
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
        emit_log(
            stderr,
            &on_log,
            &format!(
                "warning: --also-eml is accepted but not implemented (D-0071-also-eml residual); ignoring {}",
                eml.display()
            ),
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

    prepare_report_dir(&report_dir, args.overwrite)?;

    // Remove stale primary out if overwrite.
    if out.exists() && args.overwrite {
        // Re-check collision after report-dir prep (inputs must never be deleted).
        if path_collides_with_inputs(&out, &paths) {
            return Err(CliError::Usage(format!(
                "refusing to overwrite --out that equals an input PST: {}",
                out.display()
            )));
        }
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
    // Never deletes paths that equal/contain inputs; refuses if any sibling collides.
    if args.overwrite {
        clear_stale_volume_siblings(&out, &paths)?;
    }

    let summary_path = report_dir.join("summary.json");
    let mut cancelled = false;

    emit_log(stderr, &on_log, "stage=scan");
    emit_stage_progress(&on_progress, "scan", 0, 0, 0, 0, None);

    if cancel_requested(&cancel) {
        // Report dir already prepared — write a minimal cancelled summary so
        // Open report / operators see ok=false rather than a missing summary.
        emit_log(stderr, &on_log, "cancelled before scan");
        write_cancelled_summary_json(&CancelledSummaryCtx {
            summary_path: &summary_path,
            inputs: &paths,
            out: &out,
            report_dir: &report_dir,
            policy: args.policy,
            family_policy: args.family_policy,
            mode: args.mode,
            folder_layout: args.folder_layout,
            max_volume_bytes: args.max_volume_bytes,
            duration_ms: started.elapsed().as_millis() as u64,
        });
        return Ok(UniquePstOutcome {
            ok: false,
            cancelled: true,
            report_dir,
            summary_path,
            out,
            messages_written_total: 0,
            unique: 0,
            volume_count: 0,
            volumes: vec![],
            error_message: Some("cancelled".into()),
        });
    }

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
        // Cooperative cancel checked between files/folders/messages in run_scan.
        cancel: cancel.clone(),
    };

    // ── Phase 1: integrity scan ─────────────────────────────────────────────
    let outcome = run_scan(&paths, &opts)?;

    // Scan-level integrity warnings must reach on_log (GUI Log panel), not only tracing.
    {
        let s = &outcome.summary;
        if s.skipped > 0 || s.degraded_messages > 0 || s.failed_files > 0 || s.partial_files > 0 {
            emit_log(
                stderr,
                &on_log,
                &format!(
                    "warning: scan integrity degraded/skips: skipped={} degraded={} failed_files={} partial_files={}",
                    s.skipped, s.degraded_messages, s.failed_files, s.partial_files
                ),
            );
        }
        if !s.degraded_by_reason.is_empty() {
            let reasons: Vec<String> = s
                .degraded_by_reason
                .iter()
                .map(|(k, n)| format!("{k}={n}"))
                .collect();
            emit_log(
                stderr,
                &on_log,
                &format!("warning: scan degraded_by_reason: {}", reasons.join(", ")),
            );
        }
        if !s.skipped_by_reason.is_empty() {
            let reasons: Vec<String> = s
                .skipped_by_reason
                .iter()
                .map(|(k, n)| format!("{k}={n}"))
                .collect();
            emit_log(
                stderr,
                &on_log,
                &format!("warning: scan skipped_by_reason: {}", reasons.join(", ")),
            );
        }
    }

    if cancel_requested(&cancel) {
        cancelled = true;
        emit_log(stderr, &on_log, "cancelled after scan");
        // Continue to resolve so report pack can still be honest if we get far enough;
        // write loop will skip when cancelled.
    }

    let provenance = KeepSetProvenance {
        scan_integrity_schema: SCAN_INTEGRITY_SCHEMA.to_string(),
        mode: args.mode.as_str().to_string(),
        input_files: paths.iter().map(|p| p.display().to_string()).collect(),
    };

    // ── Phase 2 / 2b: resolve + promote ─────────────────────────────────────
    emit_log(stderr, &on_log, "stage=resolve");
    emit_stage_progress(&on_progress, "resolve", 0, 0, 0, 0, None);
    let mut resolved = resolve_groups(
        outcome.candidates,
        args.policy,
        args.family_policy,
        &args.prefer_path_contains,
        !args.no_tier2,
        Some(provenance),
    );

    if cancel_requested(&cancel) {
        cancelled = true;
        emit_log(stderr, &on_log, "cancelled after resolve");
    }

    emit_log(stderr, &on_log, "stage=materialize");
    emit_stage_progress(&on_progress, "materialize", 0, 0, 0, 0, None);
    // `--no-attachments` forces parents-only materialize/write so attach streams
    // are not opened (scan already omitted attach metadata when the flag is set).
    let effective_family = if args.no_attachments {
        FamilyPolicy::ParentsOnly
    } else {
        args.family_policy
    };
    // Bridge materializer soft attach/open warnings into on_log (GUI Details parity).
    let mat_warn: Option<crate::pst_materializer::MaterializeWarnCb> = on_log.as_ref().map(|log| {
        let log = Arc::clone(log);
        let stderr_w = stderr;
        Arc::new(Mutex::new(move |msg: String| {
            // emit_log style without re-locking options: prefix for GUI filter.
            let line = format!("unique-pst: {msg}");
            if stderr_w {
                let _ = writeln!(std::io::stderr(), "{line}");
            }
            if let Ok(mut g) = log.lock() {
                g(line);
            }
        })) as crate::pst_materializer::MaterializeWarnCb
    });
    let mut mat = match mat_warn {
        Some(cb) => PstMaterializer::new(effective_family).with_warn_sink(cb),
        None => PstMaterializer::new(effective_family),
    };
    let mut attach_src = PstAttachStreamSource::new();
    let _materialized_count =
        finalize_with_materialize(&mut resolved, &mut mat, &mut |_msg| Ok(()))
            .map_err(|e| CliError::Msg(format!("materialize/promote: {e}")))?;

    if cancel_requested(&cancel) {
        cancelled = true;
        emit_log(stderr, &on_log, "cancelled after materialize");
    }

    let keep_set = resolved.to_keep_set();
    let winners_total = Some(keep_set.stats.unique);

    // Prepare winners for write (keep_set order).
    emit_log(stderr, &on_log, "stage=prepare_winners");
    emit_stage_progress(&on_progress, "prepare_winners", 0, 0, 0, 0, winners_total);
    let mut prepared: Vec<PreparedWinner> = Vec::with_capacity(keep_set.winners.len());
    let mut prepare_errors: Vec<String> = Vec::new();
    for entry in &keep_set.winners {
        if cancel_requested(&cancel) {
            cancelled = true;
            break;
        }
        match prepare_winner(&mut mat, entry) {
            Ok(p) => prepared.push(p),
            Err(e) => {
                let msg = format!("nid={:#x}: {e}", entry.locus.nid);
                emit_log(
                    stderr,
                    &on_log,
                    &format!("warning: prepare/materialize error: {msg}"),
                );
                prepare_errors.push(msg);
            }
        }
    }
    if !prepare_errors.is_empty() {
        emit_log(
            stderr,
            &on_log,
            &format!(
                "warning: prepare/materialize errors total={}",
                prepare_errors.len()
            ),
        );
    }

    let folder_layout = match args.folder_layout {
        FolderLayoutArg::Preserve => FolderLayoutPolicy::PreservePaths {
            multi_source_prefix: true,
        },
        FolderLayoutArg::Flat => FolderLayoutPolicy::Flat {
            folder_display_name: "Unique Mail".to_string(),
        },
    };
    let parents_only = effective_family == FamilyPolicy::ParentsOnly || args.no_attachments;

    let write_opts_base = WritePstOpts {
        folder_display_name: "Unique Mail".to_string(),
        folder_layout,
        overwrite: args.overwrite,
        max_embedded_depth: 3,
        parents_only,
    };

    // ── Phase 3: multi-volume streaming write ───────────────────────────────
    // Re-check cancel after prepare (including empty keep-set): write loop body
    // is skipped when `prepared` is empty, so this checkpoint is required.
    if cancel_requested(&cancel) {
        cancelled = true;
        emit_log(stderr, &on_log, "cancelled before write");
    }

    emit_log(stderr, &on_log, "stage=write");
    emit_stage_progress(&on_progress, "write", 0, 0, 0, 0, winners_total);
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
    let mut messages_written_prior: u64 = 0;

    let protected: Vec<PathBuf> = paths.clone();

    if cancelled {
        export_partial = true;
        export_error = Some("cancelled".into());
    }

    while cursor < prepared.len() && !cancelled {
        if cancel_requested(&cancel) {
            cancelled = true;
            export_partial = true;
            export_error = Some("cancelled".into());
            emit_log(stderr, &on_log, "cancelled before volume write");
            break;
        }

        volume_index += 1;
        let vol_path = volume_path_for(&out, volume_index);

        // Source protection: never write/delete a volume path that collides with input.
        if path_collides_with_inputs(&vol_path, &paths) {
            export_partial = true;
            export_error = Some(format!(
                "refusing volume path equal to an input PST: {}",
                vol_path.display()
            ));
            failed_volume_index = Some(volume_index);
            break;
        }

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
            if path_collides_with_inputs(&vol_path, &paths) {
                export_partial = true;
                export_error = Some(format!(
                    "refusing to overwrite volume path equal to an input PST: {}",
                    vol_path.display()
                ));
                failed_volume_index = Some(volume_index);
                break;
            }
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

        emit_log(
            stderr,
            &on_log,
            &format!(
                "stage=write_volume volume={volume_index} path={} remaining={}",
                vol_path.display(),
                prepared.len() - cursor
            ),
        );
        emit_stage_progress(
            &on_progress,
            "write",
            volume_index,
            0,
            messages_written_prior,
            0,
            winners_total,
        );

        let mut sink = VolumeProgressSink {
            max_volume_bytes: args.max_volume_bytes,
            volume_index,
            messages_written_prior,
            stderr,
            cancel: cancel.clone(),
            winners_total,
            on_progress: on_progress.clone(),
            on_log: on_log.clone(),
            last_log_at: Instant::now()
                .checked_sub(WRITE_PROGRESS_LOG_INTERVAL)
                .unwrap_or_else(Instant::now),
            last_logged_messages: 0,
            first_write_log: true,
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
                        subject: p.subject.clone(),
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
                messages_written_prior =
                    messages_written_prior.saturating_add(report.messages_written);

                if cancel_requested(&cancel) {
                    cancelled = true;
                    export_partial = true;
                    export_error = Some("cancelled".into());
                    emit_log(stderr, &on_log, "cancelled after volume");
                    break;
                }

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
                // Writer TempGuard also deletes staging on cancel/error; this covers
                // any residual final path and same-dir temp.
                delete_incomplete_volume(&vol_path);
                export_partial = true;
                let is_cancel = matches!(e, pst_writer::WriterError::Cancelled)
                    || e.to_string().eq_ignore_ascii_case("cancelled");
                if is_cancel {
                    cancelled = true;
                    export_error = Some("cancelled".into());
                    emit_log(
                        stderr,
                        &on_log,
                        &format!("volume {volume_index} cancelled mid-write"),
                    );
                } else {
                    export_error = Some(format!("volume {volume_index} write failed: {e}"));
                }
                failed_volume_index = Some(volume_index);
                break;
            }
        }
    }

    // Empty keep-set (or loop never entered): still honour cancel so outcome
    // is not reported as a successful zero-message export when the user aborted.
    if !cancelled && cancel_requested(&cancel) {
        cancelled = true;
        export_partial = true;
        export_error = Some("cancelled".into());
        emit_log(
            stderr,
            &on_log,
            "cancelled after write phase (empty or idle)",
        );
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

    // Attachment stream failures: PST retained (not corrupt) but export is honesty-fail.
    // Always emit_log so the GUI Log panel sees the warning (not only export_error).
    if attach_failed_total > 0 {
        let msg = format!(
            "attachment write failures: {attach_failed_total} (export incomplete fidelity)"
        );
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
        if export_error.is_none() {
            export_error = Some(msg);
        }
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
    emit_log(stderr, &on_log, "stage=report");
    emit_stage_progress(
        &on_progress,
        "report",
        volume_index,
        messages_written_total,
        messages_written_total,
        0,
        winners_total,
    );
    let mut report_write_errors: Vec<String> = Vec::new();
    let mut decision_csv_out: Option<String> = None;
    if let Some(path) = &decision_csv {
        match DecisionCsvWriter::create(path) {
            Ok(mut wtr) => {
                if let Err(e) = resolved.write_decisions_csv(&mut wtr) {
                    let msg = format!("decision csv write failed: {e}");
                    tracing::warn!("{msg}");
                    emit_log(stderr, &on_log, &format!("warning: {msg}"));
                    report_write_errors.push(msg);
                } else if let Err(e) = wtr.flush() {
                    let msg = format!("decision csv flush failed: {e}");
                    tracing::warn!("{msg}");
                    emit_log(stderr, &on_log, &format!("warning: {msg}"));
                    report_write_errors.push(msg);
                } else {
                    decision_csv_out = Some(path.display().to_string());
                }
            }
            Err(e) => {
                let msg = format!("decision csv create failed: {e}");
                tracing::warn!("{msg}");
                emit_log(stderr, &on_log, &format!("warning: {msg}"));
                report_write_errors.push(msg);
            }
        }
    }

    let mut keep_set_json_out: Option<String> = None;
    if let Some(path) = &keep_set_json {
        match write_keep_set_json(path, &keep_set) {
            Ok(()) => keep_set_json_out = Some(path.display().to_string()),
            Err(e) => {
                let msg = format!("keepset.json write failed: {e}");
                tracing::warn!("{msg}");
                emit_log(stderr, &on_log, &format!("warning: {msg}"));
                report_write_errors.push(msg);
            }
        }
    }

    let volumes_csv_path = report_dir.join("volumes.csv");
    if let Err(e) = write_volumes_csv(&volumes_csv_path, &volumes) {
        let msg = format!("volumes.csv write failed: {e}");
        tracing::warn!("{msg}");
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
        report_write_errors.push(msg);
    }

    // export_messages.csv mandatory (always attempt; empty header when zero winners).
    let export_messages_path = report_dir.join("export_messages.csv");
    if messages_written_total > 0 || !export_rows.is_empty() {
        if let Err(e) = write_export_messages_csv(&export_messages_path, &export_rows) {
            let msg = format!("export_messages.csv write failed: {e}");
            tracing::warn!("{msg}");
            emit_log(stderr, &on_log, &format!("warning: {msg}"));
            report_write_errors.push(msg);
        }
    } else if let Err(e) = write_export_messages_csv(&export_messages_path, &[]) {
        let msg = format!("export_messages.csv write failed: {e}");
        tracing::warn!("{msg}");
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
        report_write_errors.push(msg);
    }

    // ── Phase 5: verify completed volumes ───────────────────────────────────
    emit_log(stderr, &on_log, "stage=verify");
    emit_stage_progress(
        &on_progress,
        "verify",
        volume_index,
        messages_written_total,
        messages_written_total,
        0,
        winners_total,
    );
    let mut verification = verify_volumes(&volumes, &export_rows, args.verify_hash);
    // Spec §3.3.1: partial export forces overall + verification honesty flags.
    if export_partial {
        verification.ok = false;
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    let exit_err = evaluate_exit_policy(&outcome.summary, &opts).err();
    let verify_err = if verification.ok {
        None
    } else if export_partial {
        // Partial already counted via export_partial / export_err; avoid double noise.
        None
    } else {
        Some("verification failed".to_string())
    };
    let export_err = export_error.clone();
    let report_err_msg = if report_write_errors.is_empty() {
        None
    } else {
        Some(format!(
            "mandatory report artifact write failed ({}): {}",
            report_write_errors.len(),
            report_write_errors.join("; ")
        ))
    };

    let ok = compute_export_ok(ExportOkInput {
        scan_ok: exit_err.is_none(),
        verify_ok: verify_err.is_none(),
        export_err_absent: export_err.is_none(),
        export_partial,
        messages_written_total,
        unique: keep_set.stats.unique,
        attach_failed_total,
        report_ok: report_write_errors.is_empty(),
    }) && !cancelled;

    let summary_error = if !ok {
        let (code, message) = if cancelled {
            ("cancelled", "cancelled".to_string())
        } else if let Some(msg) = export_err.as_ref() {
            ("export", msg.clone())
        } else if let Some(msg) = report_err_msg.as_ref() {
            ("report", msg.clone())
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

    // Fail-closed: if summary.json itself fails, force non-success exit even if
    // summary.ok was true (re-emit corrected summary is impossible; exit non-zero).
    let mut summary_write_failed: Option<String> = None;
    if let Err(e) = write_summary_json(&summary_path, &summary) {
        let msg = format!("summary.json write failed: {e}");
        tracing::warn!("{msg}");
        emit_log(stderr, &on_log, &format!("warning: {msg}"));
        summary_write_failed = Some(msg);
    }
    let ok = ok && summary_write_failed.is_none();
    let summary_error = match (ok, summary_write_failed, summary_error) {
        (false, Some(msg), None) => Some(SummaryError {
            code: "report".to_string(),
            message: msg,
        }),
        (_, _, existing) => existing,
    };

    let structured = UniquePstOutcome {
        ok,
        cancelled,
        report_dir: report_dir.clone(),
        summary_path: summary_path.clone(),
        out: out.clone(),
        messages_written_total,
        unique: keep_set.stats.unique,
        volume_count: volumes.len(),
        volumes: volumes.iter().map(UniqueVolumeDigest::from).collect(),
        error_message: summary_error
            .as_ref()
            .map(|e| e.message.clone())
            .or_else(|| {
                if cancelled {
                    Some("cancelled".into())
                } else if !ok {
                    Some("unique-pst failed".into())
                } else {
                    None
                }
            }),
    };

    emit_log(
        stderr,
        &on_log,
        &format!(
            "stage=done ok={ok} cancelled={cancelled} messages_written={messages_written_total}"
        ),
    );
    emit_stage_progress(
        &on_progress,
        "done",
        volume_index,
        messages_written_total,
        messages_written_total,
        0,
        winners_total,
    );

    // ── Phase 6: exit (CLI stdout) ──────────────────────────────────────────
    if args.json {
        // If summary.json failed after we already built a true-ok summary, patch
        // ok in the stdout JSON so operators never see a false success signal.
        let mut stdout_summary = summary;
        if !ok {
            stdout_summary.ok = false;
            if stdout_summary.error.is_none() {
                stdout_summary.error = summary_error.clone();
            }
        }
        println!("{}", serde_json::to_string_pretty(&stdout_summary)?);
        if !ok {
            let msg = summary_error
                .map(|e| e.message)
                .unwrap_or_else(|| "unique-pst failed".into());
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: crate::error::CliExit::Generic,
            });
        }
        return Ok(structured);
    }

    // Human summary only when stderr_progress is on (CLI). GUI leaves it false.
    if stderr {
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
        println!(
            "  partial:          {}  ok: {ok}  cancelled: {cancelled}",
            summary.export.partial
        );
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
    }

    // Library/GUI path: return structured outcome even when !ok (caller maps).
    // CLI `run_unique_pst` maps !ok to Err.
    Ok(structured)
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

    let subject = write_msg.subject.clone();
    Ok(PreparedWinner {
        source_path: entry.locus.source_path.clone(),
        folder_path: entry.locus.folder_path.clone(),
        nid: entry.locus.nid,
        message_id_norm: entry.message_id_norm.clone().unwrap_or_default(),
        edrm_mih: entry.edrm_mih_hex.clone().unwrap_or_default(),
        content_hash_hex,
        subject,
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

/// Remove stale multi-volume siblings (`out_vol002.pst` …) when overwriting.
///
/// Never deletes a path that equals or resolves to an input PST. If any planned
/// volume path collides with an input, refuses the export (source protection).
fn clear_stale_volume_siblings(out: &Path, inputs: &[PathBuf]) -> Result<()> {
    for i in 2u32..=MAX_VOLUME_SIBLING_INDEX {
        let p = volume_path_for(out, i);
        if path_collides_with_inputs(&p, inputs) {
            return Err(CliError::Usage(format!(
                "refusing multi-volume path equal to an input PST: {}",
                p.display()
            )));
        }
        if !p.exists() {
            // Contiguous siblings from prior runs; stop at first missing so we
            // do not scan all 998, but still guarded every existing candidate
            // we would touch. Collision check above also covers non-existing
            // planned names (e.g. input named unique_vol003.pst).
            // Continue checking non-existing planned paths for collisions only:
            // already done via path_collides; break existence scan.
            // Keep scanning for collisions against all planned indices even if
            // intermediate siblings are missing (inputs may sit at vol003+).
            continue;
        }
        if p.is_file() {
            fs::remove_file(&p)
                .map_err(|e| CliError::Msg(format!("remove stale volume {}: {e}", p.display())))?;
        }
    }
    Ok(())
}

/// True when `candidate` equals (resolved) any protected input PST path.
fn path_collides_with_inputs(candidate: &Path, inputs: &[PathBuf]) -> bool {
    inputs
        .iter()
        .any(|input| paths_equal_resolved(candidate, input) || paths_equal(candidate, input))
}

/// Inputs to the pure export-success gate (honesty).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExportOkInput {
    pub scan_ok: bool,
    pub verify_ok: bool,
    pub export_err_absent: bool,
    pub export_partial: bool,
    pub messages_written_total: u64,
    pub unique: u64,
    pub attach_failed_total: u64,
    pub report_ok: bool,
}

/// Pure gate for export success (honesty). Extracted for unit tests.
///
/// `scan_ok` / `verify_ok` / `export_err_absent` / `report_ok` are positive flags
/// (true = no failure in that dimension).
pub(crate) fn compute_export_ok(i: ExportOkInput) -> bool {
    i.scan_ok
        && i.verify_ok
        && i.export_err_absent
        && !i.export_partial
        && i.messages_written_total == i.unique
        && i.attach_failed_total == 0
        && i.report_ok
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
///
/// Checks primary `--out`, `--report-dir`, report artifacts, **and every**
/// generated multi-volume sibling path (`_vol002` … `_vol999`) against inputs
/// using resolved (parent-canonicalized) equality so junction aliases are caught.
fn guard_unique_pst_paths(
    inputs: &[PathBuf],
    out: &Path,
    report_dir: &Path,
    decision_csv: Option<&Path>,
    keep_set_json: Option<&Path>,
    integrity_csv: Option<&Path>,
) -> Result<()> {
    for input in inputs {
        if paths_equal_resolved(out, input) || paths_equal(out, input) {
            return Err(CliError::Usage(format!(
                "refusing --out equal to an input PST: {}",
                out.display()
            )));
        }
        if is_same_or_under_resolved(out, input) || is_same_or_under(out, input) {
            return Err(CliError::Usage(format!(
                "refusing --out nested under an input PST: out={} input={}",
                out.display(),
                input.display()
            )));
        }
        if paths_equal_resolved(report_dir, input) || paths_equal(report_dir, input) {
            return Err(CliError::Usage(format!(
                "refusing --report-dir equal to an input PST: {}",
                report_dir.display()
            )));
        }
        // Report-dir must not contain an input (recursive clear on overwrite).
        if is_same_or_under_resolved(input, report_dir) || is_same_or_under(input, report_dir) {
            return Err(CliError::Usage(format!(
                "refusing --report-dir that contains an input PST: report_dir={} input={}",
                report_dir.display(),
                input.display()
            )));
        }
        for art in [decision_csv, keep_set_json, integrity_csv]
            .into_iter()
            .flatten()
        {
            if paths_equal_resolved(art, input) || paths_equal(art, input) {
                return Err(CliError::Usage(format!(
                    "refusing report artifact that equals an input PST: {}",
                    art.display()
                )));
            }
        }
        // Every planned multi-volume path (vol 1 already checked as `out`).
        for vol_idx in 2u32..=MAX_VOLUME_SIBLING_INDEX {
            let vol = volume_path_for(out, vol_idx);
            if paths_equal_resolved(&vol, input) || paths_equal(&vol, input) {
                return Err(CliError::Usage(format!(
                    "refusing multi-volume path equal to an input PST: {}",
                    vol.display()
                )));
            }
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
                            // Collect *all* written message IDs/subjects so sample rows that
                            // land late in folder traversal cannot falsely fail (Codex r2 P2).
                            // Cost is O(messages_in_volume) property reads — acceptable for
                            // Phase 5 structural verify; multi-GB full-file rehash remains opt-in.
                            let mut written_mids: Vec<String> = Vec::new();
                            let mut written_subjects: Vec<String> = Vec::new();
                            for folder in &folders {
                                for &nid in &folder.message_nids {
                                    if let Ok(props) = pst.read_message_properties(nid) {
                                        if let Some(mid) = props.message_id {
                                            written_mids.push(normalize_mid_exact(&mid));
                                        }
                                        if let Some(sub) = props.subject {
                                            written_subjects.push(normalize_subject(&sub));
                                        }
                                    }
                                }
                            }
                            for r in vol_exports.iter().take(sample_n) {
                                match sample_row_matches(r, &written_mids, &written_subjects) {
                                    SampleMatch::Ok => {}
                                    SampleMatch::Fail(reason) => {
                                        sample_mid_ok = false;
                                        error = Some(reason);
                                        break;
                                    }
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

    // Empty volume list: structural verify of "nothing" is OK. Export partial /
    // count mismatch / zero-winner policy is decided by the orchestrator, not here.
    // (Previously failing empty lists made successful unique==0 exports always fail.)
    if volumes.is_empty() {
        all_ok = true;
    }

    VerificationReport {
        ok: all_ok,
        volumes: vol_results,
        rehash_ran: verify_hash,
    }
}

/// Exact normalized Message-ID for sample verification (no substring match).
fn normalize_mid_exact(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_ascii_lowercase()
}

/// Subject normalize: trim + case-insensitive compare basis.
fn normalize_subject(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

/// Result of matching one export_messages sample row against written identities.
#[derive(Debug, PartialEq, Eq)]
enum SampleMatch {
    Ok,
    Fail(String),
}

/// Exact MID equality when MID present; else exact normalized subject; fail if
/// neither identity is available on the export row.
fn sample_row_matches(
    row: &ExportMessageRow,
    written_mids: &[String],
    written_subjects: &[String],
) -> SampleMatch {
    sample_identity_matches(
        &row.message_id_norm,
        if row.subject.is_empty() {
            None
        } else {
            Some(row.subject.as_str())
        },
        written_mids,
        written_subjects,
    )
}

/// Subject-aware sample match: exact normalized MID only (no substring); for
/// empty MID compare normalized subjects; fail when neither identity exists.
fn sample_identity_matches(
    expected_mid: &str,
    expected_subject: Option<&str>,
    written_mids: &[String],
    written_subjects: &[String],
) -> SampleMatch {
    if !expected_mid.is_empty() {
        let want = normalize_mid_exact(expected_mid);
        if written_mids.iter().any(|m| m == &want) {
            return SampleMatch::Ok;
        }
        return SampleMatch::Fail(format!(
            "sample MID not found in volume (exact match): {expected_mid}"
        ));
    }
    if let Some(sub) = expected_subject {
        let want = normalize_subject(sub);
        if want.is_empty() {
            return SampleMatch::Fail("sample row has empty Message-ID and empty subject".into());
        }
        if written_subjects.iter().any(|s| s == &want) {
            return SampleMatch::Ok;
        }
        return SampleMatch::Fail(format!("sample subject not found in volume: {sub}"));
    }
    SampleMatch::Fail(
        "sample row has empty Message-ID and no subject identity for verification".into(),
    )
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

    #[test]
    fn guard_rejects_volume_3_sibling_equal_input() {
        // Input named like multi-volume sibling of --out unique.pst.
        let inputs = vec![PathBuf::from(r"C:\export\unique_vol003.pst")];
        let out = PathBuf::from(r"C:\export\unique.pst");
        let report = PathBuf::from(r"C:\export\unique_report");
        let err = guard_unique_pst_paths(&inputs, &out, &report, None, None, None).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("multi-volume") || msg.contains("vol003") || msg.contains("input"),
            "expected volume collision error, got: {msg}"
        );
    }

    fn ok_base() -> ExportOkInput {
        ExportOkInput {
            scan_ok: true,
            verify_ok: true,
            export_err_absent: true,
            export_partial: false,
            messages_written_total: 5,
            unique: 5,
            attach_failed_total: 0,
            report_ok: true,
        }
    }

    #[test]
    fn compute_export_ok_requires_zero_attach_failures() {
        assert!(compute_export_ok(ok_base()));
        let mut bad = ok_base();
        bad.attach_failed_total = 1;
        assert!(!compute_export_ok(bad));
    }

    #[test]
    fn compute_export_ok_requires_report_ok() {
        let mut bad = ok_base();
        bad.report_ok = false;
        assert!(!compute_export_ok(bad));
    }

    #[test]
    fn compute_export_ok_count_and_partial() {
        let mut partial = ok_base();
        partial.export_partial = true;
        assert!(!compute_export_ok(partial));
        let mut count = ok_base();
        count.messages_written_total = 4;
        assert!(!compute_export_ok(count));
    }

    #[test]
    fn sample_mid_exact_not_substring() {
        let written = vec!["abc@example.com".to_string()];
        // Substring-only match must fail.
        assert!(matches!(
            sample_identity_matches("bc@example", None, &written, &[]),
            SampleMatch::Fail(_)
        ));
        // Exact normalized match (angle brackets stripped).
        assert_eq!(
            sample_identity_matches("<ABC@example.com>", None, &written, &[]),
            SampleMatch::Ok
        );
    }

    #[test]
    fn sample_empty_mid_uses_subject() {
        let subjects = vec!["hello world".to_string()];
        assert_eq!(
            sample_identity_matches("", Some("Hello World"), &[], &subjects),
            SampleMatch::Ok
        );
        assert!(matches!(
            sample_identity_matches("", Some("other"), &[], &subjects),
            SampleMatch::Fail(_)
        ));
        assert!(matches!(
            sample_identity_matches("", None, &[], &subjects),
            SampleMatch::Fail(_)
        ));
    }

    #[test]
    fn normalize_mid_exact_strips_brackets_lowercase() {
        assert_eq!(normalize_mid_exact(" <Id@X.com> "), "id@x.com");
    }

    /// Sample matching must succeed against a late identity in a large set
    /// (regression for former 64-identity cap false-negative).
    #[test]
    fn sample_identity_matches_late_entry_beyond_64() {
        let mut mids: Vec<String> = (0..100).map(|i| format!("id{i}@example.com")).collect();
        mids.push("late@example.com".into());
        let mut subjects: Vec<String> = (0..100).map(|i| format!("subject {i}")).collect();
        subjects.push("late subject".into());

        assert_eq!(
            sample_identity_matches("late@example.com", None, &mids, &subjects),
            SampleMatch::Ok
        );
        assert_eq!(
            sample_identity_matches("", Some("late subject"), &mids, &subjects),
            SampleMatch::Ok
        );
    }

    fn require_sample_pst() -> PathBuf {
        let sample =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aspose_outlook.pst");
        assert!(
            sample.is_file(),
            "required fixture missing (fail-closed): {}",
            sample.display()
        );
        sample
    }

    #[test]
    fn log_and_progress_callbacks_fire() {
        let sample = require_sample_pst();
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let report = dir.path().join("report");
        let also_eml = dir.path().join("also_eml_unused");

        let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let progress: Arc<Mutex<Vec<UniquePstProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let logs_c = Arc::clone(&logs);
        let progress_c = Arc::clone(&progress);

        let args = UniquePstCliArgs {
            paths: vec![sample],
            out,
            report_dir: Some(report),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path_contains: vec![],
            decision_csv: None,
            keep_set_json: None,
            folder_layout: FolderLayoutArg::Preserve,
            max_volume_bytes: None,
            overwrite: false,
            verify_hash: false,
            // Forces a production warning through on_log (D-0071 residual).
            also_eml: Some(also_eml),
            no_tier2: false,
            no_attachments: true,
            json: false,
            mode: ScanMode::BestEffort,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
        };
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: None,
                stderr_progress: false,
                on_progress: Some(Box::new(move |p| {
                    progress_c.lock().unwrap_or_else(|e| e.into_inner()).push(p);
                })),
                on_log: Some(Box::new(move |line| {
                    logs_c.lock().unwrap_or_else(|e| e.into_inner()).push(line);
                })),
            },
        )
        .expect("run");
        assert!(outcome.ok, "outcome: {:?}", outcome.error_message);
        let log_lines = logs.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            log_lines.iter().any(|l| l.contains("stage=scan")),
            "expected stage=scan in logs: {log_lines:?}"
        );
        assert!(
            log_lines
                .iter()
                .any(|l| l.contains("warning") && l.contains("also-eml")),
            "expected also-eml warning via on_log: {log_lines:?}"
        );
        let ticks = progress.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            ticks
                .iter()
                .any(|t| t.stage == "scan" || t.stage == "write" || t.stage == "done"),
            "expected progress stages: {:?}",
            ticks.iter().map(|t| t.stage.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn cancel_before_write_returns_cancelled_outcome() {
        let sample = require_sample_pst();
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let report = dir.path().join("report");
        let cancel = Arc::new(AtomicBool::new(true)); // cancel immediately

        let args = UniquePstCliArgs {
            paths: vec![sample],
            out: out.clone(),
            report_dir: Some(report.clone()),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path_contains: vec![],
            decision_csv: None,
            keep_set_json: None,
            folder_layout: FolderLayoutArg::Preserve,
            max_volume_bytes: None,
            overwrite: false,
            verify_hash: false,
            also_eml: None,
            no_tier2: false,
            no_attachments: true,
            json: false,
            mode: ScanMode::BestEffort,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
        };
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: Some(cancel),
                stderr_progress: false,
                on_progress: None,
                on_log: None,
            },
        )
        .expect("structured outcome");
        // Hard assert: cancel-from-start must always yield cancelled outcome.
        assert!(outcome.cancelled, "expected cancelled=true");
        assert!(!outcome.ok);
        assert_eq!(outcome.error_message.as_deref(), Some("cancelled"));
        assert!(outcome.volumes.is_empty());
        // Report-dir was prepared: minimal summary.json must exist for Open report honesty.
        assert!(
            outcome.summary_path.is_file(),
            "cancelled path must write summary.json at {}",
            outcome.summary_path.display()
        );
        let body = fs::read_to_string(&outcome.summary_path).expect("read summary");
        assert!(
            body.contains("\"ok\": false") || body.contains("\"ok\":false"),
            "summary must report ok=false: {body}"
        );
        assert!(
            body.contains("cancelled"),
            "summary must mention cancelled: {body}"
        );
        // Incomplete final path must not exist; no orphan temp sibling.
        assert!(!out.exists());
        let tmp = temp_sibling_path(&out);
        assert!(!tmp.exists(), "orphan temp left: {}", tmp.display());
    }

    /// Cancel as soon as the write stage is entered (before/around first volume).
    ///
    /// Hard-asserts `cancelled == true`. True mid-message cancel while the
    /// writer is inside a large volume is proven by
    /// `pst-writer::tests::cancel_mid_write_no_final_pst_temp_cleaned` —
    /// tiny fixtures can finish the write before the flag is observed mid-loop.
    #[test]
    fn cancel_at_write_stage_returns_cancelled_no_orphan_temp() {
        let sample = require_sample_pst();
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let report = dir.path().join("report");
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_c = Arc::clone(&cancel);

        let args = UniquePstCliArgs {
            paths: vec![sample],
            out: out.clone(),
            report_dir: Some(report),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path_contains: vec![],
            decision_csv: None,
            keep_set_json: None,
            folder_layout: FolderLayoutArg::Preserve,
            max_volume_bytes: None,
            overwrite: false,
            verify_hash: false,
            also_eml: None,
            no_tier2: false,
            no_attachments: true,
            json: false,
            mode: ScanMode::BestEffort,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
        };
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: Some(cancel),
                stderr_progress: false,
                on_progress: Some(Box::new(move |p| {
                    // Trip cancel as soon as write stage begins (pre-volume or first tick).
                    if p.stage == "write" {
                        cancel_c.store(true, Ordering::SeqCst);
                    }
                })),
                on_log: None,
            },
        )
        .expect("structured outcome");
        let tmp = temp_sibling_path(&out);
        assert!(
            !tmp.exists(),
            "incomplete temp must not remain as permanent orphan: {}",
            tmp.display()
        );
        // First write-stage progress sets the flag; prepare/before-volume checkpoints
        // and empty-keep-set re-check must honour it.
        assert!(
            outcome.cancelled,
            "cancel at write stage must yield cancelled=true (got ok={} err={:?})",
            outcome.ok, outcome.error_message
        );
        assert!(!outcome.ok);
    }

    /// Trip cancel on first `stage=scan` progress (before/around `run_scan`).
    /// With cooperative scan cancel, the pipeline still returns cancelled + report.
    #[test]
    fn cancel_on_scan_stage_returns_cancelled() {
        let sample = require_sample_pst();
        let dir = tempfile::tempdir().expect("tmp");
        let out = dir.path().join("unique.pst");
        let report = dir.path().join("report");
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_c = Arc::clone(&cancel);

        let args = UniquePstCliArgs {
            paths: vec![sample],
            out: out.clone(),
            report_dir: Some(report),
            policy: KeepPolicy::FirstSeen,
            family_policy: FamilyPolicy::KeepAttachmentsWithParent,
            prefer_path_contains: vec![],
            decision_csv: None,
            keep_set_json: None,
            folder_layout: FolderLayoutArg::Preserve,
            max_volume_bytes: None,
            overwrite: false,
            verify_hash: false,
            also_eml: None,
            no_tier2: false,
            no_attachments: true,
            json: false,
            mode: ScanMode::BestEffort,
            max_skip_rate: 0.05,
            max_crc_skip_rate: 0.01,
            max_failed_file_rate: 0.0,
            allow_failed_files: false,
            integrity_csv: None,
            skip_limit: 10_000,
        };
        let outcome = run_unique_pst_with_options(
            args,
            UniquePstRunOptions {
                cancel: Some(cancel),
                stderr_progress: false,
                on_progress: Some(Box::new(move |p| {
                    if p.stage == "scan" {
                        cancel_c.store(true, Ordering::SeqCst);
                    }
                })),
                on_log: None,
            },
        )
        .expect("structured outcome");
        assert!(
            outcome.cancelled,
            "cancel on scan stage must yield cancelled=true (got ok={} err={:?})",
            outcome.ok, outcome.error_message
        );
        assert!(!outcome.ok);
        assert!(!out.exists());
        let tmp = temp_sibling_path(&out);
        assert!(!tmp.exists(), "orphan temp left: {}", tmp.display());
        assert!(
            outcome.summary_path.is_file(),
            "cancelled path must write summary.json"
        );
    }
}
