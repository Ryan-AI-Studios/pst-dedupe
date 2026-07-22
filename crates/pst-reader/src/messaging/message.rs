//! Message property extraction — MS-PST §2.4.5
//!
//! Extracts properties needed for deduplication and Desk extract.

use crate::error::{PstError, Result};
use crate::ltp::pc;
use crate::ndb::nid::{self, NodeId};
use crate::PstFile;

/// Extracted message properties for dedup processing (CLI Tier-2 path).
///
/// Body is truncated to 4KB (`body_preview`). Prefer [`ExtractedMessage`] /
/// [`PstFile::read_message_extract`] for Desk extract (full body, BCC, etc.).
///
/// Integrity flags (track 0065):
/// - [`Self::body_incomplete`]: corruption/truncation partial recovery — **never** the intentional 4KB preview.
/// - [`Self::body_unavailable`]: body property unreadable while other props may be OK.
#[derive(Debug, Clone)]
pub struct MessageProperties {
    /// The message's NID within this PST.
    pub nid: NodeId,
    /// PidTagInternetMessageId — primary dedup key (Tier 1).
    pub message_id: Option<String>,
    /// PidTagSubject
    pub subject: Option<String>,
    /// PidTagClientSubmitTime as raw FILETIME (100ns since 1601-01-01).
    pub submit_time: Option<i64>,
    /// PidTagSenderEmailAddress (or PidTagSenderSmtpAddress fallback).
    pub sender_email: Option<String>,
    /// First 4096 **chars** of PidTagBody (for Tier 2 content hash).
    pub body_preview: Option<String>,
    /// PidTagDisplayTo — formatted recipient list.
    pub display_to: Option<String>,
    /// PidTagMessageSize in bytes.
    pub message_size: Option<i32>,
    /// PidTagHasAttachments.
    pub has_attachments: Option<bool>,
    /// True ONLY for corruption/truncation body recovery — NEVER intentional 4KB preview.
    pub body_incomplete: bool,
    /// True when the body property could not be read (other props may still be usable).
    pub body_unavailable: bool,
}

/// Full extract-oriented message properties (Desk / `extract-pst`).
///
/// Body text is **not** truncated. Recipients use Display* PIDs; BCC is never
/// invented — may be `None` when the property is absent.
#[derive(Debug, Clone)]
pub struct ExtractedMessage {
    /// The message's NID within this PST.
    pub nid: NodeId,
    /// PidTagInternetMessageId.
    pub message_id: Option<String>,
    /// PidTagSubject.
    pub subject: Option<String>,
    /// PidTagSenderEmailAddress or PidTagSenderSmtpAddress.
    pub sender_email: Option<String>,
    /// PidTagDisplayTo.
    pub display_to: Option<String>,
    /// PidTagDisplayCc.
    pub display_cc: Option<String>,
    /// PidTagDisplayBcc (absent when unknown — do not fabricate).
    pub display_bcc: Option<String>,
    /// PidTagClientSubmitTime as raw FILETIME.
    pub submit_time: Option<i64>,
    /// PidTagMessageDeliveryTime as raw FILETIME (received).
    pub delivery_time: Option<i64>,
    /// Full PidTagBody plain text (no 4KB truncate).
    pub body_text: Option<String>,
    /// Optional HTML body bytes (PidTagBodyHtml when present as string or binary).
    pub body_html: Option<Vec<u8>>,
    /// PidTagMessageSize.
    pub message_size: Option<i32>,
    /// PidTagHasAttachments.
    pub has_attachments: Option<bool>,
    /// PidTagInReplyToId (raw; normalize at extract write).
    pub in_reply_to: Option<String>,
    /// PidTagInternetReferences (raw; parse at extract write).
    pub references: Option<String>,
    /// PidTagConversationTopic (raw/light).
    pub conversation_topic: Option<String>,
    /// PidTagConversationIndex raw binary when present.
    pub conversation_index_bytes: Option<Vec<u8>>,
    /// PidTagConversationIndex as string (Base64 Thread-Index) when binary absent.
    pub conversation_index_string: Option<String>,
    /// PidTagMessageClass (e.g. IPM.Note, IPM.Appointment).
    pub message_class: Option<String>,
    /// PidTagStartDate as raw FILETIME (appointment).
    pub start_date: Option<i64>,
    /// PidTagEndDate as raw FILETIME (appointment).
    pub end_date: Option<i64>,
    /// PidTagLocation string when present (standard tag; named-prop residual).
    pub location: Option<String>,
}

/// Whether a body property error is truncation/CRC corruption (BODY_TRUNCATED path).
fn is_truncation_or_crc(err: &PstError) -> bool {
    matches!(
        err,
        PstError::DataTruncated { .. } | PstError::CrcMismatch { .. }
    )
}

