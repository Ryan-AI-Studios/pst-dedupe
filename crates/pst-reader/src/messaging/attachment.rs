//! Attachment metadata extraction — MS-PST §2.4.6
//!
//! For dedup purposes we only need attachment name + size, not content.
//! Attachments are stored in a subnode of the message node.

use crate::error::Result;
use crate::ndb::nid::{self, NodeId, NidType};
use crate::ndb::block;
use crate::ltp::pc::PropContext;
use crate::PstFile;

/// Lightweight attachment metadata for dedup hashing.
#[derive(Debug, Clone)]
pub struct AttachmentMeta {
    /// Filename (PidTagAttachLongFilename or PidTagAttachFilename).
    pub filename: String,
    /// Size in bytes (PidTagAttachSize).
    pub size: u32,
}

impl PstFile {
    /// Read attachment metadata (name + size) for a message.
    ///
    /// Returns an empty vec if the message has no attachments or the
    /// attachment table can't be read.
    pub fn read_attachment_metadata(&mut self, message_nid: NodeId) -> Result<Vec<AttachmentMeta>> {
        let nbt_entry = match self.nbt.get(message_nid) {
            Some(e) => e.clone(),
            None => return Ok(Vec::new()),
        };

        if nbt_entry.bid_sub.is_null() {
            return Ok(Vec::new());
        }

        // List subnode entries to find attachment objects
        let sub_entries = block::list_subnode_entries(
            &mut self.reader,
            &self.bbt,
            nbt_entry.bid_sub,
        )?;

        let crypt = self.header.crypt_method;
        let mut attachments = Vec::new();

        for entry in &sub_entries {
            let entry_type = entry.nid.nid_type();
            if !matches!(entry_type, NidType::Attachment) {
                continue;
            }

            // Read the attachment's Property Context
            let att_data = block::read_block_data(
                &mut self.reader,
                &self.bbt,
                entry.bid_data,
                crypt,
            )?;

            if let Ok(pc) = PropContext::load(att_data) {
                let filename = pc
                    .get_string(nid::PID_TAG_ATTACH_LONG_FILENAME)?
                    .or(pc.get_string(nid::PID_TAG_ATTACH_FILENAME)?)
                    .unwrap_or_default();

                let size = pc.get_i32(nid::PID_TAG_ATTACH_SIZE)?.unwrap_or(0) as u32;

                attachments.push(AttachmentMeta { filename, size });
            }
        }

        Ok(attachments)
    }
}
