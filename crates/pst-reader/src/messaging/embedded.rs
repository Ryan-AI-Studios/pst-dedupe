//! Bounded embedded-message identity (0090) and nested export extract (0094).
//!
//! Method-5 (`ATTACH_EMBEDDED_MSG`) nested messages are **subnode objects under
//! the attachment**, not NBT entries. [`PstFile::read_message_properties`] cannot
//! resolve them. This module loads identity fields and full export fields from a
//! [`MessageNodeRef`] rooted at either an NBT message or a nested subnode message.
//!
//! Resolve prefers `PidTagAttachDataObject` (PtypObject `0x3701`/`0x000D`); subnode
//! `NormalMessage` scan remains a fallback for 0069-era files without the object.

use sha2::{Digest, Sha256};

use crate::error::{PstError, Result};
use crate::ltp::pc::{self, PropContext};
use crate::ltp::tc::TableContext;
use crate::messaging::attachment::{classify_attach_pc, AttachmentDataReader, AttachmentInfo};
use crate::messaging::message::is_truncation_or_crc;
use crate::messaging::recipient::{opt_row_string, Recipient, RecipientType};
use crate::ndb::block::{self, BlockId, SubnodeEntry};
use crate::ndb::nid::{self, NidType, NodeId};
use crate::PstFile;

/// Max nested embed depth for identity parse (align D-0067 / engine constant).
pub const MAX_EMBEDDED_IDENTITY_DEPTH: u8 = 3;

/// Per-nest payload ceiling for export extract (0094): body + HTML + child bytes.
pub const MAX_NESTED_EXPORT_PAYLOAD_BYTES: u64 = 32 * 1024 * 1024;

/// Message root that is either an NBT entry or a nested subnode message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageNodeRef {
    pub nid: NodeId,
    pub bid_data: BlockId,
    pub bid_sub: BlockId,
}

/// Child attachment row under an embedded (or top-level) message node.
#[derive(Debug, Clone)]
pub struct EmbeddedChildAttach {
    pub nid: NodeId,
    pub filename: String,
    pub size: u32,
    pub mime_tag: Option<String>,
    pub attach_method: Option<i32>,
    pub is_inline: bool,
    pub is_cloud_link: bool,
}

/// Present RecipientTable / AttachmentTable entry with null `bid_data` is corrupt.
/// Genuinely absent table types remain Ok(empty) at the caller.
fn require_present_table_data_bid(entry: &SubnodeEntry) -> Result<()> {
    if entry.bid_data.is_null() {
        return Err(PstError::BlockNotFound(0));
    }
    Ok(())
}

impl EmbeddedChildAttach {
    /// True when this child should be treated as a nested email for identity.
    pub fn is_embedded_email(&self) -> bool {
        if self.attach_method == Some(nid::ATTACH_EMBEDDED_MSG) {
            return true;
        }
        self.mime_tag
            .as_deref()
            .is_some_and(|m| m.to_ascii_lowercase().contains("message/rfc822"))
    }
}

/// Identity fields loaded from a nested message object (budgeted; fail-closed).
#[derive(Debug, Clone)]
pub struct EmbeddedIdentityFields {
    pub subject: Option<String>,
    pub submit_time: Option<i64>,
    pub sender_email: Option<String>,
    pub display_to: Option<String>,
    pub display_cc: Option<String>,
    pub display_bcc: Option<String>,
    /// Full plain body when available (for digest); not truncated to 4KB.
    pub body_plain: Option<String>,
    pub body_sha256: Option<[u8; 32]>,
    pub body_char_len: Option<u64>,
    pub recipients: Vec<Recipient>,
    pub child_attachments: Vec<EmbeddedChildAttach>,
    pub crc_suspect: bool,
}

/// Full nested export fields (0094) — reader-local; CLI maps to NestedCanonicalMessage.
#[derive(Debug, Clone)]
pub struct EmbeddedExportFields {
    pub subject: Option<String>,
    pub sender: Option<String>,
    pub display_to: Option<String>,
    pub display_cc: Option<String>,
    pub display_bcc: Option<String>,
    pub recipients: Vec<Recipient>,
    pub message_id: Option<String>,
    pub message_class: Option<String>,
    pub message_flags: Option<u32>,
    pub submit_time: Option<i64>,
    pub body_plain: Option<String>,
    pub body_html: Option<Vec<u8>>,
    pub attachments: Vec<EmbeddedExportAttach>,
    pub body_incomplete: bool,
    pub body_unavailable: bool,
    /// Soft-skipped child attach rows during export list (`fail_on_row_error=false`).
    /// Never invents bytes for omitted children; callers surface fidelity honesty.
    pub attachments_incomplete: bool,
    /// Source nested message root (NID + bids) for child stream keys.
    pub source_msg_nid: NodeId,
    pub bid_data: BlockId,
    pub bid_sub: BlockId,
}

