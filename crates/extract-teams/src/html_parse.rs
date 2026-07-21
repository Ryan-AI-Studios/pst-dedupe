//! Versioned synthetic fixture HTML parser (`html_fixture_v1`).
//!
//! Documented layout (see crate README and fixtures/teams/):
//!
//! ```html
//! <div class="conversation" data-team="…" data-channel="…" data-chat-type="channel">
//!   <div class="message" data-id="…" data-from="…" data-from-name="…" data-sent-at="…">
//!     <div class="body">…</div>
//!     <div class="reactions">
//!       <span class="reaction" data-from="…" data-emoji="👍"></span>
//!     </div>
//!     <div class="attachments">
//!       <a class="attachment" href="…" data-name="file.docx">file.docx</a>
//!     </div>
//!   </div>
//! </div>
//! ```

use crate::body::build_review_body;
use crate::bucket::ConversationKeys;
use crate::error::{Error, Result};
use crate::sanitize::{cap_text, html_to_plain_text};

/// One normalized chat message from an HTML export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedChatMessage {
    pub export_id: Option<String>,
    pub from_addr: Option<String>,
    pub from_name: Option<String>,
    pub sent_at: Option<String>,
    pub thread_key: Option<String>,
    pub plain_text: String,
    pub conversation_id: String,
    pub conversation_bucket_date: String,
    pub team_name: Option<String>,
    pub channel_name: Option<String>,
    pub chat_type: String,
}

/// Parsed multi-message HTML conversation export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHtmlExport {
    pub team_name: Option<String>,
    pub channel_name: Option<String>,
    pub chat_type: String,
    pub messages: Vec<ParsedChatMessage>,
}

/// Parse synthetic fixture HTML layout into message atoms.
pub fn parse_teams_html(html: &str, max_messages: usize) -> Result<ParsedHtmlExport> {
    if html.trim().is_empty() {
        return Err(Error::parse("empty html"));
    }

    // Require at least one conversation or message marker; hostile/corrupt → error.
    let lower = html.to_ascii_lowercase();
    if !lower.contains("class=\"conversation\"")
        && !lower.contains("class='conversation'")
        && !lower.contains("class=\"message\"")
        && !lower.contains("class='message'")
    {
        return Err(Error::parse("no conversation/message structure"));
    }

    let (team, channel, chat_type) = extract_conversation_meta(html);
    let chat_type = crate::bucket::normalize_chat_type(chat_type.as_deref());

    // Fail closed on overflow: never return a truncated success that would mark the leaf ok.
    let mut messages = Vec::new();
    for block in split_message_blocks(html) {
        if let Some(msg) =
            parse_message_block(&block, team.as_deref(), channel.as_deref(), &chat_type)
        {
            if messages.len() >= max_messages {
                return Err(Error::max_messages_exceeded(max_messages));
            }
            messages.push(msg);
        }
    }

    if messages.is_empty() {
        return Err(Error::parse("no messages extracted"));
    }

    Ok(ParsedHtmlExport {
        team_name: team,
        channel_name: channel,
        chat_type,
        messages,
    })
}

fn extract_conversation_meta(html: &str) -> (Option<String>, Option<String>, Option<String>) {
    // Prefer attributes on the conversation container.
    let team =
        attr_near(html, "conversation", "data-team").or_else(|| attr_value(html, "data-team"));
    let channel = attr_near(html, "conversation", "data-channel")
        .or_else(|| attr_value(html, "data-channel"));
    let chat_type = attr_near(html, "conversation", "data-chat-type")
        .or_else(|| attr_value(html, "data-chat-type"));
    (team, channel, chat_type)
}

