//! `matter create` / `matter info` / `matter change-passphrase` / `matter storage` commands.

use camino::Utf8Path;
use matter_core::{
    passphrase_from_env, Matter, StorageBackendConfig, StorageBackendKind, ENV_MATTER_PASSPHRASE,
};
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

/// Show non-secret storage + job backend config (schema v39).
pub fn matter_storage_show(path: &std::path::Path, json: bool) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let matter = Matter::open_for_read(&root).map_err(CliError::from)?;
    let storage = matter
        .get_storage_backend_config()
        .map_err(CliError::from)?;
    let job_kind = matter.get_job_backend_kind().map_err(CliError::from)?;
    let redacted = storage.redacted_for_audit();
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "path": root.as_str(),
                "storage": redacted,
                "job_backend_kind": job_kind.as_str(),
            })),
        )?;
    } else {
        println!("matter storage at {root}");
        println!("  kind:      {}", storage.kind.as_str());
        if let Some(b) = &storage.bucket {
            println!("  bucket:    {b}");
        }
        if let Some(r) = &storage.region {
            println!("  region:    {r}");
        }
        if let Some(e) = &storage.endpoint {
            println!("  endpoint:  {e}");
        }
        if let Some(p) = &storage.prefix {
            println!("  prefix:    {p}");
        }
        if let Some(t) = &storage.tenant_id {
            println!("  tenant_id: {t}");
        }
        if let Some(m) = &storage.matter_id {
            println!("  matter_id: {m}");
        }
        println!("  job:       {}", job_kind.as_str());
        println!("  credentials: env/IAM only (never stored in matter.db)");
    }
    Ok(())
}

/// Set non-secret storage backend config.
#[allow(clippy::too_many_arguments)]
pub fn matter_storage_set(
    path: &std::path::Path,
    kind: &str,
    bucket: Option<&str>,
    region: Option<&str>,
    endpoint: Option<&str>,
    prefix: Option<&str>,
    tenant_id: Option<&str>,
    matter_id: Option<&str>,
    json: bool,
) -> Result<()> {
    let root = resolve_matter_root(path)?;
    let kind = StorageBackendKind::parse(kind).map_err(|e| CliError::Usage(e.to_string()))?;
    let config = StorageBackendConfig {
        kind,
        bucket: bucket
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        region: region
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        endpoint: endpoint
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        prefix: prefix
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        tenant_id: tenant_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        matter_id: matter_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        sse: None,
        cache_max_bytes: None,
    };
    config
        .validate()
        .map_err(|e| CliError::Usage(format!("storage config: {e}")))?;
    let matter = Matter::open(&root).map_err(CliError::from)?;
    matter
        .set_storage_backend_config(&config)
        .map_err(CliError::from)?;
    // Seal path for encrypted matters is handled by Drop / open write.
    drop(matter);
    if json {
        emit_json(
            true,
            &ok_envelope(json!({
                "path": root.as_str(),
                "storage": config.redacted_for_audit(),
            })),
        )?;
    } else {
        println!(
            "storage backend set to kind={} (secrets remain env/IAM only)",
            config.kind.as_str()
        );
        if config.kind.is_cloud() {
            println!(
                "note: cloud CAS activates on next matter open; requires binary built with \
                 --features cloud-s3 (open fails closed without it — no local fallback)"
            );
        }
    }
    Ok(())
}