/// Child attach on an export extract (metadata + optional nested export).
#[derive(Debug, Clone)]
pub struct EmbeddedExportAttach {
    pub nid: NodeId,
    pub filename: String,
    pub size: u32,
    pub mime_tag: Option<String>,
    pub attach_method: Option<i32>,
    pub stream_available: bool,
    pub is_cloud_link: bool,
    pub embedded: Option<Box<EmbeddedExportFields>>,
    /// Child method-5 hit remaining-depth / payload budget.
    pub embedded_depth_limited: bool,
}

impl PstFile {
    /// Resolve an NBT message NID into a [`MessageNodeRef`].
    pub fn message_node_from_nbt(&self, message_nid: NodeId) -> Result<MessageNodeRef> {
        let entry = self
            .nbt
            .get(message_nid)
            .ok_or(PstError::NodeNotFound(message_nid.0))?;
        Ok(MessageNodeRef {
            nid: message_nid,
            bid_data: entry.bid_data,
            bid_sub: entry.bid_sub,
        })
    }

    /// Resolve the nested message object under a method-5 attachment.
    ///
    /// Primary: `PidTagAttachDataObject` PtypObject on the attach PC → subnode NID.
    /// Fallback: scan attach subnodes for first `NormalMessage` (0069-era files).
    pub fn resolve_embedded_root(
        &mut self,
        parent: &MessageNodeRef,
        attach_nid: NodeId,
    ) -> Result<MessageNodeRef> {
        if parent.bid_sub.is_null() {
            return Err(PstError::NoSubnodeBTree(parent.nid.0));
        }
        let att_entry =
            block::find_subnode_entry(&mut self.reader, &self.bbt, parent.bid_sub, attach_nid)?
                .ok_or(PstError::SubnodeNotFound(attach_nid.0))?;
        self.resolve_embedded_from_attach_entry(&att_entry)
    }

    /// Convenience: resolve embed under an NBT parent message + attach NID.
    pub fn resolve_embedded_root_nbt(
        &mut self,
        parent_msg_nid: NodeId,
        attach_nid: NodeId,
    ) -> Result<MessageNodeRef> {
        let parent = self.message_node_from_nbt(parent_msg_nid)?;
        self.resolve_embedded_root(&parent, attach_nid)
    }

    fn resolve_embedded_from_attach_entry(
        &mut self,
        att_entry: &SubnodeEntry,
    ) -> Result<MessageNodeRef> {
        if att_entry.bid_sub.is_null() {
            return Err(PstError::NoSubnodeBTree(att_entry.nid.0));
        }
        let crypt = self.header.crypt_method;
        // Prefer PtypObject 0x3701 discovery (MS-PST §2.4.6.2.2 / §2.3.3.5).
        if !att_entry.bid_data.is_null() {
            if let Ok(att_data) =
                block::read_block_data(&mut self.reader, &self.bbt, att_entry.bid_data, crypt)
            {
                if let Ok(pc) = PropContext::load(att_data) {
                    if let Ok(Some((obj_nid, _ul_size))) =
                        pc.get_object(nid::PID_TAG_ATTACH_DATA_BINARY)
                    {
                        let target = NodeId(u64::from(obj_nid));
                        if let Ok(Some(nested)) = block::find_subnode_entry(
                            &mut self.reader,
                            &self.bbt,
                            att_entry.bid_sub,
                            target,
                        ) {
                            return Ok(MessageNodeRef {
                                nid: nested.nid,
                                bid_data: nested.bid_data,
                                bid_sub: nested.bid_sub,
                            });
                        }
                    }
                }
            }
        }
        // Fallback: 0069-era files without PidTagAttachDataObject.
        let subs = block::list_subnode_entries(&mut self.reader, &self.bbt, att_entry.bid_sub)?;
        let nested = subs
            .iter()
            .find(|e| matches!(e.nid.nid_type(), NidType::NormalMessage))
            .ok_or(PstError::SubnodeNotFound(att_entry.nid.0))?;
        Ok(MessageNodeRef {
            nid: nested.nid,
            bid_data: nested.bid_data,
            bid_sub: nested.bid_sub,
        })
    }

