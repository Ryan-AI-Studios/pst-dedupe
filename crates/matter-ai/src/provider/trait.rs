//! AI provider trait and request/response types (spec §3.3).

use crate::error::Result;

/// Provider discriminant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiProviderKind {
    /// Disabled — `complete` must not be called.
    None,
    /// Deterministic mock for CI / tests.
    Mock,
    /// OpenAI-compatible HTTP (local Ollama/LM Studio or cloud).
    OpenAiCompatible,
}

impl AiProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Mock => "mock",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "" => Some(Self::None),
            "mock" => Some(Self::Mock),
            "openai_compatible" | "openai" => Some(Self::OpenAiCompatible),
            _ => None,
        }
    }
}

/// Chat message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

impl ChatRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }
}

/// Token usage (optional, from provider).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

/// Completion request (chat/completions-shaped).
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// When true, client sends OpenAI-style `response_format: {type: json_object}`.
    pub response_format_json_object: bool,
}

/// Completion response.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<TokenUsage>,
    pub raw_id: Option<String>,
}

/// Pluggable AI provider.
pub trait AiProvider: Send + Sync {
    fn kind(&self) -> AiProviderKind;
    /// True only for non-loopback base URL.
    fn is_remote(&self) -> bool;
    fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
}

/// Detect loopback host (local OpenAI-compatible server).
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_matches(|c| c == '[' || c == ']');
    let lower = h.to_ascii_lowercase();
    lower == "localhost" || lower == "127.0.0.1" || lower == "::1" || lower == "0:0:0:0:0:0:0:1"
}

/// Parse host from a base URL string using standards-compliant URL parsing.
///
/// Fail-closed rules:
/// - empty / unparseable → `None` (caller treats as remote)
/// - **userinfo present** (e.g. `http://127.0.0.1@evil.example/`) → `None`
///   so it cannot masquerade as loopback
pub fn host_from_base_url(base_url: &str) -> Option<String> {
    let s = base_url.trim();
    if s.is_empty() {
        return None;
    }
    // Accept with or without scheme.
    let with_scheme = if s.contains("://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    let parsed = url::Url::parse(&with_scheme).ok()?;
    // Reject userinfo authority tricks (127.0.0.1@evil.example).
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    let host = parsed.host_str()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// True when base URL is non-loopback (remote).
///
/// Unparseable URLs, userinfo authorities, and missing hosts are treated as
/// **remote** (fail closed) so `allow_remote=false` blocks them.
pub fn is_remote_base_url(base_url: &str) -> bool {
    match host_from_base_url(base_url) {
        Some(h) => !is_loopback_host(&h),
        None => true, // fail closed: unknown host treated as remote
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("::1"));
        assert!(!is_loopback_host("api.openai.com"));
        assert!(!is_remote_base_url("http://127.0.0.1:11434/v1"));
        assert!(!is_remote_base_url("http://localhost:1234/v1"));
        assert!(is_remote_base_url("https://api.openai.com/v1"));
    }

    #[test]
    fn userinfo_authority_cannot_masquerade_as_loopback() {
        // Classic authority trick: userinfo looks like loopback, host is remote.
        let evil = "http://127.0.0.1:80@evil.example/v1";
        assert!(
            is_remote_base_url(evil),
            "userinfo URL must be treated as remote (fail closed)"
        );
        assert!(
            host_from_base_url(evil).is_none(),
            "userinfo must yield no host (rejected)"
        );
        // Password form
        assert!(is_remote_base_url("http://user:pass@evil.example/v1"));
        // Legitimate loopback still local
        assert!(!is_remote_base_url("http://127.0.0.1:11434/v1"));
    }
}
