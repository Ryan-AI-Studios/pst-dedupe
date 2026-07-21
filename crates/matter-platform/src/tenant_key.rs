//! Tenant key provider hook for future per-tenant matter CMK / external KMS.
//!
//! **P0 (track 0059):** trait stub only — no cloud KMS implementation.
//! Distinct from the Platform Master Key (PMK) used for IdP client secrets.
//! Matter DEK unlock still uses the existing 0057 passphrase path.
//! Residual: D-0057-03 / D-0059 (per-tenant CMK).

use crate::error::Result;

/// Future provider for tenant-scoped matter key material.
///
/// Hosted pilots may later wrap matter DEKs under a tenant CMK. This trait is
/// the documented extension point; no production impl ships in 0059.
pub trait TenantKeyProvider: Send + Sync {
    /// Tenant id this provider is scoped to (or `"*"` for multi-tenant).
    fn tenant_id(&self) -> &str;

    /// Optionally unwrap or supply a 32-byte key for a matter.
    ///
    /// Default stub returns `None` (caller falls back to matter passphrase/DEK).
    fn matter_key(&self, _matter_id: &str) -> Result<Option<[u8; 32]>> {
        Ok(None)
    }
}

/// No-op provider used when platform CMK is not configured.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullTenantKeyProvider;

impl TenantKeyProvider for NullTenantKeyProvider {
    fn tenant_id(&self) -> &str {
        "*"
    }
}