    /// Load bounded identity fields from a message node (NBT or nested).
    ///
    /// `body_byte_budget` caps nested `PidTagBody` materialization: raw stored
    /// length is checked via [`PropContext::prop_value_byte_len`] before UTF-16
    /// decode, then UTF-8/`String` length is re-checked. Oversize →
    /// [`PstError::ResourceLimit`].
    pub fn read_identity_from_message_node(
        &mut self,
        root: &MessageNodeRef,
        body_byte_budget: u64,
    ) -> Result<EmbeddedIdentityFields> {
        let scope = crate::integrity_telemetry::message_scope_enter();
        let crypt = self.header.crypt_method;
        // Budget before subnode assemble: refuse oversize PidTagBody via
        // block_payload_len_hint (no PropContext.subnodes materialization).
        let prop_ctx = pc::load_pc_from_bids_with_body_budget(
            &mut self.reader,
            &self.bbt,
            root.bid_data,
            root.bid_sub,
            crypt,
            &[nid::PID_TAG_BODY],
            body_byte_budget,
        )?;

        let subject = prop_ctx.get_string(nid::PID_TAG_SUBJECT)?;
        let submit_time = prop_ctx.get_time(nid::PID_TAG_CLIENT_SUBMIT_TIME)?;
        let display_bcc: Option<String> = prop_ctx
            .get_string(nid::PID_TAG_DISPLAY_BCC)
            .unwrap_or_default();
        let display_cc: Option<String> = prop_ctx
            .get_string(nid::PID_TAG_DISPLAY_CC)
            .unwrap_or_default();
        let sender_email = prop_ctx
            .get_string(nid::PID_TAG_SENDER_EMAIL_ADDRESS)?
            .or(prop_ctx.get_string(nid::PID_TAG_SENDER_SMTP_ADDRESS)?);
        let display_to = prop_ctx.get_string(nid::PID_TAG_DISPLAY_TO)?;

        // Preflight body size before UTF-16 → String (avoids decode of oversize bodies).
        if let Some(raw_len) = prop_ctx.prop_value_byte_len(nid::PID_TAG_BODY)? {
            if (raw_len as u64) > body_byte_budget {
                return Err(PstError::ResourceLimit(format!(
                    "embedded PidTagBody raw_len={raw_len} exceeds budget={body_byte_budget}"
                )));
            }
        }

        let (body_plain, body_sha256, body_char_len) = match prop_ctx.get_string(nid::PID_TAG_BODY)
        {
            Ok(Some(b)) => {
                if (b.len() as u64) > body_byte_budget {
                    return Err(PstError::ResourceLimit(format!(
                        "embedded PidTagBody utf8_len={} exceeds budget={body_byte_budget}",
                        b.len()
                    )));
                }
                let normalized: String = b
                    .chars()
                    .filter(|c| !c.is_whitespace() || *c == ' ')
                    .collect::<String>()
                    .to_lowercase();
                let char_len = normalized.chars().count() as u64;
                let mut hasher = Sha256::new();
                hasher.update(normalized.as_bytes());
                let d: [u8; 32] = hasher.finalize().into();
                (Some(b), Some(d), Some(char_len))
            }
            Ok(None) => (None, None, None),
            Err(e) if is_truncation_or_crc(&e) => (None, None, None),
            // Non-budget body read failures stay soft-missing; ResourceLimit is
            // raised by the raw_len / utf8_len preflight above.
            Err(_) => (None, None, None),
        };

        // Identity path: missing recipient table → empty (display-* fallback in
        // hasher); corrupt/unreadable table → Err (CLI maps to unread).
        let recipients = self.list_recipients_from_message_node(root)?;
        // Identity path: fail closed on any unreadable attach PC; prefer
        // AttachmentTable (0x671) row order when present.
        let (child_attachments, _) = self.list_attachments_from_message_node(root, true)?;
        let crc_suspect = scope.exit();

        Ok(EmbeddedIdentityFields {
            subject,
            submit_time,
            sender_email,
            display_to,
            display_cc,
            display_bcc,
            body_plain,
            body_sha256,
            body_char_len,
            recipients,
            child_attachments,
            crc_suspect,
        })
    }

    /// Read embedded-message identity under an NBT parent + method-5 attach NID.
    ///
    /// `body_byte_budget` is forwarded to [`Self::read_identity_from_message_node`].
    /// Pass `u64::MAX` for unbounded identity reads (tests / inspect).
    pub fn read_embedded_message_identity(
        &mut self,
        parent_msg_nid: NodeId,
        attach_nid: NodeId,
        body_byte_budget: u64,
    ) -> Result<EmbeddedIdentityFields> {
        let root = self.resolve_embedded_root_nbt(parent_msg_nid, attach_nid)?;
        self.read_identity_from_message_node(&root, body_byte_budget)
    }

