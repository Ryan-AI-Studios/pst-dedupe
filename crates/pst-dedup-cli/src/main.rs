//! `pst-dedup` — CLI for PST tools and headless matter automation (track 0045).
//!
//! Designed for humans and agents: stable subcommands, `--json` stdout isolation,
//! documented exit codes, and SIGINT → graceful cancel.
//!
//! Business logic lives in the `pst_dedup_cli` library; this binary is the clap surface.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dedup_engine::format_bytes;

use dedup_engine::integrity::{IntegrityThresholds, ScanMode};
use dedup_engine::keepset::{FamilyPolicy, KeepPolicy};

use pst_dedup_cli::error::{CliError, CliExit, Result};
use pst_dedup_cli::json_io::emit_error;
use pst_dedup_cli::scan::{
    collect_dups, evaluate_exit_policy, resolve_pst_paths, run_scan, write_report, DupRow,
    ScanOptions, ScanSummary,
};
use pst_dedup_cli::{
    convenience, inspect, job_cmd, keep_set_cmd, matter_cmd, platform_cmd, production_profile_cmd,
    profile_cmd, service_cmd, unique_eml_cmd, unique_pst_cmd, workflow_cmd,
};

#[derive(Debug, Parser)]
#[command(
    name = "pst-dedup",
    version,
    about = "PST dedup + headless matter automation CLI",
    long_about = "Read-only PST tools and headless matter job/profile/workflow runs.\n\n\
PST examples:\n  \
  pst-dedup scan archive.pst --json\n  \
  pst-dedup inspect archive.pst --top 20\n\n\
Matter automation:\n  \
  pst-dedup matter create --path C:\\Matters\\M1 --name case\n  \
  pst-dedup job run --path C:\\Matters\\M1 --kind classify --json\n  \
  pst-dedup workflow run --path C:\\Matters\\M1 --workflow builtin:reduce_only_chain --json\n\n\
Exit codes: 0 ok · 2 usage · 3 busy · 4 job failed/cancelled · 5 matter IO · 1 other.\n\
With --json, only the final envelope is written to stdout; logs/progress go to stderr."
)]
struct Cli {
    /// Increase log verbosity (-v, -vv). Logs always go to stderr.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan PST file(s), run tiered dedup, print summary.
    Scan {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long)]
        no_tier2: bool,
        #[arg(long)]
        no_attachments: bool,
        #[arg(long)]
        csv: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        dups: bool,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Recoverability mode: `best-effort` (default) or `strict`.
        #[arg(long, default_value = "best-effort", value_parser = parse_scan_mode)]
        mode: ScanMode,
        /// Max skip rate before preflight recommends re-export (default 0.05).
        #[arg(long, default_value_t = 0.05, value_parser = parse_rate_threshold)]
        max_skip_rate: f64,
        /// Max CRC skip rate before re-export recommended (default 0.01).
        #[arg(long, default_value_t = 0.01, value_parser = parse_rate_threshold)]
        max_crc_skip_rate: f64,
        /// Max failed-file rate (default 0.0 = any failed file exceeds).
        #[arg(long, default_value_t = 0.0, value_parser = parse_rate_threshold)]
        max_failed_file_rate: f64,
        /// Allow exit 0 when some inputs failed but recoverable messages exist.
        #[arg(long)]
        allow_failed_files: bool,
        /// Integrity skip/degraded ledger CSV (default: sidecar `*.integrity.csv` when `--csv` set).
        #[arg(long)]
        integrity_csv: Option<PathBuf>,
        /// Cap on JSON skip sample rows (default 10000). Full ledger = integrity CSV.
        #[arg(long, default_value_t = 10_000)]
        skip_limit: usize,
    },

    /// Inspect PST structure: encryption, folder tree, message counts.
    Inspect {
        path: PathBuf,
        #[arg(long, default_value_t = 30)]
        top: usize,
        #[arg(long)]
        json: bool,
    },

    /// Scan and list only duplicate messages.
    Dups {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long)]
        no_tier2: bool,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
        /// Recoverability mode: `best-effort` (default) or `strict`.
        #[arg(long, default_value = "best-effort", value_parser = parse_scan_mode)]
        mode: ScanMode,
        #[arg(long, default_value_t = 0.05, value_parser = parse_rate_threshold)]
        max_skip_rate: f64,
        #[arg(long, default_value_t = 0.01, value_parser = parse_rate_threshold)]
        max_crc_skip_rate: f64,
        #[arg(long, default_value_t = 0.0, value_parser = parse_rate_threshold)]
        max_failed_file_rate: f64,
        #[arg(long)]
        allow_failed_files: bool,
        #[arg(long)]
        integrity_csv: Option<PathBuf>,
        #[arg(long, default_value_t = 10_000)]
        skip_limit: usize,
    },

    /// Build export keep-set (`keep_set_v1`): policy resolve, decision CSV, winners JSON.
    ///
    /// Phase 1 scan → Phase 2 fidelity/policy resolve → optional materialize+promote →
    /// Phase 3 decision stream. Source PSTs are read-only.
    #[command(name = "keep-set")]
    KeepSet {
        /// PST path(s) as positional arguments (same style as `scan`).
        #[arg(required = false)]
        paths: Vec<PathBuf>,
        /// PST path(s) via repeated `--input` (spec-style; merge with positionals).
        #[arg(long = "input", action = clap::ArgAction::Append)]
        input: Vec<PathBuf>,
        /// Winner policy after fidelity: first_seen (default), keep_largest, prefer_path.
        #[arg(long, default_value = "first_seen", value_parser = parse_keep_policy)]
        policy: KeepPolicy,
        /// Parent+attach family: keep_attachments_with_parent (default) or parents_only.
        #[arg(long, default_value = "keep_attachments_with_parent", value_parser = parse_family_policy)]
        family_policy: FamilyPolicy,
        /// Path/folder substring preferred under prefer_path (repeatable).
        #[arg(long = "prefer-path-contains")]
        prefer_path_contains: Vec<String>,
        /// Streaming decision CSV (emitted only after resolve; every recoverable row).
        #[arg(long)]
        decision_csv: Option<PathBuf>,
        /// Keep-set JSON (winners + stats; no bodies).
        #[arg(long)]
        keep_set_json: Option<PathBuf>,
        /// Materialize winners (full extract); hard fail promotes next peer.
        #[arg(long)]
        materialize: bool,
        #[arg(long)]
        no_tier2: bool,
        #[arg(long)]
        no_attachments: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "best-effort", value_parser = parse_scan_mode)]
        mode: ScanMode,
        #[arg(long, default_value_t = 0.05, value_parser = parse_rate_threshold)]
        max_skip_rate: f64,
        #[arg(long, default_value_t = 0.01, value_parser = parse_rate_threshold)]
        max_crc_skip_rate: f64,
        #[arg(long, default_value_t = 0.0, value_parser = parse_rate_threshold)]
        max_failed_file_rate: f64,
        #[arg(long)]
        allow_failed_files: bool,
        #[arg(long)]
        integrity_csv: Option<PathBuf>,
        #[arg(long, default_value_t = 10_000)]
        skip_limit: usize,
    },

    /// Export unique messages as a volume-batched EML pack (`eml_pack_v1`).
    ///
    /// Driven only by keep-set (no re-dedupe): integrity → resolve → materialize+promote →
    /// write one `.eml` per exportable winner under `VOL001`… Source PSTs are read-only.
    #[command(name = "unique-eml")]
    UniqueEml {
        /// PST path(s) as positional arguments (same style as `scan` / `keep-set`).
        #[arg(required = false)]
        paths: Vec<PathBuf>,
        /// PST path(s) via repeated `--input` (merge with positionals).
        #[arg(long = "input", action = clap::ArgAction::Append)]
        input: Vec<PathBuf>,
        /// Pack root directory (required). Created if missing; refuse non-empty unless `--overwrite`.
        #[arg(long)]
        out: PathBuf,
        /// Winner policy after fidelity: first_seen (default), keep_largest, prefer_path.
        #[arg(long, default_value = "first_seen", value_parser = parse_keep_policy)]
        policy: KeepPolicy,
        /// Parent+attach family: keep_attachments_with_parent (default) or parents_only.
        #[arg(long, default_value = "keep_attachments_with_parent", value_parser = parse_family_policy)]
        family_policy: FamilyPolicy,
        /// Path/folder substring preferred under prefer_path (repeatable).
        #[arg(long = "prefer-path-contains")]
        prefer_path_contains: Vec<String>,
        /// Streaming decision CSV (post-promotion roles).
        #[arg(long)]
        decision_csv: Option<PathBuf>,
        /// Keep-set JSON (winners + stats; no bodies).
        #[arg(long)]
        keep_set_json: Option<PathBuf>,
        /// Pack manifest JSON (default: `{out}/manifest.json`).
        #[arg(long)]
        manifest_json: Option<PathBuf>,
        /// Allow writing into a non-empty `--out` (clears contents first).
        #[arg(long)]
        overwrite: bool,
        /// Max `.eml` files per volume directory (default 10000; clamped [1000, 50000]).
        #[arg(long, default_value_t = 10_000)]
        files_per_volume: u32,
        /// Volume directory prefix (default `VOL` → `VOL001`, …).
        #[arg(long, default_value = "VOL")]
        volume_prefix: String,
        #[arg(long)]
        no_tier2: bool,
        #[arg(long)]
        no_attachments: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "best-effort", value_parser = parse_scan_mode)]
        mode: ScanMode,
        #[arg(long, default_value_t = 0.05, value_parser = parse_rate_threshold)]
        max_skip_rate: f64,
        #[arg(long, default_value_t = 0.01, value_parser = parse_rate_threshold)]
        max_crc_skip_rate: f64,
        #[arg(long, default_value_t = 0.0, value_parser = parse_rate_threshold)]
        max_failed_file_rate: f64,
        #[arg(long)]
        allow_failed_files: bool,
        #[arg(long)]
        integrity_csv: Option<PathBuf>,
        #[arg(long, default_value_t = 10_000)]
        skip_limit: usize,
    },

    /// Export unique messages as streaming PST volume(s) + report pack (`unique_export_report_v1`).
    ///
    /// Driven only by keep-set (no re-dedupe): integrity → resolve → materialize+promote →
    /// `write_unicode_pst_streaming` → report pack + verification. Source PSTs are read-only.
    #[command(name = "unique-pst")]
    UniquePst(unique_pst_cmd::UniquePstClapArgs),

    /// Matter lifecycle.
    Matter {
        #[command(subcommand)]
        cmd: MatterCmd,
    },

    /// Generic job control.
    Job {
        #[command(subcommand)]
        cmd: JobCmd,
    },

    /// Processing profiles (0043).
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },

    /// Workflows (0044).
    Workflow {
        #[command(subcommand)]
        cmd: WorkflowCmd,
    },

    /// Ingest a source package into a matter.
    Ingest {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        json: bool,
        /// Accepted for compatibility; P0 always waits.
        #[arg(long, default_value_t = true, hide = true)]
        wait: bool,
    },

    /// Export matter report CSV pack (0039).
    Report {
        #[command(subcommand)]
        cmd: ReportCmd,
    },

    /// Run production QC (0041).
    Qc {
        #[command(subcommand)]
        cmd: QcCmd,
    },

    /// Run production export (0040 / 0060 profiles).
    Produce {
        #[command(subcommand)]
        cmd: ProduceCmd,
    },

    /// Production profiles (0060): list/show/upsert/delete packaging templates.
    ///
    /// Templates are technical packaging presets — not legal compliance advice.
    #[command(name = "production-profile")]
    ProductionProfile {
        #[command(subcommand)]
        cmd: ProductionProfileCmd,
    },

    /// Run gap analysis (0042).
    Gap {
        #[command(subcommand)]
        cmd: GapCmd,
    },

    /// Multi-user matter service (0058): serve / bootstrap / users.
    Service {
        #[command(subcommand)]
        cmd: service_cmd::ServiceCmd,
    },

    /// Platform control plane (0059): tenants / IdP / matter registration.
    Platform {
        #[command(subcommand)]
        cmd: platform_cmd::PlatformCmd,
    },
}

