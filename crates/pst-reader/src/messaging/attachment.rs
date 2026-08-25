//! Attachment metadata and binary streaming — MS-PST §2.4.6
//!
//! Metadata (name + size) is used by CLI Tier-2 hashing. Desk extract uses
//! [`PstFile::list_attachments`] and [`PstFile::open_attachment_data`] to stream
//! raw attach bytes into CAS without requiring a full multi-GB `Vec<u8>` for
//! the production put path (leaf blocks are read one at a time).
//!
//! **0084 cloud/modern attach (attachment-table only):**
//! Classification uses **independent OR** signals (do not simplify to named-prop-only):
//! 1. Allowlisted named prop `AttachmentProviderType` + no usable binary payload
//! 2. Non-portable / web-reference attach method + no usable binary payload
//! 3. Conservative fallback: empty data + URL-shaped classic path/filename
//!
//! Body-only inline SharePoint/OneDrive URLs are **out of scope** (D-0084-body-cloud-links).

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
    /// True when MAPI marks this as inline/embedded (0076).
    pub is_inline: bool,
}

/// Classification of an attachment-table row (0084).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AttachKind {
    /// Ordinary by-value or embedded-message attach.
    #[default]
    Classic,
    /// Cloud/modern web-reference attach (link + provider metadata; no offline payload).
    CloudLink {
        /// Open provider string (`OneDrivePro` / `OneDriveConsumer` / other / None).
        provider: Option<String>,
        /// Best-effort URL/path from classic tags (may be empty/None).
        url: Option<String>,
    },
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
    /// True when MAPI marks inline/embedded: Content-ID present, rendered-in-body, or hidden.
    pub is_inline: bool,
    /// True when classified as attachment-table cloud/web-ref without exportable payload (0084).
    pub is_cloud_link: bool,
    /// Provider string from `PidNameAttachmentProviderType` when present (open string).
    pub cloud_provider: Option<String>,
    /// Best-effort cloud URL/path from classic pathname/filename tags.
    pub cloud_url: Option<String>,
}

/// True when `s` looks like an absolute URL (conservative cloud-path heuristic).
pub fn looks_like_url(s: &str) -> bool {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("file://")
        || lower.starts_with("onedrive:")
}

/// DNS suffixes for attach-table cloud pointer heuristics (commercial + US GCC High/DoD).
/// Local to this file — do not pull `dedup-engine` into `pst-reader` (0088).
/// Keep in sync with `dedup-engine` `ALLOWED_CLOUD_HOST_SUFFIXES` /
/// `ALLOWED_CLOUD_HOST_EXACT`.
const CLOUD_POINTER_HOST_SUFFIXES: &[&str] = &[
    "sharepoint.com",
    "sharepoint-df.com",
    "onedrive.live.com",
    "1drv.ms",
    "sharepoint.us",
    "sharepoint-mil.us",
    "dps.mil",
];

const CLOUD_POINTER_HOST_EXACT: &[&str] = &["admin.onedrive.us"];

fn host_matches_dns_suffix(host: &str, suffix: &str) -> bool {
    if host == suffix {
        return true;
    }
    let Some(rest) = host.strip_suffix(suffix) else {
        return false;
    };
    rest.ends_with('.')
}

fn is_allowed_cloud_pointer_host(host: &str) -> bool {
    if CLOUD_POINTER_HOST_EXACT.contains(&host) {
        return true;
    }
    CLOUD_POINTER_HOST_SUFFIXES
        .iter()
        .any(|&suffix| host_matches_dns_suffix(host, suffix))
}

