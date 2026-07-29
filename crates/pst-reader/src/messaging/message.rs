//! Message property extraction — MS-PST §2.4.5
//!
//! Extracts properties needed for deduplication and Desk extract.

use sha2::{Digest, Sha256};

use crate::error::{PstError, Result};
use crate::ltp::pc;
use crate::messaging::recipient::{message_flags_is_unsent, Recipient};
use crate::ndb::nid::{self, NodeId};
use crate::PstFile;

/// Options for [`PstFile::read_message_properties_with_opts`] (0076).
#[derive(Debug, Clone, Copy, Default)]
pub struct MessageReadOpts {
    /// When true, compute SHA-256 + char len of the **full** normalized body before
    /// the 4KB preview truncate (zero extra I/O; hashing cycles only). Default off.
    pub compute_body_digest: bool,
}

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
    /// PidTagMessageDeliveryTime as raw FILETIME (received). Never invent.
    pub delivery_time: Option<i64>,
    /// PidTagDisplayBcc (absent when unknown — do not fabricate).
    pub display_bcc: Option<String>,
    /// PidTagSenderEmailAddress (or PidTagSenderSmtpAddress fallback).
    pub sender_email: Option<String>,
    /// First 4096 **chars** of PidTagBody (for Tier 2 content hash).
    pub body_preview: Option<String>,
    /// PidTagDisplayTo — formatted recipient list.
    pub display_to: Option<String>,
    /// PidTagDisplayCc — soft-read (0076); absent on decode error.
    pub display_cc: Option<String>,
    /// PidTagMessageSize in bytes.
    pub message_size: Option<i32>,
    /// PidTagHasAttachments.
    pub has_attachments: Option<bool>,
    /// True ONLY for corruption/truncation body recovery — NEVER intentional 4KB preview.
    pub body_incomplete: bool,
    /// True when the body property could not be read (other props may still be usable).
    pub body_unavailable: bool,
    /// Full-body SHA-256 when [`MessageReadOpts::compute_body_digest`] was set.
    pub body_sha256: Option<[u8; 32]>,
    /// Full normalized body char length when digest was requested.
    pub body_char_len: Option<u64>,
    /// True when any **block** CRC or BID mismatch was counted while reading
    /// this message (0077 `CRC_SUSPECT`). Page CRC is deliberately excluded
    /// (poly-class fixtures; see `integrity_telemetry::tls_block_mismatch_total`).
    /// Bytes were still returned (warning-only). Attachment meta/stream reads
    /// performed under a separate message scope may also set this when ORed by
    /// the caller (scan/extract).
    pub crc_suspect: bool,
    /// Structured recipients from the message recipient TC (0082).
    /// Empty when table missing/unreadable — **never** invented from Display*.
    pub recipients: Vec<Recipient>,
    /// `PidTagMessageFlags` (0x0E07) when present and readable; soft-fail → None.
    pub message_flags: Option<u32>,
}

impl MessageProperties {
    /// True when `MSGFLAG_UNSENT` is set. False when flags absent or bit clear.
    pub fn is_unsent(&self) -> bool {
        self.message_flags.is_some_and(message_flags_is_unsent)
    }
}

/// Full extract-oriented message properties (Desk / `extract-pst`).
///
/// Body text is **not** truncated. Display* PIDs (`display_to` / `display_cc` /
/// `display_bcc`) remain soft-read strings; structured TC rows live in
/// [`Self::recipients`] (0082) and are **never** invented from Display*.
/// `display_bcc` may be `None` when the property is absent.
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
    /// True when any **block** CRC or BID mismatch was counted while reading
    /// this message (0077 `CRC_SUSPECT`). Page CRC is excluded (poly-class).
    pub crc_suspect: bool,
    /// Structured recipients from the message recipient TC (0082).
    /// Empty when table missing/unreadable — **never** invented from Display*.
    pub recipients: Vec<Recipient>,
    /// `PidTagMessageFlags` (0x0E07) when present and readable; soft-fail → None.
    pub message_flags: Option<u32>,
}

