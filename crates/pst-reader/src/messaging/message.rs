//! Message property extraction — MS-PST §2.4.5
//!
//! Extracts the properties needed for deduplication from a message node.

use crate::error::Result;
use crate::ltp::pc;
use crate::ndb::nid::{self, NodeId};
use crate::PstFile;

/// Extracted message properties for dedup processing.
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
    /// First 4096 bytes of PidTagBody (for Tier 2 content hash).
    pub body_preview: Option<String>,
    /// PidTagDisplayTo — formatted recipient list.
    pub display_to: Option<String>,
    /// PidTagMessageSize in bytes.
    pub message_size: Option<i32>,
    /// PidTagHasAttachments.
    pub has_attachments: Option<bool>,
}

impl PstFile {
    /// Extract dedup-relevant properties from a single message node.
    pub fn read_message_properties(&mut self, message_nid: NodeId) -> Result<MessageProperties> {
        let crypt = self.header.crypt_method;
        let prop_ctx = pc::load_pc(&mut self.reader, &self.nbt, &self.bbt, message_nid, crypt)?;

        let message_id = prop_ctx.get_string(nid::PID_TAG_INTERNET_MESSAGE_ID)?;
        let subject = prop_ctx.get_string(nid::PID_TAG_SUBJECT)?;
        let submit_time = prop_ctx.get_time(nid::PID_TAG_CLIENT_SUBMIT_TIME)?;

        let sender_email = prop_ctx
            .get_string(nid::PID_TAG_SENDER_EMAIL_ADDRESS)?
            .or(prop_ctx.get_string(nid::PID_TAG_SENDER_SMTP_ADDRESS)?);

        let body_full = prop_ctx.get_string(nid::PID_TAG_BODY)?;
        let body_preview = body_full.map(|b| {
            if b.len() > 4096 {
                b[..4096].to_string()
            } else {
                b
            }
        });

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
        })
    }
}
