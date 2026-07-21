//! Local multi-user **matter service** (track 0058) + optional platform SSO (0059).
//!
//! One process owns a write-open [`matter_core::Matter`] (exclusive OS lock) and
//! exposes a loopback HTTP API for concurrent reviewers. Solo Desk path is
//! unchanged — this crate is opt-in only.
//!
//! ## Architecture
//!
//! - **WriteGate:** `tokio::sync::Mutex<Matter>` serializes mutates
//! - **Auth:** matter-local users + bearer sessions; optional OIDC (Auth Code + PKCE)
//! - **OCC:** mutates require `expected_version` → 409 on stale
//! - **Locks / batches:** fail closed on foreign lock; batch feed is membership-constrained
//! - **Bind:** default `127.0.0.1`; non-loopback requires `allow_lan`
//! - **Strict actor:** service open sets `Matter::set_strict_actor_mode(true)`; body `actor` ignored
//! - **Platform mode (opt-in):** `platform.db` registry + tenant isolation + OIDC

mod auth;
mod error;
mod oidc;
mod routes;
mod state;

pub use error::{ApiError, ApiErrorBody};
pub use oidc::{
    complete_oidc_login, pkce_challenge_s256, random_urlsafe, validate_claims, MockOidcProvider,
    OidcClaims, OidcProvider, OidcRuntime, OpenIdConnectProvider,
};
pub use routes::{build_router, AppState};
pub use state::{PlatformState, ServeConfig, WriteGate};

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use camino::Utf8Path;
use matter_core::{is_encrypted_matter, passphrase_from_env, Matter, ENV_MATTER_PASSPHRASE};
use matter_platform::{load_pmk_from_env, Platform};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::info;

use crate::state::WriteGate as Wg;

/// Validate bind address policy: non-loopback requires `allow_lan`.
pub fn validate_bind(addr: SocketAddr, allow_lan: bool) -> Result<(), String> {
    if is_loopback(&addr.ip()) {
        return Ok(());
    }
    if allow_lan {
        return Ok(());
    }
    Err(format!(
        "refusing to bind non-loopback address {addr}; pass --allow-lan to enable LAN bind"
    ))
}

fn is_loopback(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// Default bind: loopback port 7749.
pub fn default_bind() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7749)
}

/// Open matter for service host: write open + multi-user required + strict actor.
///
/// Fails closed if `multi_user_enabled` is not set (use `service bootstrap-admin` first).
/// This ensures lock/batch/OCC multi-user guards are active for authenticated mutates.
pub fn open_matter_for_service(
    root: &Utf8Path,
    passphrase: Option<&str>,
) -> matter_core::Result<Matter> {
    let matter = if is_encrypted_matter(root) {
        let pass = match passphrase {
            Some(p) => p.to_string(),
            None => passphrase_from_env().ok_or_else(|| {
                matter_core::Error::PassphraseRequired(ENV_MATTER_PASSPHRASE.to_string())
            })?,
        };
        Matter::open_with_passphrase(root, &pass, true)?
    } else {
        Matter::open(root)?
    };
    if !matter.is_multi_user_enabled()? {
        return Err(matter_core::Error::Other(
            "multi-user is not enabled on this matter; run `pst-dedup service bootstrap-admin` first"
                .into(),
        ));
    }
    matter.set_strict_actor_mode(true);
    Ok(matter)
}

/// Build the axum router for an already-open matter (tests + serve).
pub fn router_from_matter(matter: Matter) -> Router {
    let state = AppState {
        gate: Wg::new(matter),
        platform: None,
    };
    build_router(state)
}

/// Build router with optional platform state (OIDC + tenant isolation).
pub fn router_from_state(state: AppState) -> Router {
    build_router(state)
}

