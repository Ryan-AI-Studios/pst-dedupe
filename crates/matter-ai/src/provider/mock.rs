//! Deterministic mock provider for CI — never hits the network.

use crate::error::{AiError, Result};
use crate::provider::{
    AiProvider, AiProviderKind, CompletionRequest, CompletionResponse, TokenUsage,
};

/// Deterministic JSON suggestions from catalog keywords in the user text.
///
/// Never performs network I/O. Suitable for default CI tests.
#[derive(Debug, Default, Clone)]
pub struct MockAiProvider;

impl MockAiProvider {
    pub fn new() -> Self {
        Self
    }
}

impl AiProvider for MockAiProvider {
    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Mock
    }

    fn is_remote(&self) -> bool {
        false
    }

    fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        if req.model.is_empty() {
            return Err(AiError::provider("mock: model must not be empty"));
        }
        // Collect catalog lines from system message(s).
        let system = req
            .messages
            .iter()
            .filter(|m| m.role.as_str() == "system")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let user = req
            .messages
            .iter()
            .filter(|m| m.role.as_str() == "user")
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let user_lower = user.to_ascii_lowercase();

        let mut suggestions: Vec<serde_json::Value> = Vec::new();
        // Parse lines like: `- id=... key=... name=... | guidance: ...`
        for line in system.lines() {
            let line = line.trim();
            if !line.starts_with('-') {
                continue;
            }
            let key = extract_field(line, "key=");
            let name = extract_field(line, "name=");
            let id = extract_field(line, "id=");
            let Some(key) = key else { continue };
            let name = name.unwrap_or_else(|| key.clone());
            // Keyword match on key or name (case-insensitive).
            let hit = user_lower.contains(&key.to_ascii_lowercase())
                || user_lower.contains(&name.to_ascii_lowercase());
            if hit {
                suggestions.push(serde_json::json!({
                    "code_id": id,
                    "code_name": key,
                    "confidence": 0.85,
                    "rationale_short": "mock keyword match"
                }));
            }
        }

        // If nothing matched but catalog exists, return empty array (valid JSON).
        let content = serde_json::to_string_pretty(&suggestions)
            .map_err(|e| AiError::provider(format!("mock serialize: {e}")))?;

        Ok(CompletionResponse {
            content,
            model: req.model,
            usage: Some(TokenUsage {
                prompt_tokens: Some(0),
                completion_tokens: Some(0),
                total_tokens: Some(0),
            }),
            raw_id: Some("mock".into()),
        })
    }
}

fn extract_field(line: &str, prefix: &str) -> Option<String> {
    let idx = line.find(prefix)?;
    let rest = &line[idx + prefix.len()..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '|')
        .unwrap_or(rest.len());
    let val = rest[..end].trim().trim_matches(',').trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ChatMessage, CompletionRequest};

    #[test]
    fn mock_matches_keyword() {
        let p = MockAiProvider::new();
        let resp = p
            .complete(CompletionRequest {
                model: "mock".into(),
                messages: vec![
                    ChatMessage::system("Codes:\n- id=c1 key=hot name=Hot | guidance: key docs"),
                    ChatMessage::user("This is a hot document about litigation."),
                ],
                temperature: Some(0.0),
                max_tokens: Some(512),
                response_format_json_object: true,
            })
            .expect("complete");
        assert!(resp.content.contains("hot"), "{}", resp.content);
    }
}
