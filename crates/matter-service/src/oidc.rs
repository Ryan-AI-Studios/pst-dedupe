//! OIDC Authorization Code + PKCE (track 0059).
//!
//! Production path uses `openidconnect` discovery + code exchange + ID-token
//! verification (iss/aud/exp/nonce/JWKS). Tests inject [`MockOidcProvider`].

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use base64::Engine;
use matter_platform::{jit_allowed, map_role_from_claims, IdpConfig, Platform};
use openidconnect::core::{CoreClient, CoreIdTokenClaims, CoreProviderMetadata, CoreTokenResponse};
use openidconnect::reqwest;
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, IssuerUrl, Nonce, PkceCodeVerifier, RedirectUrl,
    TokenResponse,
};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::error::{ApiError, ApiResult};

/// Boxed async result for the OIDC provider trait object.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Claims produced after a successful code exchange + ID token validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcClaims {
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub preferred_username: Option<String>,
    pub groups: Vec<String>,
    pub audience: Vec<String>,
    pub nonce: Option<String>,
    pub exp: i64,
    /// Issue time (unix seconds), when known from a verified ID token.
    #[serde(default)]
    pub iat: Option<i64>,
}

/// Pluggable OIDC backend (production discovery vs mock for tests).
///
/// Method names avoid clashing with `openidconnect::Client::exchange_code`.
pub trait OidcProvider: Send + Sync {
    /// Build authorize URL for Authorization Code + PKCE.
    fn start_authorization<'a>(
        &'a self,
        config: &'a IdpConfig,
        redirect_uri: &'a str,
        state: &'a str,
        nonce: &'a str,
        code_challenge_s256: &'a str,
    ) -> BoxFuture<'a, ApiResult<String>>;

    /// Exchange code + verify ID token (iss/aud/exp/nonce/JWKS). Fail closed.
    fn finish_authorization<'a>(
        &'a self,
        config: &'a IdpConfig,
        redirect_uri: &'a str,
        code: &'a str,
        code_verifier: &'a str,
        expected_nonce: &'a str,
        client_secret: &'a str,
    ) -> BoxFuture<'a, ApiResult<OidcClaims>>;
}

/// In-memory mock IdP for integration tests (no network).
#[derive(Debug, Default)]
pub struct MockOidcProvider {
    /// code → claims (minted by tests via [`MockOidcProvider::mint_code`]).
    codes: std::sync::Mutex<HashMap<String, MockCodeEntry>>,
}

#[derive(Debug, Clone)]
struct MockCodeEntry {
    claims: OidcClaims,
    code_verifier: String,
    client_id: String,
}

impl MockOidcProvider {
    pub fn new() -> Self {
        Self {
            codes: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Register a one-time authorization code that exchange will accept when verifier matches.
    pub fn mint_code(&self, code: &str, code_verifier: &str, client_id: &str, claims: OidcClaims) {
        if let Ok(mut map) = self.codes.lock() {
            map.insert(
                code.to_string(),
                MockCodeEntry {
                    claims,
                    code_verifier: code_verifier.to_string(),
                    client_id: client_id.to_string(),
                },
            );
        }
    }
}

impl OidcProvider for MockOidcProvider {
    fn start_authorization<'a>(
        &'a self,
        config: &'a IdpConfig,
        redirect_uri: &'a str,
        state: &'a str,
        nonce: &'a str,
        code_challenge_s256: &'a str,
    ) -> BoxFuture<'a, ApiResult<String>> {
        Box::pin(async move {
            Ok(format!(
                "mock://idp/authorize?client_id={}&redirect_uri={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256&issuer={}",
                urlencoding_lite(&config.client_id),
                urlencoding_lite(redirect_uri),
                urlencoding_lite(state),
                urlencoding_lite(nonce),
                urlencoding_lite(code_challenge_s256),
                urlencoding_lite(&config.issuer_url),
            ))
        })
    }