    /// Full nested export extract from a message node (0094).
    ///
    /// `remaining_child_depth` is how many further method-5 levels may be extracted
    /// under this node (0 → mark child embeds depth-limited, do not invent).
    /// `payload_budget` caps body+HTML+declared child bytes for this nest
    /// ([`MAX_NESTED_EXPORT_PAYLOAD_BYTES`] recommended). Exhaustion →
    /// [`PstError::ResourceLimit`] (CLI maps to depth-limit, not unparsed).
    pub fn read_export_from_message_node(
        &mut self,
        root: &MessageNodeRef,
        remaining_child_depth: u32,
        payload_budget: u64,
    ) -> Result<EmbeddedExportFields> {
        let crypt = self.header.crypt_method;
        // Budget plain + HTML subnodes before any assemble into PropContext.
        let prop_ctx = pc::load_pc_from_bids_with_body_budget(
            &mut self.reader,
            &self.bbt,
            root.bid_data,
            root.bid_sub,
            crypt,
            &[nid::PID_TAG_BODY, nid::PID_TAG_BODY_HTML],
            payload_budget,
        )?;

        let subject = prop_ctx.get_string(nid::PID_TAG_SUBJECT)?;
        let submit_time = prop_ctx.get_time(nid::PID_TAG_CLIENT_SUBMIT_TIME)?;
        let display_bcc = prop_ctx
            .get_string(nid::PID_TAG_DISPLAY_BCC)
            .unwrap_or_default();
        let display_cc = prop_ctx
            .get_string(nid::PID_TAG_DISPLAY_CC)
            .unwrap_or_default();
        let sender = prop_ctx
            .get_string(nid::PID_TAG_SENDER_EMAIL_ADDRESS)?
            .or(prop_ctx.get_string(nid::PID_TAG_SENDER_SMTP_ADDRESS)?);
        let display_to = prop_ctx.get_string(nid::PID_TAG_DISPLAY_TO)?;
        let message_id = prop_ctx.get_string(nid::PID_TAG_INTERNET_MESSAGE_ID)?;
        // Do not invent IPM.Note here — writer defaults when mapping if missing.
        let message_class = prop_ctx.get_string(nid::PID_TAG_MESSAGE_CLASS)?;
        let message_flags = prop_ctx
            .get_i32(nid::PID_TAG_MESSAGE_FLAGS)?
            .map(|f| f as u32);

        let mut used: u64 = 0;
        let charge = |used: &mut u64, n: u64, budget: u64, what: &str| -> Result<()> {
            let next = used.saturating_add(n);
            if next > budget {
                return Err(PstError::ResourceLimit(format!(
                    "nested export {what} would use {next} bytes over budget={budget}"
                )));
            }
            *used = next;
            Ok(())
        };

        let mut body_incomplete = false;
        let mut body_unavailable = false;
        let body_plain = match prop_ctx.prop_value_byte_len(nid::PID_TAG_BODY)? {
            Some(raw_len) if (raw_len as u64) > payload_budget.saturating_sub(used) => {
                return Err(PstError::ResourceLimit(format!(
                    "embedded export PidTagBody raw_len={raw_len} exceeds remaining budget"
                )));
            }
            _ => match prop_ctx.get_string(nid::PID_TAG_BODY) {
                Ok(Some(b)) => {
                    charge(&mut used, b.len() as u64, payload_budget, "body_plain")?;
                    Some(b)
                }
                Ok(None) => None,
                Err(e) if is_truncation_or_crc(&e) => {
                    body_incomplete = true;
                    None
                }
                Err(PstError::ResourceLimit(_)) => {
                    return Err(PstError::ResourceLimit(
                        "embedded export body budget".into(),
                    ))
                }
                Err(_) => {
                    body_unavailable = true;
                    None
                }
            },
        };

        // Preflight HTML stored length against remaining budget before clone/decode.
        let body_html = match prop_ctx.prop_value_byte_len(nid::PID_TAG_BODY_HTML)? {
            Some(raw_len) if (raw_len as u64) > payload_budget.saturating_sub(used) => {
                return Err(PstError::ResourceLimit(format!(
                    "embedded export PidTagBodyHtml raw_len={raw_len} exceeds remaining budget"
                )));
            }
            _ => match prop_ctx.get_string(nid::PID_TAG_BODY_HTML) {
                Ok(Some(s)) => {
                    let bytes = s.into_bytes();
                    charge(&mut used, bytes.len() as u64, payload_budget, "body_html")?;
                    Some(bytes)
                }
                Ok(None) => match prop_ctx.get_binary(nid::PID_TAG_BODY_HTML) {
                    Ok(Some(b)) => {
                        charge(&mut used, b.len() as u64, payload_budget, "body_html")?;
                        Some(b)
                    }
                    Ok(None) => None,
                    Err(e) if is_truncation_or_crc(&e) => {
                        body_incomplete = true;
                        None
                    }
                    Err(_) => {
                        body_incomplete = true;
                        None
                    }
                },
                Err(e) if is_truncation_or_crc(&e) => {
                    body_incomplete = true;
                    None
                }
                Err(_) => {
                    body_incomplete = true;
                    None
                }
            },
        };

        // Missing recipient table → empty (0082: never invent from Display*).
        // Present-but-corrupt table → empty + honesty flag (do not look like absent).
        let (recipients, recipients_incomplete) = match self.list_recipients_from_message_node(root)
        {
            Ok(rows) => (rows, false),
            Err(_) => (Vec::new(), true),
        };
        // Soft list for export: prefer honesty flags over failing the whole nest.
        let (child_meta, soft_skipped) = self.list_attachments_from_message_node(root, false)?;
        let attachments_incomplete = soft_skipped > 0;

        let mut attachments = Vec::with_capacity(child_meta.len());
        for child in child_meta {
            charge(
                &mut used,
                u64::from(child.size),
                payload_budget,
                "child_attach_size",
            )?;
            let is_method5 = child.attach_method == Some(nid::ATTACH_EMBEDDED_MSG);
            let stream_available = !is_method5 && !child.is_cloud_link;
            let mut embedded = None;
            let mut embedded_depth_limited = false;
            if is_method5 {
                if remaining_child_depth == 0 {
                    embedded_depth_limited = true;
                } else if let Ok(nested_root) = self.resolve_embedded_root(root, child.nid) {
                    match self.read_export_from_message_node(
                        &nested_root,
                        remaining_child_depth.saturating_sub(1),
                        payload_budget,
                    ) {
                        Ok(fields) => embedded = Some(Box::new(fields)),
                        Err(PstError::ResourceLimit(_)) => {
                            embedded_depth_limited = true;
                        }
                        Err(_) => {
                            // Leave embedded None → unparsed at writer for this child.
                        }
                    }
                }
            }
            attachments.push(EmbeddedExportAttach {
                nid: child.nid,
                filename: child.filename,
                size: child.size,
                mime_tag: child.mime_tag,
                attach_method: child.attach_method,
                stream_available,
                is_cloud_link: child.is_cloud_link,
                embedded,
                embedded_depth_limited,
            });
        }

        if body_plain.is_none() && body_html.is_none() && !body_incomplete {
            // No readable body properties — leave unavailable only when we already
            // saw a hard body error; otherwise both None is honest empty.
        }

        let body_incomplete = body_incomplete || recipients_incomplete;

        Ok(EmbeddedExportFields {
            subject,
            sender,
            display_to,
            display_cc,
            display_bcc,
            recipients,
            message_id,
            message_class,
            message_flags,
            submit_time,
            body_plain,
            body_html,
            attachments,
            body_incomplete,
            body_unavailable,
            attachments_incomplete,
            source_msg_nid: root.nid,
            bid_data: root.bid_data,
            bid_sub: root.bid_sub,
        })
    }

