//! Shared service state: write gate + optional platform control plane.

use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_core::{Matter, ZeroizingString};
use matter_platform::Platform;
use tokio::sync::Mutex;

use crate::oidc::OidcRuntime;

/// Serialize all matter mutations through one open writer.
#[derive(Clone)]
pub struct WriteGate {
    inner: Arc<Mutex<Matter>>,
}

impl WriteGate {
    pub fn new(matter: Matter) -> Self {
        Self {
            inner: Arc::new(Mutex::new(matter)),
        }
    }

    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, Matter> {
        self.inner.lock().await
    }
}

/// Optional platform mode state (track 0059).
#[derive(Clone)]
pub struct PlatformState {
    pub platform: Arc<Mutex<Platform>>,
    /// Tenant that owns the currently hosted matter.
    pub tenant_id: String,
    pub tenant_slug: String,
    pub oidc: Arc<OidcRuntime>,
    /// Public base URL for building redirect_uri (e.g. http://127.0.0.1:7749).
    pub public_base: String,
    /// When true, password login is rejected for this tenant.
    pub oidc_required: bool,
}

/// CLI / host configuration for `serve`.
///
/// Passphrase is held as [`ZeroizingString`] and should be **moved out** on open
/// (not retained for the service lifetime). Debug redacts passphrase.
#[derive(Clone)]
pub struct ServeConfig {
    pub matter_root: Utf8PathBuf,
    pub bind: std::net::SocketAddr,
    pub allow_lan: bool,
    /// Explicit passphrase for encrypted matters (else env). Zeroized on drop;
    /// take ownership when opening so it is not retained after unlock.
    pub passphrase: Option<ZeroizingString>,
    /// Optional platform.db path (enables SSO / tenant isolation).
    pub platform_db: Option<Utf8PathBuf>,
    /// Explicit PMK material (else env `PST_DEDUPE_PLATFORM_MASTER_KEY`).
    pub platform_master_key: Option<matter_platform::Pmk>,
    /// Override storage roots (else `PLATFORM_STORAGE_ROOT` env).
    pub storage_roots: Vec<std::path::PathBuf>,
    /// Inject mock OIDC for tests (production uses default runtime).
    pub use_mock_oidc: bool,
}

impl std::fmt::Debug for ServeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServeConfig")
            .field("matter_root", &self.matter_root)
            .field("bind", &self.bind)
            .field("allow_lan", &self.allow_lan)
            .field(
                "passphrase",
                &self.passphrase.as_ref().map(|_| "<redacted>"),
            )
            .field("platform_db", &self.platform_db)
            .field(
                "platform_master_key",
                &self.platform_master_key.as_ref().map(|_| "<redacted>"),
            )
            .field("storage_roots", &self.storage_roots)
            .field("use_mock_oidc", &self.use_mock_oidc)
            .finish()
    }
}
