//! `workflow list|import|run` commands.

use matter_core::WorkflowInput;
use serde_json::json;

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};
use crate::matter_cmd::{open_matter, open_matter_read, resolve_matter_root};
use crate::paths::{load_params_json, resolve_cli_path};
use crate::profile_cmd::parse_import_document;
use crate::runner_util::run_job_wait;

pub fn workflow_list(path: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let matter = open_matter_read(&root)?;
    let list = matter.list_workflows().map_err(CliError::from)?;
    if json {
        let rows: Vec<_> = list
            .iter()
            .map(|w| {
                json!({
                    "id": w.id,
                    "name": w.name,
                    "description": w.description,
                    "is_builtin": w.is_builtin,
                })
            })
            .collect();
        emit_json(
            true,
            &ok_envelope(json!({ "workflows": rows, "count": rows.len() })),
        )?;
    } else {
        for w in &list {
            let tag = if w.is_builtin { "builtin" } else { "user" };
            println!("[{tag}] {}  {}", w.id, w.name);
        }
    }
    Ok(())
}

pub fn workflow_import(path: &std::path::Path, file: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let file_path = resolve_cli_path(file)?;
    let text = std::fs::read_to_string(file_path.as_std_path())
        .map_err(|e| CliError::Usage(format!("cannot read workflow file {file_path}: {e}")))?;
    let (name, description, body_json) = parse_import_document(&text, "workflow")?;
    let matter = open_matter(&root)?;
    let wf = matter
        .upsert_workflow(WorkflowInput {
            id: None,
            name,
            description,
            body_json,
            created_by: Some("cli".into()),
        })
        .map_err(CliError::from)?;
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "id": wf.id,
                "name": wf.name,
                "is_builtin": wf.is_builtin,
            })),
        )?;
    } else {
        println!("imported workflow id={} name={}", wf.id, wf.name);
    }
    Ok(())
}

pub fn workflow_run(
    path: &std::path::Path,
    workflow: &str,
    params_json: Option<&str>,
    json: bool,
) -> Result<()> {
    if workflow.trim().is_empty() {
        return Err(CliError::Usage("workflow id/name must not be empty".into()));
    }
    let root = resolve_matter_root(path)?;
    let run_params = load_params_json(params_json)?;
    let workflow_id = resolve_workflow_id(&root, workflow)?;
    let params = json!({
        "workflow_id": workflow_id,
        "run_params": run_params,
    });
    let params_str = serde_json::to_string(&params)?;
    let _job = run_job_wait(&root, "workflow_run", &params_str, json)?;
    Ok(())
}

fn resolve_workflow_id(root: &camino::Utf8Path, workflow: &str) -> Result<String> {
    let matter = open_matter_read(root)?;
    let list = matter.list_workflows().map_err(CliError::from)?;
    if let Some(w) = list.iter().find(|w| w.id == workflow) {
        return Ok(w.id.clone());
    }
    if let Some(w) = list.iter().find(|w| w.name == workflow) {
        return Ok(w.id.clone());
    }
    // Pass through (builtin:name etc.).
    Ok(workflow.to_string())
}