fn extract_url_host_lower(url_lower: &str) -> Option<&str> {
    let after = url_lower
        .strip_prefix("https://")
        .or_else(|| url_lower.strip_prefix("http://"))
        .or_else(|| url_lower.strip_prefix("file://"))?;
    let host_port = after.split(['/', '?', '#']).next().unwrap_or("");
    if host_port.is_empty() {
        return None;
    }
    let host_port = host_port.rsplit('@').next().unwrap_or(host_port);
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// Suffix-safe cloud host mention (rejects `notsharepoint.attacker.com`).
fn text_mentions_cloud_host(lower: &str) -> bool {
    if let Some(host) = extract_url_host_lower(lower) {
        if is_allowed_cloud_pointer_host(host) {
            return true;
        }
    }
    for &needle in CLOUD_POINTER_HOST_EXACT
        .iter()
        .chain(CLOUD_POINTER_HOST_SUFFIXES.iter())
    {
        for (idx, _) in lower.match_indices(needle) {
            let ok_left = idx == 0 || {
                let b = lower.as_bytes()[idx - 1];
                b == b'.' || b == b'/' || b == b'@' || !b.is_ascii_alphanumeric()
            };
            let end = idx + needle.len();
            // Path/query/fragment or end — not another DNS label (`1drv.ms.attacker.com`).
            let ok_right = end >= lower.len() || {
                let b = lower.as_bytes()[end];
                matches!(b, b'/' | b'?' | b':' | b'#' | b'"' | b'\'' | b' ')
            };
            if ok_left && ok_right {
                return true;
            }
        }
    }
    false
}

/// True when path/filename text is URL-shaped or mentions an allowed cloud host suffix.
fn looks_like_cloud_pointer(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    if looks_like_url(t) {
        return true;
    }
    text_mentions_cloud_host(&t.to_ascii_lowercase())
}

/// True when attach method is **web-reference** (cloud/modern shaped).
///
/// Independent OR signal #2 (0084): third-party cloud add-ins may use
/// `ATTACH_BY_WEB_REFERENCE` (7) without Microsoft's `AttachmentProviderType`.
///
/// Classic filesystem reference methods (2/3/4) and OLE (6) are **not**
/// cloud-shaped by method alone — they remain `ATTACH_METHOD_UNSUPPORTED`
/// omit unless a named-prop provider hit or URL-shaped path (signal 1/3)
/// independently classifies CloudLink.
pub fn is_cloud_shaped_method(method: Option<i32>) -> bool {
    matches!(method, Some(nid::ATTACH_BY_WEB_REFERENCE))
}

/// True when the attach PC has a usable by-value binary payload reference.
fn has_usable_binary_payload(pc: &PropContext) -> bool {
    if let Ok(Some(bytes)) = pc.get_binary(nid::PID_TAG_ATTACH_DATA_BINARY) {
        if !bytes.is_empty() {
            return true;
        }
    }
    // Subnode / non-null HNID for attach binary counts as "payload may exist".
    if let Some((_ptype, value_hnid)) = pc.get_raw_hnid(nid::PID_TAG_ATTACH_DATA_BINARY) {
        if value_hnid != 0 {
            return true;
        }
    }
    false
}

/// Best-effort URL from classic attach string tags (Phase-0 order).
///
/// 1. `PidTagAttachLongPathname` (0x370D)
/// 2. `PidTagAttachPathname` (0x3708)
/// 3. Long/short filename when URL-shaped
fn extract_cloud_url(pc: &PropContext, filename: &str) -> Option<String> {
    let candidates = [
        pc.get_string(nid::PID_TAG_ATTACH_LONG_PATHNAME)
            .ok()
            .flatten(),
        pc.get_string(nid::PID_TAG_ATTACH_PATHNAME).ok().flatten(),
        Some(filename.to_string()).filter(|s| looks_like_url(s)),
        pc.get_string(nid::PID_TAG_ATTACH_LONG_FILENAME)
            .ok()
            .flatten()
            .filter(|s| looks_like_url(s)),
        pc.get_string(nid::PID_TAG_ATTACH_FILENAME)
            .ok()
            .flatten()
            .filter(|s| looks_like_url(s)),
    ];
    for c in candidates.into_iter().flatten() {
        let t = c.trim();
        if looks_like_cloud_pointer(t) {
            return Some(t.to_string());
        }
    }
    // Non-URL long pathname still useful as pointer when method is web-ref.
    if let Ok(Some(path)) = pc.get_string(nid::PID_TAG_ATTACH_LONG_PATHNAME) {
        let t = path.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Ok(Some(path)) = pc.get_string(nid::PID_TAG_ATTACH_PATHNAME) {
        let t = path.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

/// Classify an attachment-table PC row (0084 independent OR signals).
///
/// `provider_npid` is the resolved NPID for `AttachmentProviderType` when the
/// store NPMAP has that entry; `None` if map missing/unresolved.
pub fn classify_attach_pc(
    pc: &PropContext,
    provider_npid: Option<u16>,
    filename: &str,
) -> AttachKind {
    let attach_method = pc.get_i32(nid::PID_TAG_ATTACH_METHOD).ok().flatten();
    let has_payload = has_usable_binary_payload(pc);
    let url = extract_cloud_url(pc, filename);

    let mut provider: Option<String> = None;
    if let Some(npid) = provider_npid {
        if let Ok(Some(s)) = pc.get_string(npid) {
            let t = s.trim();
            if !t.is_empty() {
                provider = Some(t.to_string());
            }
        }
    }

    // Signal 1: named-prop provider hit + no usable binary.
    let named_cloud = provider.is_some() && !has_payload;
    // Signal 2: non-portable / web-ref method + no payload (even without named prop).
    let method_cloud = is_cloud_shaped_method(attach_method) && !has_payload;
    // Signal 3 (conservative): empty data + URL-shaped / cloud-host path/filename.
    let url_fallback = !has_payload && url.as_deref().is_some_and(looks_like_cloud_pointer);

    if named_cloud || method_cloud || url_fallback {
        AttachKind::CloudLink { provider, url }
    } else {
        AttachKind::Classic
    }
}

/// Streaming reader over attachment binary data.
///
/// Small heap-resident payloads are served from an in-memory buffer. Larger
/// payloads stream leaf data blocks via an independent file handle so the
/// owning [`PstFile`] can continue other reads after this reader is dropped.
///
/// **0077:** block CRC/BID hits during open or subsequent leaf reads set
/// [`Self::crc_suspect`]. Page CRC never contributes (poly-class exclusion).
pub struct AttachmentDataReader {
    inner: AttachReaderInner,
    /// True when a block CRC or BID mismatch was counted while opening or
    /// streaming this attachment (0077 `CRC_SUSPECT` surface).
    crc_suspect: bool,
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
                    // 0077: attribute leaf-block CRC/BID to this attach stream.
                    let before = crate::integrity_telemetry::tls_block_mismatch_total();
                    *chunk = block::read_leaf_block_data(reader, bbt, bid, *crypt)
                        .map_err(|e| io::Error::other(e.to_string()))?;
                    if crate::integrity_telemetry::tls_block_mismatch_total() > before {
                        self.crc_suspect = true;
                    }
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
    /// In-memory payload reader (heap-resident attach binary).
    pub(crate) fn from_memory(data: Vec<u8>) -> Self {
        Self {
            inner: AttachReaderInner::Memory { data, pos: 0 },
            crc_suspect: false,
        }
    }

    /// True when the full payload is already buffered in memory (small attaches).
    pub fn is_buffered(&self) -> bool {
        matches!(self.inner, AttachReaderInner::Memory { .. })
    }

    /// True when block CRC/BID mismatch was counted during open or stream read.
    pub fn crc_suspect(&self) -> bool {
        self.crc_suspect
    }

    /// Mark CRC/BID taint from an outer open scope (0090 nested open path).
    pub(crate) fn mark_crc_suspect(&mut self) {
        self.crc_suspect = true;
    }
}

impl PstFile {
    /// Read attachment metadata (name + size) for a message.
    ///
    /// Returns an empty vec if the message has no attachments or the
    /// attachment table can't be read.
    ///
    /// Block CRC/BID during this walk is counted under a message scope so
    /// callers can OR taint via [`crate::integrity_telemetry::with_crc_scope`]
    /// (nested scopes remain correct).
    pub fn read_attachment_metadata(&mut self, message_nid: NodeId) -> Result<Vec<AttachmentMeta>> {
        let infos = self.list_attachments(message_nid)?;
        Ok(infos
            .into_iter()
            .map(|i| AttachmentMeta {
                filename: i.filename,
                size: i.size,
                is_inline: i.is_inline,
            })
            .collect())
    }

    /// List attachments with NID + filename + size + optional mime/method + inline flags.
    ///
    /// Attachment rows whose property context fails to load are **soft-skipped**
    /// (historical BestEffort behavior for meta-only consumers). For identity
    /// that must not omit attach slots, use [`Self::list_attachments_strict`].
    ///
    /// **0077:** block reads for attach PCs run under a message CRC scope so an
    /// outer [`crate::integrity_telemetry::with_crc_scope`] (or nested enter/exit)
    /// attributes attach-meta CRC to the message. Page CRC is excluded from taint.
    pub fn list_attachments(&mut self, message_nid: NodeId) -> Result<Vec<AttachmentInfo>> {
        // Scope so standalone callers (and outer scan scopes) attribute attach-PC
        // block CRC to this message path.
        let scope = crate::integrity_telemetry::message_scope_enter();
        let result = self.list_attachments_inner(message_nid, false);
        // Drop scope: delta is visible to any outer scope via TLS totals.
        let _ = scope.exit();
        result
    }

    /// List attachments **fail-closed** on any attachment-row PC load/read error.
    ///
    /// Used by Tier-2.5 `body-recip-attach` (0086): a partial list would omit
    /// identity slots and can false-merge with fewer-attach / no-attach peers.
    /// Soft meta consumers should keep [`Self::list_attachments`].
    pub fn list_attachments_strict(&mut self, message_nid: NodeId) -> Result<Vec<AttachmentInfo>> {
        let scope = crate::integrity_telemetry::message_scope_enter();
        let result = self.list_attachments_inner(message_nid, true);
        let _ = scope.exit();
        result
    }

    fn list_attachments_inner(
        &mut self,
        message_nid: NodeId,
        fail_on_row_error: bool,
    ) -> Result<Vec<AttachmentInfo>> {
        let nbt_entry = match self.nbt.get(message_nid) {
            Some(e) => e.clone(),
            None => return Ok(Vec::new()),
        };

        if nbt_entry.bid_sub.is_null() {
            return Ok(Vec::new());
        }

        let sub_entries =
            block::list_subnode_entries(&mut self.reader, &self.bbt, nbt_entry.bid_sub)?;

        // Resolve allowlisted cloud named-prop once per list (cached NPMAP).
        // Degraded/missing map → None; classic method/URL signals still run.
        let provider_npid = self.attachment_provider_type_npid();

        let crypt = self.header.crypt_method;
        let mut attachments = Vec::new();

        for entry in &sub_entries {
            let entry_type = entry.nid.nid_type();
            if !matches!(entry_type, NidType::Attachment) {
                continue;
            }

            let att_data =
                block::read_block_data(&mut self.reader, &self.bbt, entry.bid_data, crypt)?;

            let pc = match PropContext::load(att_data) {
                Ok(pc) => pc,
                Err(e) => {
                    if fail_on_row_error {
                        return Err(e);
                    }
                    // Soft meta path: skip unreadable attach PC rows.
                    continue;
                }
            };

            let filename = match pc
                .get_string(nid::PID_TAG_ATTACH_LONG_FILENAME)
                .and_then(|long| {
                    if long.is_some() {
                        Ok(long)
                    } else {
                        pc.get_string(nid::PID_TAG_ATTACH_FILENAME)
                    }
                }) {
                Ok(v) => v.unwrap_or_default(),
                Err(e) => {
                    if fail_on_row_error {
                        return Err(e);
                    }
                    continue;
                }
            };

            let size = match pc.get_i32(nid::PID_TAG_ATTACH_SIZE) {
                Ok(v) => v.unwrap_or(0) as u32,
                Err(e) => {
                    if fail_on_row_error {
                        return Err(e);
                    }
                    continue;
                }
            };
            let mime_tag = match pc.get_string(nid::PID_TAG_ATTACH_MIME_TAG) {
                Ok(v) => v,
                Err(e) => {
                    if fail_on_row_error {
                        return Err(e);
                    }
                    continue;
                }
            };
            let attach_method = match pc.get_i32(nid::PID_TAG_ATTACH_METHOD) {
                Ok(v) => v,
                Err(e) => {
                    if fail_on_row_error {
                        return Err(e);
                    }
                    continue;
                }
            };

            // Inline/embedded detection (0076): MAPI flags, not a size threshold.
            // Soft-fail individual property reads — missing tags are normal.
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

            // 0084: attachment-table cloud classification (independent OR signals).
            let kind = classify_attach_pc(&pc, provider_npid, &filename);
            let (is_cloud_link, cloud_provider, cloud_url) = match kind {
                AttachKind::CloudLink { provider, url } => (true, provider, url),
                AttachKind::Classic => (false, None, None),
            };

            attachments.push(AttachmentInfo {
                nid: entry.nid,
                filename,
                size,
                mime_tag,
                attach_method,
                is_inline,
                is_cloud_link,
                cloud_provider,
                cloud_url,
            });
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
    ///
    /// **0077:** open runs under a message CRC scope; [`AttachmentDataReader::crc_suspect`]
    /// is set when block CRC/BID fires during open or later leaf stream reads.
    pub fn open_attachment_data(
        &mut self,
        message_nid: NodeId,
        attach_nid: NodeId,
    ) -> Result<AttachmentDataReader> {
        let scope = crate::integrity_telemetry::message_scope_enter();
        let result = self.open_attachment_data_inner(message_nid, attach_nid);
        let open_suspect = scope.exit();
        match result {
            Ok(mut reader) => {
                if open_suspect {
                    reader.crc_suspect = true;
                }
                Ok(reader)
            }
            Err(e) => Err(e),
        }
    }

    fn open_attachment_data_inner(
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
                crc_suspect: false,
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
                                crc_suspect: false,
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

    pub(crate) fn resolve_subnode_data_stream(
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

    pub(crate) fn open_block_stream(
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
                crc_suspect: false,
            });
        }

        // Single small leaf: buffer it (cheap path).
        if leaf_bids.len() == 1 {
            let data =
                block::read_leaf_block_data(&mut self.reader, &self.bbt, leaf_bids[0], crypt)?;
            if data.len() <= 1024 * 1024 {
                return Ok(AttachmentDataReader {
                    inner: AttachReaderInner::Memory { data, pos: 0 },
                    crc_suspect: false,
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
            crc_suspect: false,
        })
    }
}

#[cfg(test)]
mod classify_tests {
    use super::*;

    /// Minimal PC with only attach method + optional string props (heap-resident).
    fn build_attach_pc(
        method: Option<i32>,
        long_path: Option<&str>,
        long_name: Option<&str>,
        provider_npid: Option<(u16, &str)>,
        with_binary: bool,
    ) -> PropContext {
        // Layout: alloc1 Hid 0x20 = BTH header; alloc2 Hid 0x40 = leaf; then strings/binary.
        let mut strings: Vec<(u16, Vec<u8>)> = Vec::new();
        if let Some(p) = long_path {
            strings.push((
                nid::PID_TAG_ATTACH_LONG_PATHNAME,
                p.encode_utf16().flat_map(|c| c.to_le_bytes()).collect(),
            ));
        }
        if let Some(n) = long_name {
            strings.push((
                nid::PID_TAG_ATTACH_LONG_FILENAME,
                n.encode_utf16().flat_map(|c| c.to_le_bytes()).collect(),
            ));
        }
        if let Some((npid, prov)) = provider_npid {
            strings.push((
                npid,
                prov.encode_utf16().flat_map(|c| c.to_le_bytes()).collect(),
            ));
        }
        let binary: Option<Vec<u8>> = if with_binary {
            Some(b"payload".to_vec())
        } else {
            None
        };

        let n_var = strings.len() + usize::from(binary.is_some());
        let mut leaf_records = Vec::new();
        if let Some(m) = method {
            leaf_records.extend_from_slice(&nid::PID_TAG_ATTACH_METHOD.to_le_bytes());
            leaf_records.extend_from_slice(&0x0003u16.to_le_bytes()); // PtypInteger32
            leaf_records.extend_from_slice(&(m as u32).to_le_bytes());
        }
        // HID: bits5–15 = hidIndex (1-based), type/block 0. Alloc #1 → 0x20, #3 → 0x60.
        let mut var_hids = Vec::new();
        for i in 0..n_var {
            let hid_index = (3 + i) as u32;
            var_hids.push(hid_index << 5);
        }
        let mut vi = 0usize;
        for (prop, _bytes) in &strings {
            leaf_records.extend_from_slice(&prop.to_le_bytes());
            leaf_records.extend_from_slice(&0x001Fu16.to_le_bytes());
            leaf_records.extend_from_slice(&var_hids[vi].to_le_bytes());
            vi += 1;
        }
        if binary.is_some() {
            leaf_records.extend_from_slice(&nid::PID_TAG_ATTACH_DATA_BINARY.to_le_bytes());
            leaf_records.extend_from_slice(&0x0102u16.to_le_bytes());
            leaf_records.extend_from_slice(&var_hids[vi].to_le_bytes());
        }

        let bth_header = [0xB5u8, 0x02, 0x06, 0x00, 0x40, 0x00, 0x00, 0x00];

        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_le_bytes());
        data.push(0xEC);
        data.push(0x6C);
        data.extend_from_slice(&0x20u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&bth_header);
        data.extend_from_slice(&leaf_records);
        for (_p, bytes) in &strings {
            data.extend_from_slice(bytes);
        }
        if let Some(ref b) = binary {
            data.extend_from_slice(b);
        }

        let ib_hnpm = data.len() as u16;
        data[0..2].copy_from_slice(&ib_hnpm.to_le_bytes());

        let alloc1_start = 12u16;
        let alloc2_start = alloc1_start + bth_header.len() as u16;
        let mut starts = vec![alloc1_start, alloc2_start];
        let mut cursor = alloc2_start + leaf_records.len() as u16;
        for (_p, bytes) in &strings {
            starts.push(cursor);
            cursor += bytes.len() as u16;
        }
        if let Some(ref b) = binary {
            starts.push(cursor);
            cursor += b.len() as u16;
        }
        let alloc_end = cursor;

        data.extend_from_slice(&(starts.len() as u16).to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        for s in &starts {
            data.extend_from_slice(&s.to_le_bytes());
        }
        data.extend_from_slice(&alloc_end.to_le_bytes());

        PropContext::load(data).expect("test PC must load")
    }

    #[test]
    fn web_ref_method_without_payload_is_cloud() {
        let pc = build_attach_pc(
            Some(nid::ATTACH_BY_WEB_REFERENCE),
            Some("https://contoso.sharepoint.com/x"),
            Some("report.xlsx"),
            None,
            false,
        );
        let kind = classify_attach_pc(&pc, None, "report.xlsx");
        match kind {
            AttachKind::CloudLink { provider, url } => {
                assert!(provider.is_none());
                assert_eq!(url.as_deref(), Some("https://contoso.sharepoint.com/x"));
            }
            AttachKind::Classic => panic!("expected CloudLink"),
        }
    }

    #[test]
    fn named_provider_without_payload_is_cloud() {
        let pc = build_attach_pc(
            Some(nid::ATTACH_BY_VALUE),
            Some("https://1drv.ms/x/s!abc"),
            Some("doc.docx"),
            Some((0x8000, "OneDrivePro")),
            false,
        );
        let kind = classify_attach_pc(&pc, Some(0x8000), "doc.docx");
        match kind {
            AttachKind::CloudLink { provider, url } => {
                assert_eq!(provider.as_deref(), Some("OneDrivePro"));
                assert!(url.is_some());
            }
            AttachKind::Classic => panic!("expected CloudLink"),
        }
    }

    #[test]
    fn by_value_with_binary_is_classic() {
        let pc = build_attach_pc(
            Some(nid::ATTACH_BY_VALUE),
            None,
            Some("file.bin"),
            None,
            true,
        );
        assert_eq!(
            classify_attach_pc(&pc, None, "file.bin"),
            AttachKind::Classic
        );
    }

    #[test]
    fn looks_like_url_helpers() {
        assert!(looks_like_url("https://example.com/a"));
        assert!(looks_like_url("HTTP://X"));
        assert!(!looks_like_url("report.xlsx"));
        assert!(is_cloud_shaped_method(Some(nid::ATTACH_BY_WEB_REFERENCE)));
        assert!(!is_cloud_shaped_method(Some(nid::ATTACH_BY_VALUE)));
        // Classic filesystem reference methods are NOT cloud-shaped by method alone.
        assert!(!is_cloud_shaped_method(Some(nid::ATTACH_BY_REFERENCE)));
        assert!(!is_cloud_shaped_method(Some(nid::ATTACH_BY_REF_ONLY)));
        assert!(!is_cloud_shaped_method(Some(nid::ATTACH_BY_REF_RESOLVE)));
        assert!(!is_cloud_shaped_method(Some(nid::ATTACH_OLE)));
    }

    #[test]
    fn cloud_pointer_suffix_safe_rejects_lookalike() {
        assert!(!text_mentions_cloud_host("notsharepoint.attacker.com"));
        assert!(!text_mentions_cloud_host("evil1drv.ms.attacker.com/x"));
        assert!(!looks_like_cloud_pointer("notsharepoint.attacker.com/doc"));
        assert!(looks_like_cloud_pointer("contoso.sharepoint.com/x"));
        assert!(looks_like_cloud_pointer(
            "https://contoso-my.sharepoint.us/:w:/r/personal/u/Documents/a.docx"
        ));
        assert!(looks_like_cloud_pointer(
            "contoso-my.sharepoint-mil.us/personal/u/Documents/a.docx"
        ));
        assert!(text_mentions_cloud_host("tenant.dps.mil/sites/L/a.xlsx"));
    }

    #[test]
    fn classic_ref_method_without_url_is_not_cloud() {
        // Method 2/4 alone + no payload + non-URL path → not CloudLink
        // (remains METHOD_UNSUPPORTED omit on write path).
        let pc = build_attach_pc(
            Some(nid::ATTACH_BY_REFERENCE),
            Some(r"\\fileserver\share\doc.pdf"),
            Some("doc.pdf"),
            None,
            false,
        );
        assert_eq!(
            classify_attach_pc(&pc, None, "doc.pdf"),
            AttachKind::Classic,
            "classic ref + UNC path must not be CloudLink"
        );
    }

    /// End-to-end DoD: protocol-correct NPMAP (`w_guid=3` → first GUID-stream slot =
    /// `PSETID_Attachment`) resolves `AttachmentProviderType`, and that NPID on an
    /// attach PC classifies as CloudLink with the provider string.
    #[test]
    fn npmap_w_guid_3_provider_npid_classifies_cloud_link() {
        use crate::messaging::named_prop::{
            encode_nameid_entry, encode_string_stream_entry, NameIdMap,
            NAME_ATTACHMENT_PROVIDER_TYPE, PSETID_ATTACHMENT,
        };

        // MS-PST / MS-OXMSG: wGuid 0=none, 1=PS_MAPI, 2=PS_PUBLIC_STRINGS, ≥3 = stream[n-3].
        let guid_stream = PSETID_ATTACHMENT.to_vec();
        let string_stream = encode_string_stream_entry(NAME_ATTACHMENT_PROVIDER_TYPE);
        let entry = encode_nameid_entry(0, true, 3, 0);
        let map = NameIdMap::from_streams(&guid_stream, &entry, &string_stream);
        let npid = map
            .attachment_provider_type_npid()
            .expect("AttachmentProviderType must resolve with w_guid=3");
        assert_eq!(npid, 0x8000);
        assert_eq!(map.reverse(npid).map(|k| k.guid), Some(PSETID_ATTACHMENT));

        let pc = build_attach_pc(
            Some(nid::ATTACH_BY_VALUE),
            None,
            Some("report.xlsx"),
            Some((npid, "OneDrivePro")),
            false,
        );
        match classify_attach_pc(&pc, Some(npid), "report.xlsx") {
            AttachKind::CloudLink { provider, url: _ } => {
                assert_eq!(provider.as_deref(), Some("OneDrivePro"));
            }
            AttachKind::Classic => panic!("expected CloudLink via NPMAP-resolved NPID"),
        }
    }
}
