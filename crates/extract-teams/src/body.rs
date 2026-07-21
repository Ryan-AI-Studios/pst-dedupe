//! Reaction / attachment line injectors for plain-text review body.

/// Format: `\n[Reaction: <who> <emoji_or_name>]`
pub fn format_reaction_line(who: &str, emoji_or_name: &str) -> String {
    let who = who.trim();
    let emoji = emoji_or_name.trim();
    let who = if who.is_empty() { "unknown" } else { who };
    let emoji = if emoji.is_empty() { "unknown" } else { emoji };
    format!("[Reaction: {who} {emoji}]")
}

/// Format: `\n[Attachment: <filename>]`
pub fn format_attachment_line(filename: &str) -> String {
    let name = filename.trim();
    let name = if name.is_empty() { "unknown" } else { name };
    format!("[Attachment: {name}]")
}

/// Format: `\n[Attachment-URL: …]`
pub fn format_attachment_url_line(url: &str) -> String {
    let u = url.trim();
    let u = if u.is_empty() { "unknown" } else { u };
    format!("[Attachment-URL: {u}]")
}

/// Build full plain-text body: message text + reaction lines + attachment lines.
pub fn build_review_body(
    message_text: &str,
    reactions: &[(String, String)],
    attachments: &[(Option<String>, Option<String>)],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let body = message_text.trim();
    if !body.is_empty() {
        parts.push(body.to_string());
    }
    for (who, emoji) in reactions {
        parts.push(format_reaction_line(who, emoji));
    }
    for (name, url) in attachments {
        if let Some(n) = name {
            if !n.trim().is_empty() {
                parts.push(format_attachment_line(n));
            }
        }
        if let Some(u) = url {
            if !u.trim().is_empty() {
                parts.push(format_attachment_url_line(u));
            }
        }
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaction_line_contains_who_and_emoji() {
        let line = format_reaction_line("bob@example.com", "👍");
        assert!(line.contains("[Reaction:"));
        assert!(line.contains("bob@example.com"));
        assert!(line.contains('👍'));
    }

    #[test]
    fn attachment_line_contains_filename() {
        let line = format_attachment_line("Contract_v2.docx");
        assert!(line.contains("[Attachment:"));
        assert!(line.contains("Contract_v2.docx"));
    }

    #[test]
    fn build_review_body_order() {
        let text = build_review_body(
            "Hello",
            &[("bob".into(), "👍".into())],
            &[(Some("a.docx".into()), Some("https://x/a.docx".into()))],
        );
        assert!(text.starts_with("Hello"));
        assert!(text.contains("[Reaction: bob 👍]"));
        assert!(text.contains("[Attachment: a.docx]"));
        assert!(text.contains("[Attachment-URL: https://x/a.docx]"));
    }
}
