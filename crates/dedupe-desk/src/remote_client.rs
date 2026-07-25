//! HTTP client for matter-service Connect mode (track **0064**).
//!
//! All network I/O is intended for background workers — never call from the
//! egui UI thread. Body loads use a single-flight worker + generation token
//! (latest-wins); stale responses are discarded by the caller.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Default Connect base URL (service loopback).
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:7749";

/// Operator-facing status when mid-session 401 forces Solo (spec §3.3).
pub const AUTH_FAIL_SOLO_STATUS: &str = "Session expired or unauthorized — returned to Solo.";

/// Detect 401 / unauthorized error text from remote list, body, or codes paths.
pub fn is_auth_failure_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("401") || lower.contains("unauthorized") || lower.contains("session expired")
}

/// Clear Connected session after auth failure. Token zeroizes via `BearerToken` Drop.
/// Returns `true` if a session was present and cleared.
pub fn force_clear_connected_session(session: &mut Option<ConnectedSession>) -> bool {
    session.take().is_some()
}

/// Bearer token that zeroizes on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct BearerToken(String);

impl BearerToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BearerToken(***)")
    }
}

/// Password buffer that zeroizes on drop / clear.
#[derive(Clone, Default, Zeroize, ZeroizeOnDrop)]
pub struct SecretString(String);

impl SecretString {
    #[cfg(test)]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn clear(&mut self) {
        self.0.zeroize();
        self.0.clear();
    }

    /// Mutable access for egui password fields (contents zeroized on drop).
    pub fn expose_mut(&mut self) -> &mut String {
        &mut self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(***)")
    }
}

/// In-memory Connected session (process memory only; cleared on disconnect).
#[derive(Clone)]
pub struct ConnectedSession {
    pub base_url: String,
    pub token: BearerToken,
    pub user_id: String,
    pub display_name: String,
    pub role: String,
    pub expires_at: Option<String>,
}

impl ConnectedSession {
    pub fn banner_text(&self) -> String {
        format!(
            "Connected to {} as {} ({})",
            self.base_url, self.display_name, self.role
        )
    }

    pub fn is_read_only(&self) -> bool {
        self.role == "read_only"
    }
}

impl fmt::Debug for ConnectedSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectedSession")
            .field("base_url", &self.base_url)
            .field("token", &self.token)
            .field("user_id", &self.user_id)
            .field("display_name", &self.display_name)
            .field("role", &self.role)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Normalize base URL: trim whitespace and trailing `/`; scheme http/https only.
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let s = raw.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err("Base URL is required.".into());
    }
    let parsed = url::Url::parse(s).map_err(|e| format!("Invalid base URL: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "Base URL scheme must be http or https (got '{other}')."
            ));
        }
    }
    if parsed.host_str().is_none() {
        return Err("Base URL must include a host.".into());
    }
    // Rebuild without trailing slash; keep path if non-root.
    let mut out = format!(
        "{}://{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or_default()
    );
    if let Some(port) = parsed.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    let path = parsed.path().trim_end_matches('/');
    if !path.is_empty() && path != "/" {
        out.push_str(path);
    }
    Ok(out)
}

/// HTTP / API errors surfaced to the operator.
#[derive(Debug, Clone)]
pub enum RemoteError {
    Network(String),
    Http {
        status: u16,
        code: String,
        message: String,
        expected: Option<i64>,
        actual: Option<i64>,
    },
    Decode(String),
    Unauthorized,
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteError::Network(m) => write!(f, "Network error: {m}"),
            RemoteError::Http {
                status,
                code,
                message,
                ..
            } => write!(f, "HTTP {status} ({code}): {message}"),
            RemoteError::Decode(m) => write!(f, "Response decode error: {m}"),
            RemoteError::Unauthorized => write!(f, "Session expired or unauthorized (401)"),
        }
    }
}

impl RemoteError {
    pub fn is_version_conflict(&self) -> bool {
        matches!(
            self,
            RemoteError::Http {
                status: 409,
                code,
                ..
            } if code == "version_conflict"
        )
    }

