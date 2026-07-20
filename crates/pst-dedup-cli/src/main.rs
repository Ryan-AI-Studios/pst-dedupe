//! `pst-dedup` — CLI for PST tools and headless matter automation (track 0045).
//!
//! Designed for humans and agents: stable subcommands, `--json` stdout isolation,
//! documented exit codes, and SIGINT → graceful cancel.

mod convenience;
mod error;
mod inspect;
mod job_cmd;
mod json_io;
mod matter_cmd;
mod paths;
mod profile_cmd;
mod runner_util;
mod scan;
mod workflow_cmd;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dedup_engine::format_bytes;

use crate::error::{CliError, CliExit, Result};
use crate::json_io::emit_error;
use crate::scan::{collect_dups, resolve_pst_paths, run_scan, write_report, ScanOptions};

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
    },

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

    /// Run production export (0040).
    Produce {
        #[command(subcommand)]
        cmd: ProduceCmd,
    },

    /// Run gap analysis (0042).
    Gap {
        #[command(subcommand)]
        cmd: GapCmd,
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
        | Commands::Ingest { json, .. } => *json,
        Commands::Matter { cmd } => match cmd {
            MatterCmd::Create { json, .. } | MatterCmd::Info { json, .. } => *json,
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
        Commands::Gap { cmd } => match cmd {
            GapCmd::Run { json, .. } => *json,
        },
    }
}

fn main() -> ExitCode {
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
        } => cmd_scan(paths, no_tier2, no_attachments, csv, json, dups, limit),
        Commands::Inspect { path, top, json } => cmd_inspect(path, top, json),
        Commands::Dups {
            paths,
            no_tier2,
            limit,
            json,
        } => cmd_dups(paths, no_tier2, limit, json),
        Commands::Matter { cmd } => match cmd {
            MatterCmd::Create { path, name, json } => matter_cmd::matter_create(&path, &name, json),
            MatterCmd::Info { path, json } => matter_cmd::matter_info(&path, json),
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
                json,
                wait: _,
            } => convenience::produce_run(&path, params_json.as_deref(), json),
        },
        Commands::Gap { cmd } => match cmd {
            GapCmd::Run {
                path,
                params_json,
                json,
                wait: _,
            } => convenience::gap_run(&path, params_json.as_deref(), json),
        },
    }
}

fn cmd_scan(
    paths: Vec<PathBuf>,
    no_tier2: bool,
    no_attachments: bool,
    csv: Option<PathBuf>,
    json: bool,
    list_dups: bool,
    limit: usize,
) -> Result<()> {
    let paths = resolve_pst_paths(&paths)?;
    let opts = ScanOptions {
        enable_tier2: !no_tier2,
        include_attachments: !no_attachments,
    };
    let outcome = run_scan(&paths, &opts)?;

    if let Some(csv_path) = &csv {
        if let Some(parent) = csv_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        write_report(csv_path, &outcome)?;
    }

    let dup_limit = if limit == 0 { None } else { Some(limit) };
    let dups = if list_dups || json {
        collect_dups(&outcome, dup_limit)
    } else {
        Vec::new()
    };

    if outcome.summary.failed_files > 0 {
        let msg = format!("{} file(s) failed to scan", outcome.summary.failed_files);
        if json {
            let payload = serde_json::json!({
                "ok": false,
                "error": { "code": "scan_failed", "message": msg },
                "summary": outcome.summary,
                "csv": csv.as_ref().map(|p| p.display().to_string()),
                "duplicates": if list_dups { serde_json::to_value(&dups)? } else { serde_json::Value::Null },
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: crate::error::CliExit::Generic,
            });
        }
        print_summary_text(&outcome.summary);
        return Err(CliError::Msg(msg));
    }

    if json {
        let payload = serde_json::json!({
            "summary": outcome.summary,
            "csv": csv.as_ref().map(|p| p.display().to_string()),
            "duplicates": if list_dups { serde_json::to_value(&dups)? } else { serde_json::Value::Null },
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    print_summary_text(&outcome.summary);
    if let Some(csv_path) = &csv {
        println!("  csv:           {}", csv_path.display());
    }
    if list_dups {
        println!();
        print_dups_text(&dups);
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

fn cmd_dups(paths: Vec<PathBuf>, no_tier2: bool, limit: usize, json: bool) -> Result<()> {
    let paths = resolve_pst_paths(&paths)?;
    let opts = ScanOptions {
        enable_tier2: !no_tier2,
        include_attachments: true,
    };
    let outcome = run_scan(&paths, &opts)?;
    let dup_limit = if limit == 0 { None } else { Some(limit) };
    let dups = collect_dups(&outcome, dup_limit);

    if outcome.summary.failed_files > 0 {
        let msg = format!("{} file(s) failed to scan", outcome.summary.failed_files);
        if json {
            let payload = serde_json::json!({
                "ok": false,
                "error": { "code": "scan_failed", "message": msg },
                "summary": outcome.summary,
                "duplicates": dups,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
            return Err(CliError::AlreadyEmitted {
                message: msg,
                exit: crate::error::CliExit::Generic,
            });
        }
        print_summary_text(&outcome.summary);
        println!();
        print_dups_text(&dups);
        return Err(CliError::Msg(msg));
    }

    if json {
        let payload = serde_json::json!({
            "summary": outcome.summary,
            "duplicates": dups,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        print_summary_text(&outcome.summary);
        println!();
        print_dups_text(&dups);
    }
    Ok(())
}

fn print_summary_text(s: &scan::ScanSummary) {
    println!("=== Dedup summary ({:.2}s) ===", s.duration_secs);
    for f in &s.files {
        if let Some(err) = &f.error {
            println!("  FAIL {}: {err}", f.name);
        } else {
            println!(
                "  {}: {} folders, {} msgs, {} dups, {} skipped",
                f.name, f.folders, f.messages, f.duplicates, f.skipped
            );
        }
    }
    println!("  total:         {}", s.total_messages);
    println!("  unique:        {}", s.unique);
    println!("  duplicates:    {}", s.duplicates);
    println!("  tier1 hits:    {}", s.tier1_hits);
    println!("  tier2 hits:    {}", s.tier2_hits);
    println!("  skipped:       {}", s.skipped);
    println!(
        "  savings:       {} ({})",
        s.savings_bytes,
        format_bytes(s.savings_bytes)
    );
}

fn print_dups_text(dups: &[scan::DupRow]) {
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
