//! Shared service state: write gate + config.

use std::sync::Arc;

use camino::Utf8PathBuf;
use matter_core::Matter;
use tokio::sync::Mutex;

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

/// CLI / host configuration for `serve`.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub matter_root: Utf8PathBuf,
    pub bind: std::net::SocketAddr,
    pub allow_lan: bool,
    /// Explicit passphrase for encrypted matters (else env).
    pub passphrase: Option<String>,
}
