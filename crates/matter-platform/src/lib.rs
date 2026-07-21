//! # matter-platform
//!
//! Multi-tenant **control plane** for hosted / firm-shared deployments (track **0059**).
//!
//! - Separate `platform.db` (not case matter.db)
//! - Tenants, IdP configs (PMK-AEAD secrets or env-ref), matter registration
//! - `PLATFORM_STORAGE_ROOT` sandbox on register
//! - JIT allowlist helpers (domain and/or groups required when JIT on)
//!
//! **Isolation model:** matter boundary + registry — not a shared multi-tenant items table.
//! Desk solo and local password multi-user remain default when platform mode is off.

mod error;
mod pmk;
mod sandbox;
mod schema;
mod tenant_key;

pub use error::{Error, Result};
pub use pmk::{
    decrypt_idp_secret, encrypt_idp_secret, generate_pmk, load_pmk_from_env, parse_pmk,
    zeroize_string, DOMAIN_IDP_SECRET, ENV_PLATFORM_MASTER_KEY,
};
pub use sandbox::{assert_path_under_root, ENV_PLATFORM_STORAGE_ROOT};
pub use schema::PLATFORM_SCHEMA_VERSION;
pub use tenant_key::{NullTenantKeyProvider, TenantKeyProvider};

use std::path::{Path, PathBuf};

use camino::{Utf8Path, Utf8PathBuf};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Open platform registry handle.
pub struct Platform {
    conn: Connection,
    path: Utf8PathBuf,
    pmk: Option<[u8; 32]>,
    storage_roots: Vec<PathBuf>,
}

/// Tenant row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tenant {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub status: String,
    pub jit_provision: bool,
    pub oidc_required: bool,
    pub created_at: String,
}

/// IdP configuration for a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdpConfig {
    pub tenant_id: String,
    pub issuer_url: String,
    pub client_id: String,
    /// Env var name holding client secret (preferred over ciphertext).
    pub secret_env: Option<String>,
    /// True when AEAD ciphertext is stored (secret itself never exposed here).
    pub has_secret_ciphertext: bool,
    pub audiences: Vec<String>,
    /// Map of OIDC group/app-role claim value → matter role (`admin`/`reviewer`/`read_only`).
    pub role_claim_map: serde_json::Map<String, serde_json::Value>,
    pub allowed_email_domains: Vec<String>,
    pub required_groups: Vec<String>,
    pub enabled: bool,
    pub updated_at: String,
}

/// Registered matter under a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformMatter {
    pub id: String,
    pub tenant_id: String,
    pub matter_id: String,
    pub storage_root: String,
    pub status: String,
    pub registered_at: String,
}

/// Input for setting IdP config.
#[derive(Debug, Clone)]
pub struct SetIdpConfigInput {
    pub issuer_url: String,
    pub client_id: String,
    /// Prefer env-ref: store only the env var name.
    pub secret_env: Option<String>,
    /// Plaintext secret to AEAD-encrypt under PMK (mutually exclusive with secret_env prefer).
    pub secret_plaintext: Option<String>,
    pub audiences: Vec<String>,
    pub role_claim_map: serde_json::Map<String, serde_json::Value>,
    pub allowed_email_domains: Vec<String>,
    pub required_groups: Vec<String>,
    pub enabled: bool,
}

/// Pending OIDC login (PKCE) row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcPending {
    pub state: String,
    pub tenant_id: String,
    pub code_verifier: String,
    pub nonce: String,
    pub redirect_uri: String,
    pub expires_at: String,
    pub created_at: String,
}

