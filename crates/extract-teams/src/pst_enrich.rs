//! PST / mail-shaped Teams enrich path (metadata + body; no PST reopen).
//!
//! Prefers already-extracted item metadata + CAS text/native. Existing body
//! text **always** runs through ammonia plain-text conversion (no HTML heuristic
//! bypass). Does **not** open PST files.
//!
//! ## Conversation identity (PST)
//!
//! Hash components use team/channel when path heuristics find them under
//! `Team Chat` / `Conversation History` (`…/Team Chat/<team>/<channel>/…`).
//! When those are empty, **`channel_or_chat_key` falls back to the path parent
//! folder name** (folder containing the message leaf), so distinct chats do not
//! all collapse to `"unknown"`. `chat_type` is derived from `message_class` /
//! path heuristics and normalized to the frozen enum.

use crate::body::build_review_body;
use crate::bucket::{normalize_chat_type, ConversationKeys};
use crate::detect::is_pst_teams_shaped;
use crate::html_parse::ParsedChatMessage;
use crate::sanitize::{cap_text, html_to_plain_text};

/// Metadata inputs for [`enrich_from_metadata`].
#[derive(Debug, Clone, Default)]
pub struct PstEnrichInput<'a> {
    pub message_class: Option<&'a str>,
    pub path: Option<&'a str>,
    pub from_addr: Option<&'a str>,
    pub sent_at: Option<&'a str>,
    pub subject: Option<&'a str>,
    pub existing_text: Option<&'a str>,
    pub team_hint: Option<&'a str>,
    pub channel_hint: Option<&'a str>,
    /// Attachment filenames (and optional URLs) already on the matter item tree.
    pub attachments: &'a [(Option<String>, Option<String>)],
}

/// Build enrich fields for a PST-shaped Teams item from existing metadata + body.
pub fn enrich_from_metadata(input: &PstEnrichInput<'_>) -> Option<ParsedChatMessage> {
    if !is_pst_teams_shaped(input.message_class, input.path) {
        return None;
    }

    let (team, channel) = team_channel_from_path(input.path, input.team_hint, input.channel_hint);
    // Stable chat key when team/channel are empty: path parent folder name.
    let chat_key = channel
        .clone()
        .or_else(|| path_parent_folder(input.path))
        .or_else(|| message_class_chat_key(input.message_class));

    // Always run the ammonia plain-text path on existing body text. Heuristic
    // "looks like HTML" is insufficient (e.g. `<a>`, `<b>`, `<img>` fragments)
    // and would leave untrusted markup in CAS text, violating §3.3.4.
    let plain_body = match input.existing_text {
        Some(t) => html_to_plain_text(t),
        // Documented no-body fallback: subject only when source never had text CAS.
        // Subject is metadata (not export HTML); still pass through ammonia so
        // a malicious subject cannot inject tags into review text.
        None => html_to_plain_text(input.subject.unwrap_or("")),
    };
    let full = build_review_body(&plain_body, &[], input.attachments);
    let (plain_text, _) = cap_text(full);

    let keys =
        ConversationKeys::from_parts(team.as_deref(), chat_key.as_deref(), input.sent_at, None);
    let chat_type = derive_chat_type(input.message_class, input.path, team.as_deref());

    Some(ParsedChatMessage {
        export_id: None,
        from_addr: input.from_addr.map(|s| s.to_string()),
        from_name: None,
        sent_at: input.sent_at.map(|s| s.to_string()),
        thread_key: None,
        plain_text,
        conversation_id: keys.conversation_id(),
        conversation_bucket_date: keys.bucket_date,
        team_name: team,
        channel_name: channel.or_else(|| chat_key.clone()),
        chat_type,
    })
}

/// Best-effort team/channel from folder path heuristics.
///
/// Only segments under **`Team Chat`** are treated as team/channel
/// (`…/Team Chat/<team>/<channel>/…`). Bare `Conversation History/<chat>/…`
/// paths (1:1/group) leave team/channel empty so the parent-folder chat key
/// can distinguish them.
fn team_channel_from_path(
    path: Option<&str>,
    team_hint: Option<&str>,
    channel_hint: Option<&str>,
) -> (Option<String>, Option<String>) {
    if team_hint.is_some() || channel_hint.is_some() {
        return (
            team_hint.map(|s| s.to_string()),
            channel_hint.map(|s| s.to_string()),
        );
    }
    let Some(p) = path else {
        return (None, None);
    };
    // e.g. .../Conversation History/Team Chat/TeamName/ChannelName/item
    let parts: Vec<&str> = p.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let mut team = None;
    let mut channel = None;
    for (i, part) in parts.iter().enumerate() {
        if part.eq_ignore_ascii_case("team chat") {
            if let Some(next) = parts.get(i + 1) {
                team = Some((*next).to_string());
                // Channel is the segment after team when it is not the message leaf
                // (leaf is last). Prefer i+2 when there are at least team+channel+leaf.
                if parts.len() > i + 3 {
                    if let Some(ch) = parts.get(i + 2) {
                        channel = Some((*ch).to_string());
                    }
                } else if let Some(ch) = parts.get(i + 2) {
                    // Team Chat/TeamName/item → no channel folder; leave channel empty
                    // and let path_parent_folder supply chat key if needed.
                    let _ = ch;
                }
            }
            break;
        }
    }
    (team, channel)
}