/// Split on `<div class="message"` openings (case-insensitive).
fn split_message_blocks(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let needle = "class=\"message\"";
    let needle2 = "class='message'";
    let mut starts = Vec::new();
    let mut search_from = 0;
    while search_from < lower.len() {
        let rest = &lower[search_from..];
        let rel = rest
            .find(needle)
            .map(|i| (i, needle.len()))
            .or_else(|| rest.find(needle2).map(|i| (i, needle2.len())));
        let Some((rel_i, _nlen)) = rel else {
            break;
        };
        let abs = search_from + rel_i;
        // Walk back to '<' of the opening tag.
        let open = html[..abs].rfind('<').unwrap_or(abs);
        starts.push(open);
        search_from = abs + 1;
    }
    if starts.is_empty() {
        return Vec::new();
    }
    let mut blocks = Vec::new();
    for (i, &start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(html.len());
        blocks.push(html[start..end].to_string());
    }
    blocks
}

fn parse_message_block(
    block: &str,
    team: Option<&str>,
    channel: Option<&str>,
    chat_type: &str,
) -> Option<ParsedChatMessage> {
    let export_id = attr_value(block, "data-id");
    let from_addr = attr_value(block, "data-from");
    let from_name = attr_value(block, "data-from-name");
    let sent_at = attr_value(block, "data-sent-at");
    let thread_key = attr_value(block, "data-thread-key");

    let body_html = extract_inner_by_class(block, "body").unwrap_or_default();
    let plain_body = html_to_plain_text(&body_html);

    let reactions = extract_reactions(block);
    let attachments = extract_attachments(block);
    let full = build_review_body(&plain_body, &reactions, &attachments);
    let (plain_text, _) = cap_text(full);

    let keys =
        ConversationKeys::from_parts(team, channel, sent_at.as_deref(), thread_key.as_deref());
    let conversation_id = keys.conversation_id();
    let conversation_bucket_date = keys.bucket_date.clone();

    Some(ParsedChatMessage {
        export_id,
        from_addr,
        from_name,
        sent_at,
        thread_key,
        plain_text,
        conversation_id,
        conversation_bucket_date,
        team_name: team.map(|s| s.to_string()),
        channel_name: channel.map(|s| s.to_string()),
        chat_type: chat_type.to_string(),
    })
}

fn extract_reactions(block: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lower = block.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..]
        .find("class=\"reaction\"")
        .or_else(|| lower[search_from..].find("class='reaction'"))
    {
        let abs = search_from + rel;
        let open = block[..abs].rfind('<').unwrap_or(abs);
        let close = block[abs..]
            .find('>')
            .map(|i| abs + i + 1)
            .unwrap_or(block.len());
        let tag = &block[open..close];
        let who = attr_value(tag, "data-from").unwrap_or_else(|| "unknown".into());
        let emoji = attr_value(tag, "data-emoji")
            .or_else(|| attr_value(tag, "data-name"))
            .unwrap_or_else(|| "unknown".into());
        out.push((who, emoji));
        search_from = close;
    }
    out
}

fn extract_attachments(block: &str) -> Vec<(Option<String>, Option<String>)> {
    let mut out = Vec::new();
    let lower = block.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..]
        .find("class=\"attachment\"")
        .or_else(|| lower[search_from..].find("class='attachment'"))
    {
        let abs = search_from + rel;
        let open = block[..abs].rfind('<').unwrap_or(abs);
        let close = block[abs..]
            .find('>')
            .map(|i| abs + i + 1)
            .unwrap_or(block.len());
        let tag = &block[open..close];
        let name = attr_value(tag, "data-name");
        let href = attr_value(tag, "href");
        // Fallback: text between tags if no data-name
        let name = name.or_else(|| {
            let after = &block[close..];
            let end = after.find('<').unwrap_or(after.len());
            let t = after[..end].trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        });
        out.push((name, href));
        search_from = close;
    }
    out
}

/// Extract attribute value from a substring (first match).
fn attr_value(hay: &str, name: &str) -> Option<String> {
    let lower = hay.to_ascii_lowercase();
    let name_l = name.to_ascii_lowercase();
    let patterns = [format!("{name_l}=\""), format!("{name_l}='")];
    for (pi, pat) in patterns.iter().enumerate() {
        if let Some(i) = lower.find(pat) {
            let start = i + pat.len();
            let quote = if pi == 0 { '"' } else { '\'' };
            let rest = &hay[start..];
            let end = rest.find(quote).unwrap_or(rest.len());
            let v = rest[..end].trim();
            if !v.is_empty() {
                return Some(html_entity_decode(v));
            }
        }
    }
    None
}

