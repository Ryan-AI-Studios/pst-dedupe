//! Convenience wrappers: ingest, report export, qc, produce, gap.

use matter_core::{MatterReportParams, OverviewOptions};
use serde_json::{json, Value};

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};
use crate::matter_cmd::{open_matter, resolve_matter_root};
use crate::paths::{
    load_params_json, resolve_cli_path, resolve_cli_path_maybe_missing,
    validate_params_paths_absolute,
};
use crate::runner_util::run_job_wait;

pub fn ingest_run(path: &std::path::Path, source: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let source_abs = resolve_cli_path(source)?;
    let params = json!({ "path": source_abs.as_str() });
    validate_params_paths_absolute(&params)?;
    let params_str = serde_json::to_string(&params)?;
    let _job = run_job_wait(&root, "ingest", &params_str, json)?;
    Ok(())
}

pub fn report_export(path: &std::path::Path, out: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let out_abs = resolve_cli_path_maybe_missing(out)?;
    let matter = open_matter(&root)?;
    let result = matter
        .export_matter_report(MatterReportParams {
            output_dir: out_abs.clone(),
            overview_opts: OverviewOptions::default(),
            include_pdf: false,
            export_all_jobs: true,
        })
        .map_err(CliError::from)?;
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "output_dir": result.output_dir.as_str(),
                "files_written": result.files_written,
                "generated_at": result.generated_at,
                "pdf_written": result.pdf_written,
            })),
        )?;
    } else {
        println!(
            "report exported to {} ({} files)",
            result.output_dir,
            result.files_written.len()
        );
        for f in &result.files_written {
            println!("  {f}");
        }
    }
    Ok(())
}

pub fn qc_run(path: &std::path::Path, params_json: Option<&str>, json: bool) -> Result<()> {
    run_kind(path, "qc", params_json, json)
}

pub fn produce_run(path: &std::path::Path, params_json: Option<&str>, json: bool) -> Result<()> {
    // If params include relative output_dir, load_params_json rejects it.
    run_kind(path, "produce", params_json, json)
}

pub fn gap_run(path: &std::path::Path, params_json: Option<&str>, json: bool) -> Result<()> {
    run_kind(path, "gap", params_json, json)
}

fn run_kind(
    path: &std::path::Path,
    kind: &str,
    params_json: Option<&str>,
    json: bool,
) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let params: Value = load_params_json(params_json)?;
    let params_str = serde_json::to_string(&params)?;
    let _job = run_job_wait(&root, kind, &params_str, json)?;
    Ok(())
}
