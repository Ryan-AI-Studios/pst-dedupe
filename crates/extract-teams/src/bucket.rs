//! Day-bucketed `conversation_id` (track 0055 lock).
//!
//! ```text
//! components joined by "\0":
//!   team_key (or empty)
//!   channel_or_chat_key (channel name / chat id / "unknown")
//!   bucket_date (YYYY-MM-DD UTC of sent_at, or "unknown")
//!   thread_key_if_any (parent/export thread id or empty)
//!
//! conversation_id = lowercase hex sha256 of UTF-8 bytes of joined string
//! conversation_bucket_date = bucket_date string (denorm)
//! ```

use chrono::{DateTime, NaiveDate, Utc};
use sha2::{Digest, Sha256};

/// Null separator used between hash components (frozen).
pub const CONV_SEP: &str = "\0";

/// Keys used to build a conversation identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversationKeys {
    pub team_key: String,
    pub channel_or_chat_key: String,
    pub bucket_date: String,
    pub thread_key_if_any: String,
}

impl ConversationKeys {
    /// Build keys from optional export fields + sent_at RFC3339 (or similar).
    pub fn from_parts(
        team: Option<&str>,
        channel_or_chat: Option<&str>,
        sent_at: Option<&str>,
        thread_key: Option<&str>,
    ) -> Self {
        let team_key = team
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();
        let channel_or_chat_key = channel_or_chat
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .to_string();
        let bucket_date = utc_day_bucket(sent_at);
        let thread_key_if_any = thread_key
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();
        Self {
            team_key,
            channel_or_chat_key,
            bucket_date,
            thread_key_if_any,
        }
    }

    /// Preimage bytes for hashing.
    pub fn preimage(&self) -> String {
        format!(
            "{}{sep}{}{sep}{}{sep}{}",
            self.team_key,
            self.channel_or_chat_key,
            self.bucket_date,
            self.thread_key_if_any,
            sep = CONV_SEP
        )
    }

    /// Lowercase hex sha256 of UTF-8 preimage.
    pub fn conversation_id(&self) -> String {
        let dig = Sha256::digest(self.preimage().as_bytes());
        dig.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Normalize export / PST type strings to the frozen `chat_type` enum.
///
/// Canonical values: `one_to_one` | `group` | `channel` | `meeting` | `unknown`.
/// Aliases (case-insensitive): `1:1`/`dm`/`direct` → one_to_one; `team` → channel;
/// `groupchat` → group; etc. Unrecognized values map to `unknown`.
pub fn normalize_chat_type(raw: Option<&str>) -> String {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return "unknown".into();
    };
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "one_to_one" | "1:1" | "1-1" | "1on1" | "one-to-one" | "onetoone" | "dm" | "direct"
        | "personal" | "private" => "one_to_one".into(),
        "group" | "groupchat" | "group_chat" | "group-chat" => "group".into(),
        "channel" | "team" | "space" => "channel".into(),
        "meeting" | "call" | "conference" => "meeting".into(),
        "unknown" => "unknown".into(),
        _ => "unknown".into(),
    }
}

/// UTC calendar day `YYYY-MM-DD` of `sent_at`, or `"unknown"`.
pub fn utc_day_bucket(sent_at: Option<&str>) -> String {
    let Some(raw) = sent_at.map(str::trim).filter(|s| !s.is_empty()) else {
        return "unknown".into();
    };
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return dt.with_timezone(&Utc).format("%Y-%m-%d").to_string();
    }
    // Accept bare dates and common space-separated forms.
    if let Ok(d) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return d.format("%Y-%m-%d").to_string();
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S %z") {
        return dt.with_timezone(&Utc).format("%Y-%m-%d").to_string();
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S") {
        return naive.date().format("%Y-%m-%d").to_string();
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return naive.date().format("%Y-%m-%d").to_string();
    }
    "unknown".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_day_same_keys_same_id() {
        let a = ConversationKeys::from_parts(
            Some("Team Alpha"),
            Some("General"),
            Some("2024-06-01T10:00:00Z"),
            None,
        );
        let b = ConversationKeys::from_parts(
            Some("Team Alpha"),
            Some("General"),
            Some("2024-06-01T23:59:59Z"),
            None,
        );
        assert_eq!(a.bucket_date, "2024-06-01");
        assert_eq!(b.bucket_date, "2024-06-01");
        assert_eq!(a.conversation_id(), b.conversation_id());
    }

    #[test]
    fn different_utc_days_different_ids() {
        let a = ConversationKeys::from_parts(
            Some("Team Alpha"),
            Some("General"),
            Some("2024-06-01T10:00:00Z"),
            None,
        );
        let b = ConversationKeys::from_parts(
            Some("Team Alpha"),
            Some("General"),
            Some("2024-06-02T11:00:00Z"),
            None,
        );
        assert_eq!(a.bucket_date, "2024-06-01");
        assert_eq!(b.bucket_date, "2024-06-02");
        assert_ne!(a.conversation_id(), b.conversation_id());
    }

    #[test]
    fn missing_timestamp_unknown_bucket() {
        let k = ConversationKeys::from_parts(Some("T"), Some("C"), None, None);
        assert_eq!(k.bucket_date, "unknown");
        assert!(!k.conversation_id().is_empty());
    }

    #[test]
    fn normalize_chat_type_aliases() {
        assert_eq!(normalize_chat_type(Some("1:1")), "one_to_one");
        assert_eq!(normalize_chat_type(Some("dm")), "one_to_one");
        assert_eq!(normalize_chat_type(Some("team")), "channel");
        assert_eq!(normalize_chat_type(Some("groupchat")), "group");
        assert_eq!(normalize_chat_type(Some("Meeting")), "meeting");
        assert_eq!(normalize_chat_type(Some("weird")), "unknown");
        assert_eq!(normalize_chat_type(None), "unknown");
    }
}