    pub fn is_oidc_required(&self) -> bool {
        matches!(
            self,
            RemoteError::Http {
                status: 403,
                code,
                ..
            } if code == "oidc_required"
        )
    }

    pub fn conflict_versions(&self) -> Option<(Option<i64>, Option<i64>)> {
        match self {
            RemoteError::Http {
                status: 409,
                expected,
                actual,
                ..
            } => Some((*expected, *actual)),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    code: Option<String>,
    message: Option<String>,
    expected: Option<i64>,
    actual: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct LoginResponseDto {
    token: String,
    user: UserDto,
    expires_at: String,
}

#[derive(Debug, Deserialize, Clone)]
struct UserDto {
    id: String,
    display_name: String,
    role: String,
}

/// Thin item row from `GET /v1/items`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteItemThin {
    pub id: String,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub sent_at: Option<String>,
    pub review_version: i64,
    pub status: String,
}

/// Body payload from `GET /v1/items/{id}/body`.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteItemBody {
    pub item_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub content_type: Option<String>,
    pub text: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub digest: Option<String>,
    pub review_version: i64,
    pub truncated: bool,
}

/// Codes mutate request body — **never** includes `actor` (session is actor).
#[derive(Debug, Clone, Serialize)]
pub struct RemoteApplyCodesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub add_code_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remove_code_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub propagate_family: Option<bool>,
    pub expected_version: i64,
}