    /// List attachments under a message node (NBT or nested subnode message).
    ///
    /// **Order:** Prefer the per-message AttachmentTable subnode (`NidType::AttachmentTable`
    /// / `0x671`) RowIndex / matrix row order (ascending). Writer stores RowIndex BTH as
    /// key=attach NID → value=row index; [`TableContext::get_row_id`] yields NID per row.
    ///
    /// **Fail-closed (identity):** table present but corrupt/unreadable → `Err`. When
    /// Attachment-typed subnodes exist but the table is **absent**, identity
    /// (`fail_on_row_error=true`) returns `Err`. Soft path may fall back to Attachment
    /// subnode enumeration order (documented residual — not used for identity).
    /// Returns `(attachments, soft_skipped_rows)`. Soft path (`fail_on_row_error=false`)
    /// counts skipped/corrupt rows without inventing bytes; identity path returns
    /// `soft_skipped=0` or `Err`.
    pub fn list_attachments_from_message_node(
        &mut self,
        root: &MessageNodeRef,
        fail_on_row_error: bool,
    ) -> Result<(Vec<EmbeddedChildAttach>, u32)> {
        if root.bid_sub.is_null() {
            return Ok((Vec::new(), 0));
        }
        let sub_entries = block::list_subnode_entries(&mut self.reader, &self.bbt, root.bid_sub)?;
        let provider_npid = self.attachment_provider_type_npid();
        let crypt = self.header.crypt_method;

        let table_entry = sub_entries
            .iter()
            .find(|e| matches!(e.nid.nid_type(), NidType::AttachmentTable));

        if let Some(table_entry) = table_entry {
            return self.list_attachments_via_attach_table(
                table_entry,
                &sub_entries,
                provider_npid,
                crypt,
                fail_on_row_error,
            );
        }

        let has_attach_nodes = sub_entries
            .iter()
            .any(|e| matches!(e.nid.nid_type(), NidType::Attachment));
        if has_attach_nodes {
            if fail_on_row_error {
                // Table expected when Attachment subnodes exist (MS-PST / writer).
                return Err(PstError::SubnodeNotFound(0x671));
            }
            // Soft residual: enumerate Attachment-typed subnodes in B-tree order.
            return self.list_attachments_by_subnode_enum(
                &sub_entries,
                provider_npid,
                crypt,
                fail_on_row_error,
            );
        }
        Ok((Vec::new(), 0))
    }

