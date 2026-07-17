//! `pst-dedup` — CLI surface for PST inspection and deduplication.
//!
//! Designed for both humans and agents: stable subcommands, `--json` output,
//! and non-zero exit on hard failures.

mod error;
mod inspect;
mod scan;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dedup_engine::format_bytes;

use crate::error::Result;
use crate::scan::{collect_dups, resolve_pst_paths, run_scan, write_report, ScanOptions};

#[derive(Debug, Parser)]
#[command(
    name = "pst-dedup",
    version,
    about = "PST email deduplication CLI (scan / inspect / dups)",
    long_about = "Read-only PST inspection and tiered dedup.\n\n\
Examples:\n  \
  pst-dedup scan archive.pst --json\n  \
  pst-dedup scan a.pst b.pst --csv report.csv\n  \
  pst-dedup inspect archive.pst --top 20\n  \
  pst-dedup dups archive.pst --limit 25 --json"
)]
struct Cli {
    /// Increase log verbosity (-v, -vv).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scan PST file(s), run tiered dedup, print summary.
    Scan {
        /// One or more .pst paths.
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Disable Tier 2 content-hash fallback.
        #[arg(long)]
        no_tier2: bool,

        /// Skip attachment metadata in Tier 2 hashing.
        #[arg(long)]
        no_attachments: bool,

        /// Write full CSV report (+ summary footer) to this path.
        #[arg(long)]
        csv: Option<PathBuf>,

        /// Emit machine-readable JSON summary (and dups if --dups).
        #[arg(long)]
        json: bool,

        /// Also list duplicate rows (text or inside JSON).
        #[arg(long)]
        dups: bool,

        /// Cap listed duplicates (default 50 when --dups; 0 = all).
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },

    /// Inspect PST structure: encryption, folder tree, message counts.
    Inspect {
        /// Path to a .pst file.
        path: PathBuf,

        /// Max folders to list (sorted by message count). 0 = all.
        #[arg(long, default_value_t = 30)]
        top: usize,

        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },

    /// Scan and list only duplicate messages.
    Dups {
        /// One or more .pst paths.
        #[arg(required = true)]
        paths: Vec<PathBuf>,

        /// Disable Tier 2 content-hash fallback.
        #[arg(long)]
        no_tier2: bool,

        /// Max duplicates to print (0 = all).
        #[arg(long, default_value_t = 50)]
        limit: usize,

        /// Emit JSON array of duplicates + summary.
        #[arg(long)]
        json: bool,
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
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

    if outcome.summary.failed_files > 0 {
        return Err(error::CliError::Msg(format!(
            "{} file(s) failed to scan",
            outcome.summary.failed_files
        )));
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

    if outcome.summary.failed_files > 0 {
        return Err(error::CliError::Msg(format!(
            "{} file(s) failed to scan",
            outcome.summary.failed_files
        )));
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
