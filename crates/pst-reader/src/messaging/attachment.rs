//! Attachment metadata and binary streaming — MS-PST §2.4.6
//!
//! Metadata (name + size) is used by CLI Tier-2 hashing. Desk extract uses
//! [`PstFile::list_attachments`] and [`PstFile::open_attachment_data`] to stream
//! raw attach bytes into CAS without requiring a full multi-GB `Vec<u8>` for
//! the production put path (leaf blocks are read one at a time).

use std::fs::File;
use std::io::{self, BufReader, Read};

use crate::crypto::CryptMethod;
use crate::error::{PstError, Result};
use crate::ltp::pc::PropContext;
use crate::ndb::block::{self, BlockId, SubnodeEntry};
use crate::ndb::btree::BbtIndex;
use crate::ndb::nid::{self, NidType, NodeId};
use crate::PstFile;

/// Lightweight attachment metadata for dedup hashing.
#[derive(Debug, Clone)]
pub struct AttachmentMeta {
    /// Filename (PidTagAttachLongFilename or PidTagAttachFilename).
    pub filename: String,
    /// Size in bytes (PidTagAttachSize).
    pub size: u32,
}

/// Richer attachment descriptor for Desk extract.
#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    /// Attachment object NID (subnode of the message).
    pub nid: NodeId,
    /// Filename (long name preferred).
    pub filename: String,
    /// Declared size in bytes (PidTagAttachSize); may be 0 if missing.
    pub size: u32,
    /// PidTagAttachMimeTag when present.
    pub mime_tag: Option<String>,
    /// PidTagAttachMethod when present.
    pub attach_method: Option<i32>,
}

/// Streaming reader over attachment binary data.
///
/// Small heap-resident payloads are served from an in-memory buffer. Larger
/// payloads stream leaf data blocks via an independent file handle so the
/// owning [`PstFile`] can continue other reads after this reader is dropped.
pub struct AttachmentDataReader {
    inner: AttachReaderInner,
}

enum AttachReaderInner {
    /// Heap-resident or already-buffered payload.
    Memory { data: Vec<u8>, pos: usize },
    /// Leaf-block stream over the PST file.
    Blocks {
        reader: BufReader<File>,
        bbt: BbtIndex,
        crypt: CryptMethod,
        leaf_bids: Vec<BlockId>,
        leaf_index: usize,
        chunk: Vec<u8>,
        chunk_pos: usize,
    },
}

impl Read for AttachmentDataReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            AttachReaderInner::Memory { data, pos } => {
                if *pos >= data.len() {
                    return Ok(0);
                }
                let n = (data.len() - *pos).min(buf.len());
                buf[..n].copy_from_slice(&data[*pos..*pos + n]);
                *pos += n;
                Ok(n)
            }
            AttachReaderInner::Blocks {
                reader,
                bbt,
                crypt,
                leaf_bids,
                leaf_index,
                chunk,
                chunk_pos,
            } => {
                if *chunk_pos >= chunk.len() {
                    if *leaf_index >= leaf_bids.len() {
                        return Ok(0);
                    }
                    let bid = leaf_bids[*leaf_index];
                    *leaf_index += 1;
                    *chunk = block::read_leaf_block_data(reader, bbt, bid, *crypt)
                        .map_err(|e| io::Error::other(e.to_string()))?;
                    *chunk_pos = 0;
                    if chunk.is_empty() {
                        // Skip empty leaf; recurse once via tail call pattern.
                        return self.read(buf);
                    }
                }
                let n = (chunk.len() - *chunk_pos).min(buf.len());
                buf[..n].copy_from_slice(&chunk[*chunk_pos..*chunk_pos + n]);
                *chunk_pos += n;
                Ok(n)
            }
        }
    }
}

impl AttachmentDataReader {
    /// True when the full payload is already buffered in memory (small attaches).
    pub fn is_buffered(&self) -> bool {
        matches!(self.inner, AttachReaderInner::Memory { .. })
    }
}

impl PstFile {
    /// Read attachment metadata (name + size) for a message.
    ///
    /// Returns an empty vec if the message has no attachments or the
    /// attachment table can't be read.
    pub fn read_attachment_metadata(&mut self, message_nid: NodeId) -> Result<Vec<AttachmentMeta>> {
        let infos = self.list_attachments(message_nid)?;
        Ok(infos
            .into_iter()
            .map(|i| AttachmentMeta {
                filename: i.filename,
                size: i.size,
            })
            .collect())
    }

    /// List attachments with NID + filename + size + optional mime/method.
    pub fn list_attachments(&mut self, message_nid: NodeId) -> Result<Vec<AttachmentInfo>> {
        let nbt_entry = match self.nbt.get(message_nid) {
            Some(e) => e.clone(),
            None => return Ok(Vec::new()),
        };

        if nbt_entry.bid_sub.is_null() {
            return Ok(Vec::new());
        }

        let sub_entries =
            block::list_subnode_entries(&mut self.reader, &self.bbt, nbt_entry.bid_sub)?;

        let crypt = self.header.crypt_method;
        let mut attachments = Vec::new();

        for entry in &sub_entries {
            let entry_type = entry.nid.nid_type();
            if !matches!(entry_type, NidType::Attachment) {
                continue;
            }

            let att_data =
                block::read_block_data(&mut self.reader, &self.bbt, entry.bid_data, crypt)?;

            if let Ok(pc) = PropContext::load(att_data) {
                let filename = pc
                    .get_string(nid::PID_TAG_ATTACH_LONG_FILENAME)?
                    .or(pc.get_string(nid::PID_TAG_ATTACH_FILENAME)?)
                    .unwrap_or_default();

                let size = pc.get_i32(nid::PID_TAG_ATTACH_SIZE)?.unwrap_or(0) as u32;
                let mime_tag = pc.get_string(nid::PID_TAG_ATTACH_MIME_TAG)?;
                let attach_method = pc.get_i32(nid::PID_TAG_ATTACH_METHOD)?;

                attachments.push(AttachmentInfo {
                    nid: entry.nid,
                    filename,
                    size,
                    mime_tag,
                    attach_method,
                });
            }
        }

        Ok(attachments)
    }