    fn finish_authorization<'a>(
        &'a self,
        config: &'a IdpConfig,
        _redirect_uri: &'a str,
        code: &'a str,
        code_verifier: &'a str,
        expected_nonce: &'a str,
        _client_secret: &'a str,
    ) -> BoxFuture<'a, ApiResult<OidcClaims>> {
        Box::pin(async move {
            let mut map = self.codes.lock().map_err(|_| {
                ApiError::new(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "mock oidc lock poisoned",
                )
            })?;
            let entry = map.remove(code).ok_or_else(|| {
                ApiError::new(
                    axum::http::StatusCode::UNAUTHORIZED,
                    "oidc_invalid_code",
                    "invalid authorization code",
                )
            })?;
            if entry.code_verifier != code_verifier {
                return Err(ApiError::new(
                    axum::http::StatusCode::UNAUTHORIZED,
                    "oidc_pkce",
                    "PKCE verifier mismatch",
                ));
            }
            if entry.client_id != config.client_id {
                return Err(ApiError::new(
                    axum::http::StatusCode::UNAUTHORIZED,
                    "oidc_client",
                    "client_id mismatch",
                ));
            }
            let mut claims = entry.claims;
            if claims.nonce.is_none() {
                claims.nonce = Some(expected_nonce.to_string());
            }
            validate_claims(&claims, config, expected_nonce)?;
            Ok(claims)
        })
    }
}

/// Validate OIDC claims fail-closed (iss/aud/exp/nonce).
pub fn validate_claims(
    claims: &OidcClaims,
    config: &IdpConfig,
    expected_nonce: &str,
) -> ApiResult<()> {
    if claims.issuer.trim_end_matches('/') != config.issuer_url.trim_end_matches('/') {
        return Err(ApiError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "oidc_iss",
            "issuer mismatch",
        ));
    }
    let aud_ok = claims
        .audience
        .iter()
        .any(|a| a == &config.client_id || config.audiences.iter().any(|cfg| cfg == a));
    if !aud_ok {
        return Err(ApiError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "oidc_aud",
            "audience mismatch",
        ));
    }
    let now = chrono::Utc::now().timestamp();
    // ±2 min skew on exp
    if claims.exp + 120 < now {
        return Err(ApiError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "oidc_exp",
            "token expired",
        ));
    }
    // Optional iat freshness when present (mock + production after verify).
    if let Some(iat) = claims.iat {
        if iat > now + 120 {
            return Err(ApiError::new(
                axum::http::StatusCode::UNAUTHORIZED,
                "oidc_iat",
                "token iat is in the future",
            ));
        }
        if iat < now - 24 * 3600 {
            return Err(ApiError::new(
                axum::http::StatusCode::UNAUTHORIZED,
                "oidc_iat",
                "token iat is too old",
            ));
        }
    }
    match &claims.nonce {
        Some(n) if n == expected_nonce => Ok(()),
        Some(_) => Err(ApiError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "oidc_nonce",
            "nonce mismatch",
        )),
        None => Err(ApiError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "oidc_nonce",
            "nonce missing",
        )),
    }
}

fn http_client() -> ApiResult<reqwest::Client> {
    reqwest::ClientBuilder::new()
        // Following redirects opens the client up to SSRF vulnerabilities.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| {
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "oidc_http",
                format!("failed to build HTTP client: {e}"),
            )
        })
}

fn oidc_err(code: &'static str, msg: impl Into<String>) -> ApiError {
    ApiError::new(axum::http::StatusCode::UNAUTHORIZED, code, msg.into())
}

/// Production OIDC backend: discovery + PKCE code exchange + JWKS ID-token verify.
#[derive(Debug, Default)]
pub struct OpenIdConnectProvider {
    /// Cached provider metadata by issuer URL (normalized).
    metadata_cache: Mutex<HashMap<String, CoreProviderMetadata>>,
}

impl OpenIdConnectProvider {
    pub fn new() -> Self {
        Self {
            metadata_cache: Mutex::new(HashMap::new()),
        }
    }

    async fn provider_metadata(&self, issuer: &str) -> ApiResult<CoreProviderMetadata> {
        let key = issuer.trim_end_matches('/').to_string();
        {
            let cache = self.metadata_cache.lock().await;
            if let Some(m) = cache.get(&key) {
                return Ok(m.clone());
            }
        }
        let http = http_client()?;
        let issuer_url = IssuerUrl::new(key.clone())
            .map_err(|e| oidc_err("oidc_issuer", format!("invalid issuer URL: {e}")))?;
        let meta = CoreProviderMetadata::discover_async(issuer_url, &http)
            .await
            .map_err(|e| {
                oidc_err(
                    "oidc_discovery",
                    format!("OIDC discovery failed for {key}: {e}"),
                )
            })?;
        let mut cache = self.metadata_cache.lock().await;
        cache.insert(key, meta.clone());
        Ok(meta)
    }