/// Build codes JSON for tests / callers (guarantees no `actor` field).
#[cfg_attr(not(test), allow(dead_code))]
pub fn build_codes_request_json(
    add_code_ids: Option<Vec<String>>,
    remove_code_ids: Option<Vec<String>>,
    propagate_family: Option<bool>,
    expected_version: i64,
) -> serde_json::Value {
    let req = RemoteApplyCodesRequest {
        add_code_ids,
        remove_code_ids,
        propagate_family,
        expected_version,
    };
    serde_json::to_value(req).unwrap_or_else(|_| serde_json::json!({}))
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteApplyCodesResponse {
    pub target_item_ids: Vec<String>,
    pub review_versions: Vec<i64>,
}

/// Blocking HTTP client (use only off the UI thread).
pub struct RemoteClient {
    client: reqwest::blocking::Client,
}

impl RemoteClient {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client init failed: {e}"))?;
        Ok(Self { client })
    }

    pub fn healthz(&self, base_url: &str) -> Result<(), RemoteError> {
        let url = format!("{base_url}/healthz");
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| RemoteError::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(RemoteError::Http {
                status: status.as_u16(),
                code: "healthz_failed".into(),
                message: format!("healthz returned {status}"),
                expected: None,
                actual: None,
            });
        }
        let body: serde_json::Value = resp
            .json()
            .map_err(|e| RemoteError::Decode(e.to_string()))?;
        if body.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            return Err(RemoteError::Http {
                status: 200,
                code: "healthz_failed".into(),
                message: "healthz response missing ok:true".into(),
                expected: None,
                actual: None,
            });
        }
        Ok(())
    }

    /// Password login. Caller must zeroize password after return.
    pub fn login(
        &self,
        base_url: &str,
        name: &str,
        password: &str,
    ) -> Result<ConnectedSession, RemoteError> {
        let url = format!("{base_url}/v1/login");
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "name": name, "password": password }))
            .send()
            .map_err(|e| RemoteError::Network(e.to_string()))?;
        Self::map_login_response(base_url, resp)
    }

    /// Redeem one-time SSO handoff code (`POST /v1/oidc/exchange`).
    pub fn exchange_oidc_code(
        &self,
        base_url: &str,
        code: &str,
    ) -> Result<ConnectedSession, RemoteError> {
        let url = format!("{base_url}/v1/oidc/exchange");
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "code": code }))
            .send()
            .map_err(|e| RemoteError::Network(e.to_string()))?;
        Self::map_login_response(base_url, resp)
    }

    fn map_login_response(
        base_url: &str,
        resp: reqwest::blocking::Response,
    ) -> Result<ConnectedSession, RemoteError> {
        let status = resp.status();
        if !status.is_success() {
            return Err(Self::map_error_response(resp));
        }
        let dto: LoginResponseDto = resp
            .json()
            .map_err(|e| RemoteError::Decode(e.to_string()))?;
        Ok(ConnectedSession {
            base_url: base_url.to_string(),
            token: BearerToken::new(dto.token),
            user_id: dto.user.id,
            display_name: dto.user.display_name,
            role: dto.user.role,
            expires_at: Some(dto.expires_at),
        })
    }

    /// Best-effort logout (ignores network errors).
    pub fn logout(&self, session: &ConnectedSession) {
        let url = format!("{}/v1/logout", session.base_url);
        let _ = self
            .client
            .post(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", session.token.as_str()),
            )
            .send();
    }

    pub fn list_items(
        &self,
        session: &ConnectedSession,
        limit: Option<usize>,
        after: Option<&str>,
    ) -> Result<Vec<RemoteItemThin>, RemoteError> {
        let mut url = format!("{}/v1/items", session.base_url);
        let mut q = Vec::new();
        if let Some(n) = limit {
            q.push(format!("limit={n}"));
        }
        if let Some(a) = after {
            q.push(format!("after={}", urlencoding_query(a)));
        }
        if !q.is_empty() {
            url.push('?');
            url.push_str(&q.join("&"));
        }
        let resp = self.authed_get(session, &url)?;
        resp.json().map_err(|e| RemoteError::Decode(e.to_string()))
    }

    pub fn get_item(
        &self,
        session: &ConnectedSession,
        item_id: &str,
    ) -> Result<RemoteItemThin, RemoteError> {
        let url = format!("{}/v1/items/{}", session.base_url, item_id);
        let resp = self.authed_get(session, &url)?;
        resp.json().map_err(|e| RemoteError::Decode(e.to_string()))
    }

    pub fn get_item_body(
        &self,
        session: &ConnectedSession,
        item_id: &str,
    ) -> Result<RemoteItemBody, RemoteError> {
        let url = format!("{}/v1/items/{}/body", session.base_url, item_id);
        let resp = self.authed_get(session, &url)?;
        resp.json().map_err(|e| RemoteError::Decode(e.to_string()))
    }

    pub fn apply_codes(
        &self,
        session: &ConnectedSession,
        item_id: &str,
        req: &RemoteApplyCodesRequest,
    ) -> Result<RemoteApplyCodesResponse, RemoteError> {
        let url = format!("{}/v1/items/{}/codes", session.base_url, item_id);
        let resp = self
            .client
            .post(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", session.token.as_str()),
            )
            .json(req)
            .send()
            .map_err(|e| RemoteError::Network(e.to_string()))?;
        let status = resp.status();
        if status.as_u16() == 401 {
            return Err(RemoteError::Unauthorized);
        }
        if !status.is_success() {
            return Err(Self::map_error_response(resp));
        }
        resp.json().map_err(|e| RemoteError::Decode(e.to_string()))
    }

    fn authed_get(
        &self,
        session: &ConnectedSession,
        url: &str,
    ) -> Result<reqwest::blocking::Response, RemoteError> {
        let resp = self
            .client
            .get(url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", session.token.as_str()),
            )
            .send()
            .map_err(|e| RemoteError::Network(e.to_string()))?;
        let status = resp.status();
        if status.as_u16() == 401 {
            return Err(RemoteError::Unauthorized);
        }
        if !status.is_success() {
            return Err(Self::map_error_response(resp));
        }
        Ok(resp)
    }

    fn map_error_response(resp: reqwest::blocking::Response) -> RemoteError {
        let status = resp.status().as_u16();
        if status == 401 {
            return RemoteError::Unauthorized;
        }
        let body = resp.text().unwrap_or_default();
        if let Ok(err) = serde_json::from_str::<ApiErrorBody>(&body) {
            return RemoteError::Http {
                status,
                code: err.code.unwrap_or_else(|| "error".into()),
                message: err.message.unwrap_or_else(|| body.clone()),
                expected: err.expected,
                actual: err.actual,
            };
        }
        RemoteError::Http {
            status,
            code: "error".into(),
            message: if body.is_empty() {
                format!("HTTP {status}")
            } else {
                body
            },
            expected: None,
            actual: None,
        }
    }
}