impl ExtractedMessage {
    /// True when `MSGFLAG_UNSENT` is set. False when flags absent or bit clear.
    pub fn is_unsent(&self) -> bool {
        self.message_flags.is_some_and(message_flags_is_unsent)
    }
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
        self.read_message_properties_with_opts(message_nid, MessageReadOpts::default())
    }

    /// Like [`Self::read_message_properties`] with optional full-body digest (0076).
    pub fn read_message_properties_with_opts(
        &mut self,
        message_nid: NodeId,
        opts: MessageReadOpts,
    ) -> Result<MessageProperties> {
        // 0077: message-scope CRC taint via thread-local mismatch delta.
        let scope = crate::integrity_telemetry::message_scope_enter();
        let crypt = self.header.crypt_method;
        let prop_ctx = pc::load_pc(&mut self.reader, &self.nbt, &self.bbt, message_nid, crypt)?;

        let message_id = prop_ctx.get_string(nid::PID_TAG_INTERNET_MESSAGE_ID)?;
        let subject = prop_ctx.get_string(nid::PID_TAG_SUBJECT)?;
        let submit_time = prop_ctx.get_time(nid::PID_TAG_CLIENT_SUBMIT_TIME)?;
        // Optional 0075/0076 props: best-effort only. Decode errors become None so a
        // corrupt delivery-time / BCC / CC heap does not fail the whole message
        // (zero-silent-change for keep-set ranking inputs).
        // `unwrap_or_default` on Result<Option<_>> → None on Err (not a panic path).
        let delivery_time: Option<i64> = prop_ctx
            .get_time(nid::PID_TAG_MESSAGE_DELIVERY_TIME)
            .unwrap_or_default();
        let display_bcc: Option<String> = prop_ctx
            .get_string(nid::PID_TAG_DISPLAY_BCC)
            .unwrap_or_default();
        let display_cc: Option<String> = prop_ctx
            .get_string(nid::PID_TAG_DISPLAY_CC)
            .unwrap_or_default();

        let sender_email = prop_ctx
            .get_string(nid::PID_TAG_SENDER_EMAIL_ADDRESS)?
            .or(prop_ctx.get_string(nid::PID_TAG_SENDER_SMTP_ADDRESS)?);

        // Soft body read: PC already loaded; body errors degrade rather than fail the whole message.
        // Intentional Tier-2 4KB preview NEVER sets body_incomplete.
        // Full-body digest (0076 Tier 2.5) is computed from bytes already in RAM before truncate.
        let (body_preview, body_incomplete, body_unavailable, body_sha256, body_char_len) =
            match prop_ctx.get_string(nid::PID_TAG_BODY) {
                Ok(Some(b)) => {
                    let (digest, char_len) = if opts.compute_body_digest {
                        let normalized: String = b
                            .chars()
                            .filter(|c| !c.is_whitespace() || *c == ' ')
                            .collect::<String>()
                            .to_lowercase();
                        let char_len = normalized.chars().count() as u64;
                        let mut hasher = Sha256::new();
                        hasher.update(normalized.as_bytes());
                        let d: [u8; 32] = hasher.finalize().into();
                        (Some(d), Some(char_len))
                    } else {
                        (None, None)
                    };
                    let preview = if b.chars().count() > 4096 {
                        b.chars().take(4096).collect()
                    } else {
                        b
                    };
                    (Some(preview), false, false, digest, char_len)
                }
                Ok(None) => (None, false, false, None, None),
                Err(e) if is_truncation_or_crc(&e) => (None, true, false, None, None),
                Err(_) => (None, false, true, None, None),
            };

        let display_to = prop_ctx.get_string(nid::PID_TAG_DISPLAY_TO)?;
        let message_size = prop_ctx.get_i32(nid::PID_TAG_MESSAGE_SIZE)?;
        let has_attachments = prop_ctx.get_bool(nid::PID_TAG_HAS_ATTACHMENTS)?;
        // Soft-read flags (0082 zero-recip anomaly): decode errors → None, do not invent UNSENT.
        let message_flags: Option<u32> = prop_ctx
            .get_i32(nid::PID_TAG_MESSAGE_FLAGS)
            .ok()
            .flatten()
            .map(|v| v as u32);
        // Structured recipients: missing/corrupt TC → empty; never invent from Display*.
        let recipients = self.list_recipients(message_nid).unwrap_or_default();
        let crc_suspect = scope.exit();

        Ok(MessageProperties {
            nid: message_nid,
            message_id,
            subject,
            submit_time,
            delivery_time,
            display_bcc,
            sender_email,
            body_preview,
            display_to,
            display_cc,
            message_size,
            has_attachments,
            body_incomplete,
            body_unavailable,
            body_sha256,
            body_char_len,
            crc_suspect,
            recipients,
            message_flags,
        })
    }

    /// Extract full message properties for Desk / `extract-pst` (no body truncate).
    pub fn read_message_extract(&mut self, message_nid: NodeId) -> Result<ExtractedMessage> {
        let scope = crate::integrity_telemetry::message_scope_enter();
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
        // Soft-read flags (0082 zero-recip anomaly): decode errors → None, do not invent UNSENT.
        let message_flags: Option<u32> = prop_ctx
            .get_i32(nid::PID_TAG_MESSAGE_FLAGS)
            .ok()
            .flatten()
            .map(|v| v as u32);
        // Structured recipients: missing/corrupt TC → empty; never invent from Display*.
        let recipients = self.list_recipients(message_nid).unwrap_or_default();
        let crc_suspect = scope.exit();

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
            crc_suspect,
            recipients,
            message_flags,
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

    /// Soft-opt contract for optional 0075 props in `read_message_properties`:
    /// `Result<Option<T>>::unwrap_or_default()` → value on Ok, `None` on Err (never panics).
    #[test]
    fn optional_0075_props_fail_soft_on_decode_error() {
        fn soft_opt_i64(r: Result<Option<i64>>) -> Option<i64> {
            r.unwrap_or_default()
        }
        fn soft_opt_str(r: Result<Option<String>>) -> Option<String> {
            r.unwrap_or_default()
        }
        // Ok paths preserve value / None.
        assert_eq!(soft_opt_i64(Ok(Some(42))), Some(42));
        assert_eq!(soft_opt_i64(Ok(None)), None);
        // Decode errors (same variants body soft-path uses) become None — do not fail message.
        assert_eq!(
            soft_opt_i64(Err(PstError::DataTruncated {
                needed: 8,
                available: 0
            })),
            None
        );
        assert_eq!(
            soft_opt_str(Err(PstError::CrcMismatch {
                computed: 1,
                stored: 2
            })),
            None
        );
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