    fn build_client(
        metadata: CoreProviderMetadata,
        config: &IdpConfig,
        redirect_uri: &str,
        client_secret: &str,
    ) -> ApiResult<
        CoreClient<
            openidconnect::EndpointSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointNotSet,
            openidconnect::EndpointSet,
            openidconnect::EndpointMaybeSet,
        >,
    > {
        let secret = if client_secret.is_empty() {
            None
        } else {
            Some(ClientSecret::new(client_secret.to_string()))
        };
        let token_uri = metadata.token_endpoint().cloned().ok_or_else(|| {
            oidc_err(
                "oidc_discovery",
                "IdP discovery document missing token_endpoint",
            )
        })?;
        let client = CoreClient::from_provider_metadata(
            metadata,
            ClientId::new(config.client_id.clone()),
            secret,
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri.to_string())
                .map_err(|e| oidc_err("oidc_redirect", format!("invalid redirect_uri: {e}")))?,
        )
        .set_token_uri(token_uri);
        Ok(client)
    }
}

impl OidcProvider for OpenIdConnectProvider {
    fn start_authorization<'a>(
        &'a self,
        config: &'a IdpConfig,
        redirect_uri: &'a str,
        state: &'a str,
        nonce: &'a str,
        code_challenge_s256: &'a str,
    ) -> BoxFuture<'a, ApiResult<String>> {
        Box::pin(async move {
            // Discovery supplies the real authorization_endpoint (not a guessed path).
            let metadata = self.provider_metadata(&config.issuer_url).await?;
            let mut url = metadata.authorization_endpoint().url().clone();
            {
                let mut pairs = url.query_pairs_mut();
                pairs.append_pair("response_type", "code");
                pairs.append_pair("client_id", &config.client_id);
                pairs.append_pair("redirect_uri", redirect_uri);
                pairs.append_pair("scope", "openid profile email");
                pairs.append_pair("state", state);
                pairs.append_pair("nonce", nonce);
                pairs.append_pair("code_challenge", code_challenge_s256);
                pairs.append_pair("code_challenge_method", "S256");
            }
            Ok(url.to_string())
        })
    }

    fn finish_authorization<'a>(
        &'a self,
        config: &'a IdpConfig,
        redirect_uri: &'a str,
        code: &'a str,
        code_verifier: &'a str,
        expected_nonce: &'a str,
        client_secret: &'a str,
    ) -> BoxFuture<'a, ApiResult<OidcClaims>> {
        Box::pin(async move {
            let metadata = self.provider_metadata(&config.issuer_url).await?;
            let client = Self::build_client(metadata, config, redirect_uri, client_secret)?;
            let http = http_client()?;
            let pkce_verifier = PkceCodeVerifier::new(code_verifier.to_string());
            let token_response: CoreTokenResponse = client
                .exchange_code(AuthorizationCode::new(code.to_string()))
                .set_pkce_verifier(pkce_verifier)
                .request_async(&http)
                .await
                .map_err(|e| oidc_err("oidc_exchange", format!("token exchange failed: {e}")))?;

            let id_token = token_response
                .id_token()
                .ok_or_else(|| oidc_err("oidc_id_token", "IdP did not return an ID token"))?;
            // Enforce iat freshness (spec §3.5.2) in addition to library exp/JWKS checks.
            let verifier = client
                .id_token_verifier()
                .set_issue_time_verifier_fn(|iat| {
                    let now = chrono::Utc::now();
                    let skew = chrono::Duration::minutes(2);
                    if iat > now + skew {
                        return Err(format!("iat is in the future: {iat}"));
                    }
                    if iat < now - chrono::Duration::hours(24) {
                        return Err(format!("iat is too old: {iat}"));
                    }
                    Ok(())
                });
            let nonce = Nonce::new(expected_nonce.to_string());
            let claims: &CoreIdTokenClaims = id_token.claims(&verifier, &nonce).map_err(|e| {
                oidc_err(
                    "oidc_id_token",
                    format!("ID token verification failed: {e}"),
                )
            })?;

            let audiences: Vec<String> = claims.audiences().iter().map(|a| a.to_string()).collect();
            let groups = extract_groups_from_id_token(id_token);
            let out = OidcClaims {
                issuer: claims.issuer().to_string(),
                subject: claims.subject().to_string(),
                email: claims.email().map(|e| e.to_string()),
                preferred_username: claims.preferred_username().map(|u| u.to_string()),
                groups,
                audience: audiences,
                nonce: Some(expected_nonce.to_string()),
                exp: claims.expiration().timestamp(),
                iat: Some(claims.issue_time().timestamp()),
            };
            validate_claims(&out, config, expected_nonce)?;
            Ok(out)
        })
    }
}