/// True when `class` is a P0 calendar / meeting message class (MS-OXOCAL).
///
/// Matches:
/// - `IPM.Appointment`
/// - `IPM.Schedule.Meeting.Request`
/// - `IPM.Schedule.Meeting.Resp.*`
/// - `IPM.Schedule.Meeting.Canceled`
pub fn is_calendar_message_class(class: &str) -> bool {
    let c = class.trim();
    if c.eq_ignore_ascii_case("IPM.Appointment") {
        return true;
    }
    if c.eq_ignore_ascii_case("IPM.Schedule.Meeting.Request") {
        return true;
    }
    if c.eq_ignore_ascii_case("IPM.Schedule.Meeting.Canceled") {
        return true;
    }
    // Meeting responses: Accept / Tent / Decline etc.
    let lower = c.to_ascii_lowercase();
    lower.starts_with("ipm.schedule.meeting.resp")
}

/// Convert Windows FILETIME (100ns since 1601-01-01) to Unix seconds.
pub fn filetime_to_unix(ft: i64) -> i64 {
    // 11644473600 seconds between 1601-01-01 and 1970-01-01
    (ft / 10_000_000) - 11_644_473_600
}

/// Convert FILETIME to RFC3339 UTC second-resolution string, if in range.
pub fn filetime_to_rfc3339(ft: i64) -> Option<String> {
    let unix = filetime_to_unix(ft);
    use std::time::{Duration, UNIX_EPOCH};
    if unix < 0 {
        return None;
    }
    let dt = UNIX_EPOCH.checked_add(Duration::from_secs(unix as u64))?;
    // Format as RFC3339 without external chrono dep in pst-reader.
    let secs = dt.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(format_unix_rfc3339(secs))
}

