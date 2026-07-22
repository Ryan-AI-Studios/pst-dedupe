//! Non-secret storage backend configuration (persisted in matter.db).

use serde::{Deserialize, Serialize};

use crate::error::{Result, StorageError};

/// Backend kind for CAS blob bytes.
///
/// SQLite metadata always stays host-local; only CAS objects may move to cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StorageBackendKind {
    /// Local filesystem under matter `blobs/` (Desk default).
    #[default]
    Local,
    /// S3-compatible object store (feature `cloud-s3` required to open).
    S3,
    /// Azure Blob (feature `cloud-azure`; residual if not wired).
    Azure,
}

impl StorageBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::S3 => "s3",
            Self::Azure => "azure",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "s3" => Ok(Self::S3),
            "azure" => Ok(Self::Azure),
            other => Err(StorageError::Config(format!(
                "unknown storage backend kind: {other}"
            ))),
        }
    }

    /// True for S3/Azure (remote object stores).
    pub fn is_cloud(self) -> bool {
        matches!(self, Self::S3 | Self::Azure)
    }
}

/// Optional server-side encryption hint (additive; client-side AEAD is separate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SseMode {
    #[default]
    None,
    /// SSE-S3 AES256.
    Aes256,
    /// SSE-KMS (key id from env/IAM, not stored here as secret material).
    AwsKms,
}

/// Matter storage backend config — **no secrets**.
///
/// Credentials come from env (`AWS_ACCESS_KEY_ID` / role / profile) or keyring only.
/// Never store access keys, secret keys, or session tokens in this struct or in matter.db.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageBackendConfig {
    pub kind: StorageBackendKind,
    /// S3/Azure bucket name (required for cloud kinds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    /// AWS region or equivalent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Custom endpoint (MinIO, R2, LocalStack). Must not contain userinfo credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Optional key prefix root (may contain path segments without `..`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Tenant id for object key isolation (0059).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    /// Matter id for object key isolation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matter_id: Option<String>,
    /// Optional SSE mode (no key material).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sse: Option<SseMode>,
    /// Local cache max bytes for [`crate::CachedBlobStore`] (cloud gets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_max_bytes: Option<u64>,
}

impl Default for StorageBackendConfig {
    fn default() -> Self {
        Self {
            kind: StorageBackendKind::Local,
            bucket: None,
            region: None,
            endpoint: None,
            prefix: None,
            tenant_id: None,
            matter_id: None,
            sse: None,
            cache_max_bytes: None,
        }
    }
}

/// Substrings that look like secret fields — rejected in JSON before persist.
const FORBIDDEN_SECRET_KEYS: &[&str] = &[
    "secret",
    "password",
    "access_key",
    "secret_access_key",
    "session_token",
    "private_key",
    "credential",
    "api_key",
    "auth_token",
    "aws_secret",
    "aws_access_key_id",
    "aws_secret_access_key",
    "aws_session_token",
];

/// Value patterns that look like embedded credentials (fail closed).
const SENSITIVE_VALUE_PATTERNS: &[&str] = &[
    "akia",          // AWS access key id prefix (AKIA…)
    "password=",     // query-style secrets
    "secret=",       // query-style secrets
    "secret_key",    // mis-filed secret material
    "access_key",    // AWS / MinIO style
    "session_token", // STS
    "credential",    // generic
    "token=",        // query-style token
    "api_key",       // generic API key
    "auth_token",    // bearer-ish
    "private_key",   // PEM material misfiled
];

impl StorageBackendConfig {
    /// Local filesystem default (offline Desk).
    pub fn local() -> Self {
        Self::default()
    }