/// Open platform + matter for hosted serve; validates registry membership.
pub fn open_platform_for_service(
    config: &ServeConfig,
) -> Result<(Matter, PlatformState), Box<dyn std::error::Error + Send + Sync>> {
    let platform_path = config
        .platform_db
        .as_ref()
        .ok_or("platform_db required for platform mode")?;
    let pmk = match config.platform_master_key {
        Some(k) => Some(k),
        None => load_pmk_from_env()?,
    };
    let mut platform = Platform::open(platform_path, pmk)?;
    if !config.storage_roots.is_empty() {
        platform.set_storage_roots(config.storage_roots.clone());
    }
    // Fail closed: platform open always requires a configured storage root.
    if platform.storage_roots().is_empty() {
        return Err(format!(
            "set {} before platform serve",
            matter_platform::ENV_PLATFORM_STORAGE_ROOT
        )
        .into());
    }

    // Resolve registration for the matter path (fail closed if not registered).
    let reg = platform
        .find_registration_by_path(config.matter_root.as_std_path())?
        .ok_or_else(|| {
            format!(
                "matter path not registered in platform.db: {}",
                config.matter_root
            )
        })?;
    // Re-validate the **registered** path under current roots; open only that path.
    let open_path = platform.assert_registered_path_still_sandboxed(&reg.storage_root)?;
    let open_root = camino::Utf8PathBuf::from_path_buf(open_path).map_err(|_| {
        format!(
            "registered matter path is not valid UTF-8: {}",
            reg.storage_root
        )
    })?;
    let tenant = platform
        .get_tenant_by_id(&reg.tenant_id)?
        .ok_or_else(|| format!("tenant {} missing for registration", reg.tenant_id))?;

    // If any IdP ciphertext present, PMK must be available.
    if let Some(idp) = platform.get_idp_config(&tenant.id)? {
        if idp.has_secret_ciphertext && !platform.pmk_present() {
            return Err(
                "platform IdP ciphertext present; set PST_DEDUPE_PLATFORM_MASTER_KEY".into(),
            );
        }
    }

    let matter = open_matter_for_service(&open_root, config.passphrase.as_deref())?;
    // Bind matter tenant_id to registry tenant.
    matter.set_matter_tenant_id(Some(&tenant.id))?;

    let oidc = if config.use_mock_oidc {
        Arc::new(OidcRuntime::mock())
    } else {
        Arc::new(OidcRuntime::production())
    };

    let public_base = format!("http://{}", config.bind);
    let ps = PlatformState {
        platform: Arc::new(Mutex::new(platform)),
        tenant_id: tenant.id,
        tenant_slug: tenant.slug,
        oidc,
        public_base,
        oidc_required: tenant.oidc_required,
    };
    Ok((matter, ps))
}

/// Serve until Ctrl-C, then drop the matter (seals encrypted session).
pub async fn serve(config: ServeConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    validate_bind(config.bind, config.allow_lan)?;
    let (matter, platform) = if config.platform_db.is_some() {
        let (m, p) = open_platform_for_service(&config)?;
        (m, Some(p))
    } else {
        (
            open_matter_for_service(&config.matter_root, config.passphrase.as_deref())?,
            None,
        )
    };
    let gate = Wg::new(matter);
    let app = build_router(AppState {
        gate: gate.clone(),
        platform,
    });
    let listener = TcpListener::bind(config.bind).await?;
    let local = listener.local_addr()?;
    info!(%local, root = %config.matter_root, "matter-service listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            info!("matter-service shutdown signal received");
        })
        .await?;
    // Drop write gate after serve ends so encrypted matters seal cleanly.
    drop(gate);
    info!(root = %config.matter_root, "matter-service stopped (matter handle released)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn default_bind_is_loopback() {
        let a = default_bind();
        assert!(a.ip().is_loopback());
        assert_eq!(a.port(), 7749);
    }

    #[test]
    fn reject_lan_without_flag() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 7749);
        assert!(validate_bind(addr, false).is_err());
        assert!(validate_bind(addr, true).is_ok());
    }

    #[test]
    fn loopback_always_ok() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        assert!(validate_bind(addr, false).is_ok());
    }
}