    /// Open attachment binary as a [`Read`] stream (PidTagAttachDataBinary).
    ///
    /// Best-effort:
    /// - Heap-resident binary → in-memory buffer reader
    /// - Subnode / multi-block binary → leaf-block stream (no full multi-GB `Vec`)
    ///
    /// Returns [`PstError::PropertyNotFound`] when no binary payload is available
    /// (e.g. reference attachments, embedded messages without binary).
    pub fn open_attachment_data(
        &mut self,
        message_nid: NodeId,
        attach_nid: NodeId,
    ) -> Result<AttachmentDataReader> {
        let msg_entry = self
            .nbt
            .get(message_nid)
            .ok_or(PstError::NodeNotFound(message_nid.0))?
            .clone();
        if msg_entry.bid_sub.is_null() {
            return Err(PstError::NoSubnodeBTree(message_nid.0));
        }

        let att_entry =
            block::find_subnode_entry(&mut self.reader, &self.bbt, msg_entry.bid_sub, attach_nid)?
                .ok_or(PstError::SubnodeNotFound(attach_nid.0))?;

        let crypt = self.header.crypt_method;
        let att_data =
            block::read_block_data(&mut self.reader, &self.bbt, att_entry.bid_data, crypt)?;
        let pc = PropContext::load(att_data)?;

        // Prefer heap-resident binary when available.
        if let Some(bytes) = pc.get_binary(nid::PID_TAG_ATTACH_DATA_BINARY)? {
            return Ok(AttachmentDataReader {
                inner: AttachReaderInner::Memory {
                    data: bytes,
                    pos: 0,
                },
            });
        }

        // Subnode storage: dwValueHnid is an NID under the attachment's subnode tree.
        if let Some((_ptype, value_hnid)) = pc.get_raw_hnid(nid::PID_TAG_ATTACH_DATA_BINARY) {
            if value_hnid != 0 {
                let data_nid = NodeId(value_hnid as u64);
                if let Some(src) = self.resolve_subnode_data_stream(&att_entry, data_nid, crypt)? {
                    return Ok(src);
                }
                // Sometimes the binary lives as the sole/data subnode of the attach object.
                if !att_entry.bid_sub.is_null() {
                    // Try reading the subnode by the raw NID value.
                    if let Ok(data) = block::read_subnode_data(
                        &mut self.reader,
                        &self.bbt,
                        att_entry.bid_sub,
                        data_nid,
                        crypt,
                    ) {
                        // For modest sizes, buffer; for large, re-open as leaf stream.
                        if data.len() <= 16 * 1024 * 1024 {
                            return Ok(AttachmentDataReader {
                                inner: AttachReaderInner::Memory { data, pos: 0 },
                            });
                        }
                        // Fall through to leaf stream via subnode bid_data.
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

        // Last resort: if attach method is by-value and attach has subnode data,
        // try streaming the first data subnode that isn't the PC itself.
        if !att_entry.bid_sub.is_null() {
            let subs = block::list_subnode_entries(&mut self.reader, &self.bbt, att_entry.bid_sub)?;
            if let Some(first) = subs.first() {
                return self.open_block_stream(first.bid_data, crypt);
            }
        }

        Err(PstError::PropertyNotFound(nid::PID_TAG_ATTACH_DATA_BINARY))
    }

    fn resolve_subnode_data_stream(
        &mut self,
        att_entry: &SubnodeEntry,
        data_nid: NodeId,
        crypt: CryptMethod,
    ) -> Result<Option<AttachmentDataReader>> {
        if att_entry.bid_sub.is_null() {
            return Ok(None);
        }
        let sub = match block::find_subnode_entry(
            &mut self.reader,
            &self.bbt,
            att_entry.bid_sub,
            data_nid,
        )? {
            Some(s) => s,
            None => return Ok(None),
        };
        Ok(Some(self.open_block_stream(sub.bid_data, crypt)?))
    }

    fn open_block_stream(
        &mut self,
        bid_data: BlockId,
        crypt: CryptMethod,
    ) -> Result<AttachmentDataReader> {
        let leaf_bids = block::collect_leaf_data_bids(&mut self.reader, &self.bbt, bid_data)?;
        if leaf_bids.is_empty() {
            return Ok(AttachmentDataReader {
                inner: AttachReaderInner::Memory {
                    data: Vec::new(),
                    pos: 0,
                },
            });
        }

        // Single small leaf: buffer it (cheap path).
        if leaf_bids.len() == 1 {
            let data =
                block::read_leaf_block_data(&mut self.reader, &self.bbt, leaf_bids[0], crypt)?;
            if data.len() <= 1024 * 1024 {
                return Ok(AttachmentDataReader {
                    inner: AttachReaderInner::Memory { data, pos: 0 },
                });
            }
        }

        let path = self
            .path
            .as_ref()
            .ok_or_else(|| {
                PstError::Io(io::Error::other(
                    "PST path unavailable for attachment streaming",
                ))
            })?
            .clone();
        let file = File::open(&path)?;
        Ok(AttachmentDataReader {
            inner: AttachReaderInner::Blocks {
                reader: BufReader::with_capacity(64 * 1024, file),
                bbt: self.bbt.clone(),
                crypt,
                leaf_bids,
                leaf_index: 0,
                chunk: Vec::new(),
                chunk_pos: 0,
            },
        })
    }
}
