//! `profile list|import|run` commands.

use matter_core::ProcessingProfileInput;
use serde_json::{json, Value};

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};
use crate::matter_cmd::{open_matter, open_matter_read, resolve_matter_root};
use crate::paths::resolve_cli_path;
use crate::runner_util::run_job_wait;

pub fn profile_list(path: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let matter = open_matter_read(&root)?;
    let list = matter.list_processing_profiles().map_err(CliError::from)?;
    if json {
        let rows: Vec<_> = list
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "name": p.name,
                    "description": p.description,
                    "is_builtin": p.is_builtin,
                })
            })
            .collect();
        emit_json(
            true,
            &ok_envelope(json!({ "profiles": rows, "count": rows.len() })),
        )?;
    } else {
        for p in &list {
            let tag = if p.is_builtin { "builtin" } else { "user" };
            println!("[{tag}] {}  {}", p.id, p.name);
        }
    }
    Ok(())
}

pub fn profile_import(path: &std::path::Path, file: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let file_path = resolve_cli_path(file)?;
    let text = std::fs::read_to_string(file_path.as_std_path())
        .map_err(|e| CliError::Usage(format!("cannot read profile file {file_path}: {e}")))?;
    let (name, description, body_json) = parse_import_document(&text, "profile")?;
    let matter = open_matter(&root)?;
    let profile = matter
        .upsert_processing_profile(ProcessingProfileInput {
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
                "id": profile.id,
                "name": profile.name,
                "is_builtin": profile.is_builtin,
            })),
        )?;
    } else {
        println!("imported profile id={} name={}", profile.id, profile.name);
    }
    Ok(())
}

pub fn profile_run(path: &std::path::Path, profile: &str, json: bool) -> Result<()> {
    if profile.trim().is_empty() {
        return Err(CliError::Usage("profile id/name must not be empty".into()));
    }
    let root = resolve_matter_root(path)?;
    // Resolve name vs id for stable params.
    let params = resolve_profile_params(&root, profile)?;
    let params_str = serde_json::to_string(&params)?;
    let _job = run_job_wait(&root, "profile_run", &params_str, json)?;
    Ok(())
}

fn resolve_profile_params(root: &camino::Utf8Path, profile: &str) -> Result<Value> {
    let matter = open_matter_read(root)?;
    let list = matter.list_processing_profiles().map_err(CliError::from)?;
    // Prefer exact id match, then name.
    if let Some(p) = list.iter().find(|p| p.id == profile) {
        return Ok(json!({ "profile_id": p.id }));
    }
    if let Some(p) = list.iter().find(|p| p.name == profile) {
        return Ok(json!({ "profile_id": p.id }));
    }
    // Allow builtin:name or bare builtin name passthrough to engine.
    if profile.starts_with("builtin:") || list.iter().any(|p| p.name == profile) {
        return Ok(json!({ "profile_id": profile }));
    }
    // Still pass through — engine will validate (may be builtin name).
    Ok(json!({ "profile_id": profile }))
}

/// Parse an import file that is either:
/// - Full document: `{ "name": "…", "description": "…", "body": {…} }` or body at top with name
/// - Bare body: `{ "version": 1, "stages": … }` requiring top-level `name`
pub fn parse_import_document(text: &str, kind: &str) -> Result<(String, Option<String>, String)> {
    let root: Value = serde_json::from_str(text)
        .map_err(|e| CliError::Usage(format!("invalid {kind} JSON: {e}")))?;
    let obj = root
        .as_object()
        .ok_or_else(|| CliError::Usage(format!("{kind} JSON must be an object")))?;

    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            CliError::Usage(format!("{kind} import requires top-level \"name\" field"))
        })?;

    let description = obj
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Body may be nested under "body" or be the stages/nodes document itself.
    let body_json = if let Some(body) = obj.get("body") {
        serde_json::to_string(body)?
    } else if obj.contains_key("version")
        && (obj.contains_key("stages") || obj.contains_key("nodes"))
    {
        // Strip name/description for body-only shape.
        let mut body = obj.clone();
        body.remove("name");
        body.remove("description");
        body.remove("id");
        serde_json::to_string(&Value::Object(body))?
    } else {
        return Err(CliError::Usage(format!(
            "{kind} import needs \"body\" object or top-level version+stages/nodes"
        )));
    };

    Ok((name, description, body_json))
}