    /// Serialize to JSON for `matters.storage_backend_json`.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// Parse from JSON; reject secret-looking keys.
    pub fn from_json(s: &str) -> Result<Self> {
        reject_secret_keys_in_json(s)?;
        let cfg: StorageBackendConfig = serde_json::from_str(s)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Fail-closed validation of non-secret fields.
    pub fn validate(&self) -> Result<()> {
        // Scan all persisted string fields for credential-like patterns (all kinds).
        for (label, val) in [
            ("endpoint", self.endpoint.as_deref()),
            ("bucket", self.bucket.as_deref()),
            ("region", self.region.as_deref()),
            ("prefix", self.prefix.as_deref()),
            ("tenant_id", self.tenant_id.as_deref()),
            ("matter_id", self.matter_id.as_deref()),
        ] {
            if let Some(v) = val {
                reject_sensitive_value(label, v)?;
            }
        }

        if let Some(ep) = self
            .endpoint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            reject_endpoint_userinfo(ep)?;
        }

        match self.kind {
            StorageBackendKind::Local => Ok(()),
            StorageBackendKind::S3 | StorageBackendKind::Azure => {
                let bucket = self
                    .bucket
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                if bucket.is_none() {
                    return Err(StorageError::Config(format!(
                        "{} backend requires bucket",
                        self.kind.as_str()
                    )));
                }
                // Prefix / tenant / matter escape checks via key_layout when opening.
                if let Some(p) = self.prefix.as_deref() {
                    if p.contains("..") {
                        return Err(StorageError::Config("prefix must not contain '..'".into()));
                    }
                }
                if let Some(t) = self.tenant_id.as_deref() {
                    if t.contains("..") || t.contains('/') || t.contains('\\') {
                        return Err(StorageError::Config(
                            "tenant_id rejects path separators and '..'".into(),
                        ));
                    }
                }
                if let Some(m) = self.matter_id.as_deref() {
                    if m.contains("..") || m.contains('/') || m.contains('\\') {
                        return Err(StorageError::Config(
                            "matter_id rejects path separators and '..'".into(),
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    /// Redacted view for audit logs (no secrets; secrets never stored).
    ///
    /// Endpoint is scrubbed to host-only (or `"<set>"` when sensitive-looking).
    pub fn redacted_for_audit(&self) -> serde_json::Value {
        let endpoint = self.endpoint.as_deref().map(redact_endpoint);
        serde_json::json!({
            "kind": self.kind.as_str(),
            "bucket": self.bucket,
            "region": self.region,
            "endpoint": endpoint,
            "prefix": self.prefix,
            "tenant_id": self.tenant_id,
            "matter_id": self.matter_id,
            "sse": self.sse.as_ref().map(|s| match s {
                SseMode::None => "none",
                SseMode::Aes256 => "aes256",
                SseMode::AwsKms => "aws_kms",
            }),
            "cache_max_bytes": self.cache_max_bytes,
            "credentials": "env_or_iam_only",
        })
    }
}

/// Reject endpoints with userinfo (`user:pass@host` with or without scheme).
fn reject_endpoint_userinfo(endpoint: &str) -> Result<()> {
    // scheme://userinfo@host
    if let Some(scheme_end) = endpoint.find("://") {
        let after = &endpoint[scheme_end + 3..];
        if after.contains('@') {
            return Err(StorageError::Config(
                "endpoint must not contain userinfo credentials (user:pass@); \
                 use env/IAM credentials only"
                    .into(),
            ));
        }
    }
    // scheme-less: user:pass@host or user@host (credentials must never appear)
    if !endpoint.contains("://") && endpoint.contains('@') {
        return Err(StorageError::Config(
            "endpoint must not contain userinfo credentials (user:pass@); \
             use env/IAM credentials only"
                .into(),
        ));
    }
    Ok(())
}

fn reject_sensitive_value(label: &str, value: &str) -> Result<()> {
    let lower = value.to_ascii_lowercase();
    for pat in SENSITIVE_VALUE_PATTERNS {
        if lower.contains(pat) {
            return Err(StorageError::Config(format!(
                "storage config {label} looks like it contains credentials (matched '{pat}'); \
                 use env/IAM only"
            )));
        }
    }
    Ok(())
}

/// Scrub endpoint for audit: host-only form, or `"<set>"` if sensitive.
fn redact_endpoint(endpoint: &str) -> String {
    let lower = endpoint.to_ascii_lowercase();
    if lower.contains('@') || SENSITIVE_VALUE_PATTERNS.iter().any(|p| lower.contains(p)) {
        return "<set>".into();
    }
    // Strip path/query; keep scheme + host[:port].
    if let Some(scheme_end) = endpoint.find("://") {
        let rest = &endpoint[scheme_end + 3..];
        let hostport = rest.split('/').next().unwrap_or(rest);
        let hostport = hostport.split('?').next().unwrap_or(hostport);
        let scheme = &endpoint[..scheme_end];
        return format!("{scheme}://{hostport}");
    }
    // No scheme: take first path segment only.
    endpoint
        .split('/')
        .next()
        .unwrap_or(endpoint)
        .split('?')
        .next()
        .unwrap_or(endpoint)
        .to_string()
}

/// Scan raw JSON object keys for secret-looking names (fail closed).
pub fn reject_secret_keys_in_json(s: &str) -> Result<()> {
    let v: serde_json::Value = serde_json::from_str(s)?;
    if let Some(obj) = v.as_object() {
        for key in obj.keys() {
            let lower = key.to_ascii_lowercase();
            for bad in FORBIDDEN_SECRET_KEYS {
                if lower.contains(bad) {
                    return Err(StorageError::Config(format!(
                        "storage config must not contain secret-looking key: {key}"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Job backend kind stored on matter (P0: local only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobBackendKind {
    /// In-process process-runner (default).
    #[default]
    Local,
}

impl JobBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "local" | "" => Ok(Self::Local),
            // Residual kinds reserved for HTTP remote workers — reject at set time for P0.
            "http" | "remote" | "k8s" => Err(StorageError::Config(format!(
                "job backend '{s}' is residual (remote workers must use HTTP to matter-service; not implemented in P0)"
            ))),
            other => Err(StorageError::Config(format!(
                "unknown job backend kind: {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip_local() {
        let c = StorageBackendConfig::local();
        let j = c.to_json().expect("json");
        let back = StorageBackendConfig::from_json(&j).expect("parse");
        assert_eq!(back.kind, StorageBackendKind::Local);
    }

    #[test]
    fn serde_round_trip_s3_no_secrets() {
        let c = StorageBackendConfig {
            kind: StorageBackendKind::S3,
            bucket: Some("my-bucket".into()),
            region: Some("us-east-1".into()),
            endpoint: Some("http://localhost:9000".into()),
            prefix: Some("prod".into()),
            tenant_id: Some("t1".into()),
            matter_id: Some("m1".into()),
            sse: Some(SseMode::Aes256),
            cache_max_bytes: Some(1_000_000_000),
        };
        let j = c.to_json().expect("json");
        assert!(!j.contains("secret"));
        assert!(!j.contains("password"));
        let back = StorageBackendConfig::from_json(&j).expect("parse");
        assert_eq!(back, c);
    }

    #[test]
    fn rejects_secret_key() {
        let bad = r#"{"kind":"s3","bucket":"b","aws_secret_access_key":"x"}"#;
        assert!(StorageBackendConfig::from_json(bad).is_err());
    }

    #[test]
    fn s3_requires_bucket() {
        let c = StorageBackendConfig {
            kind: StorageBackendKind::S3,
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_endpoint_with_userinfo() {
        let c = StorageBackendConfig {
            kind: StorageBackendKind::S3,
            bucket: Some("b".into()),
            endpoint: Some("https://user:pass@minio.example:9000".into()),
            ..Default::default()
        };
        let err = c.validate().expect_err("userinfo");
        let msg = err.to_string().to_ascii_lowercase();
        assert!(
            msg.contains("userinfo") || msg.contains("credential"),
            "unexpected: {msg}"
        );
    }

    #[test]
    fn rejects_schemeless_userinfo_endpoint() {
        let c = StorageBackendConfig {
            kind: StorageBackendKind::S3,
            bucket: Some("b".into()),
            endpoint: Some("user:pass@minio.example:9000".into()),
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_token_eq_and_access_key_values() {
        for (label, val) in [
            ("endpoint", "https://example.com/?token=abc"),
            ("bucket", "access_key=abc"),
            ("region", "credential=xyz"),
            ("prefix", "session_token=sts"),
            ("tenant_id", "token=abc"),
            ("matter_id", "credential=xyz"),
        ] {
            let mut c = StorageBackendConfig {
                kind: StorageBackendKind::S3,
                bucket: Some("ok-bucket".into()),
                ..Default::default()
            };
            match label {
                "endpoint" => c.endpoint = Some(val.into()),
                "bucket" => c.bucket = Some(val.into()),
                "region" => c.region = Some(val.into()),
                "prefix" => c.prefix = Some(val.into()),
                "tenant_id" => c.tenant_id = Some(val.into()),
                "matter_id" => c.matter_id = Some(val.into()),
                _ => unreachable!(),
            }
            assert!(c.validate().is_err(), "expected reject for {label}={val}");
        }
    }

    #[test]
    fn rejects_akia_in_endpoint() {
        let c = StorageBackendConfig {
            kind: StorageBackendKind::S3,
            bucket: Some("b".into()),
            endpoint: Some("https://example.com/?key=AKIAxxxxxxxx".into()),
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_password_eq_in_bucket() {
        let c = StorageBackendConfig {
            kind: StorageBackendKind::S3,
            bucket: Some("password=secretvalue".into()),
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn redacted_endpoint_strips_path_and_userinfo() {
        let c = StorageBackendConfig {
            kind: StorageBackendKind::S3,
            bucket: Some("b".into()),
            endpoint: Some("http://localhost:9000/path?x=1".into()),
            ..Default::default()
        };
        let v = c.redacted_for_audit();
        assert_eq!(
            v.get("endpoint").and_then(|x| x.as_str()),
            Some("http://localhost:9000")
        );

        let sensitive = StorageBackendConfig {
            kind: StorageBackendKind::S3,
            bucket: Some("b".into()),
            endpoint: Some("https://user:pass@host".into()),
            ..Default::default()
        };
        // validate would reject; redaction still scrubs if called.
        let v2 = sensitive.redacted_for_audit();
        assert_eq!(v2.get("endpoint").and_then(|x| x.as_str()), Some("<set>"));
    }
}
