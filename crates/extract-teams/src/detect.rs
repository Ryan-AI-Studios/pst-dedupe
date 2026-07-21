//! Format detection for Teams/chat export leaves.

/// Export format labels written to `chat_export_format`.
pub mod export_format {
    pub const PST: &str = "pst";
    pub const HTML: &str = "html";
    pub const JSON: &str = "json";
}

/// Detect whether path/mime look like HTML conversation export.
pub fn is_html_export(path: Option<&str>, mime: Option<&str>) -> bool {
    if let Some(p) = path {
        let lower = p.to_ascii_lowercase();
        if lower.ends_with(".html") || lower.ends_with(".htm") {
            return true;
        }
    }
    if let Some(m) = mime {
        let lower = m.to_ascii_lowercase();
        if lower.contains("text/html") {
            return true;
        }
    }
    false
}

/// Detect whether path/mime look like JSON message dump.
pub fn is_json_export(path: Option<&str>, mime: Option<&str>) -> bool {
    if let Some(p) = path {
        if p.to_ascii_lowercase().ends_with(".json") {
            return true;
        }
    }
    if let Some(m) = mime {
        if m.to_ascii_lowercase().contains("application/json") {
            return true;
        }
    }
    false
}

/// Detect PST / mail-shaped Teams messages from metadata heuristics.
///
/// Signals (any one is enough):
/// - `message_class` contains `SkypeTeams` or `IPM.SkypeTeams`
/// - path contains `Team Chat` or `Conversation History` (case-insensitive)
pub fn is_pst_teams_shaped(message_class: Option<&str>, path: Option<&str>) -> bool {
    if let Some(mc) = message_class {
        let lower = mc.to_ascii_lowercase();
        if lower.contains("skypeteams") || lower.contains("ipm.skypeteams") {
            return true;
        }
    }
    if let Some(p) = path {
        let lower = p.to_ascii_lowercase();
        if lower.contains("team chat") || lower.contains("conversation history") {
            return true;
        }
    }
    false
}

/// Best-effort format classification for a candidate leaf.
pub fn detect_format(
    path: Option<&str>,
    mime: Option<&str>,
    message_class: Option<&str>,
) -> Option<&'static str> {
    // Prefer structured file formats over PST heuristics when both match.
    if is_html_export(path, mime) {
        return Some(export_format::HTML);
    }
    if is_json_export(path, mime) {
        return Some(export_format::JSON);
    }
    if is_pst_teams_shaped(message_class, path) {
        return Some(export_format::PST);
    }
    None
}

/// Sniff HTML bytes for synthetic fixture conversation marker.
pub fn looks_like_teams_html(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    head.contains("class=\"conversation\"")
        || head.contains("class='conversation'")
        || head.contains("data-channel")
        || head.contains("data-team")
}

/// Sniff JSON for Teams-ish message schema (fail closed for random config JSON).
///
/// Accepts a leading array/object that mentions documented message wrappers or
/// body/id-ish fields. Pure `{ "foo": 1 }` style objects return false.
pub fn looks_like_teams_json(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(8192)]);
    let trimmed = head.trim_start();
    if !(trimmed.starts_with('[') || trimmed.starts_with('{')) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    // Documented wrappers (object root) or common message keys (array / objects).
    lower.contains("\"messages\"")
        || lower.contains("\"items\"")
        || lower.contains("\"value\"")
        || lower.contains("\"body\"")
        || lower.contains("\"content\"")
        || lower.contains("\"bodypreview\"")
        || lower.contains("\"messageid\"")
        || lower.contains("\"message_id\"")
        || lower.contains("\"sentdatetime\"")
        || lower.contains("\"createddatetime\"")
        || lower.contains("\"sent_at\"")
        || (lower.contains("\"id\"")
            && (lower.contains("\"from\"")
                || lower.contains("\"sender\"")
                || lower.contains("\"timestamp\"")
                || lower.contains("\"text\"")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_html_path() {
        assert!(is_html_export(Some("export/Team.html"), None));
        assert!(!is_html_export(Some("export/msg.json"), None));
    }

    #[test]
    fn detects_skype_message_class() {
        assert!(is_pst_teams_shaped(
            Some("IPM.SkypeTeams.Message"),
            Some("Inbox/foo")
        ));
        assert!(is_pst_teams_shaped(
            None,
            Some("Conversation History/Team Chat/a")
        ));
    }

    #[test]
    fn looks_like_teams_html_requires_markers() {
        assert!(looks_like_teams_html(
            br#"<div class="conversation" data-team="T" data-channel="C"></div>"#
        ));
        assert!(!looks_like_teams_html(
            b"<!DOCTYPE html><html><body><h1>Invoice</h1></body></html>"
        ));
    }

    #[test]
    fn looks_like_teams_json_rejects_random_config() {
        assert!(looks_like_teams_json(
            br#"[{"id":"1","body":"hi","from":"a@x.com","timestamp":"2024-06-01T10:00:00Z"}]"#
        ));
        assert!(looks_like_teams_json(br#"{"messages":[]}"#));
        assert!(!looks_like_teams_json(
            br#"{"configVersion":1,"enabled":true,"paths":["a"]}"#
        ));
        assert!(!looks_like_teams_json(b"not json at all"));
    }
}