impl Platform {
    /// Create a new platform.db at `path` (file path).
    pub fn create(path: &Utf8Path, pmk: Option<[u8; 32]>) -> Result<Self> {
        if path.exists() {
            return Err(Error::PlatformAlreadyExists(path.to_string()));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent.as_std_path())?;
        }
        let conn = Connection::open(path.as_str())?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;",
        )?;
        schema::migrate(&conn)?;
        let storage_roots = load_storage_roots_from_env();
        Ok(Self {
            conn,
            path: path.to_path_buf(),
            pmk,
            storage_roots,
        })
    }

    /// Open an existing platform.db.
    pub fn open(path: &Utf8Path, pmk: Option<[u8; 32]>) -> Result<Self> {
        if !path.exists() {
            return Err(Error::PlatformNotFound(path.to_string()));
        }
        let conn = Connection::open(path.as_str())?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;",
        )?;
        schema::migrate(&conn)?;
        let storage_roots = load_storage_roots_from_env();
        Ok(Self {
            conn,
            path: path.to_path_buf(),
            pmk,
            storage_roots,
        })
    }

    /// Create or open (tests / CLI convenience).
    pub fn open_or_create(path: &Utf8Path, pmk: Option<[u8; 32]>) -> Result<Self> {
        if path.exists() {
            Self::open(path, pmk)
        } else {
            Self::create(path, pmk)
        }
    }

    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub fn set_pmk(&mut self, pmk: Option<[u8; 32]>) {
        self.pmk = pmk;
    }

    pub fn pmk_present(&self) -> bool {
        self.pmk.is_some()
    }

    /// Override / set storage roots (also loaded from env at open).
    pub fn set_storage_roots(&mut self, roots: Vec<PathBuf>) {
        self.storage_roots = roots;
    }

    pub fn storage_roots(&self) -> &[PathBuf] {
        &self.storage_roots
    }

    // ------------------------------------------------------------------
    // Tenants
    // ------------------------------------------------------------------

    pub fn create_tenant(
        &self,
        slug: &str,
        display_name: &str,
        jit_provision: bool,
        oidc_required: bool,
    ) -> Result<Tenant> {
        let slug = normalize_slug(slug)?;
        let name = display_name.trim();
        if name.is_empty() {
            return Err(Error::Other("display_name must not be empty".into()));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        let res = self.conn.execute(
            "INSERT INTO tenants (id, slug, display_name, status, jit_provision, oidc_required, created_at) \
             VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6)",
            params![
                id,
                slug,
                name,
                if jit_provision { 1 } else { 0 },
                if oidc_required { 1 } else { 0 },
                now
            ],
        );
        match res {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(Error::TenantSlugExists(slug));
            }
            Err(e) => return Err(Error::from(e)),
        }
        self.get_tenant_by_id(&id)?
            .ok_or_else(|| Error::Other("tenant missing after create".into()))
    }

    pub fn get_tenant_by_id(&self, id: &str) -> Result<Option<Tenant>> {
        self.conn
            .query_row(
                "SELECT id, slug, display_name, status, jit_provision, oidc_required, created_at \
                 FROM tenants WHERE id = ?1",
                params![id],
                map_tenant,
            )
            .optional()
            .map_err(Error::from)
    }

    pub fn get_tenant_by_slug(&self, slug: &str) -> Result<Option<Tenant>> {
        let slug = slug.trim().to_ascii_lowercase();
        self.conn
            .query_row(
                "SELECT id, slug, display_name, status, jit_provision, oidc_required, created_at \
                 FROM tenants WHERE lower(slug) = ?1",
                params![slug],
                map_tenant,
            )
            .optional()
            .map_err(Error::from)
    }

    pub fn list_tenants(&self) -> Result<Vec<Tenant>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, display_name, status, jit_provision, oidc_required, created_at \
             FROM tenants ORDER BY slug",
        )?;
        let rows = stmt.query_map([], map_tenant)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    // ------------------------------------------------------------------
    // IdP config
    // ------------------------------------------------------------------

    pub fn set_idp_config(&self, tenant_id: &str, input: SetIdpConfigInput) -> Result<IdpConfig> {
        let tenant = self
            .get_tenant_by_id(tenant_id)?
            .ok_or_else(|| Error::TenantNotFound(tenant_id.to_string()))?;

        // JIT open forbid: if tenant has jit_provision, config must have allowlist.
        if tenant.jit_provision {
            validate_jit_config(&input.allowed_email_domains, &input.required_groups)?;
        }

        let issuer = input.issuer_url.trim();
        let client_id = input.client_id.trim();
        if issuer.is_empty() || client_id.is_empty() {
            return Err(Error::Other(
                "issuer_url and client_id must not be empty".into(),
            ));
        }

        let secret_env = input
            .secret_env
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let (secret_ciphertext, secret_nonce): (Option<Vec<u8>>, Option<Vec<u8>>) =
            if let Some(plain) = input.secret_plaintext.as_deref().filter(|s| !s.is_empty()) {
                let pmk = self.pmk.as_ref().ok_or(Error::PmkRequired)?;
                let (nonce, ct) = encrypt_idp_secret(pmk, plain.as_bytes())?;
                (Some(ct), Some(nonce))
            } else {
                (None, None)
            };

        if secret_env.is_none() && secret_ciphertext.is_none() {
            return Err(Error::Other(
                "IdP config requires secret_env or secret_plaintext".into(),
            ));
        }

        let audiences_json = serde_json::to_string(&input.audiences)?;
        let role_map_json = serde_json::Value::Object(input.role_claim_map.clone()).to_string();
        let domains_json = serde_json::to_string(&input.allowed_email_domains)?;
        let groups_json = serde_json::to_string(&input.required_groups)?;
        let now = now_rfc3339();

        self.conn.execute(
            "INSERT INTO tenant_idp_configs (
                tenant_id, issuer_url, client_id, secret_env, secret_ciphertext, secret_nonce,
                audiences_json, role_claim_map_json, allowed_email_domains_json, required_groups_json,
                enabled, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(tenant_id) DO UPDATE SET
                issuer_url = excluded.issuer_url,
                client_id = excluded.client_id,
                secret_env = excluded.secret_env,
                secret_ciphertext = excluded.secret_ciphertext,
                secret_nonce = excluded.secret_nonce,
                audiences_json = excluded.audiences_json,
                role_claim_map_json = excluded.role_claim_map_json,
                allowed_email_domains_json = excluded.allowed_email_domains_json,
                required_groups_json = excluded.required_groups_json,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at",
            params![
                tenant_id,
                issuer,
                client_id,
                secret_env,
                secret_ciphertext,
                secret_nonce,
                audiences_json,
                role_map_json,
                domains_json,
                groups_json,
                if input.enabled { 1 } else { 0 },
                now
            ],
        )?;

        self.get_idp_config(tenant_id)?
            .ok_or_else(|| Error::Other("idp config missing after set".into()))
    }

    pub fn get_idp_config(&self, tenant_id: &str) -> Result<Option<IdpConfig>> {
        self.conn
            .query_row(
                "SELECT tenant_id, issuer_url, client_id, secret_env, secret_ciphertext, secret_nonce,
                        audiences_json, role_claim_map_json, allowed_email_domains_json,
                        required_groups_json, enabled, updated_at
                 FROM tenant_idp_configs WHERE tenant_id = ?1",
                params![tenant_id],
                map_idp,
            )
            .optional()
            .map_err(Error::from)
    }

    /// Resolve client secret: env-ref preferred, else PMK decrypt ciphertext.
    pub fn resolve_client_secret(&self, tenant_id: &str) -> Result<String> {
        type SecretRow = (Option<String>, Option<Vec<u8>>, Option<Vec<u8>>);
        let row: Option<SecretRow> = self
            .conn
            .query_row(
                "SELECT secret_env, secret_ciphertext, secret_nonce \
                 FROM tenant_idp_configs WHERE tenant_id = ?1",
                params![tenant_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((secret_env, ct, nonce)) = row else {
            return Err(Error::SecretUnavailable);
        };
        if let Some(env_name) = secret_env.as_deref().filter(|s| !s.is_empty()) {
            return std::env::var(env_name).map_err(|_| Error::SecretUnavailable);
        }
        let ct = ct.ok_or(Error::SecretUnavailable)?;
        let nonce = nonce.ok_or(Error::SecretUnavailable)?;
        let pmk = self.pmk.as_ref().ok_or(Error::PmkRequired)?;
        let plain = decrypt_idp_secret(pmk, &nonce, &ct)?;
        String::from_utf8(plain).map_err(|e| Error::Crypto(format!("secret utf8: {e}")))
    }

    // ------------------------------------------------------------------
    // Matter registration
    // ------------------------------------------------------------------

    /// Register a matter path under a tenant (sandbox enforced).
    pub fn register_matter(
        &self,
        tenant_id: &str,
        matter_id: &str,
        storage_root: &Path,
    ) -> Result<PlatformMatter> {
        let _tenant = self
            .get_tenant_by_id(tenant_id)?
            .ok_or_else(|| Error::TenantNotFound(tenant_id.to_string()))?;
        let matter_id = matter_id.trim();
        if matter_id.is_empty() {
            return Err(Error::Other("matter_id must not be empty".into()));
        }
        if self.storage_roots.is_empty() {
            return Err(Error::Other(format!(
                "set {ENV_PLATFORM_STORAGE_ROOT} (or Platform::set_storage_roots) before register"
            )));
        }
        let mut validated: Option<PathBuf> = None;
        let mut last_err = None;
        for root in &self.storage_roots {
            match assert_path_under_root(storage_root, root) {
                Ok(p) => {
                    validated = Some(p);
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let path = validated.ok_or_else(|| {
            last_err.unwrap_or_else(|| {
                Error::PathNotSandboxed(format!(
                    "path not under any storage root: {}",
                    storage_root.display()
                ))
            })
        })?;
        // Reject registering a regular file (including platform.db) as a matter root.
        if path.is_file() {
            return Err(Error::PathNotSandboxed(format!(
                "matter storage path must be a directory, not a file: {}",
                path.display()
            )));
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.eq_ignore_ascii_case("platform.db") || name.eq_ignore_ascii_case("matter.db") {
                return Err(Error::PathNotSandboxed(format!(
                    "refusing to register database file as matter path: {}",
                    path.display()
                )));
            }
        }
        let storage_str = path.to_string_lossy().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_rfc3339();
        let res = self.conn.execute(
            "INSERT INTO platform_matters (id, tenant_id, matter_id, storage_root, status, registered_at) \
             VALUES (?1, ?2, ?3, ?4, 'active', ?5)",
            params![id, tenant_id, matter_id, storage_str, now],
        );
        match res {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(Error::Conflict(
                    "matter already registered (tenant/matter_id or storage_root)".into(),
                ));
            }
            Err(e) => return Err(Error::from(e)),
        }
        self.get_matter_registration_by_row_id(&id)?
            .ok_or_else(|| Error::Other("registration missing after insert".into()))
    }

    pub fn list_matters(&self, tenant_id: &str) -> Result<Vec<PlatformMatter>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, tenant_id, matter_id, storage_root, status, registered_at \
             FROM platform_matters WHERE tenant_id = ?1 ORDER BY registered_at, id",
        )?;
        let rows = stmt.query_map(params![tenant_id], map_platform_matter)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    pub fn get_matter_registration(
        &self,
        tenant_id: &str,
        matter_id: &str,
    ) -> Result<Option<PlatformMatter>> {
        self.conn
            .query_row(
                "SELECT id, tenant_id, matter_id, storage_root, status, registered_at \
                 FROM platform_matters WHERE tenant_id = ?1 AND matter_id = ?2",
                params![tenant_id, matter_id],
                map_platform_matter,
            )
            .optional()
            .map_err(Error::from)
    }

    pub fn get_matter_by_path(
        &self,
        tenant_id: &str,
        storage_root: &Path,
    ) -> Result<Option<PlatformMatter>> {
        let candidates = path_lookup_keys(storage_root);
        let mut stmt = self.conn.prepare(
            "SELECT id, tenant_id, matter_id, storage_root, status, registered_at \
             FROM platform_matters WHERE tenant_id = ?1",
        )?;
        let rows = stmt.query_map(params![tenant_id], map_platform_matter)?;
        for row in rows {
            let m = row?;
            if candidates
                .iter()
                .any(|c| path_str_eq(&m.storage_root, c.as_str()))
            {
                return Ok(Some(m));
            }
        }
        Ok(None)
    }

    pub fn find_registration_by_path(&self, storage_root: &Path) -> Result<Option<PlatformMatter>> {
        let candidates = path_lookup_keys(storage_root);
        let mut stmt = self.conn.prepare(
            "SELECT id, tenant_id, matter_id, storage_root, status, registered_at \
             FROM platform_matters",
        )?;
        let rows = stmt.query_map([], map_platform_matter)?;
        for row in rows {
            let m = row?;
            if candidates
                .iter()
                .any(|c| path_str_eq(&m.storage_root, c.as_str()))
            {
                return Ok(Some(m));
            }
        }
        Ok(None)
    }

    /// Re-validate a registered storage path against current PLATFORM_STORAGE_ROOT(s).
    ///
    /// Fail closed if no roots are configured or the stored path is outside them.
    pub fn assert_registered_path_still_sandboxed(&self, storage_root: &str) -> Result<PathBuf> {
        if self.storage_roots.is_empty() {
            return Err(Error::Other(format!(
                "set {ENV_PLATFORM_STORAGE_ROOT} before opening a platform-hosted matter"
            )));
        }
        let path = PathBuf::from(storage_root);
        let mut last_err = None;
        for root in &self.storage_roots {
            match assert_path_under_root(&path, root) {
                Ok(p) => {
                    if p.is_file() {
                        return Err(Error::PathNotSandboxed(format!(
                            "registered matter path is a file, not a directory: {}",
                            p.display()
                        )));
                    }
                    return Ok(p);
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            Error::PathNotSandboxed(format!(
                "registered path not under current storage roots: {storage_root}"
            ))
        }))
    }

    fn get_matter_registration_by_row_id(&self, id: &str) -> Result<Option<PlatformMatter>> {
        self.conn
            .query_row(
                "SELECT id, tenant_id, matter_id, storage_root, status, registered_at \
                 FROM platform_matters WHERE id = ?1",
                params![id],
                map_platform_matter,
            )
            .optional()
            .map_err(Error::from)
    }

    /// Fail closed unless path is registered to `tenant_id`.
    pub fn assert_tenant_owns_path(&self, tenant_id: &str, path: &Path) -> Result<PlatformMatter> {
        match self.get_matter_by_path(tenant_id, path)? {
            Some(m) if m.status == "active" => Ok(m),
            Some(_) => Err(Error::Forbidden("matter registration not active".into())),
            None => Err(Error::MatterNotRegistered),
        }
    }

    // ------------------------------------------------------------------
    // OIDC pending (PKCE state)
    // ------------------------------------------------------------------

    pub fn store_oidc_pending(
        &self,
        state: &str,
        tenant_id: &str,
        code_verifier: &str,
        nonce: &str,
        redirect_uri: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        let now = Utc::now();
        let expires = now + chrono::Duration::seconds(ttl_secs.max(30));
        self.conn.execute(
            "INSERT INTO oidc_pending (state, tenant_id, code_verifier, nonce, redirect_uri, expires_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                state,
                tenant_id,
                code_verifier,
                nonce,
                redirect_uri,
                expires.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            ],
        )?;
        Ok(())
    }

    /// Take (single-use) pending OIDC state; fail closed if missing/expired.
    pub fn take_oidc_pending(&self, state: &str) -> Result<OidcPending> {
        let now = now_rfc3339();
        let row: Option<OidcPending> = self
            .conn
            .query_row(
                "SELECT state, tenant_id, code_verifier, nonce, redirect_uri, expires_at, created_at \
                 FROM oidc_pending WHERE state = ?1",
                params![state],
                |r| {
                    Ok(OidcPending {
                        state: r.get(0)?,
                        tenant_id: r.get(1)?,
                        code_verifier: r.get(2)?,
                        nonce: r.get(3)?,
                        redirect_uri: r.get(4)?,
                        expires_at: r.get(5)?,
                        created_at: r.get(6)?,
                    })
                },
            )
            .optional()?;
        let Some(pending) = row else {
            return Err(Error::Forbidden("invalid or unknown OIDC state".into()));
        };
        self.conn
            .execute("DELETE FROM oidc_pending WHERE state = ?1", params![state])?;
        if pending.expires_at.as_str() <= now.as_str() {
            return Err(Error::Forbidden("OIDC state expired".into()));
        }
        Ok(pending)
    }
}

// ---------------------------------------------------------------------------
// JIT allowlist
// ---------------------------------------------------------------------------

/// When JIT is enabled, at least one allowlist must be non-empty.
pub fn validate_jit_config(domains: &[String], groups: &[String]) -> Result<()> {
    let has_domain = domains.iter().any(|d| !d.trim().is_empty());
    let has_group = groups.iter().any(|g| !g.trim().is_empty());
    if !has_domain && !has_group {
        return Err(Error::JitOpenForbidden);
    }
    Ok(())
}

/// Return true if email domain / groups satisfy JIT allowlist.
///
/// - If domains non-empty: email's domain must match (case-insensitive).
/// - If groups non-empty: at least one required group must be present.
/// - If both non-empty: **both** domain and group conditions must pass.
/// - Empty allowlists → false (open JIT forbidden at validation time too).
pub fn jit_allowed(email: Option<&str>, groups: &[String], config: &IdpConfig) -> bool {
    let domains: Vec<String> = config
        .allowed_email_domains
        .iter()
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    let required: Vec<String> = config
        .required_groups
        .iter()
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect();
    if domains.is_empty() && required.is_empty() {
        return false;
    }
    let domain_ok = if domains.is_empty() {
        true
    } else {
        email
            .and_then(|e| e.rsplit_once('@').map(|(_, d)| d.to_ascii_lowercase()))
            .map(|d| domains.iter().any(|a| a == &d))
            .unwrap_or(false)
    };
    let group_ok = if required.is_empty() {
        true
    } else {
        required.iter().any(|req| {
            groups
                .iter()
                .any(|g| g.trim().eq_ignore_ascii_case(req.as_str()))
        })
    };
    domain_ok && group_ok
}

/// Map OIDC groups through role_claim_map; default `reviewer`.
pub fn map_role_from_claims(
    groups: &[String],
    role_claim_map: &serde_json::Map<String, serde_json::Value>,
    default_role: &str,
) -> String {
    for g in groups {
        if let Some(v) = role_claim_map.get(g.as_str()) {
            if let Some(s) = v.as_str() {
                let r = s.trim();
                if matches!(r, "admin" | "reviewer" | "read_only") {
                    return r.to_string();
                }
            }
        }
    }
    let d = default_role.trim();
    if matches!(d, "admin" | "reviewer" | "read_only") {
        d.to_string()
    } else {
        "reviewer".into()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_storage_roots_from_env() -> Vec<PathBuf> {
    match std::env::var(ENV_PLATFORM_STORAGE_ROOT) {
        Ok(v) => {
            let t = v.trim();
            if t.is_empty() {
                Vec::new()
            } else {
                // Support path-list via `;` on Windows / `:` style is ambiguous — use `;` only.
                t.split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            }
        }
        Err(_) => Vec::new(),
    }
}

fn normalize_slug(slug: &str) -> Result<String> {
    let s = slug.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err(Error::Other("slug must not be empty".into()));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(Error::Other(
            "slug must be lowercase alphanumeric with - or _".into(),
        ));
    }
    Ok(s)
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn map_tenant(row: &rusqlite::Row<'_>) -> rusqlite::Result<Tenant> {
    let jit: i64 = row.get(4)?;
    let oidc: i64 = row.get(5)?;
    Ok(Tenant {
        id: row.get(0)?,
        slug: row.get(1)?,
        display_name: row.get(2)?,
        status: row.get(3)?,
        jit_provision: jit != 0,
        oidc_required: oidc != 0,
        created_at: row.get(6)?,
    })
}

fn map_idp(row: &rusqlite::Row<'_>) -> rusqlite::Result<IdpConfig> {
    let secret_env: Option<String> = row.get(3)?;
    let ct: Option<Vec<u8>> = row.get(4)?;
    let audiences_json: String = row.get(6)?;
    let role_json: String = row.get(7)?;
    let domains_json: String = row.get(8)?;
    let groups_json: String = row.get(9)?;
    let enabled: i64 = row.get(10)?;
    // Fail closed on malformed JSON — never silently empty allowlists/audiences.
    let audiences: Vec<String> = parse_json_vec(&audiences_json, "audiences_json")?;
    let role_claim_map = parse_json_object(&role_json, "role_claim_map_json")?;
    let allowed_email_domains: Vec<String> =
        parse_json_vec(&domains_json, "allowed_email_domains_json")?;
    let required_groups: Vec<String> = parse_json_vec(&groups_json, "required_groups_json")?;
    Ok(IdpConfig {
        tenant_id: row.get(0)?,
        issuer_url: row.get(1)?,
        client_id: row.get(2)?,
        secret_env,
        has_secret_ciphertext: ct.as_ref().map(|c| !c.is_empty()).unwrap_or(false),
        audiences,
        role_claim_map,
        allowed_email_domains,
        required_groups,
        enabled: enabled != 0,
        updated_at: row.get(11)?,
    })
}

fn parse_json_vec(raw: &str, field: &str) -> rusqlite::Result<Vec<String>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(t).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("malformed {field}: {e}"),
            )),
        )
    })
}