/// Best-effort group/role extraction from a verified ID token (claim names vary by IdP).
fn extract_groups_from_id_token(id_token: &openidconnect::core::CoreIdToken) -> Vec<String> {
    // CoreIdToken Display yields the compact JWT serialization.
    extract_groups_from_jwt_payload(&id_token.to_string())
}

fn extract_groups_from_jwt_payload(id_token: &str) -> Vec<String> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 {
        return Vec::new();
    }
    let payload = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]) {
        Ok(b) => b,
        Err(_) => match base64::engine::general_purpose::URL_SAFE.decode(parts[1]) {
            Ok(b) => b,
            Err(_) => return Vec::new(),
        },
    };
    let v: serde_json::Value = match serde_json::from_slice(&payload) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut groups = Vec::new();
    for key in ["groups", "roles", "wids"] {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    groups.push(s.to_string());
                }
            }
        } else if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            groups.push(s.to_string());
        }
    }
    groups.sort();
    groups.dedup();
    groups
}

/// Shared OIDC runtime state.
pub struct OidcRuntime {
    pub provider: Arc<dyn OidcProvider>,
    /// Optional in-memory pending store (also mirrored to platform.db when available).
    pub memory_pending: Mutex<HashMap<String, PendingLogin>>,
    /// When using mock IdP, typed handle so tests can mint codes.
    pub mock: Option<Arc<MockOidcProvider>>,
}

