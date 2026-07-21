//! Platform control-plane CLI (track 0059): tenants, IdP, matter registration.

use std::path::PathBuf;
use std::process::ExitCode;

use camino::Utf8PathBuf;
use clap::Subcommand;
use matter_platform::{
    generate_pmk, load_pmk_from_env, parse_pmk, Platform, SetIdpConfigInput,
    ENV_PLATFORM_MASTER_KEY, ENV_PLATFORM_STORAGE_ROOT,
};

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};

#[derive(Debug, Subcommand)]
pub enum PlatformCmd {
    /// Create a tenant in platform.db.
    Tenant {
        #[command(subcommand)]
        cmd: PlatformTenantCmd,
    },
    /// Configure tenant IdP (OIDC).
    Idp {
        #[command(subcommand)]
        cmd: PlatformIdpCmd,
    },
    /// Register a matter path under a tenant.
    Matter {
        #[command(subcommand)]
        cmd: PlatformMatterCmd,
    },
    /// Create an empty platform.db (and optional PMK note).
    Init {
        #[arg(long)]
        platform: PathBuf,
        /// Generate a random PMK and print hex (does not write secrets to disk).
        #[arg(long, default_value_t = false)]
        print_pmk: bool,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlatformTenantCmd {
    Create {
        #[arg(long)]
        platform: PathBuf,
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = false)]
        jit: bool,
        #[arg(long, default_value_t = false)]
        oidc_required: bool,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    List {
        #[arg(long)]
        platform: PathBuf,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlatformIdpCmd {
    Set {
        #[arg(long)]
        platform: PathBuf,
        /// Tenant slug or id.
        #[arg(long)]
        tenant: String,
        #[arg(long)]
        issuer: String,
        #[arg(long)]
        client_id: String,
        /// Env var name holding client secret (preferred).
        #[arg(long)]
        secret_env: Option<String>,
        /// Read secret from file (encrypted into platform.db under PMK).
        #[arg(long)]
        secret_file: Option<PathBuf>,
        /// Inline secret (discouraged; prefer --secret-env). Requires PMK.
        #[arg(long)]
        secret: Option<String>,
        /// Comma-separated allowed email domains for JIT.
        #[arg(long)]
        allowed_domains: Option<String>,
        /// Comma-separated required OIDC groups for JIT.
        #[arg(long)]
        required_groups: Option<String>,
        /// JSON object mapping group → role.
        #[arg(long)]
        role_claim_map: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PlatformMatterCmd {
    Register {
        #[arg(long)]
        platform: PathBuf,
        #[arg(long)]
        tenant: String,
        #[arg(long)]
        path: PathBuf,
        /// Optional matter id override (default: matter.db matters.id if openable, else folder name).
        #[arg(long)]
        matter_id: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    List {
        #[arg(long)]
        platform: PathBuf,
        #[arg(long)]
        tenant: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

pub fn run_platform(cmd: PlatformCmd) -> Result<ExitCode> {
    match cmd {
        PlatformCmd::Init {
            platform,
            print_pmk,
            json,
        } => {
            let path = utf8_path(&platform)?;
            let pmk = load_pmk_from_env().map_err(|e| CliError::Msg(e.to_string()))?;
            let _plat = Platform::create(&path, pmk).map_err(|e| CliError::Msg(e.to_string()))?;
            if print_pmk {
                let k = generate_pmk();
                let pmk_hex = hex_encode(&k);
                eprintln!("Generated PMK (hex). Set {ENV_PLATFORM_MASTER_KEY} and store securely:");
                eprintln!("{pmk_hex}");
            }
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "platform.init",
                    "platform": path.as_str(),
                    "pmk_generated": print_pmk,
                })),
            )?;
            Ok(ExitCode::SUCCESS)
        }
        PlatformCmd::Tenant { cmd } => run_tenant(cmd),
        PlatformCmd::Idp { cmd } => run_idp(cmd),
        PlatformCmd::Matter { cmd } => run_matter(cmd),
    }
}

fn run_tenant(cmd: PlatformTenantCmd) -> Result<ExitCode> {
    match cmd {
        PlatformTenantCmd::Create {
            platform,
            slug,
            name,
            jit,
            oidc_required,
            json,
        } => {
            let path = utf8_path(&platform)?;
            let plat = open_platform(&path)?;
            let t = plat
                .create_tenant(&slug, &name, jit, oidc_required)
                .map_err(|e| CliError::Msg(e.to_string()))?;
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "platform.tenant.create",
                    "id": t.id,
                    "slug": t.slug,
                    "display_name": t.display_name,
                    "jit_provision": t.jit_provision,
                    "oidc_required": t.oidc_required,
                })),
            )?;
            Ok(ExitCode::SUCCESS)
        }
        PlatformTenantCmd::List { platform, json } => {
            let path = utf8_path(&platform)?;
            let plat = open_platform(&path)?;
            let tenants = plat
                .list_tenants()
                .map_err(|e| CliError::Msg(e.to_string()))?;
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "platform.tenant.list",
                    "tenants": tenants,
                })),
            )?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_idp(cmd: PlatformIdpCmd) -> Result<ExitCode> {
    match cmd {
        PlatformIdpCmd::Set {
            platform,
            tenant,
            issuer,
            client_id,
            secret_env,
            secret_file,
            secret,
            allowed_domains,
            required_groups,
            role_claim_map,
            json,
        } => {
            let path = utf8_path(&platform)?;
            let mut plat = open_platform(&path)?;
            // Ensure PMK loaded if we need ciphertext.
            if (secret.is_some() || secret_file.is_some()) && !plat.pmk_present() {
                let pmk = load_pmk_from_env()
                    .map_err(|e| CliError::Msg(e.to_string()))?
                    .ok_or_else(|| {
                        CliError::Usage(format!(
                            "storing IdP secret requires {ENV_PLATFORM_MASTER_KEY}"
                        ))
                    })?;
                plat.set_pmk(Some(pmk));
            }
            let tenant_row = resolve_tenant(&plat, &tenant)?;
            let secret_plaintext = if let Some(s) = secret {
                Some(s)
            } else if let Some(f) = secret_file {
                let raw = std::fs::read_to_string(&f)
                    .map_err(|e| CliError::Msg(format!("read secret file: {e}")))?;
                Some(raw.trim().to_string())
            } else {
                None
            };
            if secret_env.is_none() && secret_plaintext.is_none() {
                return Err(CliError::Usage(
                    "provide --secret-env and/or --secret / --secret-file".into(),
                ));
            }
            let domains = split_csv(allowed_domains.as_deref());
            let groups = split_csv(required_groups.as_deref());
            let role_map = if let Some(raw) = role_claim_map {
                let v: serde_json::Value = serde_json::from_str(&raw)
                    .map_err(|e| CliError::Usage(format!("--role-claim-map JSON: {e}")))?;
                v.as_object().cloned().ok_or_else(|| {
                    CliError::Usage("--role-claim-map must be a JSON object".into())
                })?
            } else {
                Default::default()
            };
            let cfg = plat
                .set_idp_config(
                    &tenant_row.id,
                    SetIdpConfigInput {
                        issuer_url: issuer,
                        client_id: client_id.clone(),
                        secret_env,
                        secret_plaintext,
                        audiences: vec![client_id],
                        role_claim_map: role_map,
                        allowed_email_domains: domains,
                        required_groups: groups,
                        enabled: true,
                    },
                )
                .map_err(|e| CliError::Msg(e.to_string()))?;
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "platform.idp.set",
                    "tenant_id": cfg.tenant_id,
                    "issuer_url": cfg.issuer_url,
                    "client_id": cfg.client_id,
                    "secret_env": cfg.secret_env,
                    "has_secret_ciphertext": cfg.has_secret_ciphertext,
                    "allowed_email_domains": cfg.allowed_email_domains,
                    "required_groups": cfg.required_groups,
                })),
            )?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_matter(cmd: PlatformMatterCmd) -> Result<ExitCode> {
    match cmd {
        PlatformMatterCmd::Register {
            platform,
            tenant,
            path,
            matter_id,
            json,
        } => {
            let plat_path = utf8_path(&platform)?;
            let mut plat = open_platform(&plat_path)?;
            // Storage root from env is required for sandbox.
            if plat.storage_roots().is_empty() {
                return Err(CliError::Usage(format!(
                    "set {ENV_PLATFORM_STORAGE_ROOT} before registering matters"
                )));
            }
            // Re-load env roots in case open missed (always set from env at open).
            let _ = &mut plat;
            let tenant_row = resolve_tenant(&plat, &tenant)?;
            let matter_path = path.canonicalize().unwrap_or(path.clone());
            let mid = if let Some(id) = matter_id {
                id
            } else if let Ok(root) = Utf8PathBuf::from_path_buf(matter_path.clone()) {
                // Try open matter for id.
                match matter_core::Matter::open_for_read(&root) {
                    Ok(m) => m.id().to_string(),
                    Err(_) => matter_path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "matter".into()),
                }
            } else {
                matter_path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "matter".into())
            };
            let reg = plat
                .register_matter(&tenant_row.id, &mid, &matter_path)
                .map_err(|e| CliError::Msg(e.to_string()))?;
            // Stamp tenant_id on matter.db when possible.
            if let Ok(root) = Utf8PathBuf::from_path_buf(matter_path.clone()) {
                if let Ok(m) = matter_core::Matter::open(&root) {
                    let _ = m.set_matter_tenant_id(Some(&tenant_row.id));
                }
            }
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "platform.matter.register",
                    "tenant_id": reg.tenant_id,
                    "matter_id": reg.matter_id,
                    "storage_root": reg.storage_root,
                })),
            )?;
            Ok(ExitCode::SUCCESS)
        }
        PlatformMatterCmd::List {
            platform,
            tenant,
            json,
        } => {
            let path = utf8_path(&platform)?;
            let plat = open_platform(&path)?;
            let tenant_row = resolve_tenant(&plat, &tenant)?;
            let matters = plat
                .list_matters(&tenant_row.id)
                .map_err(|e| CliError::Msg(e.to_string()))?;
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "platform.matter.list",
                    "tenant_id": tenant_row.id,
                    "matters": matters,
                })),
            )?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn open_platform(path: &camino::Utf8Path) -> Result<Platform> {
    let pmk = load_pmk_from_env().map_err(|e| CliError::Msg(e.to_string()))?;
    // Also allow explicit hex/base64 in env only (already via load_pmk_from_env).
    let _ = parse_pmk; // keep import used for future
    if path.exists() {
        Platform::open(path, pmk).map_err(|e| CliError::Msg(e.to_string()))
    } else {
        Platform::create(path, pmk).map_err(|e| CliError::Msg(e.to_string()))
    }
}

fn resolve_tenant(plat: &Platform, tenant: &str) -> Result<matter_platform::Tenant> {
    if let Some(t) = plat
        .get_tenant_by_slug(tenant)
        .map_err(|e| CliError::Msg(e.to_string()))?
    {
        return Ok(t);
    }
    if let Some(t) = plat
        .get_tenant_by_id(tenant)
        .map_err(|e| CliError::Msg(e.to_string()))?
    {
        return Ok(t);
    }
    Err(CliError::Usage(format!("tenant not found: {tenant}")))
}

fn split_csv(raw: Option<&str>) -> Vec<String> {
    raw.map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(|x| x.to_string())
            .collect()
    })
    .unwrap_or_default()
}

fn utf8_path(p: &std::path::Path) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(p.to_path_buf())
        .map_err(|_| CliError::Usage(format!("path is not valid UTF-8: {}", p.display())))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}