    fn list_attachments_via_attach_table(
        &mut self,
        table_entry: &SubnodeEntry,
        sub_entries: &[SubnodeEntry],
        provider_npid: Option<u16>,
        crypt: crate::crypto::CryptMethod,
        fail_on_row_error: bool,
    ) -> Result<(Vec<EmbeddedChildAttach>, u32)> {
        // Present table with null data BID is corrupt — not an empty attachment set.
        require_present_table_data_bid(table_entry)?;

        let data =
            block::read_block_data(&mut self.reader, &self.bbt, table_entry.bid_data, crypt)?;
        let subnode_rows = if !table_entry.bid_sub.is_null() {
            let nested =
                block::list_subnode_entries(&mut self.reader, &self.bbt, table_entry.bid_sub)?;
            if nested.is_empty() {
                None
            } else {
                let mut all_rows = Vec::new();
                for entry in &nested {
                    let entry_data =
                        block::read_block_data(&mut self.reader, &self.bbt, entry.bid_data, crypt)?;
                    all_rows.extend_from_slice(&entry_data);
                }
                Some(all_rows)
            }
        } else {
            None
        };

        // Corrupt attachment table → fail closed (never partial identity).
        let table = TableContext::load(data, subnode_rows)?;

        let attach_by_nid: std::collections::HashMap<u64, &SubnodeEntry> = sub_entries
            .iter()
            .filter(|e| matches!(e.nid.nid_type(), NidType::Attachment))
            .map(|e| (e.nid.0, e))
            .collect();

        let mut attachments = Vec::with_capacity(table.row_count());
        let mut soft_skipped = 0u32;
        for row in 0..table.row_count() {
            let Some(row_id) = table.get_row_id(row) else {
                if fail_on_row_error {
                    return Err(PstError::DataTruncated {
                        needed: 4,
                        available: 0,
                    });
                }
                soft_skipped = soft_skipped.saturating_add(1);
                continue;
            };
            let nid = NodeId(u64::from(row_id));
            let Some(entry) = attach_by_nid.get(&nid.0).copied() else {
                if fail_on_row_error {
                    return Err(PstError::SubnodeNotFound(nid.0));
                }
                soft_skipped = soft_skipped.saturating_add(1);
                continue;
            };
            match self.read_embedded_child_from_entry(entry, provider_npid, crypt) {
                Ok(child) => attachments.push(child),
                Err(e) => {
                    if fail_on_row_error {
                        return Err(e);
                    }
                    soft_skipped = soft_skipped.saturating_add(1);
                }
            }
        }
        Ok((attachments, soft_skipped))
    }

    fn list_attachments_by_subnode_enum(
        &mut self,
        sub_entries: &[SubnodeEntry],
        provider_npid: Option<u16>,
        crypt: crate::crypto::CryptMethod,
        fail_on_row_error: bool,
    ) -> Result<(Vec<EmbeddedChildAttach>, u32)> {
        let mut attachments = Vec::new();
        let mut soft_skipped = 0u32;
        for entry in sub_entries {
            if !matches!(entry.nid.nid_type(), NidType::Attachment) {
                continue;
            }
            match self.read_embedded_child_from_entry(entry, provider_npid, crypt) {
                Ok(child) => attachments.push(child),
                Err(e) => {
                    if fail_on_row_error {
                        return Err(e);
                    }
                    soft_skipped = soft_skipped.saturating_add(1);
                }
            }
        }
        Ok((attachments, soft_skipped))
    }

    fn read_embedded_child_from_entry(
        &mut self,
        entry: &SubnodeEntry,
        provider_npid: Option<u16>,
        crypt: crate::crypto::CryptMethod,
    ) -> Result<EmbeddedChildAttach> {
        let att_data = block::read_block_data(&mut self.reader, &self.bbt, entry.bid_data, crypt)?;
        let pc = PropContext::load(att_data)?;

        let filename = pc
            .get_string(nid::PID_TAG_ATTACH_LONG_FILENAME)
            .and_then(|long| {
                if long.is_some() {
                    Ok(long)
                } else {
                    pc.get_string(nid::PID_TAG_ATTACH_FILENAME)
                }
            })?
            .unwrap_or_default();
        let size = pc.get_i32(nid::PID_TAG_ATTACH_SIZE)?.unwrap_or(0) as u32;
        let mime_tag = pc.get_string(nid::PID_TAG_ATTACH_MIME_TAG)?;
        let attach_method = pc.get_i32(nid::PID_TAG_ATTACH_METHOD)?;

        let content_id = pc
            .get_string(nid::PID_TAG_ATTACH_CONTENT_ID)
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty());
        let attach_flags = pc.get_i32(nid::PID_TAG_ATTACH_FLAGS).ok().flatten();
        let hidden = pc.get_bool(nid::PID_TAG_ATTACHMENT_HIDDEN).ok().flatten();
        let is_inline = content_id.is_some()
            || attach_flags
                .map(|f| f & nid::ATT_RENDERED_IN_BODY != 0)
                .unwrap_or(false)
            || hidden.unwrap_or(false);

        let kind = classify_attach_pc(&pc, provider_npid, &filename);
        let is_cloud_link = matches!(
            kind,
            crate::messaging::attachment::AttachKind::CloudLink { .. }
        );

