//! Best-effort JSON message dump parser.
//!
//! ## Documented field mapping (P0)
//!
//! Accepts either:
//! - a JSON **array** of message objects, or
//! - an object with `messages` / `items` / `value` array.
//!
//! | Field | Accepted keys (first hit) |
//! |---|---|
//! | id | `id`, `messageId`, `message_id` |
//! | body | `body`, `content`, `text`, `bodyPreview` |
//! | from | `from`, `from_addr`, `sender`, `user` (string or `{email,address,userPrincipalName,displayName}`) |
//! | timestamp | `timestamp`, `sent_at`, `sentDateTime`, `createdDateTime`, `created` |
//! | reactions | `reactions` array of `{from,user,emoji,reactionType,displayName}` |
//! | attachments | `attachments` array of `{name,filename,contentUrl,url}` |
//! | team | root or message `team`, `teamName`, `team_name` |
//! | channel | root or message `channel`, `channelName`, `channel_name` |
//! | chat / conversation id | `conversationId`, `conversation_id`, `chatId`, `chat_id` (used as channel_or_chat_key when team/channel absent) |
//! | chatType | root or message `chatType`, `chat_type`, `type` (normalized to enum) |
//!
//! Unknown schema → parse error (fail closed). Message count above
//! `max_messages_per_file` → [`Error::max_messages_exceeded`] (no silent truncate).

use serde_json::Value;

use crate::body::build_review_body;
use crate::bucket::{normalize_chat_type, ConversationKeys};
use crate::error::{Error, Result};
use crate::html_parse::ParsedChatMessage;
use crate::sanitize::{cap_text, html_to_plain_text};

/// Keys accepted as non-channel conversation / chat identifiers.
const CHAT_ID_KEYS: &[&str] = &["conversationId", "conversation_id", "chatId", "chat_id"];

/// Parse best-effort JSON into chat messages.
pub fn parse_teams_json(bytes: &[u8], max_messages: usize) -> Result<Vec<ParsedChatMessage>> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| Error::parse(format!("json: {e}")))?;

    let (root_team, root_channel, root_chat_id, root_chat_type, arr) = match &v {
        Value::Array(a) => (None, None, None, None, a.as_slice()),
        Value::Object(map) => {
            let team = map_str(map, &["team", "teamName", "team_name"]);
            let channel = map_str(map, &["channel", "channelName", "channel_name"]);
            let chat_id = map_str(map, CHAT_ID_KEYS);
            let chat_type = map_str(map, &["chatType", "chat_type", "type"]);
            let arr = map
                .get("messages")
                .or_else(|| map.get("items"))
                .or_else(|| map.get("value"))
                .and_then(|x| x.as_array())
                .map(|a| a.as_slice())
                .ok_or_else(|| Error::parse("json: no messages/items/value array"))?;
            (team, channel, chat_id, chat_type, arr)
        }
        _ => return Err(Error::parse("json: expected array or object")),
    };

    if arr.is_empty() {
        return Err(Error::parse("json: empty messages array"));
    }

    // Fail closed on overflow: never return a truncated list that would mark the leaf ok.
    let mut out = Vec::new();
    for item in arr.iter() {
        let Some(obj) = item.as_object() else {
            continue;
        };
        // Require at least a body-ish or id field to accept.
        let body_raw = map_str(obj, &["body", "content", "text", "bodyPreview"]);
        let export_id = map_str(obj, &["id", "messageId", "message_id"]);
        if body_raw.is_none() && export_id.is_none() {
            continue;
        }

        let from = extract_from(obj);
        let sent_at = map_str(
            obj,
            &[
                "timestamp",
                "sent_at",
                "sentDateTime",
                "createdDateTime",
                "created",
            ],
        );
        let team = map_str(obj, &["team", "teamName", "team_name"]).or_else(|| root_team.clone());
        let channel = map_str(obj, &["channel", "channelName", "channel_name"])
            .or_else(|| root_channel.clone());
        let chat_id = map_str(obj, CHAT_ID_KEYS).or_else(|| root_chat_id.clone());
        // Prefer explicit channel; otherwise stable chat/conversation id for 1:1/group buckets.
        let channel_or_chat = channel.clone().or_else(|| chat_id.clone());
        let chat_type = normalize_chat_type(
            map_str(obj, &["chatType", "chat_type", "type"])
                .or_else(|| root_chat_type.clone())
                .as_deref(),
        );
        let thread_key = map_str(obj, &["threadId", "thread_id", "replyToId"]);

        let plain_body = match body_raw.as_deref() {
            Some(b) if b.contains('<') => html_to_plain_text(b),
            Some(b) => b.to_string(),
            None => String::new(),
        };

        let reactions = extract_json_reactions(obj);
        let attachments = extract_json_attachments(obj);
        let full = build_review_body(&plain_body, &reactions, &attachments);
        let (plain_text, _) = cap_text(full);

        let keys = ConversationKeys::from_parts(
            team.as_deref(),
            channel_or_chat.as_deref(),
            sent_at.as_deref(),
            thread_key.as_deref(),
        );

        if out.len() >= max_messages {
            return Err(Error::max_messages_exceeded(max_messages));
        }

        out.push(ParsedChatMessage {
            export_id,
            from_addr: from.clone(),
            from_name: None,
            sent_at,
            thread_key,
            plain_text,
            conversation_id: keys.conversation_id(),
            conversation_bucket_date: keys.bucket_date,
            team_name: team,
            // Preserve explicit channel name when present; chat id is identity-only.
            channel_name: channel.or(chat_id),
            chat_type,
        });
    }

    if out.is_empty() {
        return Err(Error::parse("json: no usable message fields"));
    }
    Ok(out)
}

