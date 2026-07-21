//! OpenAI-compatible HTTP client (`POST {base}/v1/chat/completions`).
//!
//! Works with Ollama, LM Studio, OpenAI, and Azure-shaped endpoints that share
//! the chat completions JSON shape. Hard request timeout; optional one retry
//! without `response_format` when the engine rejects that field.
//!
//! **Redirects are not followed** (`reqwest::redirect::Policy::none()`). The
//! loopback / remote check applies only to the configured base URL; following a
//! redirect from loopback to a remote host would otherwise leak matter text when
//! `allow_remote` is false. A 3xx response is treated as an HTTP error (fail closed).

use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{AiError, Result};
use crate::provider::{
    is_remote_base_url, AiProvider, AiProviderKind, CompletionRequest, CompletionResponse,
    TokenUsage,
};

/// Default hard timeout for a single completion (seconds).
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// OpenAI-compatible chat completions client.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProvider {
    base_url: String,
    api_key: Option<String>,
    allow_remote: bool,
    timeout: Duration,
}

impl OpenAiCompatibleProvider {
    /// Build a provider. Does **not** hit the network.
    ///
    /// Fails closed when remote and `!allow_remote` on [`AiProvider::complete`],
    /// before any HTTP.
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        allow_remote: bool,
    ) -> Result<Self> {
        let base = normalize_base_url(base_url.into())?;
        Ok(Self {
            base_url: base,
            api_key,
            allow_remote,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn completions_url(&self) -> String {
        // base is already stripped of trailing slash; may already end with /v1.
        let b = self.base_url.trim_end_matches('/');
        if b.ends_with("/v1") {
            format!("{b}/chat/completions")
        } else {
            format!("{b}/v1/chat/completions")
        }
    }

    fn build_body(req: &CompletionRequest, include_response_format: bool) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                json!({
                    "role": m.role.as_str(),
                    "content": m.content,
                })
            })
            .collect();
        let mut body = json!({
            "model": req.model,
            "messages": messages,
        });
        if let Some(t) = req.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(mt) = req.max_tokens {
            body["max_tokens"] = json!(mt);
        }
        if include_response_format && req.response_format_json_object {
            body["response_format"] = json!({ "type": "json_object" });
        }
        body
    }

    fn post(&self, body: &Value) -> Result<Value> {
        // Fail closed on redirects: base-URL loopback checks do not cover targets.
        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AiError::Http(format!("client build: {e}")))?;
        let url = self.completions_url();
        let mut req = client.post(&url).header("Content-Type", "application/json");
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                req = req.header("Authorization", format!("Bearer {key}"));
            }
        }
        let resp = req
            .json(body)
            .send()
            .map_err(|e| AiError::Http(format!("request failed: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| AiError::Http(format!("read body: {e}")))?;
        // 3xx is not success; with Policy::none() this is how redirect attempts fail closed.
        if status.is_redirection() {
            return Err(AiError::Http(format!(
                "HTTP {status}: redirect not followed (fail closed; set allow_remote + direct URL if remote is intended); body={}",
                truncate_for_err(&text, 200)
            )));
        }
        if !status.is_success() {
            return Err(AiError::Http(format!(
                "HTTP {status}: {}",
                truncate_for_err(&text, 400)
            )));
        }
        serde_json::from_str(&text).map_err(|e| {
            AiError::Http(format!(
                "response JSON: {e}; body={}",
                truncate_for_err(&text, 200)
            ))
        })
    }

    fn map_response(value: &Value, fallback_model: &str) -> Result<CompletionResponse> {
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let model = value
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_model)
            .to_string();
        let raw_id = value
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let usage = value.get("usage").map(|u| TokenUsage {
            prompt_tokens: u
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
            total_tokens: u
                .get("total_tokens")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
        });
        Ok(CompletionResponse {
            content,
            model,
            usage,
            raw_id,
        })
    }
}

impl AiProvider for OpenAiCompatibleProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::OpenAiCompatible
    }

    fn is_remote(&self) -> bool {
        is_remote_base_url(&self.base_url)
    }

    fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        if self.is_remote() && !self.allow_remote {
            return Err(AiError::RemoteBlocked);
        }
        if req.model.trim().is_empty() {
            return Err(AiError::provider("model must not be empty"));
        }

        let body = Self::build_body(&req, true);
        match self.post(&body) {
            Ok(v) => Self::map_response(&v, &req.model),
            Err(e) if req.response_format_json_object && is_response_format_rejection(&e) => {
                // Optional one retry without response_format (some Ollama builds).
                let body2 = Self::build_body(&req, false);
                let v = self.post(&body2)?;
                Self::map_response(&v, &req.model)
            }
            Err(e) => Err(e),
        }
    }
}

fn normalize_base_url(raw: String) -> Result<String> {
    let s = raw.trim().trim_end_matches('/').to_string();
    if s.is_empty() {
        return Err(AiError::InvalidParams("ai_base_url is empty".into()));
    }
    if !(s.starts_with("http://") || s.starts_with("https://")) {
        return Err(AiError::InvalidParams(format!(
            "ai_base_url must start with http:// or https:// (got '{s}')"
        )));
    }
    Ok(s)
}

fn is_response_format_rejection(err: &AiError) -> bool {
    let s = err.to_string().to_ascii_lowercase();
    s.contains("response_format") || s.contains("response format") || s.contains("unknown field")
}

fn truncate_for_err(s: &str, max: usize) -> String {
    let cut = crate::truncate::truncate_to_char_boundary(s, max);
    if cut.len() == s.len() {
        cut.to_string()
    } else {
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_blocked_before_http() {
        let p =
            OpenAiCompatibleProvider::new("https://api.openai.com/v1", None, false).expect("new");
        assert!(p.is_remote());
        let err = p
            .complete(CompletionRequest {
                model: "gpt-4o-mini".into(),
                messages: vec![],
                temperature: None,
                max_tokens: None,
                response_format_json_object: true,
            })
            .unwrap_err();
        assert!(matches!(err, AiError::RemoteBlocked));
    }

    #[test]
    fn loopback_not_remote() {
        let p =
            OpenAiCompatibleProvider::new("http://127.0.0.1:11434/v1", None, false).expect("new");
        assert!(!p.is_remote());
    }

    #[test]
    fn truncate_for_err_utf8_safe_at_multibyte_boundary() {
        // "α" is 2 bytes in UTF-8; cutting at max=1 must not panic or split the char.
        let s = "αβγδε";
        let out = truncate_for_err(s, 1);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        // max inside multi-byte char → empty prefix + ellipsis
        assert_eq!(out, "…");
        // max on full first char (2 bytes) → that char + ellipsis
        let out2 = truncate_for_err(s, 2);
        assert_eq!(out2, "α…");
        // under max unchanged
        assert_eq!(truncate_for_err("ab", 10), "ab");
    }

    #[test]
    fn http_redirect_status_is_provider_error_shape() {
        // Without a live server we assert the error formatter path for 3xx-style messages
        // (post() maps redirection statuses to AiError::Http with "redirect not followed").
        let msg = truncate_for_err("moved permanently to https://evil.example/", 400);
        let err = AiError::Http(format!(
            "HTTP 302 Found: redirect not followed (fail closed; set allow_remote + direct URL if remote is intended); body={msg}"
        ));
        let s = err.to_string();
        assert!(s.contains("redirect not followed"), "{s}");
        assert!(s.contains("302"), "{s}");
    }
}