/// Parent folder of the message leaf path component (stable chat key fallback).
///
/// Example: `Conversation History/Chat with Bob/msg` → `"Chat with Bob"`.
fn path_parent_folder(path: Option<&str>) -> Option<String> {
    let p = path?;
    let parts: Vec<&str> = p.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        Some(parts[parts.len() - 2].to_string())
    } else {
        parts.first().map(|s| (*s).to_string())
    }
}

fn message_class_chat_key(message_class: Option<&str>) -> Option<String> {
    message_class
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Derive frozen `chat_type` from message_class / path when possible.
fn derive_chat_type(message_class: Option<&str>, path: Option<&str>, team: Option<&str>) -> String {
    if let Some(mc) = message_class {
        let lower = mc.to_ascii_lowercase();
        if lower.contains("channel") {
            return "channel".into();
        }
        if lower.contains("meeting") {
            return "meeting".into();
        }
        if lower.contains("group") {
            return "group".into();
        }
        if lower.contains("oneonone")
            || lower.contains("one_to_one")
            || lower.contains("1on1")
            || lower.contains(".dm")
        {
            return "one_to_one".into();
        }
        // Explicit aliases if present as suffix/class fragments.
        let from_class = normalize_chat_type(Some(mc));
        if from_class != "unknown" {
            return from_class;
        }
    }
    if team.is_some() {
        return "channel".into();
    }
    if let Some(p) = path {
        let lower = p.to_ascii_lowercase();
        if lower.contains("team chat") {
            return "channel".into();
        }
        if lower.contains("meeting") {
            return "meeting".into();
        }
    }
    "unknown".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrich_skype_message_class() {
        let msg = enrich_from_metadata(&PstEnrichInput {
            message_class: Some("IPM.SkypeTeams.Message"),
            path: Some("Conversation History/Team Chat/Alpha/General/item"),
            from_addr: Some("alice@example.com"),
            sent_at: Some("2024-06-01T10:00:00Z"),
            subject: Some("hello"),
            existing_text: Some("Hello <script>alert(1)</script> world"),
            ..Default::default()
        })
        .unwrap();
        assert!(!msg.plain_text.to_lowercase().contains("<script"));
        assert_eq!(msg.conversation_bucket_date, "2024-06-01");
        assert_eq!(msg.team_name.as_deref(), Some("Alpha"));
        assert_eq!(msg.channel_name.as_deref(), Some("General"));
        assert_eq!(msg.chat_type, "channel");
    }

    #[test]
    fn generic_html_tags_always_sanitized() {
        // Narrow tag heuristics previously missed <a>/<b>/<img>; always ammonia.
        let msg = enrich_from_metadata(&PstEnrichInput {
            message_class: Some("IPM.SkypeTeams.Message"),
            path: Some("Conversation History/Team Chat/Alpha/General/item"),
            existing_text: Some(
                r#"Hello <a href="javascript:alert(1)">world</a> <b>bold</b> <img src=x onerror=evil()>"#,
            ),
            ..Default::default()
        })
        .unwrap();
        assert!(!msg.plain_text.contains('<'));
        assert!(!msg.plain_text.to_lowercase().contains("javascript"));
        assert!(!msg.plain_text.to_lowercase().contains("onerror"));
        assert!(msg.plain_text.contains("Hello"));
        assert!(msg.plain_text.contains("world"));
        assert!(msg.plain_text.contains("bold"));
    }

    #[test]
    fn attachment_filenames_injected() {
        let atts = [(Some("Contract.docx".into()), None)];
        let msg = enrich_from_metadata(&PstEnrichInput {
            message_class: Some("IPM.SkypeTeams.Message"),
            path: Some("Conversation History/Team Chat/Alpha/General/item"),
            existing_text: Some("see attached"),
            attachments: &atts,
            ..Default::default()
        })
        .unwrap();
        assert!(msg.plain_text.contains("[Attachment: Contract.docx]"));
    }

    #[test]
    fn distinct_path_folders_get_distinct_chat_keys() {
        let a = enrich_from_metadata(&PstEnrichInput {
            message_class: Some("IPM.SkypeTeams.Message"),
            path: Some("Conversation History/Chat with Alice/item"),
            sent_at: Some("2024-06-01T10:00:00Z"),
            existing_text: Some("hi"),
            ..Default::default()
        })
        .unwrap();
        let b = enrich_from_metadata(&PstEnrichInput {
            message_class: Some("IPM.SkypeTeams.Message"),
            path: Some("Conversation History/Chat with Bob/item"),
            sent_at: Some("2024-06-01T10:00:00Z"),
            existing_text: Some("hi"),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(a.conversation_id, b.conversation_id);
        assert_eq!(a.channel_name.as_deref(), Some("Chat with Alice"));
        assert_eq!(b.channel_name.as_deref(), Some("Chat with Bob"));
    }

    #[test]
    fn non_teams_returns_none() {
        assert!(enrich_from_metadata(&PstEnrichInput {
            message_class: Some("IPM.Note"),
            path: Some("Inbox/foo"),
            existing_text: Some("hi"),
            ..Default::default()
        })
        .is_none());
    }
}