fn map_str(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = map.get(*k) {
            match v {
                Value::String(s) if !s.trim().is_empty() => return Some(s.clone()),
                Value::Number(n) => return Some(n.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn extract_from(obj: &serde_json::Map<String, Value>) -> Option<String> {
    for k in ["from", "from_addr", "sender", "user"] {
        if let Some(v) = obj.get(k) {
            match v {
                Value::String(s) if !s.trim().is_empty() => return Some(s.clone()),
                Value::Object(m) => {
                    if let Some(s) = map_str(
                        m,
                        &[
                            "email",
                            "address",
                            "userPrincipalName",
                            "displayName",
                            "name",
                        ],
                    ) {
                        return Some(s);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn extract_json_reactions(obj: &serde_json::Map<String, Value>) -> Vec<(String, String)> {
    let Some(Value::Array(arr)) = obj.get("reactions") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in arr {
        let Some(m) = r.as_object() else {
            continue;
        };
        let who = map_str(m, &["from", "user", "displayName", "userId"])
            .unwrap_or_else(|| "unknown".into());
        let emoji = map_str(m, &["emoji", "reactionType", "name", "displayName"])
            .unwrap_or_else(|| "unknown".into());
        out.push((who, emoji));
    }
    out
}

fn extract_json_attachments(
    obj: &serde_json::Map<String, Value>,
) -> Vec<(Option<String>, Option<String>)> {
    let Some(Value::Array(arr)) = obj.get("attachments") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for a in arr {
        let Some(m) = a.as_object() else {
            continue;
        };
        let name = map_str(m, &["name", "filename", "fileName"]);
        let url = map_str(m, &["contentUrl", "url", "href"]);
        if name.is_some() || url.is_some() {
            out.push((name, url));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_array() {
        let raw = br#"[
          {"id":"1","body":"hi","from":"a@x.com","timestamp":"2024-06-01T10:00:00Z","team":"T","channel":"C"},
          {"id":"2","content":"day2","from":"b@x.com","sentDateTime":"2024-06-02T11:00:00Z","team":"T","channel":"C"}
        ]"#;
        let msgs = parse_teams_json(raw, 50).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_ne!(msgs[0].conversation_id, msgs[1].conversation_id);
        assert!(msgs[0].plain_text.contains("hi"));
    }

    #[test]
    fn unknown_schema_fails() {
        let raw = br#"{"foo": 1}"#;
        assert!(parse_teams_json(raw, 50).is_err());
    }

    #[test]
    fn max_messages_exceeded_errors_no_partial() {
        let raw = br#"[
          {"id":"1","body":"a","timestamp":"2024-06-01T10:00:00Z"},
          {"id":"2","body":"b","timestamp":"2024-06-01T11:00:00Z"}
        ]"#;
        let r = parse_teams_json(raw, 1);
        assert!(r.is_err());
        assert_eq!(
            r.unwrap_err().code(),
            crate::error::codes::MAX_MESSAGES_EXCEEDED
        );
    }

    #[test]
    fn different_chat_ids_same_day_different_conversation_ids() {
        let raw = br#"[
          {"id":"1","body":"hi","chatId":"chat-aaa","timestamp":"2024-06-01T10:00:00Z","chatType":"1:1"},
          {"id":"2","body":"yo","chatId":"chat-bbb","timestamp":"2024-06-01T11:00:00Z","chatType":"dm"}
        ]"#;
        let msgs = parse_teams_json(raw, 50).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_ne!(msgs[0].conversation_id, msgs[1].conversation_id);
        assert_eq!(msgs[0].chat_type, "one_to_one");
        assert_eq!(msgs[1].chat_type, "one_to_one");
        assert_eq!(msgs[0].channel_name.as_deref(), Some("chat-aaa"));
        assert_eq!(msgs[1].channel_name.as_deref(), Some("chat-bbb"));
    }
}
