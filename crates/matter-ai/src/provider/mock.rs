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
                // Citation: matched keyword as quote with approximate byte offsets
                // into the user body (after the "Item text..." header when present).
                let needle = if user_lower.contains(&key.to_ascii_lowercase()) {
                    key.as_str()
                } else {
                    name.as_str()
                };
                let (start, end, quote) = find_keyword_offsets(&user, needle);
                let mut obj = serde_json::json!({
                    "code_id": id,
                    "code_name": key,
                    "confidence": 0.85,
                    "rationale_short": "mock keyword match",
                });
                if let (Some(s), Some(e), Some(q)) = (start, end, quote) {
                    obj["citations"] = serde_json::json!([{
                        "quote": q,
                        "start_offset": s,
                        "end_offset": e,
                        "field": "text",
                    }]);
                } else {
                    obj["citations"] = serde_json::json!([]);
                }
                suggestions.push(obj);
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

/// Find case-insensitive keyword in the item body portion of the user message.
///
/// Offsets are relative to the body after the `"\n\n"` following the header so they
/// align with prepared text used for verify (re-find still repairs on mismatch).
fn find_keyword_offsets(user: &str, needle: &str) -> (Option<i64>, Option<i64>, Option<String>) {
    if needle.is_empty() {
        return (None, None, None);
    }
    // Body starts after first blank line following the "Item text..." header.
    let body = if let Some(idx) = user.find("\n\n") {
        let rest = &user[idx + 2..];
        // Drop trailing instruction line if present.
        if let Some(end) = rest.rfind("\n\n") {
            &rest[..end]
        } else {
            rest
        }
    } else {
        user
    };
    let lower = body.to_ascii_lowercase();
    let n_lower = needle.to_ascii_lowercase();
    if let Some(pos) = lower.find(&n_lower) {
        let end = pos + needle.len();
        if end <= body.len() && body.is_char_boundary(pos) && body.is_char_boundary(end) {
            let quote = body[pos..end].to_string();
            return (Some(pos as i64), Some(end as i64), Some(quote));
        }
    }
    // Still return the needle as quote so verify can re-find in prepared text.
    (None, None, Some(needle.to_string()))
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
