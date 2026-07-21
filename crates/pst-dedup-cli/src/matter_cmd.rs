//! `matter create` / `matter info` / `matter change-passphrase` commands.

use camino::Utf8Path;
use matter_core::{passphrase_from_env, Matter, ENV_MATTER_PASSPHRASE};
use serde_json::json;

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};
use crate::paths::resolve_cli_path_maybe_missing;

/// Env var for the new passphrase when changing (`matter change-passphrase`).
pub const ENV_MATTER_NEW_PASSPHRASE: &str = "PST_DEDUPE_MATTER_NEW_PASSPHRASE";
/// Confirmation of new passphrase (must match `ENV_MATTER_NEW_PASSPHRASE`).
pub const ENV_MATTER_NEW_PASSPHRASE_CONFIRM: &str = "PST_DEDUPE_MATTER_NEW_PASSPHRASE_CONFIRM";

pub fn matter_create(path: &std::path::Path, name: &str, encrypt: bool, json: bool) -> Result<()> {
    if name.trim().is_empty() {
        return Err(CliError::Usage("matter name must not be empty".into()));
    }
    let root = resolve_cli_path_maybe_missing(path)?;
    let matter = if encrypt {
        let passphrase = passphrase_from_env().ok_or_else(|| {
            CliError::Usage(format!(
                "--encrypt requires env {ENV_MATTER_PASSPHRASE} (non-empty)"
            ))
        })?;
        Matter::create_encrypted(&root, name, &passphrase).map_err(CliError::from)?
    } else {
        Matter::create(&root, name).map_err(CliError::from)?
    };
    let info = matter.info().map_err(CliError::from)?;
    let encrypted = matter.encryption_enabled();
    // Seal encrypted session before process exit (Drop order on local matter).
    drop(matter);
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
                "encryption_enabled": encrypted,
            })),
        )?;
    } else {
        println!(
            "created matter '{}' id={} path={} schema={} encrypted={}",
            info.name, info.id, root, info.schema_version, encrypted
        );
    }
    Ok(())
}

pub fn matter_info(path: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let matter = Matter::open_for_read(&root).map_err(CliError::from)?;
    let info = matter.info().map_err(CliError::from)?;
    let encrypted = matter.encryption_enabled();
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
                "encryption_enabled": encrypted,
            })),
        )?;
    } else {
        println!("matter: {}", info.name);
        println!("  id:      {}", info.id);
        println!("  path:    {root}");
        println!("  schema:  {}", info.schema_version);
        println!("  created: {}", info.created_at);
        println!("  encrypt: {encrypted}");
    }
    Ok(())
}

/// Re-wrap DEK under a new passphrase.
///
/// Reads current passphrase from [`ENV_MATTER_PASSPHRASE`] and new passphrase
/// from [`ENV_MATTER_NEW_PASSPHRASE`]. Neither is logged.
pub fn matter_change_passphrase(path: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    if !Matter::is_encrypted_on_disk(&root) {
        return Err(CliError::Usage(
            "matter is not encrypted; change-passphrase is a no-op".into(),
        ));
    }
    let old = passphrase_from_env().ok_or_else(|| {
        CliError::Usage(format!(
            "set env {ENV_MATTER_PASSPHRASE} to the current passphrase"
        ))
    })?;
    let new = std::env::var(ENV_MATTER_NEW_PASSPHRASE)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "set env {ENV_MATTER_NEW_PASSPHRASE} to the new passphrase"
            ))
        })?;
    let confirm = std::env::var(ENV_MATTER_NEW_PASSPHRASE_CONFIRM)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "set env {ENV_MATTER_NEW_PASSPHRASE_CONFIRM} to confirm the new passphrase"
            ))
        })?;
    if confirm != new {
        return Err(CliError::Usage(
            "new passphrase and confirmation do not match".into(),
        ));
    }
    let matter = Matter::open_with_passphrase(&root, &old, true).map_err(CliError::from)?;
    matter
        .change_passphrase(&old, &new)
        .map_err(CliError::from)?;
    // Explicit seal so rekey durability errors surface (not only Drop).
    matter.seal_encrypted().map_err(CliError::from)?;
    if json {
        emit_json(
            true,
            &ok_envelope(json!({ "changed": true, "path": root.as_str() })),
        )?;
    } else {
        println!("passphrase changed for matter at {root}");
    }
    Ok(())
}

/// Resolve and verify a matter root (must contain matter.db or recoverable seal temp).
pub fn resolve_matter_root(path: &std::path::Path) -> Result<camino::Utf8PathBuf> {
    let root = resolve_cli_path_maybe_missing(path)?;
    // Promote crash seal temp before requiring matter.db (Windows replace gap).
    let _ = matter_core::crypto::recover_seal_temp(&root);
    let _ = matter_core::crypto::recover_header_temp(&root);
    let db = root.join("matter.db");
    if !db.as_std_path().exists() {
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