#[derive(Debug, Clone)]
pub struct PendingLogin {
    pub tenant_id: String,
    pub code_verifier: String,
    pub nonce: String,
    pub redirect_uri: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl OidcRuntime {
    pub fn mock() -> Self {
        let mock = Arc::new(MockOidcProvider::new());
        Self {
            provider: mock.clone(),
            memory_pending: Mutex::new(HashMap::new()),
            mock: Some(mock),
        }
    }

    pub fn with_provider(provider: Arc<dyn OidcProvider>) -> Self {
        Self {
            provider,
            memory_pending: Mutex::new(HashMap::new()),
            mock: None,
        }
    }

    pub fn production() -> Self {
        Self::with_provider(Arc::new(OpenIdConnectProvider::new()))
    }

    pub fn mock_provider(&self) -> Option<Arc<MockOidcProvider>> {
        self.mock.clone()
    }

    pub fn is_mock(&self) -> bool {
        self.mock.is_some()
    }
}

/// Canonical callback redirect for a public base URL (exact allowlist entry).
pub fn canonical_callback_uri(public_base: &str) -> String {
    format!("{}/v1/oidc/callback", public_base.trim_end_matches('/'))
}

/// Fail closed unless `redirect_uri` exactly matches the service callback allowlist.
pub fn assert_redirect_allowed(public_base: &str, redirect_uri: &str) -> ApiResult<()> {
    let allowed = canonical_callback_uri(public_base);
    if redirect_uri == allowed {
        return Ok(());
    }
    Err(ApiError::new(
        axum::http::StatusCode::BAD_REQUEST,
        "oidc_redirect",
        "redirect_uri is not on the exact allowlist",
    ))
}

/// PKCE S256 challenge from verifier.
pub fn pkce_challenge_s256(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let dig = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(dig)
}

pub fn random_urlsafe(bytes_len: usize) -> String {
    let mut buf = vec![0u8; bytes_len];
    OsRng.fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// Complete login against platform IdP config + matter user linking.
pub fn complete_oidc_login(
    platform: &Platform,
    matter: &matter_core::Matter,
    tenant_id: &str,
    claims: &OidcClaims,
) -> ApiResult<matter_core::SessionIssue> {
    let tenant = platform
        .get_tenant_by_id(tenant_id)
        .map_err(|e| {
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "platform",
                e.to_string(),
            )
        })?
        .ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "not_found",
                "tenant not found",
            )
        })?;

    // Session must bind to matter tenant when set.
    if let Some(mt) = matter.get_matter_tenant_id().map_err(ApiError::from)? {
        if mt != tenant.id {
            return Err(ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "not_found",
                "matter not found",
            ));
        }
    }

    let idp = platform
        .get_idp_config(&tenant.id)
        .map_err(|e| {
            ApiError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "platform",
                e.to_string(),
            )
        })?
        .ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::BAD_REQUEST,
                "idp_missing",
                "IdP not configured for tenant",
            )
        })?;
    if !idp.enabled {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "idp_disabled",
            "IdP disabled",
        ));
    }

    // Existing linked user may sign in without re-JIT.
    if let Some(user) = matter
        .find_user_by_oidc(&claims.issuer, &claims.subject)
        .map_err(ApiError::from)?
    {
        return matter
            .issue_session_for_user(&user.id, matter_core::DEFAULT_SESSION_TTL_HOURS)
            .map_err(ApiError::from);
    }

    if !tenant.jit_provision {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "jit_disabled",
            "user not provisioned and JIT disabled",
        ));
    }

    if !jit_allowed(claims.email.as_deref(), &claims.groups, &idp) {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "jit_denied",
            "email domain or groups not allowlisted for this tenant",
        ));
    }

    let role = map_role_from_claims(&claims.groups, &idp.role_claim_map, "reviewer");
    let display = claims
        .preferred_username
        .as_deref()
        .or(claims.email.as_deref())
        .unwrap_or(claims.subject.as_str());
    let user = matter
        .create_or_link_oidc_user(display, &role, &claims.issuer, &claims.subject, "oidc")
        .map_err(ApiError::from)?;
    matter
        .issue_session_for_user(&user.id, matter_core::DEFAULT_SESSION_TTL_HOURS)
        .map_err(ApiError::from)
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0xf) as usize]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> IdpConfig {
        IdpConfig {
            tenant_id: "t".into(),
            issuer_url: "https://issuer.example".into(),
            client_id: "client-1".into(),
            secret_env: None,
            has_secret_ciphertext: false,
            audiences: vec!["client-1".into()],
            role_claim_map: Default::default(),
            allowed_email_domains: vec!["firma.com".into()],
            required_groups: vec![],
            enabled: true,
            updated_at: String::new(),
        }
    }

    #[test]
    fn validate_claims_happy_and_bad_nonce_aud_exp() {
        let cfg = sample_config();
        let now = chrono::Utc::now().timestamp();
        let good = OidcClaims {
            issuer: "https://issuer.example".into(),
            subject: "sub-1".into(),
            email: Some("a@firma.com".into()),
            preferred_username: None,
            groups: vec![],
            audience: vec!["client-1".into()],
            nonce: Some("n1".into()),
            exp: now + 3600,
            iat: Some(now - 30),
        };
        assert!(validate_claims(&good, &cfg, "n1").is_ok());

        let mut bad_nonce = good.clone();
        bad_nonce.nonce = Some("other".into());
        assert!(validate_claims(&bad_nonce, &cfg, "n1").is_err());

        let mut bad_aud = good.clone();
        bad_aud.audience = vec!["other-client".into()];
        assert!(validate_claims(&bad_aud, &cfg, "n1").is_err());

        let mut expired = good.clone();
        expired.exp = now - 600;
        assert!(validate_claims(&expired, &cfg, "n1").is_err());

        let mut stale_iat = good.clone();
        stale_iat.iat = Some(now - 48 * 3600);
        assert!(validate_claims(&stale_iat, &cfg, "n1").is_err());

        let mut future_iat = good.clone();
        future_iat.iat = Some(now + 600);
        assert!(validate_claims(&future_iat, &cfg, "n1").is_err());
    }

    #[test]
    fn redirect_allowlist_exact() {
        let base = "http://127.0.0.1:7749";
        assert!(assert_redirect_allowed(base, "http://127.0.0.1:7749/v1/oidc/callback").is_ok());
        assert!(assert_redirect_allowed(base, "https://evil.example/cb").is_err());
        assert!(
            assert_redirect_allowed(base, "http://127.0.0.1:7749/v1/oidc/callback/extra").is_err()
        );
    }

    #[tokio::test]
    async fn live_provider_fails_closed_on_unreachable_issuer() {
        let provider = OpenIdConnectProvider::new();
        let cfg = IdpConfig {
            tenant_id: "t".into(),
            issuer_url: "https://127.0.0.1:1".into(), // nothing listening
            client_id: "client-1".into(),
            secret_env: None,
            has_secret_ciphertext: false,
            audiences: vec!["client-1".into()],
            role_claim_map: Default::default(),
            allowed_email_domains: vec![],
            required_groups: vec![],
            enabled: true,
            updated_at: String::new(),
        };
        let err = provider
            .finish_authorization(
                &cfg,
                "http://127.0.0.1:7749/v1/oidc/callback",
                "code",
                "verifier",
                "nonce",
                "secret",
            )
            .await
            .expect_err("must fail closed");
        // Must not be the old oidc_not_wired stub.
        assert_ne!(err.body.code, "oidc_not_wired");
        assert!(
            err.body.code == "oidc_discovery"
                || err.body.code == "oidc_exchange"
                || err.body.code == "oidc_http",
            "unexpected code {}",
            err.body.code
        );
    }
}