fn urlencoding_query(s: &str) -> String {
    // Minimal query escaping for item ids (alphanumeric + -_).
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            _ => {
                for b in c.to_string().as_bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

/// Whether a post-auth handoff URL is loopback-only (Desk SSO).
pub fn is_loopback_handoff_url(raw: &str) -> bool {
    let Ok(u) = url::Url::parse(raw.trim()) else {
        return false;
    };
    if u.scheme() != "http" && u.scheme() != "https" {
        return false;
    }
    match u.host() {
        Some(url::Host::Ipv4(v4)) => v4.is_loopback(),
        Some(url::Host::Ipv6(v6)) => v6.is_loopback(),
        Some(url::Host::Domain(d)) => {
            let d = d.to_ascii_lowercase();
            d == "localhost" || d == "localhost."
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_slash_and_rejects_bad_scheme() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:7749/").unwrap(),
            "http://127.0.0.1:7749"
        );
        assert_eq!(
            normalize_base_url("  https://example.com/svc/  ").unwrap(),
            "https://example.com/svc"
        );
        assert!(normalize_base_url("ftp://x").is_err());
        assert!(normalize_base_url("not-a-url").is_err());
        assert!(normalize_base_url("").is_err());
    }

    #[test]
    fn codes_request_has_expected_version_no_actor() {
        let v = build_codes_request_json(Some(vec!["c1".into()]), None, Some(false), 7);
        assert_eq!(v["expected_version"], 7);
        assert!(v.get("actor").is_none());
        assert_eq!(v["add_code_ids"][0], "c1");
    }

    #[test]
    fn secrets_debug_redacted() {
        let t = BearerToken::new("super-secret-token");
        let p = SecretString::new("hunter2");
        let dbg_t = format!("{t:?}");
        let dbg_p = format!("{p:?}");
        assert!(!dbg_t.contains("super-secret"));
        assert!(!dbg_p.contains("hunter2"));
        assert!(dbg_t.contains("***"));
        let session = ConnectedSession {
            base_url: "http://127.0.0.1:7749".into(),
            token: BearerToken::new("tok-abc"),
            user_id: "u1".into(),
            display_name: "Alice".into(),
            role: "reviewer".into(),
            expires_at: None,
        };
        let s = format!("{session:?}");
        assert!(!s.contains("tok-abc"));
        assert!(session.banner_text().contains("Alice"));
        assert!(!session.banner_text().contains("tok-abc"));
    }

    #[test]
    fn loopback_handoff_accepts_only_loopback() {
        assert!(is_loopback_handoff_url(
            "http://127.0.0.1:54321/connect/callback"
        ));
        assert!(is_loopback_handoff_url("http://localhost:9/cb"));
        assert!(is_loopback_handoff_url("http://[::1]:99/cb"));
        assert!(!is_loopback_handoff_url("http://192.168.1.1:80/cb"));
        assert!(!is_loopback_handoff_url("https://evil.example/cb"));
        assert!(!is_loopback_handoff_url("not-a-url"));
    }

    #[test]
    fn auth_failure_message_and_session_clear() {
        assert!(is_auth_failure_message(
            "Session expired or unauthorized (401)"
        ));
        assert!(is_auth_failure_message("HTTP 401 (error): no"));
        assert!(is_auth_failure_message("unauthorized"));
        assert!(!is_auth_failure_message("version conflict"));
        let mut session = Some(ConnectedSession {
            base_url: "http://127.0.0.1:7749".into(),
            token: BearerToken::new("tok"),
            user_id: "u".into(),
            display_name: "N".into(),
            role: "reviewer".into(),
            expires_at: None,
        });
        assert!(force_clear_connected_session(&mut session));
        assert!(session.is_none());
    }
}