#[derive(Debug, Subcommand)]
enum MatterCmd {
    /// Create a new matter at --path.
    Create {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        name: String,
        /// Encrypt at rest (requires env PST_DEDUPE_MATTER_PASSPHRASE).
        #[arg(long)]
        encrypt: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show matter metadata (open-for-read).
    Info {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Re-wrap DEK under a new passphrase (encrypted matters only).
    ///
    /// Old: env `PST_DEDUPE_MATTER_PASSPHRASE`. New: env `PST_DEDUPE_MATTER_NEW_PASSPHRASE`.
    ChangePassphrase {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Show matter storage backend config (non-secret; schema v39 / track 0061).
    Storage {
        #[command(subcommand)]
        cmd: MatterStorageCmd,
    },
}

#[derive(Debug, Subcommand)]
enum MatterStorageCmd {
    /// Show storage backend + job backend kind.
    Show {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Set non-secret storage backend config (credentials stay in env/IAM).
    ///
    /// Config can always be stored; open activates S3 only with `--features cloud-s3` (fail closed).
    Set {
        #[arg(long)]
        path: PathBuf,
        /// Backend kind: local | s3 | azure
        #[arg(long)]
        kind: String,
        #[arg(long)]
        bucket: Option<String>,
        #[arg(long)]
        region: Option<String>,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        tenant_id: Option<String>,
        #[arg(long)]
        matter_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum JobCmd {
    /// Start a job and wait for terminal state.
    Run {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        kind: String,
        /// Inline JSON object or @file path.
        #[arg(long)]
        params_json: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = true, hide = true)]
        wait: bool,
    },
    /// Resume a paused/failed job and wait.
    Resume {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        job_id: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = true, hide = true)]
        wait: bool,
    },
    /// Mark a non-terminal job cancelled in the matter DB.
    Cancel {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        job_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Show one job's status.
    Status {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        job_id: String,
        #[arg(long)]
        json: bool,
    },
    /// List jobs (optionally children of --parent).
    List {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ProfileCmd {
    List {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Import {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = true, hide = true)]
        wait: bool,
    },
}

#[derive(Debug, Subcommand)]
enum WorkflowCmd {
    List {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Import {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        workflow: String,
        #[arg(long)]
        params_json: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = true, hide = true)]
        wait: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ReportCmd {
    /// Export matter report CSV pack to --out (must not already exist).
    Export {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum QcCmd {
    Run {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        params_json: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = true, hide = true)]
        wait: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ProduceCmd {
    /// Run production export.
    ///
    /// Bates start is job-time only and **required** (`--bates-start` or params
    /// `bates_start`). Production profile selects load-file/layout/QC pack
    /// (`--profile` or params `production_profile`).
    Run {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        params_json: Option<String>,
        /// Production profile slug (e.g. us_concordance_native_text_v1).
        #[arg(long)]
        profile: Option<String>,
        /// Job-time Bates start sequence (required; never stored in a profile).
        #[arg(long = "bates-start")]
        bates_start: Option<u64>,
        /// Override Bates prefix (job > profile).
        #[arg(long = "bates-prefix")]
        bates_prefix: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = true, hide = true)]
        wait: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ProductionProfileCmd {
    /// List built-in + matter-local production profiles.
    List {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Show one production profile (slug or id).
    Show {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        slug: String,
        #[arg(long)]
        json: bool,
    },
    /// Upsert a matter-local production profile from a JSON file.
    Upsert {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Delete a matter-local production profile (built-ins cannot be deleted).
    Delete {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        slug: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum GapCmd {
    Run {
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        params_json: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = true, hide = true)]
        wait: bool,
    },
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .with_writer(std::io::stderr)
        .try_init();
}

fn command_wants_json(cmd: &Commands) -> bool {
    match cmd {
        Commands::Scan { json, .. }
        | Commands::Inspect { json, .. }
        | Commands::Dups { json, .. }
        | Commands::KeepSet { json, .. }
        | Commands::UniqueEml { json, .. }
        | Commands::Ingest { json, .. } => *json,
        Commands::UniquePst(a) => a.json,
        Commands::Matter { cmd } => match cmd {
            MatterCmd::Create { json, .. }
            | MatterCmd::Info { json, .. }
            | MatterCmd::ChangePassphrase { json, .. } => *json,
            MatterCmd::Storage { cmd } => match cmd {
                MatterStorageCmd::Show { json, .. } | MatterStorageCmd::Set { json, .. } => *json,
            },
        },
        Commands::Job { cmd } => match cmd {
            JobCmd::Run { json, .. }
            | JobCmd::Resume { json, .. }
            | JobCmd::Cancel { json, .. }
            | JobCmd::Status { json, .. }
            | JobCmd::List { json, .. } => *json,
        },
        Commands::Profile { cmd } => match cmd {
            ProfileCmd::List { json, .. }
            | ProfileCmd::Import { json, .. }
            | ProfileCmd::Run { json, .. } => *json,
        },
        Commands::Workflow { cmd } => match cmd {
            WorkflowCmd::List { json, .. }
            | WorkflowCmd::Import { json, .. }
            | WorkflowCmd::Run { json, .. } => *json,
        },
        Commands::Report { cmd } => match cmd {
            ReportCmd::Export { json, .. } => *json,
        },
        Commands::Qc { cmd } => match cmd {
            QcCmd::Run { json, .. } => *json,
        },
        Commands::Produce { cmd } => match cmd {
            ProduceCmd::Run { json, .. } => *json,
        },
        Commands::ProductionProfile { cmd } => match cmd {
            ProductionProfileCmd::List { json, .. }
            | ProductionProfileCmd::Show { json, .. }
            | ProductionProfileCmd::Upsert { json, .. }
            | ProductionProfileCmd::Delete { json, .. } => *json,
        },
        Commands::Gap { cmd } => match cmd {
            GapCmd::Run { json, .. } => *json,
        },
        Commands::Service { cmd } => match cmd {
            service_cmd::ServiceCmd::Serve { json, .. }
            | service_cmd::ServiceCmd::BootstrapAdmin { json, .. } => *json,
            service_cmd::ServiceCmd::User { cmd } => match cmd {
                service_cmd::ServiceUserCmd::Add { json, .. }
                | service_cmd::ServiceUserCmd::List { json, .. }
                | service_cmd::ServiceUserCmd::Disable { json, .. } => *json,
            },
        },
        Commands::Platform { cmd } => match cmd {
            platform_cmd::PlatformCmd::Init { json, .. } => *json,
            platform_cmd::PlatformCmd::Tenant { cmd } => match cmd {
                platform_cmd::PlatformTenantCmd::Create { json, .. }
                | platform_cmd::PlatformTenantCmd::List { json, .. } => *json,
            },
            platform_cmd::PlatformCmd::Idp { cmd } => match cmd {
                platform_cmd::PlatformIdpCmd::Set { json, .. } => *json,
            },
            platform_cmd::PlatformCmd::Matter { cmd } => match cmd {
                platform_cmd::PlatformMatterCmd::Register { json, .. }
                | platform_cmd::PlatformMatterCmd::List { json, .. } => *json,
            },
        },
    }
}

fn main() -> ExitCode {
    // Windows default stack (~1 MiB) overflows in debug when clap builds help /
    // parses this large Commands tree (matter + unique-eml + unique-pst, …).
    const MAIN_STACK: usize = 8 * 1024 * 1024;
    let worker = std::thread::Builder::new()
        .name("pst-dedup-main".into())
        .stack_size(MAIN_STACK)
        .spawn(|| {
            let cli = Cli::parse();
            init_tracing(cli.verbose);
            let json = command_wants_json(&cli.command);

            match run(cli) {
                Ok(()) => CliExit::Success.into(),
                Err(e) => {
                    // JobFailed / AlreadyEmitted already wrote the operator payload.
                    if !e.already_emitted() {
                        emit_error(json, &e);
                    }
                    e.exit_code().into()
                }
            }
        });
    match worker {
        Ok(handle) => match handle.join() {
            Ok(code) => code,
            Err(_) => ExitCode::from(CliExit::Generic as u8),
        },
        Err(e) => {
            eprintln!("failed to spawn main thread: {e}");
            ExitCode::from(CliExit::Generic as u8)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Scan {
            paths,
            no_tier2,
            no_attachments,
            csv,
            json,
            dups,
            limit,
            mode,
            max_skip_rate,
            max_crc_skip_rate,
            max_failed_file_rate,
            allow_failed_files,
            integrity_csv,
            skip_limit,
        } => cmd_scan(ScanCliArgs {
            paths,
            no_tier2,
            no_attachments,
            csv,
            json,
            list_dups: dups,
            limit,
            mode,
            max_skip_rate,
            max_crc_skip_rate,
            max_failed_file_rate,
            allow_failed_files,
            integrity_csv,
            skip_limit,
        }),
        Commands::Inspect { path, top, json } => cmd_inspect(path, top, json),
        Commands::Dups {
            paths,
            no_tier2,
            limit,
            json,
            mode,
            max_skip_rate,
            max_crc_skip_rate,
            max_failed_file_rate,
            allow_failed_files,
            integrity_csv,
            skip_limit,
        } => cmd_dups(ScanCliArgs {
            paths,
            no_tier2,
            no_attachments: false,
            csv: None,
            json,
            list_dups: true,
            limit,
            mode,
            max_skip_rate,
            max_crc_skip_rate,
            max_failed_file_rate,
            allow_failed_files,
            integrity_csv,
            skip_limit,
        }),
        Commands::KeepSet {
            paths,
            input,
            policy,
            family_policy,
            prefer_path_contains,
            decision_csv,
            keep_set_json,
            materialize,
            no_tier2,
            no_attachments,
            json,
            mode,
            max_skip_rate,
            max_crc_skip_rate,
            max_failed_file_rate,
            allow_failed_files,
            integrity_csv,
            skip_limit,
        } => {
            let mut all = paths;
            all.extend(input);
            if all.is_empty() {
                return Err(CliError::Usage(
                    "keep-set requires at least one PST path (positional or --input)".into(),
                ));
            }
            keep_set_cmd::run_keep_set(keep_set_cmd::KeepSetCliArgs {
                paths: all,
                policy,
                family_policy,
                prefer_path_contains,
                decision_csv,
                keep_set_json,
                materialize,
                no_tier2,
                no_attachments,
                json,
                mode,
                max_skip_rate,
                max_crc_skip_rate,
                max_failed_file_rate,
                allow_failed_files,
                integrity_csv,
                skip_limit,
            })
        }
        Commands::UniqueEml {
            paths,
            input,
            out,
            policy,
            family_policy,
            prefer_path_contains,
            decision_csv,
            keep_set_json,
            manifest_json,
            overwrite,
            files_per_volume,
            volume_prefix,
            no_tier2,
            no_attachments,
            json,
            mode,
            max_skip_rate,
            max_crc_skip_rate,
            max_failed_file_rate,
            allow_failed_files,
            integrity_csv,
            skip_limit,
        } => {
            let mut all = paths;
            all.extend(input);
            if all.is_empty() {
                return Err(CliError::Usage(
                    "unique-eml requires at least one PST path (positional or --input)".into(),
                ));
            }
            unique_eml_cmd::run_unique_eml(unique_eml_cmd::UniqueEmlCliArgs {
                paths: all,
                out,
                policy,
                family_policy,
                prefer_path_contains,
                decision_csv,
                keep_set_json,
                manifest_json,
                overwrite,
                files_per_volume,
                volume_prefix,
                no_tier2,
                no_attachments,
                json,
                mode,
                max_skip_rate,
                max_crc_skip_rate,
                max_failed_file_rate,
                allow_failed_files,
                integrity_csv,
                skip_limit,
            })
        }
        Commands::UniquePst(clap_args) => {
            let args = clap_args.into_cli_args()?;
            unique_pst_cmd::run_unique_pst(args)
        }
        Commands::Matter { cmd } => match cmd {
            MatterCmd::Create {
                path,
                name,
                encrypt,
                json,
            } => matter_cmd::matter_create(&path, &name, encrypt, json),
            MatterCmd::Info { path, json } => matter_cmd::matter_info(&path, json),
            MatterCmd::ChangePassphrase { path, json } => {
                matter_cmd::matter_change_passphrase(&path, json)
            }
            MatterCmd::Storage { cmd } => match cmd {
                MatterStorageCmd::Show { path, json } => {
                    matter_cmd::matter_storage_show(&path, json)
                }
                MatterStorageCmd::Set {
                    path,
                    kind,
                    bucket,
                    region,
                    endpoint,
                    prefix,
                    tenant_id,
                    matter_id,
                    json,
                } => matter_cmd::matter_storage_set(
                    &path,
                    &kind,
                    bucket.as_deref(),
                    region.as_deref(),
                    endpoint.as_deref(),
                    prefix.as_deref(),
                    tenant_id.as_deref(),
                    matter_id.as_deref(),
                    json,
                ),
            },
        },
        Commands::Job { cmd } => match cmd {
            JobCmd::Run {
                path,
                kind,
                params_json,
                json,
                wait: _,
            } => job_cmd::job_run(&path, &kind, params_json.as_deref(), json),
            JobCmd::Resume {
                path,
                job_id,
                json,
                wait: _,
            } => job_cmd::job_resume(&path, &job_id, json),
            JobCmd::Cancel { path, job_id, json } => job_cmd::job_cancel(&path, &job_id, json),
            JobCmd::Status { path, job_id, json } => job_cmd::job_status(&path, &job_id, json),
            JobCmd::List {
                path,
                parent,
                limit,
                json,
            } => job_cmd::job_list(&path, parent.as_deref(), limit, json),
        },
        Commands::Profile { cmd } => match cmd {
            ProfileCmd::List { path, json } => profile_cmd::profile_list(&path, json),
            ProfileCmd::Import { path, file, json } => {
                profile_cmd::profile_import(&path, &file, json)
            }
            ProfileCmd::Run {
                path,
                profile,
                json,
                wait: _,
            } => profile_cmd::profile_run(&path, &profile, json),
        },
        Commands::Workflow { cmd } => match cmd {
            WorkflowCmd::List { path, json } => workflow_cmd::workflow_list(&path, json),
            WorkflowCmd::Import { path, file, json } => {
                workflow_cmd::workflow_import(&path, &file, json)
            }
            WorkflowCmd::Run {
                path,
                workflow,
                params_json,
                json,
                wait: _,
            } => workflow_cmd::workflow_run(&path, &workflow, params_json.as_deref(), json),
        },
        Commands::Ingest {
            path,
            source,
            json,
            wait: _,
        } => convenience::ingest_run(&path, &source, json),
        Commands::Report { cmd } => match cmd {
            ReportCmd::Export { path, out, json } => convenience::report_export(&path, &out, json),
        },
        Commands::Qc { cmd } => match cmd {
            QcCmd::Run {
                path,
                params_json,
                json,
                wait: _,
            } => convenience::qc_run(&path, params_json.as_deref(), json),
        },
        Commands::Produce { cmd } => match cmd {
            ProduceCmd::Run {
                path,
                params_json,
                profile,
                bates_start,
                bates_prefix,
                json,
                wait: _,
            } => convenience::produce_run(
                &path,
                params_json.as_deref(),
                profile.as_deref(),
                bates_start,
                bates_prefix.as_deref(),
                json,
            ),
        },
        Commands::ProductionProfile { cmd } => match cmd {
            ProductionProfileCmd::List { path, json } => {
                production_profile_cmd::production_profile_list(&path, json)
            }
            ProductionProfileCmd::Show { path, slug, json } => {
                production_profile_cmd::production_profile_show(&path, &slug, json)
            }
            ProductionProfileCmd::Upsert { path, file, json } => {
                production_profile_cmd::production_profile_upsert(&path, &file, json)
            }
            ProductionProfileCmd::Delete { path, slug, json } => {
                production_profile_cmd::production_profile_delete(&path, &slug, json)
            }
        },
        Commands::Gap { cmd } => match cmd {
            GapCmd::Run {
                path,
                params_json,
                json,
                wait: _,
            } => convenience::gap_run(&path, params_json.as_deref(), json),
        },
        Commands::Service { cmd } => service_cmd::run_service(cmd).map(|_| ()),
        Commands::Platform { cmd } => platform_cmd::run_platform(cmd).map(|_| ()),
    }
}

/// Validate preflight rate knobs: finite and in [0.0, 1.0].
fn parse_rate_threshold(s: &str) -> std::result::Result<f64, String> {
    let v: f64 = s
        .parse()
        .map_err(|_| format!("invalid rate '{s}': expected a number"))?;
    if !v.is_finite() {
        return Err(format!("invalid rate '{s}': must be finite (not NaN/Inf)"));
    }
    if !(0.0..=1.0).contains(&v) {
        return Err(format!("invalid rate '{s}': must be in [0.0, 1.0]"));
    }
    Ok(v)
}

fn parse_scan_mode(s: &str) -> std::result::Result<ScanMode, String> {
    ScanMode::parse(s).ok_or_else(|| format!("invalid mode '{s}': expected best-effort or strict"))
}

fn parse_keep_policy(s: &str) -> std::result::Result<KeepPolicy, String> {
    KeepPolicy::parse(s).ok_or_else(|| {
        format!("invalid policy '{s}': expected first_seen, keep_largest, or prefer_path")
    })
}

fn parse_family_policy(s: &str) -> std::result::Result<FamilyPolicy, String> {
    FamilyPolicy::parse(s).ok_or_else(|| {
        format!(
            "invalid family-policy '{s}': expected keep_attachments_with_parent or parents_only"
        )
    })
}

/// Packed CLI args for `scan` / `dups` (avoids too-many-arguments).
struct ScanCliArgs {
    paths: Vec<PathBuf>,
    no_tier2: bool,
    no_attachments: bool,
    csv: Option<PathBuf>,
    json: bool,
    list_dups: bool,
    limit: usize,
    mode: ScanMode,
    max_skip_rate: f64,
    max_crc_skip_rate: f64,
    max_failed_file_rate: f64,
    allow_failed_files: bool,
    integrity_csv: Option<PathBuf>,
    skip_limit: usize,
}

fn cmd_scan(args: ScanCliArgs) -> Result<()> {
    let paths = resolve_pst_paths(&args.paths)?;
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
        integrity_csv: args.integrity_csv,
        csv: args.csv.clone(),
        skip_limit: args.skip_limit,
        retain_rows: args.list_dups,
        retain_candidates: false,
        cancel: None,
    };
    // Artifacts (CSV/integrity) are streamed and flushed inside run_scan before return.
    let outcome = run_scan(&paths, &opts)?;

    if let Some(csv_path) = &args.csv {
        // Append summary footer (rows already streamed when csv was set).
        write_report(csv_path, &outcome)?;
    }

    let dup_limit = if args.limit == 0 {
        None
    } else {
        Some(args.limit)
    };
    let dups = if args.list_dups || args.json {
        collect_dups(&outcome, dup_limit)
    } else {
        Vec::new()
    };

    let exit_err = evaluate_exit_policy(&outcome.summary, &opts).err();

    if args.json {
        let ok = exit_err.is_none();
        let mut payload = serde_json::json!({
            "ok": ok,
            "summary": outcome.summary,
            "csv": args.csv.as_ref().map(|p| p.display().to_string()),
            "duplicates": if args.list_dups { serde_json::to_value(&dups)? } else { serde_json::Value::Null },
        });
        if let Some(msg) = &exit_err {
            payload["error"] = serde_json::json!({
                "code": "scan_integrity",
                "message": msg,
            });
        }
        println!("{}", serde_json::to_string_pretty(&payload)?);
        if let Some(msg) = exit_err {
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: CliExit::Generic,
            });
        }
        return Ok(());
    }

    print_summary_text(&outcome.summary);
    if let Some(csv_path) = &args.csv {
        println!("  csv:           {}", csv_path.display());
    }
    if let Some(ic) = &outcome.summary.integrity_csv {
        println!("  integrity_csv: {ic}");
    }
    if args.list_dups {
        println!();
        print_dups_text(&dups);
    }
    if let Some(msg) = exit_err {
        return Err(CliError::Msg(msg));
    }
    Ok(())
}

fn cmd_inspect(path: PathBuf, top: usize, json: bool) -> Result<()> {
    let max = if top == 0 { None } else { Some(top) };
    let report = inspect::inspect_pst(&path, max)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("PST: {}", report.path);
    println!(
        "  size:     {} ({})",
        report.file_size,
        format_bytes(report.file_size)
    );
    println!("  crypt:    {}", report.crypt);
    println!("  folders:  {}", report.folders);
    println!("  messages: {}", report.total_messages);
    println!();
    println!(
        "Folders{}:",
        if top == 0 {
            String::new()
        } else {
            format!(" (top {top} by message count)")
        }
    );
    for f in &report.folder_rows {
        if f.messages == 0 {
            continue;
        }
        println!("  {:>5}  {}", f.messages, f.path);
    }
    Ok(())
}

fn cmd_dups(args: ScanCliArgs) -> Result<()> {
    let paths = resolve_pst_paths(&args.paths)?;
    let opts = ScanOptions {
        enable_tier2: !args.no_tier2,
        include_attachments: true,
        mode: args.mode,
        thresholds: IntegrityThresholds {
            max_skip_rate: args.max_skip_rate,
            max_crc_skip_rate: args.max_crc_skip_rate,
            max_failed_file_rate: args.max_failed_file_rate,
        },
        allow_failed_files: args.allow_failed_files,
        integrity_csv: args.integrity_csv,
        csv: None,
        skip_limit: args.skip_limit,
        retain_rows: true,
        retain_candidates: false,
        cancel: None,
    };
    let outcome = run_scan(&paths, &opts)?;
    let dup_limit = if args.limit == 0 {
        None
    } else {
        Some(args.limit)
    };
    let dups = collect_dups(&outcome, dup_limit);
    let exit_err = evaluate_exit_policy(&outcome.summary, &opts).err();

    if args.json {
        let ok = exit_err.is_none();
        let mut payload = serde_json::json!({
            "ok": ok,
            "summary": outcome.summary,
            "duplicates": dups,
        });
        if let Some(msg) = &exit_err {
            payload["error"] = serde_json::json!({
                "code": "scan_integrity",
                "message": msg,
            });
        }
        println!("{}", serde_json::to_string_pretty(&payload)?);
        if let Some(msg) = exit_err {
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: CliExit::Generic,
            });
        }
        return Ok(());
    }

    print_summary_text(&outcome.summary);
    println!();
    print_dups_text(&dups);
    if let Some(msg) = exit_err {
        return Err(CliError::Msg(msg));
    }
    Ok(())
}

fn print_summary_text(s: &ScanSummary) {
    println!(
        "=== Dedup summary ({:.2}s) mode={} schema={} ===",
        s.duration_secs, s.mode, s.schema
    );
    for f in &s.files {
        if let Some(err) = &f.error {
            let code = f.error_code.map(|c| c.as_str()).unwrap_or("OPEN_FAILED");
            println!("  FAIL [{}] {}: {err}", code, f.name);
        } else {
            println!(
                "  [{}] {}: {} folders, {} msgs, {} dups, {} skipped, {} degraded",
                f.status.as_str(),
                f.name,
                f.folders,
                f.messages,
                f.duplicates,
                f.skipped,
                f.degraded_messages
            );
        }
    }
    println!("  total:         {}", s.total_messages);
    println!("  unique:        {}", s.unique);
    println!("  duplicates:    {}", s.duplicates);
    println!("  tier1 hits:    {}", s.tier1_hits);
    println!("  tier2 hits:    {}", s.tier2_hits);
    println!("  skipped:       {}", s.skipped);
    if !s.skipped_by_reason.is_empty() {
        println!("  skipped_by_reason: {:?}", s.skipped_by_reason);
    }
    println!("  degraded:      {}", s.degraded_messages);
    if !s.degraded_by_reason.is_empty() {
        println!("  degraded_by_reason: {:?}", s.degraded_by_reason);
    }
    println!("  orphaned:      {}", s.orphaned_messages);
    println!(
        "  files:         opened={} partial={} failed={}",
        s.opened_files, s.partial_files, s.failed_files
    );
    println!(
        "  preflight:     {} {:?}",
        s.preflight.recommendation.as_str(),
        s.preflight.reasons
    );
    println!(
        "  savings:       {} ({})",
        s.savings_bytes,
        format_bytes(s.savings_bytes)
    );
}

fn print_dups_text(dups: &[DupRow]) {
    if dups.is_empty() {
        println!("No duplicates listed.");
        return;
    }
    println!("Duplicates ({} shown):", dups.len());
    for (i, d) in dups.iter().enumerate() {
        println!(
            "  [{:02}] [{}] {} | {} | {} bytes",
            i + 1,
            d.tier,
            truncate(&d.subject, 60),
            truncate(&d.sender, 40),
            d.size
        );
        println!("       folder: {}", truncate(&d.folder, 90));
        println!(
            "       original: {} @ {}",
            truncate(&d.original_subject, 50),
            truncate(&d.original_folder, 60)
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let t: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{t}…")
}
