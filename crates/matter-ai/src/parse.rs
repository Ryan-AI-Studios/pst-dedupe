//! Robust JSON extraction from chatty model output (spec §3.5.1).

use serde::Deserialize;

use crate::error::{AiError, Result};

/// One parsed code suggestion from the model.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ParsedCodeSuggestion {
    #[serde(default)]
    pub code_id: Option<String>,
    #[serde(default)]
    pub code_name: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub rationale_short: Option<String>,
}

impl ParsedCodeSuggestion {
    /// Prefer code_name; fall back to code_id.
    pub fn display_name(&self) -> Option<&str> {
        self.code_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.code_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
    }
}

/// Extract and validate an array of code suggestions from model content.
///
/// Steps: strip prefixes; fence extract; balanced brace/bracket scan; serde.
pub fn extract_code_suggestions(content: &str) -> Result<Vec<ParsedCodeSuggestion>> {
    let json_slice = extract_json_value(content)
        .ok_or_else(|| AiError::json_parse("no JSON array/object found in model content"))?;
    parse_suggestions_json(&json_slice)
}

fn parse_suggestions_json(json_slice: &str) -> Result<Vec<ParsedCodeSuggestion>> {
    let v: serde_json::Value =
        serde_json::from_str(json_slice).map_err(|e| AiError::json_parse(format!("serde: {e}")))?;
    let arr = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(map) => {
            // Tolerate { "suggestions": [...] } wrappers.
            if let Some(serde_json::Value::Array(a)) = map
                .get("suggestions")
                .or_else(|| map.get("codes"))
                .or_else(|| map.get("results"))
            {
                a.clone()
            } else {
                return Err(AiError::json_parse(
                    "JSON object is not a suggestions array wrapper",
                ));
            }
        }
        _ => {
            return Err(AiError::json_parse(
                "top-level JSON must be array or object with suggestions",
            ));
        }
    };
    let mut out = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let s: ParsedCodeSuggestion = serde_json::from_value(item.clone())
            .map_err(|e| AiError::json_parse(format!("item {i}: {e}")))?;
        if s.display_name().is_none() {
            return Err(AiError::json_parse(format!(
                "item {i}: missing code_name and code_id"
            )));
        }
        out.push(s);
    }
    Ok(out)
}

/// Extract a JSON value string from free-form model content.
fn extract_json_value(content: &str) -> Option<String> {
    let mut s = content.trim();
    // Strip common chatty prefixes (first line only if short).
    if let Some(rest) = strip_chatty_prefix(s) {
        s = rest;
    }
    // Fence: ```json ... ``` or ``` ... ```
    if let Some(inner) = extract_fence(s) {
        return Some(inner);
    }
    // Balanced array preferred, else object.
    if let Some(arr) = find_balanced(s, '[', ']') {
        return Some(arr);
    }
    if let Some(obj) = find_balanced(s, '{', '}') {
        return Some(obj);
    }
    None
}

fn strip_chatty_prefix(s: &str) -> Option<&str> {
    let lower = s.to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "here you go:",
        "here are",
        "here's",
        "sure,",
        "sure!",
        "of course,",
        "certainly,",
        "absolutely,",
    ];
    for p in PREFIXES {
        if lower.starts_with(p) {
            let rest = s.get(p.len()..)?.trim_start();
            return Some(rest);
        }
    }
    None
}

fn extract_fence(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"```" {
            let after = i + 3;
            // optional language tag
            let line_end = s[after..]
                .find('\n')
                .map(|n| after + n + 1)
                .unwrap_or(after);
            let start = line_end;
            if let Some(rel) = s[start..].find("```") {
                let end = start + rel;
                let inner = s[start..end].trim();
                if !inner.is_empty() {
                    return Some(inner.to_string());
                }
            }
            return None;
        }
        i += 1;
    }
    None
}

fn find_balanced(s: &str, open: char, close: char) -> Option<String> {
    let start = s.find(open)?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (idx, ch) in s[start..].char_indices() {
        if in_str {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            c if c == open => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    let end = start + idx + ch.len_utf8();
                    return Some(s[start..end].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_plus_fence() {
        let content = r#"Here you go:
```json
[{"code_name":"hot","confidence":0.9,"rationale_short":"key"}]
```
"#;
        let s = extract_code_suggestions(content).expect("parse");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].code_name.as_deref(), Some("hot"));
    }

    #[test]
    fn trailing_prose() {
        let content = r#"[
  {"code_name": "responsive", "confidence": 0.7}
]
Hope this helps!"#;
        let s = extract_code_suggestions(content).expect("parse");
        assert_eq!(s[0].code_name.as_deref(), Some("responsive"));
    }

    #[test]
    fn bare_array() {
        let content = r#"[{"code_id":"c1","code_name":"privilege"}]"#;
        let s = extract_code_suggestions(content).expect("parse");
        assert_eq!(s[0].code_id.as_deref(), Some("c1"));
    }

    #[test]
    fn garbage_fails() {
        let err = extract_code_suggestions("no json here at all, sorry").unwrap_err();
        assert!(err.to_string().contains("ai_json_parse") || err.to_string().contains("JSON"));
    }
}