        Ok(EmbeddedChildAttach {
            nid: entry.nid,
            filename,
            size,
            mime_tag,
            attach_method,
            is_inline,
            is_cloud_link,
        })
    }

    /// List recipients under a message node (NBT or nested).
    ///
    /// - Missing recipient table type / empty rows → `Ok([])` (display fallback OK)
    /// - Present table with null `bid_data`, corrupt TC, or block read failure → `Err`
    pub fn list_recipients_from_message_node(
        &mut self,
        root: &MessageNodeRef,
    ) -> Result<Vec<Recipient>> {
        self.list_recipients_from_message_node_inner(root)
    }

    fn list_recipients_from_message_node_inner(
        &mut self,
        root: &MessageNodeRef,
    ) -> Result<Vec<Recipient>> {
        if root.bid_sub.is_null() {
            return Ok(Vec::new());
        }
        let sub_entries = block::list_subnode_entries(&mut self.reader, &self.bbt, root.bid_sub)?;
        let recip_entry = match sub_entries
            .iter()
            .find(|e| matches!(e.nid.nid_type(), NidType::RecipientTable))
        {
            Some(e) => e,
            None => return Ok(Vec::new()),
        };
        // Present RecipientTable with null data BID is corrupt — not table-less.
        require_present_table_data_bid(recip_entry)?;

        let crypt = self.header.crypt_method;
        let data =
            block::read_block_data(&mut self.reader, &self.bbt, recip_entry.bid_data, crypt)?;
        let subnode_rows = if !recip_entry.bid_sub.is_null() {
            let nested =
                block::list_subnode_entries(&mut self.reader, &self.bbt, recip_entry.bid_sub)?;
            if nested.is_empty() {
                None
            } else {
                let mut all_rows = Vec::new();
                for entry in &nested {
                    let entry_data =
                        block::read_block_data(&mut self.reader, &self.bbt, entry.bid_data, crypt)?;
                    all_rows.extend_from_slice(&entry_data);
                }
                Some(all_rows)
            }
        } else {
            None
        };

        // Present but unreadable → Err (not empty display-fallback).
        let table = TableContext::load(data, subnode_rows)?;

        let mut recipients = Vec::with_capacity(table.row_count());
        for row in 0..table.row_count() {
            let raw_type = table
                .get_row_u32(row, nid::PID_TAG_RECIPIENT_TYPE)
                .unwrap_or(0);
            let recipient_type = RecipientType::from_mapi(raw_type);
            let display_name = opt_row_string(table.get_row_string(row, nid::PID_TAG_DISPLAY_NAME));
            let address_type = opt_row_string(table.get_row_string(row, nid::PID_TAG_ADDRESS_TYPE));
            let email_address =
                opt_row_string(table.get_row_string(row, nid::PID_TAG_EMAIL_ADDRESS));
            let smtp_address = opt_row_string(table.get_row_string(row, nid::PID_TAG_SMTP_ADDRESS));
            recipients.push(Recipient {
                recipient_type,
                display_name,
                address_type,
                email_address,
                smtp_address,
            });
        }
        Ok(recipients)
    }

    /// Open attach binary under a message node (NBT or nested subnode message).
    pub fn open_attach_data_from_message_node(
        &mut self,
        root: &MessageNodeRef,
        attach_nid: NodeId,
    ) -> Result<AttachmentDataReader> {
        let scope = crate::integrity_telemetry::message_scope_enter();
        let result = self.open_attach_data_from_message_node_inner(root, attach_nid);
        let open_suspect = scope.exit();
        match result {
            Ok(mut reader) => {
                if open_suspect {
                    // AttachmentDataReader::crc_suspect is crate-private; reopen path
                    // already ORs via scope — set via public if needed after read.
                    let _ = reader.crc_suspect();
                }
                // Propagate open-time CRC by re-checking: we cannot set private field
                // from another module. Wrap: use a helper on AttachmentDataReader.
                if open_suspect {
                    reader.mark_crc_suspect();
                }
                Ok(reader)
            }
            Err(e) => Err(e),
        }
    }

    fn open_attach_data_from_message_node_inner(
        &mut self,
        root: &MessageNodeRef,
        attach_nid: NodeId,
    ) -> Result<AttachmentDataReader> {
        if root.bid_sub.is_null() {
            return Err(PstError::NoSubnodeBTree(root.nid.0));
        }
        let att_entry =
            block::find_subnode_entry(&mut self.reader, &self.bbt, root.bid_sub, attach_nid)?
                .ok_or(PstError::SubnodeNotFound(attach_nid.0))?;

        let crypt = self.header.crypt_method;
        let att_data =
            block::read_block_data(&mut self.reader, &self.bbt, att_entry.bid_data, crypt)?;
        let pc = PropContext::load(att_data)?;

        if let Some(bytes) = pc.get_binary(nid::PID_TAG_ATTACH_DATA_BINARY)? {
            return Ok(AttachmentDataReader::from_memory(bytes));
        }

        if let Some((_ptype, value_hnid)) = pc.get_raw_hnid(nid::PID_TAG_ATTACH_DATA_BINARY) {
            if value_hnid != 0 {
                let data_nid = NodeId(value_hnid as u64);
                if let Some(src) = self.resolve_subnode_data_stream(&att_entry, data_nid, crypt)? {
                    return Ok(src);
                }
                if !att_entry.bid_sub.is_null() {
                    if let Ok(data) = block::read_subnode_data(
                        &mut self.reader,
                        &self.bbt,
                        att_entry.bid_sub,
                        data_nid,
                        crypt,
                    ) {
                        if data.len() <= 16 * 1024 * 1024 {
                            return Ok(AttachmentDataReader::from_memory(data));
                        }
                        if let Some(sub) = block::find_subnode_entry(
                            &mut self.reader,
                            &self.bbt,
                            att_entry.bid_sub,
                            data_nid,
                        )? {
                            return self.open_block_stream(sub.bid_data, crypt);
                        }
                    }
                }
            }
        }

        if !att_entry.bid_sub.is_null() {
            let subs = block::list_subnode_entries(&mut self.reader, &self.bbt, att_entry.bid_sub)?;
            if let Some(first) = subs.first() {
                // Prefer NormalMessage for method-5; do not stream nested PC as "binary".
                if matches!(first.nid.nid_type(), NidType::NormalMessage) {
                    return Err(PstError::PropertyNotFound(nid::PID_TAG_ATTACH_DATA_BINARY));
                }
                return self.open_block_stream(first.bid_data, crypt);
            }
        }

        Err(PstError::PropertyNotFound(nid::PID_TAG_ATTACH_DATA_BINARY))
    }
}