/// Attribute near a class marker (within the opening tag that contains the class).
fn attr_near(html: &str, class_name: &str, attr: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("class=\"{class_name}\"");
    let needle2 = format!("class='{class_name}'");
    let pos = lower.find(&needle).or_else(|| lower.find(&needle2))?;
    let open = html[..pos].rfind('<')?;
    let close = html[pos..].find('>').map(|i| pos + i + 1)?;
    attr_value(&html[open..close], attr)
}

/// Inner HTML of first element with class `class_name`.
fn extract_inner_by_class(hay: &str, class_name: &str) -> Option<String> {
    let lower = hay.to_ascii_lowercase();
    let needle = format!("class=\"{class_name}\"");
    let needle2 = format!("class='{class_name}'");
    let pos = lower.find(&needle).or_else(|| lower.find(&needle2))?;
    let open = hay[..pos].rfind('<')?;
    let after_open = hay[open..].find('>')? + open + 1;
    // Naive: find next closing </div>
    let close_tag = "</div>";
    let end_rel = hay[after_open..].to_ascii_lowercase().find(close_tag)?;
    Some(hay[after_open..after_open + end_rel].to_string())
}

fn html_entity_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<!DOCTYPE html>
<html><head><title>Team Alpha / General</title></head>
<body>
  <div class="conversation" data-team="Team Alpha" data-channel="General" data-chat-type="channel">
    <div class="message" data-id="msg-1" data-from="alice@example.com" data-from-name="Alice" data-sent-at="2024-06-01T10:00:00Z">
      <div class="body">Hello <script>alert(1)</script> world</div>
      <div class="reactions">
        <span class="reaction" data-from="bob@example.com" data-emoji="👍"></span>
      </div>
      <div class="attachments">
        <a class="attachment" href="https://sharepoint.example/Contract_v2.docx" data-name="Contract_v2.docx">Contract_v2.docx</a>
      </div>
    </div>
    <div class="message" data-id="msg-2" data-from="bob@example.com" data-sent-at="2024-06-02T11:00:00Z">
      <div class="body">Next day message</div>
    </div>
  </div>
</body></html>"#;

    #[test]
    fn parses_two_messages_multi_day() {
        let p = parse_teams_html(SAMPLE, 50).unwrap();
        assert_eq!(p.messages.len(), 2);
        assert_eq!(p.team_name.as_deref(), Some("Team Alpha"));
        assert_eq!(p.channel_name.as_deref(), Some("General"));
        assert_ne!(p.messages[0].conversation_id, p.messages[1].conversation_id);
        assert_eq!(p.messages[0].conversation_bucket_date, "2024-06-01");
        assert_eq!(p.messages[1].conversation_bucket_date, "2024-06-02");
    }

    #[test]
    fn xss_stripped_from_body() {
        let p = parse_teams_html(SAMPLE, 50).unwrap();
        let t = &p.messages[0].plain_text;
        assert!(!t.to_lowercase().contains("<script"));
        assert!(!t.contains("alert(1)"));
        assert!(t.contains("Hello"));
        assert!(t.contains("world"));
    }

    #[test]
    fn reaction_and_attachment_injected() {
        let p = parse_teams_html(SAMPLE, 50).unwrap();
        let t = &p.messages[0].plain_text;
        assert!(t.contains("[Reaction:"));
        assert!(t.contains("bob@example.com"));
        assert!(t.contains("[Attachment: Contract_v2.docx]"));
    }

    #[test]
    fn corrupt_html_errors() {
        let r = parse_teams_html("<html><body>no structure</body></html>", 50);
        assert!(r.is_err());
    }

    #[test]
    fn max_messages_exceeded_errors_no_partial() {
        let r = parse_teams_html(SAMPLE, 1);
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert_eq!(e.code(), crate::error::codes::MAX_MESSAGES_EXCEEDED);
    }
}