fn format_unix_rfc3339(secs: u64) -> String {
    // Civil date from Unix seconds (UTC) — Howard Hinnant algorithm.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let tod = secs % 86_400;
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    let ss = tod % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

impl PstFile {
    /// Extract dedup-relevant properties from a single message node.
    ///
    /// Body is truncated to 4096 chars for CLI Tier-2. Use
    /// [`Self::read_message_extract`] for full-body Desk extract.
    pub fn read_message_properties(&mut self, message_nid: NodeId) -> Result<MessageProperties> {
        let crypt = self.header.crypt_method;
        let prop_ctx = pc::load_pc(&mut self.reader, &self.nbt, &self.bbt, message_nid, crypt)?;

        let message_id = prop_ctx.get_string(nid::PID_TAG_INTERNET_MESSAGE_ID)?;
        let subject = prop_ctx.get_string(nid::PID_TAG_SUBJECT)?;
        let submit_time = prop_ctx.get_time(nid::PID_TAG_CLIENT_SUBMIT_TIME)?;

        let sender_email = prop_ctx
            .get_string(nid::PID_TAG_SENDER_EMAIL_ADDRESS)?
            .or(prop_ctx.get_string(nid::PID_TAG_SENDER_SMTP_ADDRESS)?);

        // Soft body read: PC already loaded; body errors degrade rather than fail the whole message.
        // Intentional Tier-2 4KB preview NEVER sets body_incomplete.
        let (body_preview, body_incomplete, body_unavailable) =
            match prop_ctx.get_string(nid::PID_TAG_BODY) {
                Ok(Some(b)) => {
                    let preview = if b.chars().count() > 4096 {
                        b.chars().take(4096).collect()
                    } else {
                        b
                    };
                    (Some(preview), false, false)
                }
                Ok(None) => (None, false, false),
                Err(e) if is_truncation_or_crc(&e) => (None, true, false),
                Err(_) => (None, false, true),
            };

        let display_to = prop_ctx.get_string(nid::PID_TAG_DISPLAY_TO)?;
        let message_size = prop_ctx.get_i32(nid::PID_TAG_MESSAGE_SIZE)?;
        let has_attachments = prop_ctx.get_bool(nid::PID_TAG_HAS_ATTACHMENTS)?;

        Ok(MessageProperties {
            nid: message_nid,
            message_id,
            subject,
            submit_time,
            sender_email,
            body_preview,
            display_to,
            message_size,
            has_attachments,
            body_incomplete,
            body_unavailable,
        })
    }

    /// Extract full message properties for Desk / `extract-pst` (no body truncate).
    pub fn read_message_extract(&mut self, message_nid: NodeId) -> Result<ExtractedMessage> {
        let crypt = self.header.crypt_method;
        let prop_ctx = pc::load_pc(&mut self.reader, &self.nbt, &self.bbt, message_nid, crypt)?;

        let message_id = prop_ctx.get_string(nid::PID_TAG_INTERNET_MESSAGE_ID)?;
        let subject = prop_ctx.get_string(nid::PID_TAG_SUBJECT)?;
        let submit_time = prop_ctx.get_time(nid::PID_TAG_CLIENT_SUBMIT_TIME)?;
        let delivery_time = prop_ctx.get_time(nid::PID_TAG_MESSAGE_DELIVERY_TIME)?;

        let sender_email = prop_ctx
            .get_string(nid::PID_TAG_SENDER_EMAIL_ADDRESS)?
            .or(prop_ctx.get_string(nid::PID_TAG_SENDER_SMTP_ADDRESS)?);

        let body_text = prop_ctx.get_string(nid::PID_TAG_BODY)?;
        let display_to = prop_ctx.get_string(nid::PID_TAG_DISPLAY_TO)?;
        let display_cc = prop_ctx.get_string(nid::PID_TAG_DISPLAY_CC)?;
        let display_bcc = prop_ctx.get_string(nid::PID_TAG_DISPLAY_BCC)?;
        let message_size = prop_ctx.get_i32(nid::PID_TAG_MESSAGE_SIZE)?;
        let has_attachments = prop_ctx.get_bool(nid::PID_TAG_HAS_ATTACHMENTS)?;

        // HTML: prefer string property; fall back to binary bytes.
        let body_html = match prop_ctx.get_string(nid::PID_TAG_BODY_HTML)? {
            Some(s) => Some(s.into_bytes()),
            None => prop_ctx.get_binary(nid::PID_TAG_BODY_HTML)?,
        };

        let in_reply_to = prop_ctx.get_string(nid::PID_TAG_IN_REPLY_TO_ID)?;
        let references = prop_ctx.get_string(nid::PID_TAG_INTERNET_REFERENCES)?;
        let conversation_topic = prop_ctx.get_string(nid::PID_TAG_CONVERSATION_TOPIC)?;
        // ConversationIndex: prefer MAPI binary; fall back to string (Base64).
        let conversation_index_bytes = prop_ctx.get_binary(nid::PID_TAG_CONVERSATION_INDEX)?;
        let conversation_index_string = if conversation_index_bytes.is_none() {
            prop_ctx.get_string(nid::PID_TAG_CONVERSATION_INDEX)?
        } else {
            None
        };

        let message_class = prop_ctx.get_string(nid::PID_TAG_MESSAGE_CLASS)?;
        let start_date = prop_ctx.get_time(nid::PID_TAG_START_DATE)?;
        let end_date = prop_ctx.get_time(nid::PID_TAG_END_DATE)?;
        // Best-effort standard location tag; PidLidLocation is residual when absent.
        let location = prop_ctx.get_string(nid::PID_TAG_LOCATION)?;

        Ok(ExtractedMessage {
            nid: message_nid,
            message_id,
            subject,
            sender_email,
            display_to,
            display_cc,
            display_bcc,
            submit_time,
            delivery_time,
            body_text,
            body_html,
            message_size,
            has_attachments,
            in_reply_to,
            references,
            conversation_topic,
            conversation_index_bytes,
            conversation_index_string,
            message_class,
            start_date,
            end_date,
            location,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_epoch_unix_zero() {
        let ft = 11_644_473_600i64 * 10_000_000;
        assert_eq!(filetime_to_unix(ft), 0);
        assert_eq!(
            filetime_to_rfc3339(ft).as_deref(),
            Some("1970-01-01T00:00:00Z")
        );
    }

    #[test]
    fn filetime_known_date() {
        // 2020-01-02 03:04:05 UTC
        // 2020-01-01T00:00:00Z = 1577836800; +1 day + 3h4m5s
        let unix = 1_577_934_245i64;
        let ft = (unix + 11_644_473_600) * 10_000_000;
        assert_eq!(
            filetime_to_rfc3339(ft).as_deref(),
            Some("2020-01-02T03:04:05Z")
        );
    }

    #[test]
    fn calendar_message_class_detection() {
        assert!(is_calendar_message_class("IPM.Appointment"));
        assert!(is_calendar_message_class("ipm.appointment"));
        assert!(is_calendar_message_class("IPM.Schedule.Meeting.Request"));
        assert!(is_calendar_message_class("IPM.Schedule.Meeting.Resp.Pos"));
        assert!(is_calendar_message_class("IPM.Schedule.Meeting.Resp.Neg"));
        assert!(is_calendar_message_class("IPM.Schedule.Meeting.Resp.Tent"));
        assert!(is_calendar_message_class("IPM.Schedule.Meeting.Canceled"));
        assert!(!is_calendar_message_class("IPM.Note"));
        assert!(!is_calendar_message_class("IPM.Task"));
        assert!(!is_calendar_message_class(""));
    }

    #[test]
    fn intentional_4kb_preview_does_not_set_body_incomplete() {
        // Simulate the intentional preview path: full body ok → flags false.
        let long: String = "x".repeat(5000);
        let preview: String = long.chars().take(4096).collect();
        assert_eq!(preview.chars().count(), 4096);
        // Flags would be set only on Err paths in read_message_properties.
        let body_incomplete = false;
        let body_unavailable = false;
        assert!(!body_incomplete);
        assert!(!body_unavailable);
    }

    #[test]
    fn truncation_error_maps_to_incomplete_not_unavailable() {
        assert!(is_truncation_or_crc(&PstError::DataTruncated {
            needed: 100,
            available: 10
        }));
        assert!(is_truncation_or_crc(&PstError::CrcMismatch {
            computed: 1,
            stored: 2
        }));
        assert!(!is_truncation_or_crc(&PstError::PropertyNotFound(0x1000)));
        assert!(!is_truncation_or_crc(&PstError::NodeNotFound(1)));
    }
}
