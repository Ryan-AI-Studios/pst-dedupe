//! Multi-user matter service CLI (track 0058).

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use camino::Utf8PathBuf;
use clap::Subcommand;
use matter_core::{is_encrypted_matter, Matter, ENV_MATTER_PASSPHRASE};
use matter_service::{default_bind, serve, validate_bind, ServeConfig};

use crate::error::{CliError, Result};
use crate::json_io::{emit_json, ok_envelope};

#[derive(Debug, Subcommand)]
pub enum ServiceCmd {
    /// Host a matter over loopback HTTP (exclusive write open).
    Serve {
        #[arg(long)]
        matter: PathBuf,
        /// Bind address (default 127.0.0.1:7749). Non-loopback requires --allow-lan.
        #[arg(long)]
        bind: Option<String>,
        /// Permit non-loopback bind (LAN).
        #[arg(long, default_value_t = false)]
        allow_lan: bool,
        /// Env var name holding passphrase for encrypted matters.
        #[arg(long, default_value = ENV_MATTER_PASSPHRASE)]
        passphrase_env: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Create the first admin user and enable multi-user mode.
    BootstrapAdmin {
        #[arg(long)]
        matter: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        password: Option<String>,
        /// Read password from this env var (preferred over --password).
        #[arg(long)]
        password_env: Option<String>,
        #[arg(long, default_value = ENV_MATTER_PASSPHRASE)]
        passphrase_env: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// User admin helpers.
    User {
        #[command(subcommand)]
        cmd: ServiceUserCmd,
    },
}

#[derive(Debug, Subcommand)]
pub enum ServiceUserCmd {
    Add {
        #[arg(long)]
        matter: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        password_env: Option<String>,
        /// Actor user id (required under multi-user; free-form when not strict).
        #[arg(long, default_value = "cli")]
        actor: String,
        #[arg(long, default_value = ENV_MATTER_PASSPHRASE)]
        passphrase_env: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    List {
        #[arg(long)]
        matter: PathBuf,
        #[arg(long, default_value = ENV_MATTER_PASSPHRASE)]
        passphrase_env: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Disable {
        #[arg(long)]
        matter: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long, default_value = "cli")]
        actor: String,
        #[arg(long, default_value = ENV_MATTER_PASSPHRASE)]
        passphrase_env: String,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

pub fn run_service(cmd: ServiceCmd) -> Result<ExitCode> {
    match cmd {
        ServiceCmd::Serve {
            matter,
            bind,
            allow_lan,
            passphrase_env,
            json,
        } => {
            let root = utf8_path(&matter)?;
            let bind_addr: SocketAddr = match bind {
                Some(s) => s
                    .parse()
                    .map_err(|e| CliError::Usage(format!("invalid --bind: {e}")))?,
                None => default_bind(),
            };
            validate_bind(bind_addr, allow_lan).map_err(CliError::Usage)?;
            let passphrase = if is_encrypted_matter(&root) {
                Some(std::env::var(&passphrase_env).map_err(|_| {
                    CliError::Usage(format!("encrypted matter requires env {passphrase_env}"))
                })?)
            } else {
                None
            };
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "service.serve",
                    "matter": root.as_str(),
                    "bind": bind_addr.to_string(),
                    "allow_lan": allow_lan,
                })),
            )?;
            let config = ServeConfig {
                matter_root: root,
                bind: bind_addr,
                allow_lan,
                passphrase,
            };
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| CliError::Msg(format!("tokio runtime: {e}")))?;
            rt.block_on(async move {
                serve(config)
                    .await
                    .map_err(|e| CliError::Msg(format!("serve failed: {e}")))
            })?;
            Ok(ExitCode::SUCCESS)
        }
        ServiceCmd::BootstrapAdmin {
            matter,
            name,
            password,
            password_env,
            passphrase_env,
            json,
        } => {
            let root = utf8_path(&matter)?;
            let pass = resolve_password(password, password_env)?;
            let matter = open_matter_cli(&root, &passphrase_env)?;
            matter.enable_multi_user("system")?;
            let user = matter.create_user(&name, "admin", &pass, "system")?;
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "service.bootstrap_admin",
                    "user_id": user.id,
                    "display_name": user.display_name,
                    "role": user.role,
                    "multi_user_enabled": true,
                })),
            )?;
            if !json {
                eprintln!(
                    "bootstrap-admin: created admin '{}' ({})",
                    user.display_name, user.id
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        ServiceCmd::User { cmd } => run_user(cmd),
    }
}

fn run_user(cmd: ServiceUserCmd) -> Result<ExitCode> {
    match cmd {
        ServiceUserCmd::Add {
            matter,
            name,
            role,
            password,
            password_env,
            actor,
            passphrase_env,
            json,
        } => {
            let root = utf8_path(&matter)?;
            let pass = resolve_password(password, password_env)?;
            let matter = open_matter_cli(&root, &passphrase_env)?;
            let user = matter.create_user(&name, &role, &pass, &actor)?;
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "service.user.add",
                    "user_id": user.id,
                    "display_name": user.display_name,
                    "role": user.role,
                })),
            )?;
            if !json {
                eprintln!(
                    "user add: {} ({}) role={}",
                    user.display_name, user.id, user.role
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        ServiceUserCmd::List {
            matter,
            passphrase_env,
            json,
        } => {
            let root = utf8_path(&matter)?;
            let matter = open_matter_cli(&root, &passphrase_env)?;
            let users = matter.list_users()?;
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "service.user.list",
                    "users": users.iter().map(|u| serde_json::json!({
                        "id": u.id,
                        "display_name": u.display_name,
                        "role": u.role,
                        "disabled_at": u.disabled_at,
                    })).collect::<Vec<_>>(),
                })),
            )?;
            if !json {
                for u in users {
                    let flag = if u.disabled_at.is_some() {
                        " [disabled]"
                    } else {
                        ""
                    };
                    println!("{}  {}  {}{flag}", u.id, u.display_name, u.role);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ServiceUserCmd::Disable {
            matter,
            id,
            actor,
            passphrase_env,
            json,
        } => {
            let root = utf8_path(&matter)?;
            let matter = open_matter_cli(&root, &passphrase_env)?;
            matter.disable_user(&id, &actor)?;
            emit_json(
                json,
                &ok_envelope(serde_json::json!({
                    "action": "service.user.disable",
                    "user_id": id,
                })),
            )?;
            if !json {
                eprintln!("user disable: {id}");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn resolve_password(password: Option<String>, password_env: Option<String>) -> Result<String> {
    if let Some(env_name) = password_env {
        return std::env::var(&env_name)
            .map_err(|_| CliError::Usage(format!("password env {env_name} is not set")));
    }
    password.ok_or_else(|| CliError::Usage("provide --password or --password-env".into()))
}

fn open_matter_cli(root: &Utf8PathBuf, passphrase_env: &str) -> Result<Matter> {
    // Admin CLI keeps strict actor **off** so bootstrap can use free-form "system"
    // before any matter_users exist. Service `serve` enables strict mode separately.
    if is_encrypted_matter(root) {
        let pass = std::env::var(passphrase_env).map_err(|_| {
            CliError::Usage(format!("encrypted matter requires env {passphrase_env}"))
        })?;
        Matter::open_with_passphrase(root, &pass, true).map_err(CliError::from)
    } else {
        Matter::open(root).map_err(CliError::from)
    }
}

fn utf8_path(p: &Path) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(p.to_path_buf())
        .map_err(|_| CliError::Usage(format!("path is not valid UTF-8: {}", p.display())))
}
