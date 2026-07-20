//! `matter create` / `matter info` commands.

use camino::Utf8Path;
use matter_core::Matter;
use serde_json::json;

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};
use crate::paths::resolve_cli_path_maybe_missing;

pub fn matter_create(path: &std::path::Path, name: &str, json: bool) -> Result<()> {
    if name.trim().is_empty() {
        return Err(CliError::Usage("matter name must not be empty".into()));
    }
    let root = resolve_cli_path_maybe_missing(path)?;
    let matter = Matter::create(&root, name).map_err(CliError::from)?;
    let info = matter.info().map_err(CliError::from)?;
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "id": info.id,
                "name": info.name,
                "path": root.as_str(),
                "schema_version": info.schema_version,
                "created_at": info.created_at,
                "storage_root": info.storage_root,
            })),
        )?;
    } else {
        println!(
            "created matter '{}' id={} path={} schema={}",
            info.name, info.id, root, info.schema_version
        );
    }
    Ok(())
}

pub fn matter_info(path: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let matter = Matter::open_for_read(&root).map_err(CliError::from)?;
    let info = matter.info().map_err(CliError::from)?;
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "id": info.id,
                "name": info.name,
                "path": root.as_str(),
                "schema_version": info.schema_version,
                "created_at": info.created_at,
                "storage_root": info.storage_root,
            })),
        )?;
    } else {
        println!("matter: {}", info.name);
        println!("  id:      {}", info.id);
        println!("  path:    {root}");
        println!("  schema:  {}", info.schema_version);
        println!("  created: {}", info.created_at);
    }
    Ok(())
}

/// Resolve and verify a matter root (must contain matter.db).
pub fn resolve_matter_root(path: &std::path::Path) -> Result<camino::Utf8PathBuf> {
    let root = resolve_cli_path_maybe_missing(path)?;
    let db = root.join("matter.db");
    if !db.as_std_path().exists() {
        // Try without canonicalize for not-yet-created — open will fail.
        // For info/run, require existing db.
        return Err(CliError::MatterIo(format!(
            "not a matter root (missing matter.db): {root}"
        )));
    }
    Ok(root)
}

pub fn open_matter(root: &Utf8Path) -> Result<Matter> {
    Matter::open(root).map_err(CliError::from)
}

pub fn open_matter_read(root: &Utf8Path) -> Result<Matter> {
    Matter::open_for_read(root).map_err(CliError::from)
}