fn parse_json_object(
    raw: &str,
    field: &str,
) -> rusqlite::Result<serde_json::Map<String, serde_json::Value>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(serde_json::Map::new());
    }
    let val: serde_json::Value = serde_json::from_str(t).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("malformed {field}: {e}"),
            )),
        )
    })?;
    match val {
        serde_json::Value::Object(m) => Ok(m),
        serde_json::Value::Null => Ok(serde_json::Map::new()),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{field} must be a JSON object, got {other}"),
            )),
        )),
    }
}

fn map_platform_matter(row: &rusqlite::Row<'_>) -> rusqlite::Result<PlatformMatter> {
    Ok(PlatformMatter {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        matter_id: row.get(2)?,
        storage_root: row.get(3)?,
        status: row.get(4)?,
        registered_at: row.get(5)?,
    })
}

fn path_str_eq(a: &str, b: &str) -> bool {
    #[cfg(windows)]
    {
        // Normalize separators for Windows comparisons.
        let na = a.replace('/', "\\");
        let nb = b.replace('/', "\\");
        na.eq_ignore_ascii_case(&nb)
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// Keys to try when matching a client path against a registered storage_root.
fn path_lookup_keys(path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    out.push(path.to_string_lossy().into_owned());
    if let Ok(can) = std::fs::canonicalize(path) {
        let s = can.to_string_lossy().into_owned();
        // Strip Windows `\\?\` extended prefix for equality with non-extended forms.
        #[cfg(windows)]
        {
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                out.push(stripped.to_string());
            }
        }
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn utf8_tmp() -> (tempfile::TempDir, Utf8PathBuf) {
        let d = tempdir().expect("tmp");
        let p = Utf8PathBuf::from_path_buf(d.path().to_path_buf()).expect("utf8");
        (d, p)
    }

    #[test]
    fn registry_crud_and_isolation() {
        let (_tmp, base) = utf8_tmp();
        let storage = base.join("matters");
        std::fs::create_dir_all(storage.as_std_path()).expect("mkdir");
        let db = base.join("platform.db");
        let pmk = generate_pmk();
        let mut plat = Platform::create(&db, Some(pmk)).expect("create");
        plat.set_storage_roots(vec![storage.as_std_path().to_path_buf()]);

        let a = plat
            .create_tenant("firm-a", "Firm A", true, false)
            .expect("a");
        let b = plat
            .create_tenant("firm-b", "Firm B", false, true)
            .expect("b");

        // JIT without allowlist on set_idp should fail for firm-a (jit on)
        let bad = plat.set_idp_config(
            &a.id,
            SetIdpConfigInput {
                issuer_url: "https://idp.example/a".into(),
                client_id: "client-a".into(),
                secret_env: Some("SECRET_A".into()),
                secret_plaintext: None,
                audiences: vec!["client-a".into()],
                role_claim_map: Default::default(),
                allowed_email_domains: vec![],
                required_groups: vec![],
                enabled: true,
            },
        );
        assert!(matches!(bad, Err(Error::JitOpenForbidden)));

        plat.set_idp_config(
            &a.id,
            SetIdpConfigInput {
                issuer_url: "https://idp.example/a".into(),
                client_id: "client-a".into(),
                secret_env: Some("SECRET_A".into()),
                secret_plaintext: None,
                audiences: vec!["client-a".into()],
                role_claim_map: Default::default(),
                allowed_email_domains: vec!["firma.com".into()],
                required_groups: vec![],
                enabled: true,
            },
        )
        .expect("idp a");

        // Ciphertext path
        plat.set_idp_config(
            &b.id,
            SetIdpConfigInput {
                issuer_url: "https://idp.example/b".into(),
                client_id: "client-b".into(),
                secret_env: None,
                secret_plaintext: Some("plain-secret-value-xyz".into()),
                audiences: vec!["client-b".into()],
                role_claim_map: Default::default(),
                allowed_email_domains: vec![],
                required_groups: vec![],
                enabled: true,
            },
        )
        .expect("idp b");

        // Secret not plaintext on disk
        let raw = std::fs::read(db.as_std_path()).expect("read db");
        assert!(
            !raw.windows(b"plain-secret-value-xyz".len())
                .any(|w| w == b"plain-secret-value-xyz"),
            "plaintext secret must not appear in platform.db"
        );

        let case_a = storage.join("firm-a").join("case1");
        std::fs::create_dir_all(case_a.as_std_path()).expect("mkdir case");
        let reg = plat
            .register_matter(&a.id, "matter-a1", case_a.as_std_path())
            .expect("reg");
        assert_eq!(reg.tenant_id, a.id);

        let listed_a = plat.list_matters(&a.id).expect("list a");
        assert_eq!(listed_a.len(), 1);
        let listed_b = plat.list_matters(&b.id).expect("list b");
        assert!(
            listed_b.is_empty(),
            "foreign tenant must not see firm-a matter"
        );

        assert!(plat
            .assert_tenant_owns_path(&b.id, case_a.as_std_path())
            .is_err());
        assert!(plat
            .assert_tenant_owns_path(&a.id, case_a.as_std_path())
            .is_ok());
    }

    #[test]
    fn bad_path_rejected() {
        let (_tmp, base) = utf8_tmp();
        let storage = base.join("matters");
        std::fs::create_dir_all(storage.as_std_path()).expect("mkdir");
        let db = base.join("platform.db");
        let mut plat = Platform::create(&db, None).expect("create");
        plat.set_storage_roots(vec![storage.as_std_path().to_path_buf()]);
        let t = plat
            .create_tenant("firm-a", "Firm A", false, false)
            .expect("t");
        let foreign = base.join("outside");
        std::fs::create_dir_all(foreign.as_std_path()).expect("mkdir");
        let err = plat
            .register_matter(&t.id, "m1", foreign.as_std_path())
            .expect_err("outside");
        assert!(matches!(err, Error::PathNotSandboxed(_)));
    }

    #[test]
    fn jit_domain_check() {
        let cfg = IdpConfig {
            tenant_id: "t".into(),
            issuer_url: "https://idp".into(),
            client_id: "c".into(),
            secret_env: None,
            has_secret_ciphertext: false,
            audiences: vec![],
            role_claim_map: Default::default(),
            allowed_email_domains: vec!["firma.com".into()],
            required_groups: vec![],
            enabled: true,
            updated_at: String::new(),
        };
        assert!(jit_allowed(Some("bob@firma.com"), &[], &cfg));
        assert!(!jit_allowed(Some("bob@firmb.com"), &[], &cfg));
        assert!(!jit_allowed(None, &[], &cfg));
    }

    #[test]
    fn resolve_secret_env_and_ciphertext() {
        let (_tmp, base) = utf8_tmp();
        let db = base.join("platform.db");
        let pmk = generate_pmk();
        let plat = Platform::create(&db, Some(pmk)).expect("create");
        let t = plat
            .create_tenant("firm-x", "Firm X", false, false)
            .expect("t");
        plat.set_idp_config(
            &t.id,
            SetIdpConfigInput {
                issuer_url: "https://idp".into(),
                client_id: "c".into(),
                secret_env: None,
                secret_plaintext: Some("ciphertext-secret-abc".into()),
                audiences: vec!["c".into()],
                role_claim_map: Default::default(),
                allowed_email_domains: vec![],
                required_groups: vec![],
                enabled: true,
            },
        )
        .expect("set");
        let s = plat.resolve_client_secret(&t.id).expect("resolve");
        assert_eq!(s, "ciphertext-secret-abc");
    }

    #[test]
    fn open_revalidates_storage_root_and_rejects_db_file() {
        let (_tmp, base) = utf8_tmp();
        let storage = base.join("matters");
        std::fs::create_dir_all(storage.as_std_path()).expect("mkdir");
        let db = base.join("platform.db");
        let mut plat = Platform::create(&db, None).expect("create");
        plat.set_storage_roots(vec![storage.as_std_path().to_path_buf()]);
        let t = plat
            .create_tenant("firm-a", "Firm A", false, false)
            .expect("t");
        let case = storage.join("case1");
        std::fs::create_dir_all(case.as_std_path()).expect("mkdir");
        let reg = plat
            .register_matter(&t.id, "c1", case.as_std_path())
            .expect("reg");
        // Still under root → ok
        plat.assert_registered_path_still_sandboxed(&reg.storage_root)
            .expect("still ok");
        // Clear roots → fail closed
        plat.set_storage_roots(vec![]);
        assert!(plat
            .assert_registered_path_still_sandboxed(&reg.storage_root)
            .is_err());
        // File registration rejected
        plat.set_storage_roots(vec![storage.as_std_path().to_path_buf()]);
        let file_path = storage.join("platform.db");
        std::fs::write(file_path.as_std_path(), b"not-a-matter").expect("write");
        let err = plat
            .register_matter(&t.id, "dbfile", file_path.as_std_path())
            .expect_err("file");
        assert!(matches!(err, Error::PathNotSandboxed(_)));
    }

    #[test]
    fn oidc_pending_is_single_use() {
        let (_tmp, base) = utf8_tmp();
        let db = base.join("platform.db");
        let plat = Platform::create(&db, None).expect("create");
        let t = plat
            .create_tenant("firm-a", "Firm A", false, false)
            .expect("t");
        plat.store_oidc_pending("state-1", &t.id, "ver", "nonce", "http://cb", 600)
            .expect("store");
        let p = plat.take_oidc_pending("state-1").expect("take1");
        assert_eq!(p.tenant_id, t.id);
        assert!(plat.take_oidc_pending("state-1").is_err());
    }
}