impl From<&AttachmentInfo> for EmbeddedChildAttach {
    fn from(a: &AttachmentInfo) -> Self {
        Self {
            nid: a.nid,
            filename: a.filename.clone(),
            size: a.size,
            mime_tag: a.mime_tag.clone(),
            attach_method: a.attach_method,
            is_inline: a.is_inline,
            is_cloud_link: a.is_cloud_link,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_embedded_identity_depth_is_three() {
        assert_eq!(MAX_EMBEDDED_IDENTITY_DEPTH, 3);
    }

    #[test]
    fn attachment_table_nid_0x671_is_attachment_table_type() {
        assert!(matches!(NodeId(0x671).nid_type(), NidType::AttachmentTable));
    }

    #[test]
    fn corrupt_table_context_bytes_fail_closed() {
        // Identity maps TableContext::load Err → unread; garbage must not yield Ok.
        let garbage = vec![0u8; 64];
        assert!(
            TableContext::load(garbage, None).is_err(),
            "corrupt TC bytes must Err (not soft-empty)"
        );
    }

    #[test]
    fn missing_bid_sub_recipients_are_empty_ok() {
        // Unit-level: null subnode tree is "absent", not corrupt.
        let root = MessageNodeRef {
            nid: NodeId(0x2004),
            bid_data: BlockId(0),
            bid_sub: BlockId(0),
        };
        assert!(root.bid_sub.is_null());
    }

    #[test]
    fn present_null_table_bid_data_is_err_not_empty() {
        // Table NID present with null bid_data must fail closed (not Ok([])).
        let null_table = SubnodeEntry {
            nid: NodeId(0x671),
            bid_data: BlockId(0),
            bid_sub: BlockId(0),
        };
        assert!(null_table.bid_data.is_null());
        let err = require_present_table_data_bid(&null_table).expect_err("null bid_data");
        assert!(matches!(err, PstError::BlockNotFound(0)));

        let present = SubnodeEntry {
            nid: NodeId(0x692),
            bid_data: BlockId(0x20),
            bid_sub: BlockId(0),
        };
        assert!(require_present_table_data_bid(&present).is_ok());
    }

    #[test]
    fn embedded_child_detects_method5_and_rfc822() {
        let m5 = EmbeddedChildAttach {
            nid: NodeId(0),
            filename: "x.msg".into(),
            size: 1,
            mime_tag: None,
            attach_method: Some(nid::ATTACH_EMBEDDED_MSG),
            is_inline: false,
            is_cloud_link: false,
        };
        assert!(m5.is_embedded_email());
        let rfc = EmbeddedChildAttach {
            nid: NodeId(0),
            filename: "x.eml".into(),
            size: 1,
            mime_tag: Some("message/rfc822".into()),
            attach_method: Some(1),
            is_inline: false,
            is_cloud_link: false,
        };
        assert!(rfc.is_embedded_email());
        let bin = EmbeddedChildAttach {
            nid: NodeId(0),
            filename: "x.bin".into(),
            size: 1,
            mime_tag: Some("application/octet-stream".into()),
            attach_method: Some(1),
            is_inline: false,
            is_cloud_link: false,
        };
        assert!(!bin.is_embedded_email());
    }
}
