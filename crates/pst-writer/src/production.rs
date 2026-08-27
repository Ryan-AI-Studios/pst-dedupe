//! Production Unicode PST writer v1 (Ledgerful track 0068).
//!
//! Evolves the fixture-scale writer (`crate::write_pst_from_emls`, kept as-is for
//! existing callers) into a writer that can emit a **valid Unicode, unencrypted
//! PST** for keep-set winners, with:
//!
//! - Full plain/HTML bodies via XBLOCK/XXBLOCK (no silent 2000-char truncate).
//! - A real `Root → IPM_SUBTREE → <folder>` hierarchy with a store
//!   `PidTagIpmSubtreeEntryId` (§3.2).
//! - `PidTagNativeBody` / `PidTagMessageEditorFormat` / `PidTagInternetCodepage`
//!   set to match what was actually written (§3.3.1) — never stale RTF hints
//!   (v1 never writes RTF at all).
//! - `PidTagMessageSize` computed from bytes actually written, never copied from
//!   a (possibly inflated) source size (§3.3.2).
//! - `Result`-only allocation: nothing in this module's call graph reaches the
//!   fixture path's `assert!`-based `Layout::add_node`.
//!
//! ## Multi-GB streaming scale (track 0070)
//!
//! Production path supports [`write_unicode_pst_streaming`]: messages are
//! consumed one at a time (no all-bodies pre-collect), attachments can stream
//! via [`AttachStreamSource::open_attach_stream`] into a chunked data chain
//! (`MAX_BLOCK_DATA` = 8176) without assembling a full multi-GB `Vec`, layout
//! offsets are **AMap-aware** (MS-PST first AMap at `0x4400`, interval
//! `253952`), progress exposes **physical** temp size, cooperative
//! `stop_and_finalize` finalizes a clean partial volume, and the report carries
//! SHA-256 + MD5 of the final file. See `docs/pst-writer-fidelity-v1.md`.
//!
//! ## Large single-property values: subnode storage (not silent truncation)
//!
//! This writer's [`HeapBuilder`] is **single-page** (`MAX_BLOCK_DATA` = 8176).
//! values that would overflow that page are moved to a **subnode** (NID in
//! `dwValueHnid`) instead of being clipped. MS-PST itself allows multi-block HN
//! (§2.3.1.6) and a format per-value threshold of 3580 before subnode
//! (§2.6.1.2.2 / §2.6.2.3.2); our [`MAX_HEAP_VALUE_SIZE`] = 2048 is a **documented
//! single-page HeapBuilder deviation**, not an inherent MS-PST limit. Helper
//! strings (MID / subject / sender / Display* / `message_class`) also divert
//! under a **cumulative** budget (escalate largest inline helpers when the
//! MessageSize probe heap would overflow). **Recipient-table node data** uses
//! [`PagedHeapBuilder`] (0100 Strategy A: HNHDR + HNPAGEHDR, HID `hidBlockIndex`).
//! `pst-reader`'s `PropContext` resolves subnode-typed HNIDs for PtypString/PtypBinary so round-trip verification works.
//!
//! ## Scope (v1.1 / track 0069)
//!
//! File attachments (by-value + attachment table + XBLOCK), folder-path
//! preservation under IPM_SUBTREE, MessageFlags READ|HASATTACH, and embedded
//! message depth cap. Multi-GB streaming: track **0070**. See
//! `docs/pst-writer-fidelity-v1.md`.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use byteorder::{LittleEndian, WriteBytesExt};
use md5::{Digest as Md5Digest, Md5};
use sha2::{Digest as Sha2Digest, Sha256};

use crate::{
    write_data_block, BlockEntry, HeapBuilder, Layout, NodeEntry, PagedHeapBuilder, Result,
    WriterError, CLIENT_MAGIC, HEADER_SIZE, MAX_BLOCK_DATA, NID_ASSOC_CONTENTS_TABLE_TEMPLATE,
    NID_ATTACHMENT_TABLE_TEMPLATE, NID_CONTENTS_TABLE_TEMPLATE, NID_HIERARCHY_TABLE_TEMPLATE,
    NID_MESSAGE_STORE, NID_NAME_TO_ID_MAP, NID_RECIPIENT_TABLE_TEMPLATE, NID_ROOT_FOLDER,
    NID_SEARCH_CONTENTS_TABLE_TEMPLATE, NID_TYPE_NORMAL_FOLDER, NID_TYPE_NORMAL_MESSAGE,
    NID_TYPE_SEARCH_FOLDER, PAGE_SIZE, PID_TAG_CLIENT_SUBMIT_TIME, PID_TAG_CONTENT_COUNT,
    PID_TAG_DISPLAY_NAME, PID_TAG_HAS_ATTACHMENTS, PID_TAG_INTERNET_MESSAGE_ID, PID_TAG_LTP_ROW_ID,
    PID_TAG_SENDER_EMAIL_ADDRESS, PID_TAG_SUBJECT, PST_MAGIC, PTYP_BOOLEAN, PTYP_INTEGER_32,
    PTYP_INTEGER_64, PTYP_STRING, PTYP_TIME, UNICODE_VERSION,
};

/// Peak stream chunk for multi-GB attach/body leaf writes (= one data block).
/// Documented memory model: never allocate a single buffer ≥ attach size on the
/// chunked path; with eager spill (`Layout::eager`), leaf payloads are written
/// to the same-dir temp immediately and cleared from `Layout` (`on_disk = true`).
pub const STREAM_CHUNK_SIZE: usize = MAX_BLOCK_DATA;

// ── New property tags needed for the production path ────────────────────────

const PID_TAG_MESSAGE_CLASS: u16 = 0x001A;
const PID_TAG_MESSAGE_FLAGS: u16 = 0x0E07;
const PID_TAG_CREATION_TIME: u16 = 0x3007;
const PID_TAG_LAST_MODIFICATION_TIME: u16 = 0x3008;
const PID_TAG_DISPLAY_TO: u16 = 0x0E04;
/// PidTagDisplayCc (MS-OXPROPS) — written when present (track 0080 §3.11).
const PID_TAG_DISPLAY_CC: u16 = 0x0E03;
const PID_TAG_NATIVE_BODY: u16 = 0x1016;
const PID_TAG_MESSAGE_EDITOR_FORMAT: u16 = 0x5909;
const PID_TAG_INTERNET_CODEPAGE: u16 = 0x3FDE;
const PID_TAG_BODY_HTML: u16 = 0x1013;
const PID_TAG_BODY: u16 = 0x1000;
const PID_TAG_MESSAGE_SIZE: u16 = 0x0E08;
const PID_TAG_IPM_SUBTREE_ENTRYID: u16 = 0x35E0;
/// PidTagIpmWastebasketEntryId (MS-PST §5493a0eb, "Minimum Set of Required
/// Properties" for a message store PC — track 0068 round 9, verified).
const PID_TAG_IPM_WASTEBASKET_ENTRYID: u16 = 0x35E3;
/// PidTagFinderEntryId (same source as above).
const PID_TAG_FINDER_ENTRYID: u16 = 0x35E7;
const PID_TAG_CONTAINER_CLASS: u16 = 0x3613;
const PID_TAG_RECORD_KEY: u16 = 0x0FF9;
/// PidTagContentUnreadCount — required IPM_SUBTREE initialization (MS-PST
/// §ea4d8b8a, "Top of Personal Folders" schema — track 0068 round 9).
const PID_TAG_CONTENT_UNREAD_COUNT: u16 = 0x3603;
/// PidTagSubfolders — same source as above.
const PID_TAG_SUBFOLDERS: u16 = 0x360A;
const PTYP_BINARY: u16 = 0x0102;
/// PtypObject — MS-PST §2.3.3.5 (`{Nid, ulSize}` heap allocation).
const PTYP_OBJECT: u16 = 0x000D;

// Attachment property tags (MS-PST / MS-OXPROPS) — track 0069 / 0084 / 0094.
/// Same property id as PidTagAttachDataObject; type discriminates Binary vs Object.
const PID_TAG_ATTACH_DATA_BINARY: u16 = 0x3701;
const PID_TAG_ATTACH_METHOD: u16 = 0x3705;
const PID_TAG_ATTACH_MIME_TAG: u16 = 0x370E;
const PID_TAG_ATTACH_FILENAME: u16 = 0x3704;
const PID_TAG_ATTACH_LONG_FILENAME: u16 = 0x3707;
const PID_TAG_ATTACH_LONG_PATHNAME: u16 = 0x370D;
/// PidTagAttachPathname — optional classic short path (0092 older-client tolerance).
const PID_TAG_ATTACH_PATHNAME: u16 = 0x3708;
const PID_TAG_ATTACH_SIZE: u16 = 0x0E20;
/// MS-OXCMSG attach methods used by CloudLink pointer-row honesty (0084).
const ATTACH_BY_REFERENCE: i32 = 0x0000_0002;
const ATTACH_BY_REF_RESOLVE: i32 = 0x0000_0003;
const ATTACH_BY_REF_ONLY: i32 = 0x0000_0004;
/// Preferred method encoding for CloudLink pointer rows (no binary payload).
const ATTACH_BY_WEB_REFERENCE: i32 = 0x0000_0007;

/// Fixed subnode NID for a message's attachment table (same value as the
/// PST-level template [`NID_ATTACHMENT_TABLE_TEMPLATE`]).
const NID_ATTACHMENT_TABLE: u64 = NID_ATTACHMENT_TABLE_TEMPLATE;
/// Fixed subnode NID for a message's recipient table (same value as the
/// PST-level template [`NID_RECIPIENT_TABLE_TEMPLATE`] = 0x692).
const NID_RECIPIENT_TABLE: u64 = NID_RECIPIENT_TABLE_TEMPLATE;
/// NID type for attachment objects (low 5 bits).
const NID_TYPE_ATTACHMENT: u8 = 0x05;
/// PidTagRenderingPosition — typical "not rendered in body" sentinel.
const PID_TAG_RENDERING_POSITION: u16 = 0x370B;
/// PidTagLtpRowVer — TC row version column.
const PID_TAG_LTP_ROW_VER: u16 = 0x67F3;

// Recipient table property tags (MS-PST Recipient Table Template MUST set + product).
const PID_TAG_DISPLAY_BCC: u16 = 0x0E02;
const PID_TAG_RECIPIENT_TYPE: u16 = 0x0C15;
const PID_TAG_RESPONSIBILITY: u16 = 0x0E0F;
const PID_TAG_OBJECT_TYPE: u16 = 0x0FFE;
const PID_TAG_ENTRY_ID: u16 = 0x0FFF;
const PID_TAG_ADDRESS_TYPE: u16 = 0x3002;
const PID_TAG_EMAIL_ADDRESS: u16 = 0x3003;
const PID_TAG_SEARCH_KEY: u16 = 0x300B;
const PID_TAG_DISPLAY_TYPE: u16 = 0x3900;
const PID_TAG_SMTP_ADDRESS: u16 = 0x39FE;
const PID_TAG_7BIT_DISPLAY_NAME: u16 = 0x39FF;
const PID_TAG_SEND_RICH_INFO: u16 = 0x3A40;
/// MAPI_MAILUSER — default `PidTagObjectType` for recipient rows.
const MAPI_MAILUSER: i32 = 6;
/// DT_MAILUSER — default `PidTagDisplayType`.
const DT_MAILUSER: i32 = 0;

const MSGFLAG_READ: i32 = 0x0000_0001;
const MSGFLAG_HASATTACH: i32 = 0x0000_0010;
const ATTACH_BY_VALUE: i32 = 0x0000_0001;
const ATTACH_EMBEDDED_MSG: i32 = 0x0000_0005;

/// Max folder path segments before routing to residual (spec §3.2).
const MAX_FOLDER_DEPTH: usize = 32;
/// PtypMultipleInteger32 — used only by the FAI Contents Table Template's
/// `PidTagFlatUrgency`-shaped column (0x6805). This repo's TC column model has
/// no existing precedent for a genuine PtypMultiple* value; per the verified
/// source data's own guidance, its row-column width is conservatively modeled
/// as a 4-byte HNID reference (like `PtypString`/`PtypBinary`), never as an
/// inline fixed-size value — this table has zero rows in v1 regardless, so
/// only the TCOLDESC byte-width bookkeeping is exercised, not real multi-value
/// storage/decoding. Documented judgment call — see final report.
const PTYP_MULTIPLE_INTEGER_32: u16 = 0x1003;

/// Max inlined PC heap value (UTF-16 / binary), in bytes post-encoding.
///
/// **Single-page HeapBuilder budget** (`MAX_BLOCK_DATA` = 8176), not the MS-PST
/// format per-value rule (3580 → HN, >3580 → subnode). Headroom leaves room for
/// HN header, sibling props, BTH leaf, and HNPAGEMAP on one page. Aggregate
/// helper strings still use escalate+reprobe when per-value diversion is not
/// enough (track 0093).
const MAX_HEAP_VALUE_SIZE: usize = 2048;

/// Cap on in-process `recipient_tc_truncated_events` (mirrors attach-event shape).
pub const RECIPIENT_TC_EVENTS_CAP: usize = 1000;

/// Helper string props eligible for cumulative subnode diversion (0093).
const HELPER_STRING_PIDS: &[u16] = &[
    PID_TAG_INTERNET_MESSAGE_ID,
    PID_TAG_SUBJECT,
    PID_TAG_SENDER_EMAIL_ADDRESS,
    PID_TAG_DISPLAY_TO,
    PID_TAG_DISPLAY_CC,
    PID_TAG_DISPLAY_BCC,
    PID_TAG_MESSAGE_CLASS,
];

/// Max BID entries in one XBLOCK/XXBLOCK: `(MAX_BLOCK_DATA - 8) / 8`.
const MAX_XBLOCK_ENTRIES: usize = (MAX_BLOCK_DATA - 8) / 8;

/// BTree intermediate/leaf-of-BBT entry size (key+BREF or BBTENTRY), used to size
/// how many child references fit in the 488-byte entries region of one page.
const INTERMEDIATE_ENTRY_SIZE: usize = 24;
const INTERMEDIATE_ENTRIES_PER_PAGE: usize = 488 / INTERMEDIATE_ENTRY_SIZE;

const NBT_LEAF_ENTRY_SIZE: usize = 32;
const BBT_LEAF_ENTRY_SIZE: usize = 24;

const PTYPE_BBT: u8 = 0x80;
const PTYPE_NBT: u8 = 0x81;
const PTYPE_AMAP: u8 = 0x84;

// ── Public API (spec §3.6 / 0069) ────────────────────────────────────────────

/// One attachment on a [`WriteMessage`] (track 0069).
///
/// Prefer small payloads in [`Self::data`]. Resolution order:
/// 1. **`data: Some(...)`** — used as the payload even when the `Vec` is
///    **empty** (valid zero-byte file attach). Stream is **not** consulted.
/// 2. **`data: None`** — try optional [`AttachStreamSource`] (see
///    [`write_unicode_pst_with_streams`]); `Ok(Some(_))` including empty is
///    valid; `Ok(None)` / `Err` soft-fails (`attachments_failed++`).
/// 3. No data and no stream → soft-fail.
///
/// This writer never invents attachment bytes.
#[derive(Debug, Clone, Default)]
pub struct WriteAttachment {
    pub filename: String,
    pub mime: Option<String>,
    /// Declared size; actual written length may differ if payload is shorter.
    pub size: u32,
    /// `ATTACH_BY_VALUE` (1) / `ATTACH_EMBEDDED_MSG` (5) / other.
    pub attach_method: Option<i32>,
    /// Inline / pre-buffered payload. `Some(vec![])` is a **valid zero-byte**
    /// attach; only `None` falls through to [`AttachStreamSource`].
    pub data: Option<Vec<u8>>,
    /// When true, a stream *could* be opened by a higher-level materializer
    /// (hint for callers; the writer consults [`AttachStreamSource`] only when
    /// `data` is `None`).
    pub stream_available: bool,
    pub attach_nid: Option<u64>,
    pub source_path: Option<String>,
    pub parent_nid: Option<u64>,
    /// Nested message for `ATTACH_EMBEDDED_MSG` when extractable.
    pub embedded_message: Option<Box<WriteMessage>>,
    /// Materialize hit export depth/byte budget (0094) — emit `ATTACH_DEPTH_LIMIT`
    /// even when [`Self::embedded_message`] is `None` (avoids miscount as unparsed).
    pub embedded_depth_limited: bool,
    /// Cloud/modern web-reference attach (0084): write metadata/pointer row only
    /// — never invent binary payload bytes.
    pub is_cloud_link: bool,
    /// Provider string for ledger / pointer honesty (open string).
    pub cloud_provider: Option<String>,
    /// Best-effort URL/path written on classic long-pathname when present.
    pub cloud_url: Option<String>,
    /// Optional `AttachmentPermissionType` (0092 MAY) — written only when `Some`.
    pub cloud_permission_type: Option<i32>,
}

/// Owned [`Read`] for chunked attachment payload (track 0070).
///
/// Prefer constructing via [`AttachRead::from_reader`] for true multi-GB
/// streams. [`AttachRead::from_vec`] exists only for the default
/// [`AttachStreamSource::open_attach_stream`] compatibility shim.
///
/// **0077:** optional shared [`Self::crc_suspect`] flag — a reader wrapper around
/// `AttachmentDataReader` may set it when warning-only block CRC/BID fires during
/// a successful stream. The production writer records `ATTACH_STREAM_CRC` (info)
/// after a successful read so taint is not lost when the concrete reader type is
/// erased to `Box<dyn Read>`.
pub struct AttachRead {
    inner: AttachReadInner,
    crc_suspect: Arc<AtomicBool>,
}

enum AttachReadInner {
    Cursor(Cursor<Vec<u8>>),
    Dyn(Box<dyn Read>),
}

impl AttachRead {
    /// Wrap an already-buffered payload (compat / small attaches).
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self {
            inner: AttachReadInner::Cursor(Cursor::new(data)),
            crc_suspect: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Wrap any owned [`Read`] (preferred multi-GB path).
    pub fn from_reader(reader: Box<dyn Read>) -> Self {
        Self {
            inner: AttachReadInner::Dyn(reader),
            crc_suspect: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Wrap a reader with a shared CRC-suspect flag set by the caller's wrapper.
    ///
    /// After the stream is fully consumed, check [`Self::crc_suspect`].
    pub fn from_reader_with_crc(reader: Box<dyn Read>, crc_suspect: Arc<AtomicBool>) -> Self {
        Self {
            inner: AttachReadInner::Dyn(reader),
            crc_suspect,
        }
    }

    /// True when a warning-only CRC/BID hit was recorded on this stream (0077).
    pub fn crc_suspect(&self) -> bool {
        self.crc_suspect.load(Ordering::Relaxed)
    }
}

impl Read for AttachRead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.inner {
            AttachReadInner::Cursor(c) => c.read(buf),
            AttachReadInner::Dyn(r) => r.read(buf),
        }
    }
}

/// Optional source for attachment bytes when [`WriteAttachment::data`] is
/// `None`. Soft-fail: `Err` or `Ok(None)` skips that attach and
/// increments [`WritePstReport::attachments_failed`]. `Ok(Some(vec![]))` /
/// empty stream is a valid zero-byte payload.
///
/// ## Streaming (track 0070)
///
/// Prefer [`Self::open_attach_stream`] so the writer can chunk into
/// `MAX_BLOCK_DATA` leaf blocks **without** assembling a full multi-GB
/// `Vec`. The default implementation wraps [`Self::open_attach`] in
/// [`AttachRead::from_vec`] for backward compatibility only.
pub trait AttachStreamSource {
    /// Open attach bytes for a [`WriteAttachment`] key as a full `Vec`.
    ///
    /// Prefer overriding [`Self::open_attach_stream`] for multi-GB attaches.
    fn open_attach(
        &mut self,
        source_path: Option<&str>,
        parent_nid: Option<u64>,
        attach_nid: Option<u64>,
        filename: &str,
    ) -> std::result::Result<Option<Vec<u8>>, String>;

    /// Preferred chunked path: return a [`Read`] for one attach.
    ///
    /// Default: call [`Self::open_attach`] and wrap in [`AttachRead::from_vec`].
    fn open_attach_stream(
        &mut self,
        source_path: Option<&str>,
        parent_nid: Option<u64>,
        attach_nid: Option<u64>,
        filename: &str,
    ) -> std::result::Result<Option<AttachRead>, String> {
        match self.open_attach(source_path, parent_nid, attach_nid, filename) {
            Ok(Some(bytes)) => Ok(Some(AttachRead::from_vec(bytes))),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Write pipeline stage for progress reporting (0070 / 0071 volume split).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStage {
    Planning,
    WritingMessages,
    FinalizingNdb,
    Renaming,
}

/// Progress snapshot for [`WriteProgressSink`].
#[derive(Debug, Clone)]
pub struct WriteProgress {
    pub messages_written: u64,
    pub messages_total_hint: Option<u64>,
    /// Logical payload bytes accounted (diagnostics; not physical size).
    pub payload_bytes_accounted: u64,
    /// Actual on-disk size of the in-progress temp file after flush.
    pub current_physical_size: u64,
    pub stage: WriteStage,
}

/// Cooperative progress + volume-split hook (0071 consumes this).
///
/// `should_stop_and_finalize` and [`should_cancel`](WriteProgressSink::should_cancel)
/// are checked only on **safe boundaries** (after each fully written message).
/// Mid-attach stop is not supported.
///
/// **Cancel vs early-finalize:** `should_stop_and_finalize` keeps a partial PST
/// as a completed volume (multi-volume split). `should_cancel` aborts without
/// finalizing/renaming — the incomplete same-dir temp is deleted (TempGuard).
pub trait WriteProgressSink {
    fn on_progress(&mut self, p: &WriteProgress);
    fn should_stop_and_finalize(&self, p: &WriteProgress) -> bool {
        let _ = p;
        false
    }
    /// Cooperative cancel (GUI). Default: never cancel.
    ///
    /// When true, [`write_unicode_pst_streaming`] returns [`WriterError::Cancelled`]
    /// without renaming the temp file to the final path.
    fn should_cancel(&self, p: &WriteProgress) -> bool {
        let _ = p;
        false
    }
}

/// MAPI recipient type for a [`WriteRecipient`] row (0082).
///
/// Writer-local mirror of `pst_reader::RecipientType` so production does not
/// depend on the reader crate (reader is a dev-dep for round-trip tests only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WriteRecipientType {
    /// MAPI_TO = 1
    #[default]
    To,
    /// MAPI_CC = 2
    Cc,
    /// MAPI_BCC = 3
    Bcc,
    /// Any other value (including MAPI_ORIG = 0).
    Other(u32),
}

impl WriteRecipientType {
    /// Map a raw MAPI `PidTagRecipientType` value.
    pub fn from_mapi(value: u32) -> Self {
        match value {
            1 => Self::To,
            2 => Self::Cc,
            3 => Self::Bcc,
            other => Self::Other(other),
        }
    }

    /// Raw MAPI integer for this variant.
    pub fn to_mapi(self) -> u32 {
        match self {
            Self::To => 1,
            Self::Cc => 2,
            Self::Bcc => 3,
            Self::Other(v) => v,
        }
    }

    /// True when this row is BCC (gated by [`WritePstOpts::include_bcc_recipients`]).
    pub fn is_bcc(self) -> bool {
        matches!(self, Self::Bcc)
    }
}

/// One recipient row for the per-message recipient TC (0082).
///
/// Structural columns (`ObjectType`, `Responsibility`, `RecordKey`, `EntryId`,
/// `SearchKey`, `DisplayType`, `SendRichInfo`, Ltp row ids) are synthesized by
/// the writer when omitted. Callers supply identity fields only.
#[derive(Debug, Clone, Default)]
pub struct WriteRecipient {
    pub recipient_type: WriteRecipientType,
    pub display_name: Option<String>,
    /// Address type: SMTP, EX, …
    pub address_type: Option<String>,
    pub email_address: Option<String>,
    /// `PidTagSmtpAddress` (0x39FE) when known (product extra column).
    pub smtp_address: Option<String>,
}

/// A plain message DTO the production writer consumes. Deliberately independent
/// of `dedup_engine::CanonicalMessage` — see [`from_canonical_message`].
#[derive(Debug, Clone, Default)]
pub struct WriteMessage {
    pub message_id: Option<String>,
    pub subject: String,
    pub sender: Option<String>,
    pub display_to: Option<String>,
    /// PidTagDisplayCc when present (0080 §3.11).
    pub display_cc: Option<String>,
    /// PidTagDisplayBcc — written only when [`WritePstOpts::include_bcc_recipients`]
    /// is true (0082 BCC disclosure policy; default omit).
    pub display_bcc: Option<String>,
    /// Structured recipient TC rows (0082). Empty vec still yields a zero-row
    /// recipient table subnode (MS-PST MUST). BCC rows are filtered when
    /// `include_bcc_recipients` is false.
    pub recipients: Vec<WriteRecipient>,
    /// Absolute FILETIME passthrough (100ns since 1601-01-01), if present.
    pub submit_time: Option<i64>,
    pub body_plain: Option<String>,
    pub body_html: Option<Vec<u8>>,
    pub message_class: Option<String>,
    /// Source `PidTagMessageFlags` when readable (0094 BestEffort). `None` keeps
    /// legacy synthesis (`MSGFLAG_READ`); `Some` preserves those bits (HASATTACH
    /// is still OR'd when attaches are written).
    pub message_flags: Option<u32>,
    /// Fidelity flag for reporting only — never written as a MAPI property.
    pub body_incomplete: bool,
    /// Soft-skipped / unreadable child attach rows (0094 nested extract). Emits
    /// `ATTACH_META_FAILED` at write without inventing placeholder attaches.
    pub attachments_incomplete: bool,
    /// Fidelity flag for reporting only — when true, no body is written at all
    /// (never invented) regardless of `body_plain`/`body_html` contents.
    pub body_unavailable: bool,
    /// File / embedded attachments (written unless `WritePstOpts::parents_only`).
    pub attachments: Vec<WriteAttachment>,
    /// Relative folder path from the source PST (e.g. `Inbox/Projects`).
    pub source_folder_path: Option<String>,
    /// Absolute source PST path (multi-source prefix key).
    pub source_path: Option<String>,
    /// Source message NID (locus) for attach fidelity events when attach list is empty.
    pub source_msg_nid: Option<u64>,
    /// `list_attachments` / attach-meta failed during materialize — emit one
    /// `ATTACH_META_FAILED` fail event at write time (track 0073).
    pub attach_list_failed: bool,
}

/// How user folders are laid out under IPM_SUBTREE (track 0069).
#[derive(Debug, Clone)]
pub enum FolderLayoutPolicy {
    /// Preserve `source_folder_path` under IPM_SUBTREE; multi-source unique prefixes.
    PreservePaths { multi_source_prefix: bool },
    /// Escape hatch: all messages in one folder (0068 behavior).
    Flat { folder_display_name: String },
}

impl Default for FolderLayoutPolicy {
    fn default() -> Self {
        Self::PreservePaths {
            multi_source_prefix: true,
        }
    }
}

/// How the store's 16-byte `PidTagRecordKey` / EntryID ProviderUID is built
/// (track **0087**). Default is **deterministic** from export inputs — not
/// wall-clock / PID salted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoreRecordKeyMode {
    /// Domain-separated SHA-256 preimage (§2.6). Path-independent.
    #[default]
    Deterministic,
    /// Escape hatch: time + pid style key for rare "force unique store identity".
    Ephemeral,
}

/// Options for [`write_unicode_pst`].
#[derive(Debug, Clone)]
pub struct WritePstOpts {
    /// Residual / flat folder display name (BC from 0068; default `"Unique Mail"`).
    /// Used as the residual bucket under `PreservePaths`, and as the single
    /// folder name when `folder_layout` is not set to a custom `Flat` name.
    pub folder_display_name: String,
    /// Folder layout policy (default: preserve paths with multi-source prefixes).
    pub folder_layout: FolderLayoutPolicy,
    /// Safety gate (§3.7 rule 3): by default `write_unicode_pst` refuses to
    /// write when `path` already exists. Set `true` to explicitly allow
    /// replacing it. This knob only ever governs **stale output** the caller
    /// is allowed to clobber — it never overrides the `protected_source_paths`
    /// function parameter of [`write_unicode_pst`], which is a separate,
    /// non-overridable rule (§3.7 rule 2). `write_unicode_pst` never mutates
    /// an existing file in place either way — it always writes a fresh temp
    /// file and renames over the destination on success (Windows `rename`
    /// replaces the target).
    pub overwrite: bool,
    /// Max nested `ATTACH_EMBEDDED_MSG` depth (default 3; clamped to [1, 8]).
    /// Depth 0 = top-level message; each embedded attach increments depth.
    pub max_embedded_depth: u32,
    /// When true, omit all attaches (family policy `parents_only`).
    pub parents_only: bool,
    /// When true, write Bcc recipient TC rows and `PidTagDisplayBcc` (0082).
    /// Default **false**: To+Cc only; BCC omitted from the deliverable by policy.
    pub include_bcc_recipients: bool,
    /// 0-based volume index for multi-volume unique-pst (0087). Default `0`.
    /// Bound into the store RecordKey so each volume is a distinct store.
    pub volume_index: u32,
    /// Optional job-global 32-byte seed (0087). When `Some`, each volume key is
    /// bound to the whole export job and re-bound with `volume_index` + the
    /// volume-local message fingerprint. When `None`, only the volume-local
    /// fingerprint is used (bare writer tests / single-shot writes).
    pub store_key_material: Option<[u8; 32]>,
    /// Deterministic (default) or ephemeral store RecordKey mode (0087).
    pub store_record_key_mode: StoreRecordKeyMode,
    /// Allowlisted NPMAP write plan (0092). Empty → empty stub map.
    /// Callers should [`NamedPropWritePlan::scan_messages`] before streaming write.
    pub named_prop_plan: crate::named_prop_map::NamedPropWritePlan,
    /// Known distinct source PST paths (0095). When `multi_source_prefix` is on
    /// and this list yields ≥2 distinct sources, file-stem prefixes are stable
    /// from message 1 (closes D-0070 stream-order race). Empty = discover from
    /// the message stream only.
    pub known_source_paths: Vec<String>,
}

impl Default for WritePstOpts {
    fn default() -> Self {
        Self {
            folder_display_name: "Unique Mail".to_string(),
            folder_layout: FolderLayoutPolicy::default(),
            overwrite: false,
            max_embedded_depth: 3,
            parents_only: false,
            include_bcc_recipients: false,
            volume_index: 0,
            store_key_material: None,
            store_record_key_mode: StoreRecordKeyMode::Deterministic,
            named_prop_plan: crate::named_prop_map::NamedPropWritePlan::empty(),
            known_source_paths: Vec::new(),
        }
    }
}

impl WritePstOpts {
    /// Clamped embedded depth in `[1, 8]`.
    fn embedded_depth_limit(&self) -> u32 {
        self.max_embedded_depth.clamp(1, 8)
    }

    fn residual_folder_name(&self) -> String {
        if self.folder_display_name.is_empty() {
            "Unique Mail".to_string()
        } else {
            self.folder_display_name.clone()
        }
    }
}

/// Best-effort normalized comparison path for output-safety checks: prefers
/// `canonicalize()` (resolves symlinks/relative components/case on Windows),
/// but falls back to the path as given when canonicalization fails — which is
/// expected and normal for a destination that does not exist yet. This must
/// never be used to *grant* access, only to compare two paths for equality, so
/// a fallback that's merely "less normalized" (not "insecure") is acceptable.
fn normalize_for_comparison(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Shared enforcement for spec §3.7 rule 2 / Core Mandate #3: refuse (typed
/// [`WriterError::RefusedSourceOverwrite`]) if `candidate` matches any entry
/// in `protected_source_paths`, by the same best-effort canonicalized
/// comparison used everywhere else in this module. Factored out of
/// `write_unicode_pst` so the exact same logic — not a hand-copied variant
/// that could quietly drift — is applied both to the final destination path
/// and to the temp-staging path (see `temp_sibling_path`): the temp path is
/// where bytes are *actually* written first, via `File::create`, so it needs
/// the identical protection, applied before that call, not just the final
/// rename target.
fn check_not_protected_source(candidate: &Path, protected_source_paths: &[PathBuf]) -> Result<()> {
    let normalized = normalize_for_comparison(candidate);
    if protected_source_paths
        .iter()
        .any(|src| normalize_for_comparison(src) == normalized)
    {
        return Err(WriterError::RefusedSourceOverwrite(candidate.to_path_buf()));
    }
    Ok(())
}

/// Result of a successful [`write_unicode_pst`] call.
#[derive(Debug, Clone)]
pub struct WritePstReport {
    pub messages_written: u64,
    /// Always 0 in v1: any per-message hard error fails the whole write rather
    /// than silently omitting a message (see module docs / final report).
    pub messages_skipped: u64,
    pub path: PathBuf,
    pub bytes: u64,
    /// Count of written messages whose source `WriteMessage.body_incomplete`
    /// was `true` (spec §2.4: written with available props + partial body,
    /// never invented — this surfaces how many in the write report).
    pub messages_with_incomplete_body: u64,
    /// Count of written messages whose source `WriteMessage.body_unavailable`
    /// was `true` (written with no body at all, never invented — this
    /// surfaces how many in the write report).
    pub messages_with_unavailable_body: u64,
    /// Attachments successfully written (by-value or embedded within depth).
    pub attachments_written: u64,
    /// Attachments skipped due to soft open/method/data failure.
    pub attachments_failed: u64,
    /// Attachments omitted because `parents_only` was set.
    pub attachments_omitted_by_policy: u64,
    /// User folders created under IPM_SUBTREE (residual + path folders; excludes
    /// Deleted Items / Search Root / IPM itself).
    pub folders_created: u64,
    /// Nested embedded messages written under `ATTACH_EMBEDDED_MSG`.
    pub embedded_messages_written: u64,
    /// Times an embedded attach was halted by `max_embedded_depth`.
    /// Report-level DoD-8 surface (not a per-message stored property).
    pub embedded_depth_limit_hits: u64,
    /// Method-5 attaches skipped because no extractable nested message was
    /// provided (never invented). Report-level DoD-8 surface.
    pub embedded_unparsed: u64,
    /// Messages whose empty/missing `source_folder_path` routed to the residual
    /// folder (normal; empty path alone is not an error).
    pub folder_paths_residual: u64,
    /// Messages whose path was sanitized (forbidden chars), over-depth, or
    /// contained `..` (routed to residual or altered segments).
    pub folder_paths_degraded: u64,
    /// Per-attachment fidelity events (DoD-8 surface for depth/unparsed embeds).
    /// Capped at [`ATTACHMENT_FIDELITY_EVENTS_CAP`] (first-N); see total/truncated.
    pub attachment_fidelity_events: Vec<AttachmentFidelityEvent>,
    /// Total attach events observed (may exceed Vec len when capped; 0077).
    pub attachment_fidelity_events_total: u64,
    /// True when events past the first-N cap were dropped from the Vec (0077).
    pub attachment_fidelity_events_truncated: bool,
    /// Exact count of successful attach streams that hit warning-only CRC (uncapped; 0077).
    pub attach_stream_crc_events: u64,
    /// Messages whose recipient TC was budget-truncated (0093 Strategy B; uncapped).
    pub recipient_tc_truncated_messages: u64,
    /// Total recipient rows dropped by TC budget cap (0093; uncapped).
    pub recipient_rows_truncated: u64,
    /// Per-message recipient TC truncate events (capped Vec; see total/truncated).
    pub recipient_tc_truncated_events: Vec<RecipientTcTruncatedEvent>,
    /// Total truncate events observed (may exceed Vec len when capped).
    pub recipient_tc_truncated_events_total: u64,
    /// True when truncate events past the first-N cap were dropped from the Vec.
    pub recipient_tc_truncated_events_truncated: bool,
    /// SHA-256 (hex, lowercase) of the final PST file bytes after all seeks
    /// and before rename. Strategy: hash the complete temp file after NDB
    /// finalize (header/BBT/NBT/AMaps written); matches on-disk bytes after
    /// rename.
    pub sha256_hex: String,
    /// MD5 (hex, lowercase) of the same final bytes (legacy load-file interop).
    pub md5_hex: String,
    /// Wall time of the final-hash pass (SHA-256 + MD5), milliseconds (0079).
    pub hash_ms: u64,
    /// True when [`WriteProgressSink::should_stop_and_finalize`] ended the
    /// volume early (partial message batch).
    pub finalized_early: bool,
}

/// Severity of an attachment accounting event (track 0073).
///
/// `Fail` increments [`WritePstReport::attachments_failed`]. `Info` does not
/// (policy omit, ledger truncation marker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachEventSeverity {
    Fail,
    Info,
}

impl AttachEventSeverity {
    /// Stable lowercase string for CSV / summary (`fail` | `info`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fail => "fail",
            Self::Info => "info",
        }
    }
}

/// Kind of per-attachment fidelity honesty event (DoD-8 / track 0073).
///
/// Stable public API is [`Self::as_code`] / [`Self::as_str`] (`SCREAMING_SNAKE`).
/// Former two-variant surface (`DepthLimitExceeded`, `EmbeddedUnparsed`) maps to
/// [`Self::DepthLimit`] and [`Self::EmbeddedUnparsed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentFidelityKind {
    /// method ∉ {BY_VALUE=1, EMBEDDED_MSG=5}.
    MethodUnsupported,
    /// Cloud/modern web-reference attach; pointer row may still be written (0084).
    /// Prefer over [`Self::MethodUnsupported`] when CloudLink classified.
    CloudLink,
    /// Cannot resolve/open payload (`Ok(None)` / open err).
    StreamOpenFailed,
    /// Mid-stream I/O while writing the attach chain.
    StreamReadFailed,
    /// CRC mismatch on attach stream (when distinguishable).
    StreamCrc,
    /// Block missing on attach path.
    BlockNotFound,
    /// Truncated attach data.
    DataTruncated,
    /// Documented size cap rejected the attach.
    SizeCap,
    /// Nested `ATTACH_EMBEDDED_MSG` halted by `max_embedded_depth`.
    /// Was `DepthLimitExceeded` (0069).
    DepthLimit,
    /// Method-5 attach had no extractable nested message (never invented).
    EmbeddedUnparsed,
    /// Materialize/list attach meta failed (align 0065) when surfaced at export.
    MetaFailed,
    /// `parents_only` / family omit — severity `info`, not a fail count.
    OmittedByPolicy,
    /// Last-resort unclassified soft-fail (should be rare).
    Unknown,
    /// CLI ledger CSV row-cap marker (info; not an attach failure itself).
    LedgerTruncated,
}

impl AttachmentFidelityKind {
    /// Stable reason code (`SCREAMING_SNAKE`) for CSV / summary histogram.
    pub fn as_code(self) -> &'static str {
        match self {
            Self::MethodUnsupported => "ATTACH_METHOD_UNSUPPORTED",
            Self::CloudLink => "ATTACH_CLOUD_LINK",
            Self::StreamOpenFailed => "ATTACH_STREAM_OPEN_FAILED",
            Self::StreamReadFailed => "ATTACH_STREAM_READ_FAILED",
            Self::StreamCrc => "ATTACH_STREAM_CRC",
            Self::BlockNotFound => "ATTACH_BLOCK_NOT_FOUND",
            Self::DataTruncated => "ATTACH_DATA_TRUNCATED",
            Self::SizeCap => "ATTACH_SIZE_CAP",
            Self::DepthLimit => "ATTACH_DEPTH_LIMIT",
            Self::EmbeddedUnparsed => "ATTACH_EMBEDDED_UNPARSED",
            Self::MetaFailed => "ATTACH_META_FAILED",
            Self::OmittedByPolicy => "ATTACH_OMITTED_BY_POLICY",
            Self::Unknown => "ATTACH_UNKNOWN",
            Self::LedgerTruncated => "ATTACH_LEDGER_TRUNCATED",
        }
    }

    /// Alias of [`Self::as_code`].
    pub fn as_str(self) -> &'static str {
        self.as_code()
    }

    /// Default severity for this kind (omit / ledger marker → Info; else Fail).
    ///
    /// `StreamCrc` is **Info** (0077): CRC stays warning-only; successful bytes
    /// are still written, and the event must not increment `attachments_failed`.
    pub fn default_severity(self) -> AttachEventSeverity {
        match self {
            Self::OmittedByPolicy | Self::LedgerTruncated | Self::StreamCrc => {
                AttachEventSeverity::Info
            }
            _ => AttachEventSeverity::Fail,
        }
    }
}

/// Per-attachment fidelity record with locus keys (track 0073).
///
/// Subject/filename remain for display; joinable identity is
/// `source_path` + `msg_nid` + `attach_index` / `attach_nid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentFidelityEvent {
    /// Source message subject (display only; not a primary key).
    pub message_subject: String,
    /// Attachment filename as supplied on the DTO.
    pub attach_filename: String,
    pub kind: AttachmentFidelityKind,
    /// Source PST path (same encoding as export_messages when threaded).
    pub source_path: String,
    /// Source folder path (best-effort; empty if unknown).
    pub folder_path: String,
    /// Parent message source NID (`parent_nid` / 0 if unknown).
    pub msg_nid: u64,
    /// Source attach NID when known.
    pub attach_nid: Option<u64>,
    /// 0-based index within the parent message's attach list.
    pub attach_index: u32,
    /// Declared or actual size when known.
    pub size: Option<u64>,
    /// Raw `PidTagAttachMethod` (MS-OXCMSG); `-1` if unknown.
    pub attach_method: i32,
    /// Cloud provider when kind is [`AttachmentFidelityKind::CloudLink`] (0084).
    pub cloud_provider: String,
    /// Cloud URL when kind is [`AttachmentFidelityKind::CloudLink`] (0084).
    pub cloud_url: String,
    pub severity: AttachEventSeverity,
}

/// Streaming sink for attachment accounting events (track 0073).
///
/// Critical path must not fsync-per-row: implementations should enqueue/tally
/// only. CLI wires an mpsc → background CSV writer.
pub trait AttachEventSink {
    fn on_attach_event(&mut self, event: &AttachmentFidelityEvent);
}

/// Mutable counters accumulated during a write (internal).
#[derive(Debug, Default)]
struct WriteCounters {
    messages_with_incomplete_body: u64,
    messages_with_unavailable_body: u64,
    attachments_written: u64,
    attachments_failed: u64,
    attachments_omitted_by_policy: u64,
    folders_created: u64,
    embedded_messages_written: u64,
    embedded_depth_limit_hits: u64,
    embedded_unparsed: u64,
    folder_paths_residual: u64,
    folder_paths_degraded: u64,
    attachment_fidelity_events: Vec<AttachmentFidelityEvent>,
    /// Total attach events observed (including those past the Vec cap).
    attachment_fidelity_events_total: u64,
    /// True when events beyond the first-N cap were dropped from the Vec.
    attachment_fidelity_events_truncated: bool,
    /// Exact count of [`AttachmentFidelityKind::StreamCrc`] events (uncapped; 0077 export_risk).
    attach_stream_crc_events: u64,
    recipient_tc_truncated_messages: u64,
    recipient_rows_truncated: u64,
    recipient_tc_truncated_events: Vec<RecipientTcTruncatedEvent>,
    recipient_tc_truncated_events_total: u64,
    recipient_tc_truncated_events_truncated: bool,
}

/// Cap on in-process `attachment_fidelity_events` (0077 / D-0073-vec-events).
pub const ATTACHMENT_FIDELITY_EVENTS_CAP: usize = 1000;

/// Writer-surfaced recipient TC truncation (0093 Strategy B).
///
/// Reason code is always [`RecipientTcTruncatedEvent::REASON`]. QC treats a
/// matching event as `KnownGap`; unexplained row loss without an event is Defect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientTcTruncatedEvent {
    pub message_subject: String,
    pub source_path: String,
    pub folder_path: String,
    pub msg_nid: u64,
    /// Normalized / raw Message-ID when known (QC join key fallback).
    pub message_id: String,
    pub source_count: u32,
    pub kept_count: u32,
    pub kept_to: u32,
    pub kept_cc: u32,
    pub kept_bcc: u32,
    pub dropped_to: u32,
    pub dropped_cc: u32,
    pub dropped_bcc: u32,
}

impl RecipientTcTruncatedEvent {
    /// Stable reason code for summary / QC (`SCREAMING_SNAKE`).
    pub const REASON: &'static str = "RECIPIENT_TC_TRUNCATED";

    pub fn reason(&self) -> &'static str {
        Self::REASON
    }
}

/// Build a locus-bearing attach event from message + attachment DTOs.
fn make_attach_event(
    msg: &WriteMessage,
    attach: &WriteAttachment,
    attach_index: u32,
    kind: AttachmentFidelityKind,
    severity: AttachEventSeverity,
) -> AttachmentFidelityEvent {
    let method = attach.attach_method.unwrap_or(-1);
    let msg_nid = attach.parent_nid.or(msg.source_msg_nid).unwrap_or(0);
    let size = if attach.size > 0 {
        Some(u64::from(attach.size))
    } else {
        attach.data.as_ref().map(|d| d.len() as u64)
    };
    AttachmentFidelityEvent {
        message_subject: msg.subject.clone(),
        attach_filename: attach.filename.clone(),
        kind,
        source_path: attach
            .source_path
            .clone()
            .or_else(|| msg.source_path.clone())
            .unwrap_or_default(),
        folder_path: msg.source_folder_path.clone().unwrap_or_default(),
        msg_nid,
        attach_nid: attach.attach_nid,
        attach_index,
        size,
        attach_method: method,
        severity,
        cloud_provider: attach.cloud_provider.clone().unwrap_or_default(),
        cloud_url: attach.cloud_url.clone().unwrap_or_default(),
    }
}

/// Record an attach event: count always; first-N kept in Vec (cap 1000); Fail
/// severity increments `attachments_failed`; optional sink receives a borrow.
fn record_attach_event(
    counters: &mut WriteCounters,
    event: AttachmentFidelityEvent,
    sink: &mut Option<&mut dyn AttachEventSink>,
) {
    if event.severity == AttachEventSeverity::Fail {
        counters.attachments_failed = counters.attachments_failed.saturating_add(1);
    }
    if let Some(s) = sink.as_mut() {
        s.on_attach_event(&event);
    }
    counters.attachment_fidelity_events_total =
        counters.attachment_fidelity_events_total.saturating_add(1);
    if event.kind == AttachmentFidelityKind::StreamCrc {
        // Uncapped: export_risk must not lose CRC evidence past the Vec cap (0077).
        counters.attach_stream_crc_events = counters.attach_stream_crc_events.saturating_add(1);
    }
    if counters.attachment_fidelity_events.len() < ATTACHMENT_FIDELITY_EVENTS_CAP {
        counters.attachment_fidelity_events.push(event);
    } else {
        counters.attachment_fidelity_events_truncated = true;
    }
}

/// Map a canonical recipient into a writer TC row (0082).
fn write_recipient_from_canonical(r: &dedup_engine::keepset::CanonicalRecipient) -> WriteRecipient {
    WriteRecipient {
        recipient_type: match r.recipient_type {
            dedup_engine::keepset::CanonicalRecipientType::To => WriteRecipientType::To,
            dedup_engine::keepset::CanonicalRecipientType::Cc => WriteRecipientType::Cc,
            dedup_engine::keepset::CanonicalRecipientType::Bcc => WriteRecipientType::Bcc,
            dedup_engine::keepset::CanonicalRecipientType::Other(v) => WriteRecipientType::Other(v),
        },
        display_name: r.display_name.clone(),
        address_type: r.address_type.clone(),
        email_address: r.email_address.clone(),
        smtp_address: r.smtp_address.clone(),
    }
}

/// Map a `CanonicalMessage` (0066 keep-set winner) to the plain `WriteMessage`
/// DTO this writer consumes.
///
/// Design choice (documented per spec §3.6): rather than adding an adapter crate
/// or duplicating the mapping in every caller, `pst-writer` takes a normal
/// dependency on `dedup-engine` for exactly this one free function — `pst-writer`
/// never depends on `pst-dedup-cli`, and `dedup-engine` never depends back on
/// `pst-writer`, so no cycle is introduced.
///
/// Attachments are **mapped** (0069). The second return value counts fields the
/// adapter deliberately does not map for default write fidelity (0080: non-empty
/// `display_bcc` still increments `dropped` because default write policy omits
/// BCC — 0082 maps it onto [`WriteMessage::display_bcc`] for opt-in write via
/// [`WritePstOpts::include_bcc_recipients`]). Structured `recipients` are mapped
/// in full (including Bcc rows); the write path filters Bcc when the flag is off.
/// Optional small attach `data` always maps; large attach bytes are filled by
/// the caller (or left `None` for soft-fail at write time).
pub fn from_canonical_message(
    msg: &dedup_engine::keepset::CanonicalMessage,
) -> (WriteMessage, u64) {
    let attachments: Vec<WriteAttachment> = msg
        .attachments
        .iter()
        .map(|a| {
            write_attachment_from_canonical(a, Some(&msg.locus.source_path), Some(msg.locus.nid))
        })
        .collect();
    // Only true list_attachments failure: AttachMetaFailed with an empty list.
    // Per-attach payload probe soft-fails also push AttachMetaFailed but still
    // leave metadata rows; those must not synthesize a message-level MetaFailed
    // (would double-count when the writer later emits STREAM_* for that attach).
    let attach_list_failed = attachments.is_empty()
        && msg
            .fidelity
            .degraded_reasons
            .contains(&dedup_engine::IntegrityReason::AttachMetaFailed);
    let mut dropped = 0u64;
    let source_has_bcc = msg
        .display_bcc
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
        || msg.recipients.iter().any(|r| r.recipient_type.is_bcc());
    if source_has_bcc {
        // Default write policy still omits BCC (0082 gate); count as known_gap
        // for 0080 fidelity accounting. Field is mapped onto WriteMessage for
        // opt-in `--include-bcc-recipients` / WritePstOpts.
        dropped = dropped.saturating_add(1);
    }
    let recipients: Vec<WriteRecipient> = msg
        .recipients
        .iter()
        .map(write_recipient_from_canonical)
        .collect();
    let write_msg = WriteMessage {
        message_id: msg.message_id.clone(),
        subject: msg.subject.clone().unwrap_or_default(),
        sender: msg.sender.clone(),
        display_to: msg.display_to.clone(),
        display_cc: msg.display_cc.clone(),
        display_bcc: msg.display_bcc.clone(),
        recipients,
        submit_time: msg.submit_time,
        body_plain: msg.body_plain.clone(),
        body_html: msg.body_html.clone(),
        message_class: msg.message_class.clone(),
        message_flags: msg.message_flags,
        body_incomplete: msg.body_incomplete,
        attachments_incomplete: false,
        body_unavailable: msg.body_unavailable,
        attachments,
        source_folder_path: Some(msg.locus.folder_path.clone()),
        source_path: Some(msg.locus.source_path.clone()),
        source_msg_nid: Some(msg.locus.nid),
        attach_list_failed,
    };
    (write_msg, dropped)
}

/// By-value conversion: **moves** bodies and buffered attach payloads (0079 D11).
///
/// Prefer this on the unique-pst hot path when the [`CanonicalMessage`] is dropped
/// immediately afterward — avoids a full per-winner memcpy and the transient
/// double-residency of body/attach buffers.
pub fn from_canonical_message_owned(
    msg: dedup_engine::keepset::CanonicalMessage,
) -> (WriteMessage, u64) {
    let source_path = msg.locus.source_path.clone();
    let parent_nid = msg.locus.nid;
    let folder_path = msg.locus.folder_path.clone();
    let attach_list_failed = msg.attachments.is_empty()
        && msg
            .fidelity
            .degraded_reasons
            .contains(&dedup_engine::IntegrityReason::AttachMetaFailed);
    let mut dropped = 0u64;
    let source_has_bcc = msg
        .display_bcc
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
        || msg.recipients.iter().any(|r| r.recipient_type.is_bcc());
    if source_has_bcc {
        dropped = dropped.saturating_add(1);
    }
    let recipients: Vec<WriteRecipient> = msg
        .recipients
        .iter()
        .map(write_recipient_from_canonical)
        .collect();
    let attachments: Vec<WriteAttachment> = msg
        .attachments
        .into_iter()
        .map(|a| {
            write_attachment_from_canonical_owned(a, Some(source_path.clone()), Some(parent_nid))
        })
        .collect();
    let write_msg = WriteMessage {
        message_id: msg.message_id,
        subject: msg.subject.unwrap_or_default(),
        sender: msg.sender,
        display_to: msg.display_to,
        display_cc: msg.display_cc,
        display_bcc: msg.display_bcc,
        recipients,
        submit_time: msg.submit_time,
        body_plain: msg.body_plain,
        body_html: msg.body_html,
        message_class: msg.message_class,
        message_flags: msg.message_flags,
        body_incomplete: msg.body_incomplete,
        attachments_incomplete: false,
        body_unavailable: msg.body_unavailable,
        attachments,
        source_folder_path: Some(folder_path),
        source_path: Some(source_path),
        source_msg_nid: Some(parent_nid),
        attach_list_failed,
    };
    (write_msg, dropped)
}

/// Map a nested export DTO into a [`WriteMessage`] (0094). Nests have no folder locus.
pub fn write_message_from_nested(
    n: &dedup_engine::keepset::NestedCanonicalMessage,
    source_path: Option<&str>,
) -> WriteMessage {
    let nested_nid = n.source_msg_nid;
    let attachments: Vec<WriteAttachment> = n
        .attachments
        .iter()
        .map(|a| write_attachment_from_canonical(a, source_path, nested_nid))
        .collect();
    WriteMessage {
        message_id: n.message_id.clone(),
        subject: n.subject.clone().unwrap_or_default(),
        sender: n.sender.clone(),
        display_to: n.display_to.clone(),
        display_cc: n.display_cc.clone(),
        display_bcc: n.display_bcc.clone(),
        recipients: n
            .recipients
            .iter()
            .map(write_recipient_from_canonical)
            .collect(),
        submit_time: n.submit_time,
        body_plain: n.body_plain.clone(),
        body_html: n.body_html.clone(),
        message_class: n.message_class.clone(),
        message_flags: n.message_flags,
        body_incomplete: n.body_incomplete,
        attachments_incomplete: n.attachments_incomplete,
        body_unavailable: n.body_unavailable,
        attachments,
        source_folder_path: None,
        source_path: source_path.map(str::to_string),
        source_msg_nid: nested_nid,
        attach_list_failed: false,
    }
}

fn write_attachment_from_canonical(
    a: &dedup_engine::keepset::CanonicalAttachment,
    source_path: Option<&str>,
    parent_nid: Option<u64>,
) -> WriteAttachment {
    let embedded_message = a
        .embedded_message
        .as_ref()
        .map(|n| Box::new(write_message_from_nested(n, source_path)));
    WriteAttachment {
        filename: a.filename.clone(),
        mime: a.mime.clone(),
        size: a.size,
        attach_method: a.attach_method,
        data: a.data.clone(),
        stream_available: a.stream_available,
        attach_nid: a.attach_nid,
        source_path: source_path.map(str::to_string),
        parent_nid,
        embedded_message,
        embedded_depth_limited: a.embedded_extract_limit,
        is_cloud_link: a.is_cloud_link,
        cloud_provider: a.cloud_provider.clone(),
        cloud_url: a.cloud_url.clone(),
        cloud_permission_type: a.cloud_permission_type,
    }
}

fn write_attachment_from_canonical_owned(
    a: dedup_engine::keepset::CanonicalAttachment,
    source_path: Option<String>,
    parent_nid: Option<u64>,
) -> WriteAttachment {
    let src = source_path.as_deref();
    let embedded_message = a
        .embedded_message
        .as_ref()
        .map(|n| Box::new(write_message_from_nested(n, src)));
    WriteAttachment {
        filename: a.filename,
        mime: a.mime,
        size: a.size,
        attach_method: a.attach_method,
        data: a.data,
        stream_available: a.stream_available,
        attach_nid: a.attach_nid,
        source_path,
        parent_nid,
        embedded_message,
        embedded_depth_limited: a.embedded_extract_limit,
        is_cloud_link: a.is_cloud_link,
        cloud_provider: a.cloud_provider,
        cloud_url: a.cloud_url,
        cloud_permission_type: a.cloud_permission_type,
    }
}

/// Write a production-scope Unicode, unencrypted PST containing `messages`.
///
/// See module docs and `docs/pst-writer-fidelity-v1.md` for what v1 does and
/// does not do. Writes to a `.tmp-<pid>-<entropy>` sibling of `path` (see
/// `temp_sibling_path`) and renames over `path` only after the full file is
/// written successfully (§3.7).
///
/// ## Why `protected_source_paths` is a mandatory function parameter, not a
/// field on `WritePstOpts`
///
/// It used to be a `WritePstOpts` field defaulting to `Vec::new()`. That made
/// it trivially easy to get **zero** source-overwrite protection completely
/// silently: `WritePstOpts::default()` and `WritePstOpts { overwrite: true,
/// ..Default::default() }` are both completely ordinary, easy-to-write
/// patterns, and neither of them raises any compiler warning, runtime
/// warning, or friction of any kind — the protection only existed if the
/// caller happened to remember to populate that one specific field. Making it
/// a required, separate function parameter instead means every call site must
/// type *something* for it, even a deliberately empty `&[]` — that is a
/// conscious, visible choice to opt out of protection, not an invisible
/// default. This crate deliberately does not parse or track source PSTs
/// itself (that is the caller's — e.g. a future 0069/0071 CLI's —
/// responsibility), so this can never be *complete* enforcement: the library
/// still has no way to verify the caller passed the right paths, or all of
/// them. That residual trust boundary is inherent to any library that
/// doesn't independently know its caller's inputs, and is stated here
/// plainly rather than hidden behind a struct field that looks like it
/// "just works" when left at its default.
///
/// Two independent output-safety checks (§3.7):
/// 1. **Always**, regardless of `opts.overwrite`: refuses (typed
///    [`WriterError::RefusedSourceOverwrite`]) if `path` matches any entry in
///    `protected_source_paths` — this project never mutates PST inputs
///    (Core Mandate #3), and no opt-in can override it. **This same check is
///    also applied to the computed temp-staging path** (see
///    `temp_sibling_path`), before that path is ever passed to
///    `File::create` — the temp path is where bytes are *actually* written
///    first, so checking only the final rename target would be an incomplete
///    promise: a source PST that happened to collide with the temp name
///    would otherwise be silently truncated during staging, before the
///    rename step that the final-path check guards. See `temp_sibling_path`
///    for how its name is derived to make that collision unlikely in the
///    first place, on top of this explicit check.
/// 2. By default (unless `opts.overwrite` is `true`): refuses (typed
///    [`WriterError::Refused`]) to overwrite an existing `path` at all — this
///    one *can* be legitimately overridden, since it only ever concerns stale
///    **output** the caller is allowed to clobber.
///
/// It never mutates an existing file in place either way.
///
/// Equivalent to [`write_unicode_pst_with_streams`] with no stream source
/// (`streams = None`). Prefer that entrypoint when attachment bytes live
/// outside [`WriteAttachment::data`].
pub fn write_unicode_pst(
    path: &Path,
    messages: impl IntoIterator<Item = WriteMessage>,
    protected_source_paths: &[PathBuf],
    opts: &WritePstOpts,
) -> Result<WritePstReport> {
    write_unicode_pst_with_streams(path, messages, protected_source_paths, opts, None)
}

/// Like [`write_unicode_pst`], with an optional [`AttachStreamSource`] used
/// **only** when a by-value attach has `data: None`.
///
/// Thin wrapper around [`write_unicode_pst_streaming`] with no progress or
/// attach-event sinks.
pub fn write_unicode_pst_with_streams(
    path: &Path,
    messages: impl IntoIterator<Item = WriteMessage>,
    protected_source_paths: &[PathBuf],
    opts: &WritePstOpts,
    streams: Option<&mut dyn AttachStreamSource>,
) -> Result<WritePstReport> {
    write_unicode_pst_streaming(
        path,
        messages,
        protected_source_paths,
        opts,
        streams,
        None,
        None,
    )
}

/// Streaming production write: messages are consumed **one at a time** from the
/// iterator (no full `WriteMessage` pre-collect), bodies/attaches stripped after
/// each write, AMap-aware layout, chunked attach streams with eager on-disk leaf
/// spill, physical-size progress, cooperative `stop_and_finalize`, inline
/// final-file hashes.
///
/// **Folder plan:** incremental ([`IncrementalFolderPlan`]) — each message
/// ensures folders and allocates NIDs for new folders only. Preserve residual
/// `"Unique Mail"` is **lazy** (first residual-routed message). Flat still
/// eagerly creates the display-name folder. Multi-source prefixes are stable
/// from message 1 when [`WritePstOpts::known_source_paths`] lists ≥2 distinct
/// sources (closes D-0070); otherwise prefixes are discovered from the stream.
/// Callers that already hold a `Vec<WriteMessage>` own that RAM; this path does
/// not force lazy iterators to materialize.
///
/// **Memory model (peak bound targets):**
/// - `O(N × thin folder/message metadata)` after strip — not all bodies of all messages
/// - `O(1) × STREAM_CHUNK_SIZE` (`MAX_BLOCK_DATA` = 8176) for attach stream reads
/// - Leaf block payloads spilled eagerly to same-dir temp (`on_disk = true`);
///   residual in-memory: small XBLOCK/XXBLOCK/SLBLOCK/PC heaps only
/// - Same-directory temp only; rename-only finalize (no multi-GB copy fallback)
pub fn write_unicode_pst_streaming(
    path: &Path,
    messages: impl IntoIterator<Item = WriteMessage>,
    protected_source_paths: &[PathBuf],
    opts: &WritePstOpts,
    mut streams: Option<&mut dyn AttachStreamSource>,
    mut progress: Option<&mut dyn WriteProgressSink>,
    mut attach_event_sink: Option<&mut dyn AttachEventSink>,
) -> Result<WritePstReport> {
    check_not_protected_source(path, protected_source_paths)?;

    if path.exists() && !opts.overwrite {
        return Err(WriterError::Refused(format!(
            "destination {} already exists; pass WritePstOpts {{ overwrite: true, .. }} to replace \
             it (pst-writer never overwrites by default and never mutates an existing PST in place)",
            path.display()
        )));
    }

    // Same-dir temp early (fail before multi-GB work). Parent must match `path`.
    let tmp_path = temp_sibling_path(path);
    if let (Some(out_parent), Some(tmp_parent)) = (path.parent(), tmp_path.parent()) {
        if out_parent != tmp_parent {
            return Err(WriterError::Layout(format!(
                "same-directory temp required: out parent {} != temp parent {}",
                out_parent.display(),
                tmp_parent.display()
            )));
        }
    }
    check_not_protected_source(&tmp_path, protected_source_paths)?;

    // Remove incomplete temp on any error before rename succeeds.
    struct TempGuard {
        path: PathBuf,
        keep: bool,
    }
    impl Drop for TempGuard {
        fn drop(&mut self) {
            if !self.keep {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
    let mut temp_guard = TempGuard {
        path: tmp_path.clone(),
        keep: false,
    };

    let mut layout = Layout::new();
    // Open same-dir temp + zeroed header immediately; leaf blocks spill here.
    layout.attach_eager(crate::EagerWriteCtx::create(&tmp_path)?);

    let mut payload_bytes_accounted: u64 = 0;
    let mut finalized_early = false;

    let emit_progress = |progress: &mut Option<&mut dyn WriteProgressSink>,
                         stage: WriteStage,
                         messages_written: u64,
                         payload: u64,
                         physical: u64| {
        if let Some(sink) = progress.as_mut() {
            let p = WriteProgress {
                messages_written,
                messages_total_hint: None,
                payload_bytes_accounted: payload,
                current_physical_size: physical,
                stage,
            };
            sink.on_progress(&p);
        }
    };

    emit_progress(
        &mut progress,
        WriteStage::Planning,
        0,
        0,
        layout.current_physical_size(),
    );

    // One-pass consume: no full DTO collect. Multi-GB attach bytes never live as
    // one full attach Vec (chunked stream + eager leaf spill).

    // ── Named property map (0092: allowlisted NPMAP when plan non-empty) ─────
    let named_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let named_props = crate::named_prop_map::build_named_prop_map_pc(&opts.named_prop_plan);
        let hid = build_pc_v2(&mut heap, &named_props)?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_NAME_TO_ID_MAP, named_heap, 0, 0)?;

    // ── Root folder → IPM_SUBTREE → <folder> hierarchy (§3.2) ────────────────
    let ipm_subtree_nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);

    let root_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (PID_TAG_DISPLAY_NAME, PcValue::String("Root".to_string())),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(0)),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_ROOT_FOLDER, root_heap, 0, 0)?;

    let root_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let rows = vec![(ipm_subtree_nid as u32).to_le_bytes().to_vec()];
        let hid = build_tc_inline_checked(&mut heap, &columns, &rows)?;
        heap.finalize(hid)
    };
    layout.add_node_data((NID_ROOT_FOLDER & !0x1F) | 0x0D, root_hier_heap, 0, 0)?;

    let root_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((NID_ROOT_FOLDER & !0x1F) | 0x0E, root_cont_heap, 0, 0)?;

    // Associated-contents (FAI) table, empty (§ MS-PST 2.4.2 — a complete
    // Folder object is PC + hierarchy TC + contents TC + associated-contents
    // TC, even when the latter is empty; codex round-6 P1 finding, Item 2).
    // NID type suffix 0x0F: confirmed against this repo's own canonical
    // NID-type scheme in `pst_reader::ndb::nid::NodeId::associated_contents_table`
    // (`(self.0 & !0x1F) | 0x0F`) and `NidType::AssocContentsTable`, not guessed.
    let root_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((NID_ROOT_FOLDER & !0x1F) | 0x0F, root_assoc_cont_heap, 0, 0)?;

    // Incremental folder plan: preserve residual Unique Mail is lazy (0095);
    // flat still eager. NIDs for new folders on each message. No full DTO
    // collect (DoD-1 / D-0070-dto-collect closed).
    let mut folder_plan = IncrementalFolderPlan::start(&mut layout, opts);

    // Deleted Items / Search Root (§2/§3/§4 of the round-9 verified MS-PST
    // data — supersedes the prior D-0068-05 decline, see
    // `docs/pst-writer-fidelity-v1.md`). Allocated here so both are available
    // before the IPM_SUBTREE hierarchy TC (which references Deleted Items) and
    // the message store PC (which references both via
    // `PidTagIpmWastebasketEntryId` / `PidTagFinderEntryId`) are built below.
    // IPM hierarchy TC itself is deferred until after the message loop so
    // incrementally discovered top-level user folders can be listed.
    let deleted_items_nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);
    // NID_TYPE_SEARCH_FOLDER (0x03) — verified from
    // https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/2dfb3012-b81c-466b-831c-2d2f0c29e591:
    // "The search Folder object is implemented as a PC that is identified by
    // a special NID_TYPE of NID_TYPE_SEARCH_FOLDER (0x03)." Not a child of
    // IPM_SUBTREE's hierarchy TC (the verified "Top of Personal Folders"
    // hierarchy-TC row list names only Deleted Items) — referenced solely via
    // the store's `PidTagFinderEntryId` below, so `nid_parent` is 0 like the
    // other top-level objects (store/root/named-prop-map/templates).
    let search_root_nid = layout.alloc_nid(NID_TYPE_SEARCH_FOLDER);

    // `PidTagContainerClass` (0x3613) is deliberately NOT set on the
    // IPM_SUBTREE folder itself (§3.2, review round 3 P2). Real-world
    // Unicode PSTs generated by Outlook leave the IPM_SUBTREE ("Top of
    // Personal Folders") node's own `PidTagContainerClass` absent/empty —
    // the container class convention (MS-PST/MAPI: `IPF.Note`, `IPF.Contact`,
    // etc.) exists to tell Outlook what *kind of items* a leaf mail-holding
    // folder contains, not to classify the subtree root itself, which has no
    // single item type. It is set instead on the "Unique Mail" folder below,
    // the actual folder that holds `IPM.Note` messages — see that folder's
    // PC build and `docs/pst-writer-fidelity-v1.md` for the full reasoning.
    //
    // DisplayName/ContentCount/ContentUnreadCount/Subfolders values below are
    // the exact required initialization values verified (round 9) from
    // https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/ea4d8b8a-6062-4930-94ee-555527a274d1
    // ("Top of Personal Folders" / IPM_SUBTREE schema-properties table) —
    // this supersedes the prior literal-string bug where this folder's
    // PidTagDisplayName was written as "IPM_SUBTREE" instead of the
    // MS-PST-required "Top of Personal Folders".
    let ipm_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PcValue::String("Top of Personal Folders".to_string()),
                ),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(1)),
                (PID_TAG_CONTENT_UNREAD_COUNT, PcValue::I32(0)),
                (PID_TAG_SUBFOLDERS, PcValue::Bool(true)),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(ipm_subtree_nid, ipm_heap, NID_ROOT_FOLDER, 0)?;

    // IPM hierarchy TC deferred until after message loop (incremental roots).
    // Contents + associated-contents are empty at the IPM level (messages live
    // under user folders).

    let ipm_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((ipm_subtree_nid & !0x1F) | 0x0E, ipm_cont_heap, 0, 0)?;

    // Associated-contents (FAI) table, empty — see the Root folder's comment
    // above for the MS-PST §2.4.2 rationale and NID-suffix cross-check.
    let ipm_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((ipm_subtree_nid & !0x1F) | 0x0F, ipm_assoc_cont_heap, 0, 0)?;

    // ── Deleted Items folder (§3 of the round-9 verified MS-PST data) ───────
    //
    // Same PC + hierarchy TC (empty) + contents TC (empty) + associated-
    // contents TC (empty) shape as "Unique Mail" below, per the exact
    // instruction: create it "exactly like the existing Unique Mail folder".
    // v1 never invents deleted-items content — this folder is always empty;
    // it exists to satisfy the verified MS-PST structural requirement (a
    // hierarchy-TC row under IPM_SUBTREE) and to give
    // `PidTagIpmWastebasketEntryId` (on the message store PC, below) a real
    // folder to reference instead of a dangling NID.
    let deleted_items_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PcValue::String("Deleted Items".to_string()),
                ),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(0)),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(deleted_items_nid, deleted_items_heap, ipm_subtree_nid, 0)?;

    let deleted_items_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (deleted_items_nid & !0x1F) | 0x0D,
        deleted_items_hier_heap,
        0,
        0,
    )?;

    let deleted_items_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (deleted_items_nid & !0x1F) | 0x0E,
        deleted_items_cont_heap,
        0,
        0,
    )?;

    let deleted_items_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (deleted_items_nid & !0x1F) | 0x0F,
        deleted_items_assoc_cont_heap,
        0,
        0,
    )?;

    // ── Search Root folder (§4 of the round-9 verified MS-PST data) ─────────
    //
    // "The basic schema requirements of the search Folder object PC are
    // identical to the Folder object PC" (verified, round 9) — this is given
    // the same PC + hierarchy TC (empty) + contents TC (empty) + associated-
    // contents TC (empty) shape as the other folders here (the safer,
    // more-complete-looking interpretation over a bare PC-only guess). v1
    // never implements search-criteria semantics or search-execution logic
    // and never populates this with results — it is a minimal, valid,
    // always-empty container, referenced by `PidTagFinderEntryId` on the
    // message store PC below. NOT a child of IPM_SUBTREE's hierarchy TC (see
    // `search_root_nid`'s allocation comment above).
    let search_root_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PcValue::String("Search Root".to_string()),
                ),
                (PID_TAG_CONTENT_COUNT, PcValue::I32(0)),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(search_root_nid, search_root_heap, 0, 0)?;

    let search_root_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (search_root_nid & !0x1F) | 0x0D,
        search_root_hier_heap,
        0,
        0,
    )?;

    let search_root_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (search_root_nid & !0x1F) | 0x0E,
        search_root_cont_heap,
        0,
        0,
    )?;

    let search_root_assoc_cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        (search_root_nid & !0x1F) | 0x0F,
        search_root_assoc_cont_heap,
        0,
        0,
    )?;

    // ── User folders + messages (0069 folder tree, incremental 0070) ─────────
    let mut counters = WriteCounters::default();

    let msg_iter = messages.into_iter();
    let total_hint = match msg_iter.size_hint() {
        (lo, Some(hi)) if lo == hi => Some(lo as u64),
        _ => None,
    };
    let mut message_nids: Vec<u64> = match total_hint {
        Some(n) => Vec::with_capacity(n as usize),
        None => Vec::new(),
    };
    emit_progress(
        &mut progress,
        WriteStage::WritingMessages,
        0,
        0,
        layout.current_physical_size(),
    );

    // 0087: accumulate volume-local fingerprint fields in write order while
    // streaming (bodies/attaches are freed after each message; only MID /
    // subject / submit / folder land in the store-key preimage).
    let mut volume_local_hasher = Sha256::new();
    for (msg_index, mut msg) in msg_iter.enumerate() {
        update_volume_local_fingerprint(&mut volume_local_hasher, &msg);
        let parent = folder_plan.assign_message(&mut layout, &msg, opts, msg_index);
        let body_incomplete = msg.body_incomplete;
        let body_unavailable = msg.body_unavailable;
        let approx_payload = msg.body_plain.as_ref().map(|s| s.len() as u64).unwrap_or(0)
            + msg.body_html.as_ref().map(|b| b.len() as u64).unwrap_or(0)
            + msg
                .attachments
                .iter()
                .map(|a| {
                    a.data
                        .as_ref()
                        .map(|d| d.len() as u64)
                        .unwrap_or(a.size as u64)
                })
                .sum::<u64>();

        let nid = build_message_node(
            &mut layout,
            &msg,
            parent,
            opts,
            &mut counters,
            0,
            &mut streams,
            &mut attach_event_sink,
        )?;
        message_nids.push(nid);
        if body_incomplete {
            counters.messages_with_incomplete_body += 1;
        }
        if body_unavailable {
            counters.messages_with_unavailable_body += 1;
        }
        payload_bytes_accounted = payload_bytes_accounted.saturating_add(approx_payload);

        // Free body/attach RAM before next message is pulled from the iterator.
        msg.body_plain = None;
        msg.body_html = None;
        for att in &mut msg.attachments {
            att.data = None;
            att.embedded_message = None;
        }
        drop(msg);

        let messages_written_so_far = message_nids.len() as u64;
        if let Some(sink) = progress.as_mut() {
            let p = WriteProgress {
                messages_written: messages_written_so_far,
                messages_total_hint: total_hint,
                payload_bytes_accounted,
                // True cumulative physical size of the same-dir temp (eager).
                current_physical_size: layout.current_physical_size(),
                stage: WriteStage::WritingMessages,
            };
            sink.on_progress(&p);
            // Cancel takes precedence over multi-volume early-finalize: abort
            // without renaming temp (TempGuard deletes incomplete staging).
            if sink.should_cancel(&p) {
                return Err(WriterError::Cancelled);
            }
            if sink.should_stop_and_finalize(&p) {
                finalized_early = true;
                break;
            }
        }
    }
    let messages_written = message_nids.len() as u64;
    let volume_local_fp: [u8; 32] = volume_local_hasher.finalize().into();

    // Folder counters come from the incremental plan (not pre-scanned).
    counters.folders_created = folder_plan.folders_created;
    counters.folder_paths_residual = folder_plan.folder_paths_residual;
    counters.folder_paths_degraded = folder_plan.folder_paths_degraded;

    // Hierarchy TC: all top-level user folders under IPM + Deleted Items.
    // Written after the message loop so incrementally created roots are listed.
    let folder_plan = folder_plan.into_folder_plan();
    let ipm_hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let mut rows: Vec<Vec<u8>> = folder_plan
            .roots
            .iter()
            .map(|f| (f.nid as u32).to_le_bytes().to_vec())
            .collect();
        rows.push((deleted_items_nid as u32).to_le_bytes().to_vec());
        let hid = build_tc_inline_checked(&mut heap, &columns, &rows)?;
        heap.finalize(hid)
    };
    layout.add_node_data((ipm_subtree_nid & !0x1F) | 0x0D, ipm_hier_heap, 0, 0)?;

    // Write every planned folder object (PC + hierarchy + contents + assoc).
    write_folder_tree_nodes(&mut layout, &folder_plan, ipm_subtree_nid, &message_nids)?;

    // ── Message store (PidTagIpmSubtreeEntryId — §3.2, review fold #2;
    // PidTagRecordKey — round-5 cross-model review finding, Part A;
    // PidTagIpmWastebasketEntryId / PidTagFinderEntryId — §1 of the round-9
    // verified MS-PST data, superseding the prior D-0068-05 decline) ────────
    //
    // The store's own `PidTagRecordKey` (0x0FF9) and each EntryID's 16-byte
    // ProviderUID must be the *same* value: a store-internal EntryID's
    // provider UID is conventionally the store's own unique record key, not
    // an arbitrary placeholder, so every EntryID genuinely identifies this
    // specific store. Derived once per write (0087: deterministic by default
    // from volume messages + optional job material; path is never in preimage)
    // and reused in all three EntryIDs plus the record key property itself.
    let record_key = match opts.store_record_key_mode {
        StoreRecordKeyMode::Deterministic => {
            let content_fp = resolve_content_fingerprint(
                opts.store_key_material.as_ref(),
                opts.volume_index,
                messages_written,
                &volume_local_fp,
            );
            derive_store_record_key(opts.volume_index, messages_written, &content_fp)
        }
        StoreRecordKeyMode::Ephemeral => generate_ephemeral_store_record_key(messages_written),
    };
    let ipm_subtree_entry_id = build_folder_entry_id(ipm_subtree_nid, &record_key);
    let wastebasket_entry_id = build_folder_entry_id(deleted_items_nid, &record_key);
    let finder_entry_id = build_folder_entry_id(search_root_nid, &record_key);
    let store_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(
            &mut heap,
            &[
                (
                    PID_TAG_DISPLAY_NAME,
                    PcValue::String("Personal Folders".to_string()),
                ),
                (
                    PID_TAG_IPM_SUBTREE_ENTRYID,
                    PcValue::Binary(ipm_subtree_entry_id),
                ),
                (
                    PID_TAG_IPM_WASTEBASKET_ENTRYID,
                    PcValue::Binary(wastebasket_entry_id),
                ),
                (PID_TAG_FINDER_ENTRYID, PcValue::Binary(finder_entry_id)),
                (PID_TAG_RECORD_KEY, PcValue::Binary(record_key.to_vec())),
            ],
        )?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_MESSAGE_STORE, store_heap, 0, 0)?;

    // ── Fixed MS-PST "template objects" (§5 of the round-9 verified MS-PST
    // data, superseding the prior round-6 template-objects decline note) ────
    //
    // Four fixed-NID, always-zero-row TCs: each MUST have no data rows
    // (verified on every one of the four source pages) — only the TCINFO
    // column-descriptor byte-width bookkeeping needs to be correct, not any
    // row content. Registered the same way as other top-level nodes with no
    // parent/subnode (`NID_MESSAGE_STORE`/`NID_NAME_TO_ID_MAP` above).
    // User-folder satellite TCs must not share these NIDs — `alloc_nid`
    // skips nidIndex 0x30 / 0x33 / 0x34 (track 0098).
    let hierarchy_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) =
            build_template_tc_columns(&HIERARCHY_TABLE_TEMPLATE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_HIERARCHY_TABLE_TEMPLATE, hierarchy_template_heap, 0, 0)?;

    let contents_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) =
            build_template_tc_columns(&CONTENTS_TABLE_TEMPLATE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_CONTENTS_TABLE_TEMPLATE, contents_template_heap, 0, 0)?;

    let assoc_contents_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) =
            build_template_tc_columns(&ASSOC_CONTENTS_TABLE_TEMPLATE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        NID_ASSOC_CONTENTS_TABLE_TEMPLATE,
        assoc_contents_template_heap,
        0,
        0,
    )?;

    let search_contents_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) =
            build_template_tc_columns(&SEARCH_CONTENTS_TABLE_TEMPLATE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        NID_SEARCH_CONTENTS_TABLE_TEMPLATE,
        search_contents_template_heap,
        0,
        0,
    )?;

    // Attachment Table Template (NID 0x671) — zero rows, full column schema
    // (MS-PST attachment-table template; same NID used as per-message subnode).
    let attachment_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) = build_template_tc_columns(&ATTACHMENT_TABLE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(
        NID_ATTACHMENT_TABLE_TEMPLATE,
        attachment_template_heap,
        0,
        0,
    )?;

    // Recipient Table Template (NID 0x692) — zero rows, full 14 MUST columns
    // + product PidTagSmtpAddress (MS-PST Recipient Table Template; same NID
    // used as per-message subnode key under each message).
    let recipient_template_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let (columns, total_row_width) = build_template_tc_columns(&RECIPIENT_TABLE_COLUMNS);
        let hid = build_tc_inline_checked_sized(&mut heap, &columns, &[], total_row_width)?;
        heap.finalize(hid)
    };
    layout.add_node_data(NID_RECIPIENT_TABLE_TEMPLATE, recipient_template_heap, 0, 0)?;

    // ── AMap + BTree pages, then real file offsets ───────────────────────────
    // AMap pages are placed only at MS-PST fixed offsets by calculate_offsets
    // (do not reserve a floating AMap).
    emit_progress(
        &mut progress,
        WriteStage::FinalizingNdb,
        messages_written,
        payload_bytes_accounted,
        layout.current_physical_size(),
    );

    let nbt_plan = layout.plan_tree(PTYPE_NBT, NBT_LEAF_ENTRY_SIZE, layout.nodes.len());
    let bbt_plan = layout.plan_tree(PTYPE_BBT, BBT_LEAF_ENTRY_SIZE, layout.blocks.len());

    layout.calculate_offsets();

    // ── Finalize into the already-open same-dir temp, then rename-only ───────
    {
        let mut eager = layout
            .take_eager()
            .ok_or_else(|| WriterError::Layout("eager temp writer missing at finalize".into()))?;
        let file = &mut eager.file;
        // File cursor is wherever the last eager block write left it — header
        // must be rewritten at absolute offset 0.
        file.seek(SeekFrom::Start(0))?;
        write_header_v1(file, &layout, &nbt_plan, &bbt_plan)?;
        // Rewrite all AMap pages (stubs + any newly placed at finalize).
        write_all_amap_pages_v1(file, &layout)?;

        let page_offsets = page_offset_map(&layout);
        write_nbt(file, &layout, &nbt_plan, &page_offsets)?;
        write_bbt(file, &layout, &bbt_plan, &page_offsets)?;

        for block in &layout.blocks {
            if block.on_disk {
                // Already written during message/attach streaming.
                continue;
            }
            file.seek(SeekFrom::Start(block.offset))?;
            write_data_block(file, block.bid, &block.data)?;
        }
        file.flush()?;

        if let Some(sink) = progress.as_mut() {
            let physical = file.metadata().map(|m| m.len()).unwrap_or(eager.cursor);
            let p = WriteProgress {
                messages_written,
                messages_total_hint: total_hint,
                payload_bytes_accounted,
                current_physical_size: physical,
                stage: WriteStage::FinalizingNdb,
            };
            sink.on_progress(&p);
        }
        // Drop closes the file handle before hash/rename.
        drop(eager);
    }

    // Hash complete finalized temp (all seeks done) before rename.
    let hash_started = std::time::Instant::now();
    let (sha256_hex, md5_hex) = hash_file_hex(&tmp_path)?;
    let hash_ms = hash_started.elapsed().as_millis() as u64;
    let bytes = fs::metadata(&tmp_path)
        .map(|m| m.len())
        .unwrap_or_else(|_| layout.file_size());

    emit_progress(
        &mut progress,
        WriteStage::Renaming,
        messages_written,
        payload_bytes_accounted,
        bytes,
    );

    if let Err(e) = fs::rename(&tmp_path, path) {
        return Err(WriterError::Io(e));
    }
    temp_guard.keep = true;

    Ok(WritePstReport {
        messages_written,
        messages_skipped: 0,
        path: path.to_path_buf(),
        bytes,
        messages_with_incomplete_body: counters.messages_with_incomplete_body,
        messages_with_unavailable_body: counters.messages_with_unavailable_body,
        attachments_written: counters.attachments_written,
        attachments_failed: counters.attachments_failed,
        attachments_omitted_by_policy: counters.attachments_omitted_by_policy,
        folders_created: counters.folders_created,
        embedded_messages_written: counters.embedded_messages_written,
        embedded_depth_limit_hits: counters.embedded_depth_limit_hits,
        embedded_unparsed: counters.embedded_unparsed,
        folder_paths_residual: counters.folder_paths_residual,
        folder_paths_degraded: counters.folder_paths_degraded,
        attachment_fidelity_events: counters.attachment_fidelity_events,
        attachment_fidelity_events_total: counters.attachment_fidelity_events_total,
        attachment_fidelity_events_truncated: counters.attachment_fidelity_events_truncated,
        attach_stream_crc_events: counters.attach_stream_crc_events,
        recipient_tc_truncated_messages: counters.recipient_tc_truncated_messages,
        recipient_rows_truncated: counters.recipient_rows_truncated,
        recipient_tc_truncated_events: counters.recipient_tc_truncated_events,
        recipient_tc_truncated_events_total: counters.recipient_tc_truncated_events_total,
        recipient_tc_truncated_events_truncated: counters.recipient_tc_truncated_events_truncated,
        sha256_hex,
        md5_hex,
        hash_ms,
        finalized_early,
    })
}

fn is_heap_page_overflow(err: &WriterError) -> bool {
    matches!(err, WriterError::Layout(msg) if msg.contains("heap page overflow"))
}

fn recipient_class_rank(t: WriteRecipientType) -> u8 {
    match t {
        WriteRecipientType::To => 0,
        WriteRecipientType::Cc => 1,
        WriteRecipientType::Bcc => 2,
        WriteRecipientType::Other(_) => 3,
    }
}

/// Order To → Cc → Bcc (stable within class). Do not rely on source order.
fn order_recipients_for_tc<'a>(rows: &[&'a WriteRecipient]) -> Vec<&'a WriteRecipient> {
    let mut indexed: Vec<(usize, &'a WriteRecipient)> = rows.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| {
        recipient_class_rank(a.1.recipient_type)
            .cmp(&recipient_class_rank(b.1.recipient_type))
            .then(a.0.cmp(&b.0))
    });
    indexed.into_iter().map(|(_, r)| r).collect()
}

/// Divert the largest remaining inline helper string to a subnode.
/// Returns `true` when a property was diverted.
fn escalate_largest_inline_helper(
    props: &mut [(u16, PcValue)],
    layout: &mut Layout,
    subnode_counter: &mut u32,
    subnode_entries: &mut Vec<(u64, u64, u64)>,
    written_content_bytes: &mut u64,
) -> Result<bool> {
    let mut best_idx: Option<usize> = None;
    let mut best_len = 0usize;
    for (i, (pid, val)) in props.iter().enumerate() {
        if !HELPER_STRING_PIDS.contains(pid) {
            continue;
        }
        if let PcValue::String(s) = val {
            let n = utf16le_bytes(s).len();
            if n >= best_len {
                best_len = n;
                best_idx = Some(i);
            }
        }
    }
    let Some(i) = best_idx else {
        return Ok(false);
    };
    let pid = props[i].0;
    let s = match &props[i].1 {
        PcValue::String(s) => s.clone(),
        _ => return Ok(false),
    };
    let bytes = utf16le_bytes(&s);
    *written_content_bytes = written_content_bytes.saturating_add(bytes.len() as u64);
    let sub_nid = next_subnode_nid(subnode_counter);
    let bid_data = layout.write_data_chain(bytes)?;
    subnode_entries.push((sub_nid, bid_data, 0));
    props[i] = (pid, PcValue::SubnodeString(sub_nid));
    Ok(true)
}

/// Lowercase hex of a raw digest (sha2 0.11 / md-5 hybrid-array output).
fn digest_to_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let b = bytes.as_ref();
    let mut s = String::with_capacity(b.len() * 2);
    for &byte in b {
        s.push(HEX[(byte >> 4) as usize] as char);
        s.push(HEX[(byte & 0xf) as usize] as char);
    }
    s
}

/// SHA-256 + MD5 of the complete file on disk (lowercase hex).
///
/// **0079 D7:** when the pass is hash-bound (MD5 ~500 MB/s ceiling), running
/// both digests concurrently over the same buffer is a pure CPU win with no
/// new dependency. Buffer is 1 MiB (was 256 KiB) for sequential I/O.
///
/// Note: Windows `FILE_FLAG_SEQUENTIAL_SCAN` must be set at `CreateFile` time;
/// std `File::open` does not expose that flag, so no sequential-scan hint is
/// applied here (correctness unchanged; residual D-0079-seq-scan if measured).
fn hash_file_hex(path: &Path) -> Result<(String, String)> {
    let mut file = File::open(path)?;
    let mut sha = Sha256::new();
    let mut md5 = Md5::new();
    // 1 MiB buffer: sequential hash pass benefits from larger reads on multi-GB.
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        // Concurrent digests over the same buffer (0079 §3.7).
        std::thread::scope(|s| {
            s.spawn(|| {
                sha.update(chunk);
            });
            s.spawn(|| {
                md5.update(chunk);
            });
        });
    }
    Ok((digest_to_hex(sha.finalize()), digest_to_hex(md5.finalize())))
}

/// After early stop on a collect-based plan, drop message indices never written.
#[cfg(test)]
fn trim_folder_plan_to_written(plan: &mut FolderPlan, written: usize) {
    fn trim_node(node: &mut PlannedFolder, written: usize) {
        node.message_indices.retain(|&i| i < written);
        for child in &mut node.children {
            trim_node(child, written);
        }
    }
    for root in &mut plan.roots {
        trim_node(root, written);
    }
    if plan.message_folder.len() > written {
        plan.message_folder.truncate(written);
    }
}

/// Process-wide entropy suffix for temp-staging filenames (see
/// `temp_sibling_path`), computed lazily once per process and cached.
///
/// Staging-only entropy (time + pid CRC32) — **not** final PST bytes and not
/// the store RecordKey path (0087 uses deterministic SHA-256 for that). A
/// `crc32fast::hash` over wall-clock nanoseconds plus process ID is cached
/// per-process so repeated `temp_sibling_path` calls for the same destination
/// within one run observe the identical value. This only needs to reduce the
/// ambient chance that a temp-staging name collides with an unrelated file;
/// `write_unicode_pst` also runs `check_not_protected_source` against the
/// computed temp path before `File::create`.
fn process_entropy_suffix() -> &'static str {
    static SUFFIX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SUFFIX.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();

        let mut seed = Vec::with_capacity(24);
        seed.extend_from_slice(&nanos.to_le_bytes());
        seed.extend_from_slice(&pid.to_le_bytes());

        format!("{:08x}", crc32fast::hash(&seed))
    })
}

/// Compute the temp-staging sibling path `write_unicode_pst` writes the full
/// file to before atomically renaming over `path` on success (§3.7 rule 1).
///
/// The name is `<file_name>.tmp-<pid>-<entropy>`, where `<entropy>` is
/// [`process_entropy_suffix`] — an 8-hex-digit `crc32fast` hash over
/// wall-clock nanoseconds and the process ID, not just the PID alone. A
/// purely PID-based name (the v1 scheme this replaces) is a known anti-
/// pattern for temp-file naming: PIDs are reused across process lifetimes
/// and form a small, predictable space, so a stale file left by a crashed
/// prior run, or an adversarial/mistaken input, could plausibly share the
/// exact computed name. Adding the entropy suffix reduces that ambient
/// collision likelihood; it does not need to eliminate it, because
/// `write_unicode_pst` also runs an explicit `check_not_protected_source`
/// against the returned path before `File::create` ever touches it — this
/// function's job is defense in depth, not the sole guarantee.
///
/// `pub` (not `pub(crate)`) specifically so the `pst-writer` integration
/// test suite (`tests/writer_v1.rs`, a separate crate) can call it directly
/// to compute the *exact* temp path `write_unicode_pst` will use for a given
/// destination, rather than re-guessing the naming scheme in test code and
/// risking silent drift from the real implementation.
pub fn temp_sibling_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output.pst".to_string());
    let tmp_name = format!(
        "{file_name}.tmp-{}-{}",
        std::process::id(),
        process_entropy_suffix()
    );
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(tmp_name),
        _ => PathBuf::from(tmp_name),
    }
}

// ── Folder tree planning (0069 §3.2) ─────────────────────────────────────────

/// One folder node in the planned tree under IPM_SUBTREE.
#[derive(Debug, Clone)]
struct PlannedFolder {
    display_name: String,
    /// Case-folded key for case-insensitive routing.
    key: String,
    children: Vec<PlannedFolder>,
    /// Message indices assigned directly to this folder.
    message_indices: Vec<usize>,
    nid: u64,
}

/// Result of planning the user-folder layout for a write (collect or incremental).
#[derive(Debug)]
struct FolderPlan {
    roots: Vec<PlannedFolder>,
    /// message index → leaf folder NID (collect-path / tests; streaming uses assign return).
    #[allow(dead_code)]
    message_folder: Vec<u64>,
    #[allow(dead_code)]
    folders_created: u64,
    #[allow(dead_code)]
    folder_paths_residual: u64,
    #[allow(dead_code)]
    folder_paths_degraded: u64,
}

/// One-pass folder planner: each message ensures path segments and allocates
/// NIDs only for **new** folders.
///
/// **Preserve:** residual `"Unique Mail"` is **lazy** (allocated on first
/// residual-routed message). **Flat:** the display-name folder is eager.
///
/// Multi-source prefixes (`PreservePaths { multi_source_prefix: true }`) use
/// [`WritePstOpts::known_source_paths`] when ≥2 distinct sources are pre-seeded
/// (stable from message 1; closes D-0070). Otherwise sources are discovered
/// from the stream; prefixes appear once a second source is seen.
#[derive(Debug)]
struct IncrementalFolderPlan {
    roots: Vec<PlannedFolder>,
    message_folder: Vec<u64>,
    folders_created: u64,
    folder_paths_residual: u64,
    folder_paths_degraded: u64,
    residual_name: String,
    multi_source: bool,
    /// Distinct `source_path` values observed so far (unsorted; prefixes recompute).
    sources_seen: Vec<String>,
    prefix_map: HashMap<String, String>,
}

impl IncrementalFolderPlan {
    fn start(layout: &mut Layout, opts: &WritePstOpts) -> Self {
        let residual_name = match &opts.folder_layout {
            FolderLayoutPolicy::Flat {
                folder_display_name,
            } => {
                if folder_display_name.is_empty() {
                    opts.residual_folder_name()
                } else {
                    folder_display_name.clone()
                }
            }
            FolderLayoutPolicy::PreservePaths { .. } => opts.residual_folder_name(),
        };
        let multi_source = matches!(
            opts.folder_layout,
            FolderLayoutPolicy::PreservePaths {
                multi_source_prefix: true
            }
        );

        // Flat: eager display-name folder. Preserve: lazy residual (0095).
        let mut roots = Vec::new();
        let mut folders_created = 0u64;
        if matches!(opts.folder_layout, FolderLayoutPolicy::Flat { .. }) {
            let residual_nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);
            roots.push(PlannedFolder {
                display_name: residual_name.clone(),
                key: case_fold_key(&residual_name),
                children: Vec::new(),
                message_indices: Vec::new(),
                nid: residual_nid,
            });
            folders_created = 1;
        }

        let mut sources_seen = Vec::new();
        let mut prefix_map = HashMap::new();
        if multi_source {
            for p in &opts.known_source_paths {
                if p.is_empty() {
                    continue;
                }
                if !sources_seen.iter().any(|s| s == p) {
                    sources_seen.push(p.clone());
                }
            }
            if sources_seen.len() >= 2 {
                prefix_map = unique_source_prefixes(&sources_seen);
            }
        }

        Self {
            roots,
            message_folder: Vec::new(),
            folders_created,
            folder_paths_residual: 0,
            folder_paths_degraded: 0,
            residual_name,
            multi_source,
            sources_seen,
            prefix_map,
        }
    }

    /// Ensure residual / flat display folder exists with a real NID.
    fn ensure_residual<'a>(&'a mut self, layout: &mut Layout) -> &'a mut PlannedFolder {
        let key = case_fold_key(&self.residual_name);
        if let Some(idx) = self.roots.iter().position(|c| c.key == key) {
            return &mut self.roots[idx];
        }
        let nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);
        self.folders_created = self.folders_created.saturating_add(1);
        self.roots.push(PlannedFolder {
            display_name: self.residual_name.clone(),
            key,
            children: Vec::new(),
            message_indices: Vec::new(),
            nid,
        });
        let last = self.roots.len() - 1;
        &mut self.roots[last]
    }

    /// Route `msg` into the folder tree; return parent folder NID.
    fn assign_message(
        &mut self,
        layout: &mut Layout,
        msg: &WriteMessage,
        opts: &WritePstOpts,
        msg_index: usize,
    ) -> u64 {
        // Track multi-source set and recompute prefixes once ≥2 sources seen.
        if self.multi_source {
            if let Some(sp) = msg.source_path.as_ref() {
                if !self.sources_seen.iter().any(|s| s == sp) {
                    self.sources_seen.push(sp.clone());
                    if self.sources_seen.len() >= 2 {
                        self.prefix_map = unique_source_prefixes(&self.sources_seen);
                    }
                }
            }
        }

        let parent_nid = match &opts.folder_layout {
            FolderLayoutPolicy::Flat { .. } => {
                // Intentional single-folder layout — not path residual/degraded.
                let residual = self.ensure_residual(layout);
                residual.message_indices.push(msg_index);
                residual.nid
            }
            FolderLayoutPolicy::PreservePaths { .. } => {
                let outcome = match msg.source_folder_path.as_deref() {
                    None => PathParseOutcome::Residual { degraded: false },
                    Some(p) => parse_folder_path(p),
                };

                match outcome {
                    PathParseOutcome::Segments { segs, degraded } => {
                        if degraded {
                            self.folder_paths_degraded += 1;
                        }
                        let mut full_segs: Vec<String> = Vec::new();
                        if let Some(prefix) = msg
                            .source_path
                            .as_ref()
                            .and_then(|p| self.prefix_map.get(p))
                            .cloned()
                        {
                            full_segs.push(prefix);
                        }
                        full_segs.extend(segs);
                        let leaf = ensure_path_alloc(
                            &mut self.roots,
                            &full_segs,
                            layout,
                            &mut self.folders_created,
                        );
                        leaf.message_indices.push(msg_index);
                        leaf.nid
                    }
                    PathParseOutcome::Residual { degraded } => {
                        self.folder_paths_residual += 1;
                        if degraded {
                            self.folder_paths_degraded += 1;
                        }
                        let residual = self.ensure_residual(layout);
                        residual.message_indices.push(msg_index);
                        residual.nid
                    }
                }
            }
        };

        debug_assert_eq!(self.message_folder.len(), msg_index);
        self.message_folder.push(parent_nid);
        parent_nid
    }

    fn into_folder_plan(self) -> FolderPlan {
        FolderPlan {
            roots: self.roots,
            message_folder: self.message_folder,
            folders_created: self.folders_created,
            folder_paths_residual: self.folder_paths_residual,
            folder_paths_degraded: self.folder_paths_degraded,
        }
    }
}

fn case_fold_key(s: &str) -> String {
    s.to_uppercase()
}

/// Well-known IPM / root container aliases (case-folded ASCII).
///
/// Strip only a **consecutive leading** run of these from source folder paths
/// (0095). Never strip a later user folder that happens to match.
fn is_leading_folder_alias(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "root"
            | "top of personal folders"
            | "top of information store"
            | "top of outlook data file"
            | "ipm_subtree"
    )
}

/// Drop consecutive leading IPM/root aliases; stop at the first non-alias.
fn strip_leading_folder_aliases(segs: &mut Vec<String>) {
    while segs.first().is_some_and(|s| is_leading_folder_alias(s)) {
        segs.remove(0);
    }
}

/// Sanitize one folder display-name segment (writer path rules).
///
/// Forbidden chars → `_`; trim; collapse empty / `.` / `..`; trim trailing
/// dots/spaces. Public so QC can apply the same rules when building expected keys.
pub fn sanitize_folder_segment(s: &str) -> String {
    sanitize_segment(s)
}

fn sanitize_segment(s: &str) -> String {
    let forbidden = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let mut out: String = s
        .chars()
        .map(|c| {
            if forbidden.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    out = out.trim().to_string();
    if out.is_empty() || out == "." || out == ".." {
        out = "_".to_string();
    }
    // Trim trailing dots/spaces (Windows display-name safety).
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() {
        out = "_".to_string();
    }
    out
}

/// Normalize a folder path key for QC / comparison (0095).
///
/// Trims, unifies slashes, strips consecutive leading IPM/root aliases, applies
/// writer [`sanitize_folder_segment`] to each remaining segment, joins with `/`,
/// and ASCII-lowercases. Must stay aligned with [`parse_folder_path`].
///
/// Returns `""` for residual inputs (`..`, empty, alias-only, over-depth). For
/// export-row expected keys that must mirror writer Unique Mail routing, use
/// [`folder_path_qc_expected_key`] instead.
pub fn normalize_folder_path_key(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let path = path.trim_matches('/');
    if path.is_empty() {
        return String::new();
    }
    let mut raw_segs: Vec<String> = Vec::new();
    for part in path.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            // Same as writer residual routing: no stable key for traversal.
            return String::new();
        }
        raw_segs.push(part.to_string());
    }
    strip_leading_folder_aliases(&mut raw_segs);
    if raw_segs.is_empty() {
        return String::new();
    }
    // Over-depth is residual in the writer; no structural key here.
    if raw_segs.len() > MAX_FOLDER_DEPTH {
        return String::new();
    }
    raw_segs
        .iter()
        .map(|s| sanitize_segment(s).to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

/// QC expected folder key for an export-row `folder_path`, mirroring writer routing.
///
/// Residual outcomes from [`parse_folder_path`] (empty / whitespace-only, `..`,
/// alias-only after strip, over-depth) map to `"unique mail"` — the same leaf
/// the writer uses. Preserved paths join sanitized lowercased segments with `/`.
pub fn folder_path_qc_expected_key(path: &str) -> String {
    match parse_folder_path(path) {
        PathParseOutcome::Residual { .. } => "unique mail".to_string(),
        PathParseOutcome::Segments { segs, .. } => segs
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("/"),
    }
}

/// Outcome of parsing a relative folder path for layout routing.
#[derive(Debug)]
enum PathParseOutcome {
    /// Route to residual folder. `degraded` when forced by `..`, over-depth, or
    /// empty-after-sanitize of a non-empty input — not for plain empty/missing.
    Residual { degraded: bool },
    /// Use these sanitized segments under IPM. `degraded` when any segment was
    /// altered by sanitize (forbidden chars, trailing dots, etc.).
    Segments { segs: Vec<String>, degraded: bool },
}

/// Parse a relative folder path into sanitized segments or residual.
///
/// Order: split → drop empty/`.` → reject `..` → strip leading aliases on raw
/// names → sanitize remaining → depth check.
fn parse_folder_path(path: &str) -> PathParseOutcome {
    let path = path.trim().trim_start_matches(['/', '\\']);
    if path.is_empty() {
        return PathParseOutcome::Residual { degraded: false };
    }
    let mut raw_segs = Vec::new();
    for part in path.split(['/', '\\']) {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return PathParseOutcome::Residual { degraded: true };
        }
        raw_segs.push(part.to_string());
    }
    strip_leading_folder_aliases(&mut raw_segs);
    if raw_segs.is_empty() {
        // Non-empty input was only aliases / `.` segments.
        return PathParseOutcome::Residual { degraded: true };
    }
    let mut segs = Vec::new();
    let mut degraded = false;
    for part in raw_segs {
        let sanitized = sanitize_segment(&part);
        if sanitized != part {
            degraded = true;
        }
        segs.push(sanitized);
    }
    if segs.len() > MAX_FOLDER_DEPTH {
        return PathParseOutcome::Residual { degraded: true };
    }
    PathParseOutcome::Segments { segs, degraded }
}

fn file_stem_label(path: &str) -> String {
    let p = Path::new(path);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "source".to_string());
    sanitize_segment(&stem)
}

/// Map absolute source path → unique prefix label (stable by sorted path).
///
/// Uniqueness is enforced on the **case-folded** label key (same comparison
/// folder routing uses), so `Archive.pst` and `archive.pst` never merge under
/// case-insensitive IPM children. Generated suffixes (`archive (2)`, …) are
/// also reserved globally so a third source literally named `archive (2).pst`
/// cannot collide with a disambiguated label.
fn unique_source_prefixes(sources: &[String]) -> HashMap<String, String> {
    let mut sorted: Vec<String> = sources.to_vec();
    sorted.sort();
    sorted.dedup();

    // Group by case-folded stem so Archive/archive collide intentionally.
    let mut by_stem_key: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for p in &sorted {
        let stem = file_stem_label(p);
        let key = case_fold_key(&stem);
        by_stem_key.entry(key).or_default().push((p.clone(), stem));
    }

    // Stable order of groups by first path in each group.
    let mut groups: Vec<Vec<(String, String)>> = by_stem_key.into_values().collect();
    groups.sort_by(|a, b| a[0].0.cmp(&b[0].0));
    for g in &mut groups {
        g.sort_by(|a, b| a.0.cmp(&b.0));
    }

    let mut used_keys: HashMap<String, ()> = HashMap::new();
    let mut map = HashMap::new();

    // Pre-reserve exact stems that appear as sole members of their group so
    // disambiguation for multi-member groups can see them? Actually we allocate
    // in path-sorted order across all sources for global uniqueness.
    // Flatten in sorted path order and assign first-available label per path.
    let mut all: Vec<(String, String)> = Vec::new();
    for g in groups {
        for item in g {
            all.push(item);
        }
    }
    all.sort_by(|a, b| a.0.cmp(&b.0));

    for (path, preferred_stem) in all {
        // Spec §3.2.2: first free = stem; then stem (2), stem (3), …
        // (not stem (1)). Case-folded keys reserve both preferred and suffixes.
        let mut attempt = 0u32;
        let label = loop {
            let candidate = if attempt == 0 {
                preferred_stem.clone()
            } else {
                format!("{preferred_stem} ({})", attempt + 1)
            };
            let key = case_fold_key(&candidate);
            use std::collections::hash_map::Entry;
            match used_keys.entry(key) {
                Entry::Vacant(v) => {
                    v.insert(());
                    break candidate;
                }
                Entry::Occupied(_) => {
                    attempt = attempt.saturating_add(1);
                    if attempt > 10_000 {
                        let forced = format!("{preferred_stem} [{}]", path.len());
                        used_keys.insert(case_fold_key(&forced), ());
                        break forced;
                    }
                }
            }
        };
        map.insert(path, label);
    }
    map
}

#[cfg(test)]
fn find_child_mut<'a>(
    children: &'a mut Vec<PlannedFolder>,
    display: &str,
) -> &'a mut PlannedFolder {
    let key = case_fold_key(display);
    if let Some(idx) = children.iter().position(|c| c.key == key) {
        return &mut children[idx];
    }
    children.push(PlannedFolder {
        display_name: display.to_string(),
        key,
        children: Vec::new(),
        message_indices: Vec::new(),
        nid: 0,
    });
    let last = children.len() - 1;
    &mut children[last]
}

#[cfg(test)]
fn ensure_path<'a>(
    roots: &'a mut Vec<PlannedFolder>,
    segments: &[String],
) -> &'a mut PlannedFolder {
    // Collect-based path: NIDs assigned later by `allocate_folder_nids`.
    ensure_path_with_nid(roots, segments, None, None)
}

/// Ensure folder path; when `layout` is provided, allocate NIDs for new folders only.
fn ensure_path_alloc<'a>(
    roots: &'a mut Vec<PlannedFolder>,
    segments: &[String],
    layout: &mut Layout,
    folders_created: &mut u64,
) -> &'a mut PlannedFolder {
    ensure_path_with_nid(roots, segments, Some(layout), Some(folders_created))
}

fn ensure_path_with_nid<'a>(
    roots: &'a mut Vec<PlannedFolder>,
    segments: &[String],
    mut layout: Option<&mut Layout>,
    mut folders_created: Option<&mut u64>,
) -> &'a mut PlannedFolder {
    // Ensure first segment exists under roots.
    {
        let key = case_fold_key(&segments[0]);
        if !roots.iter().any(|c| c.key == key) {
            let nid = if let Some(l) = layout.as_mut() {
                if let Some(c) = folders_created.as_mut() {
                    **c = c.saturating_add(1);
                }
                l.alloc_nid(NID_TYPE_NORMAL_FOLDER)
            } else {
                0
            };
            roots.push(PlannedFolder {
                display_name: segments[0].clone(),
                key,
                children: Vec::new(),
                message_indices: Vec::new(),
                nid,
            });
        }
    }
    // Walk by indices so we can re-borrow at each level.
    let mut idxs: Vec<usize> = Vec::with_capacity(segments.len());
    {
        let key0 = case_fold_key(&segments[0]);
        let i0 = roots.iter().position(|c| c.key == key0).unwrap_or(0);
        idxs.push(i0);
    }
    for seg in &segments[1..] {
        let parent = {
            let mut node = &mut roots[idxs[0]];
            for &ix in &idxs[1..] {
                node = &mut node.children[ix];
            }
            node
        };
        let key = case_fold_key(seg);
        let child_idx = if let Some(i) = parent.children.iter().position(|c| c.key == key) {
            i
        } else {
            let nid = if let Some(l) = layout.as_mut() {
                if let Some(c) = folders_created.as_mut() {
                    **c = c.saturating_add(1);
                }
                l.alloc_nid(NID_TYPE_NORMAL_FOLDER)
            } else {
                0
            };
            parent.children.push(PlannedFolder {
                display_name: seg.clone(),
                key,
                children: Vec::new(),
                message_indices: Vec::new(),
                nid,
            });
            parent.children.len() - 1
        };
        idxs.push(child_idx);
    }
    let mut node = &mut roots[idxs[0]];
    for &ix in &idxs[1..] {
        node = &mut node.children[ix];
    }
    node
}

/// Collect-all folder planner (unit tests / multi-source fidelity comparison).
/// Production streaming uses [`IncrementalFolderPlan`] instead.
#[cfg(test)]
fn plan_folder_tree(messages: &[WriteMessage], opts: &WritePstOpts) -> FolderPlan {
    let residual_name = match &opts.folder_layout {
        FolderLayoutPolicy::Flat {
            folder_display_name,
        } => {
            if folder_display_name.is_empty() {
                opts.residual_folder_name()
            } else {
                folder_display_name.clone()
            }
        }
        FolderLayoutPolicy::PreservePaths { .. } => opts.residual_folder_name(),
    };

    let multi_source = matches!(
        opts.folder_layout,
        FolderLayoutPolicy::PreservePaths {
            multi_source_prefix: true
        }
    );

    let mut distinct_sources: Vec<String> = opts.known_source_paths.clone();
    for m in messages {
        if let Some(sp) = &m.source_path {
            if !sp.is_empty() {
                distinct_sources.push(sp.clone());
            }
        }
    }
    distinct_sources.sort();
    distinct_sources.dedup();
    let prefix_map = if multi_source && distinct_sources.len() >= 2 {
        unique_source_prefixes(&distinct_sources)
    } else {
        HashMap::new()
    };

    let mut roots: Vec<PlannedFolder> = Vec::new();
    let message_folder = vec![0u64; messages.len()];
    let mut folder_paths_residual = 0u64;
    let mut folder_paths_degraded = 0u64;

    // Flat: eager display-name folder. Preserve: lazy residual (0095).
    if matches!(opts.folder_layout, FolderLayoutPolicy::Flat { .. }) {
        let _ = find_child_mut(&mut roots, &residual_name);
    }

    for (i, msg) in messages.iter().enumerate() {
        match &opts.folder_layout {
            FolderLayoutPolicy::Flat { .. } => {
                // Intentional single-folder layout — not path residual/degraded.
                let residual = find_child_mut(&mut roots, &residual_name);
                residual.message_indices.push(i);
            }
            FolderLayoutPolicy::PreservePaths { .. } => {
                let outcome = match msg.source_folder_path.as_deref() {
                    None => PathParseOutcome::Residual { degraded: false },
                    Some(p) => parse_folder_path(p),
                };

                match outcome {
                    PathParseOutcome::Segments { segs, degraded } => {
                        if degraded {
                            folder_paths_degraded += 1;
                        }
                        let mut full_segs: Vec<String> = Vec::new();
                        if let Some(prefix) = msg
                            .source_path
                            .as_ref()
                            .and_then(|p| prefix_map.get(p))
                            .cloned()
                        {
                            full_segs.push(prefix);
                        }
                        full_segs.extend(segs);
                        let leaf = ensure_path(&mut roots, &full_segs);
                        leaf.message_indices.push(i);
                    }
                    PathParseOutcome::Residual { degraded } => {
                        folder_paths_residual += 1;
                        if degraded {
                            folder_paths_degraded += 1;
                        }
                        let residual = find_child_mut(&mut roots, &residual_name);
                        residual.message_indices.push(i);
                    }
                }
            }
        }
    }

    // Count folders
    fn count_folders(nodes: &[PlannedFolder]) -> u64 {
        nodes.iter().map(|n| 1 + count_folders(&n.children)).sum()
    }
    let folders_created = count_folders(&roots);

    FolderPlan {
        roots,
        message_folder,
        folders_created,
        folder_paths_residual,
        folder_paths_degraded,
    }
}

#[cfg(test)]
fn allocate_folder_nids(layout: &mut Layout, plan: &mut FolderPlan) {
    fn alloc(layout: &mut Layout, node: &mut PlannedFolder) {
        node.nid = layout.alloc_nid(NID_TYPE_NORMAL_FOLDER);
        for child in &mut node.children {
            alloc(layout, child);
        }
    }
    for root in &mut plan.roots {
        alloc(layout, root);
    }

    // Fill message_folder from message_indices
    fn fill(node: &PlannedFolder, message_folder: &mut [u64]) {
        for &i in &node.message_indices {
            if i < message_folder.len() {
                message_folder[i] = node.nid;
            }
        }
        for child in &node.children {
            fill(child, message_folder);
        }
    }
    for root in &plan.roots {
        fill(root, &mut plan.message_folder);
    }
}

fn write_one_folder(
    layout: &mut Layout,
    node: &PlannedFolder,
    parent_nid: u64,
    message_nids: &[u64],
) -> Result<()> {
    let content_count = node.message_indices.len() as i32;
    let has_subfolders = !node.children.is_empty();

    let folder_heap = {
        let mut heap = HeapBuilder::new(0x6C);
        let mut props = vec![
            (
                PID_TAG_DISPLAY_NAME,
                PcValue::String(node.display_name.clone()),
            ),
            (PID_TAG_CONTENT_COUNT, PcValue::I32(content_count)),
            (
                PID_TAG_CONTAINER_CLASS,
                PcValue::String("IPF.Note".to_string()),
            ),
        ];
        if has_subfolders {
            props.push((PID_TAG_SUBFOLDERS, PcValue::Bool(true)));
            props.push((PID_TAG_CONTENT_UNREAD_COUNT, PcValue::I32(0)));
        }
        let hid = build_pc_v2(&mut heap, &props)?;
        heap.finalize(hid)
    };
    layout.add_node_data(node.nid, folder_heap, parent_nid, 0)?;

    // Hierarchy: child folder NIDs
    let hier_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let rows: Vec<Vec<u8>> = node
            .children
            .iter()
            .map(|c| (c.nid as u32).to_le_bytes().to_vec())
            .collect();
        let hid = build_tc_inline_checked(&mut heap, &columns, &rows)?;
        heap.finalize(hid)
    };
    layout.add_node_data((node.nid & !0x1F) | 0x0D, hier_heap, 0, 0)?;

    // Contents: message NIDs
    let cont_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let rows: Vec<Vec<u8>> = node
            .message_indices
            .iter()
            .filter_map(|&i| {
                message_nids
                    .get(i)
                    .map(|n| (*n as u32).to_le_bytes().to_vec())
            })
            .collect();
        let hid = build_tc_inline_checked(&mut heap, &columns, &rows)?;
        heap.finalize(hid)
    };
    layout.add_node_data((node.nid & !0x1F) | 0x0E, cont_heap, 0, 0)?;

    // Associated contents empty
    let assoc_heap = {
        let mut heap = HeapBuilder::new(0xBC);
        let columns = [(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0u16, 4u8, 0u8)];
        let hid = build_tc_inline_checked(&mut heap, &columns, &[])?;
        heap.finalize(hid)
    };
    layout.add_node_data((node.nid & !0x1F) | 0x0F, assoc_heap, 0, 0)?;

    for child in &node.children {
        write_one_folder(layout, child, node.nid, message_nids)?;
    }
    Ok(())
}

fn write_folder_tree_nodes(
    layout: &mut Layout,
    plan: &FolderPlan,
    ipm_subtree_nid: u64,
    message_nids: &[u64],
) -> Result<()> {
    for root in &plan.roots {
        write_one_folder(layout, root, ipm_subtree_nid, message_nids)?;
    }
    Ok(())
}

// ── Message node building (§3.3 / 0069 attaches) ─────────────────────────────

fn next_subnode_nid(counter: &mut u32) -> u64 {
    *counter += 1;
    // Low 5 bits = 0x1F (LTP type marker) so `Hid::hid_type() != 0`, distinguishing
    // a subnode NID from a heap HID (whose low 5 bits are always 0 by construction).
    ((*counter as u64) << 5) | 0x1F
}

fn next_attach_nid(counter: &mut u32) -> u64 {
    *counter += 1;
    ((*counter as u64) << 5) | (NID_TYPE_ATTACHMENT as u64)
}

fn utf16le_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
}

/// Short 8.3-ish attach filename fallback from a long name.
fn short_attach_filename(long: &str) -> String {
    let name = Path::new(long)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| long.to_string());
    if name.len() <= 12 {
        return name;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) => (&name[..i], &name[i..]),
        None => (name.as_str(), ""),
    };
    let stem_trunc: String = stem.chars().take(8).collect();
    format!("{stem_trunc}{ext}")
}

/// Resolved by-value attach payload: either a small in-memory buffer or a
/// chunked [`AttachRead`] stream (multi-GB path).
enum ResolvedAttach {
    Bytes(Vec<u8>),
    Stream(AttachRead),
}

/// Resolve by-value attach: present `data` (including **zero-length**
/// `Some(vec![])`), else preferred [`AttachStreamSource::open_attach_stream`].
/// Soft-fail returns `None` when data is absent and the stream is missing or
/// errors.
fn resolve_attach_payload(
    attach: &WriteAttachment,
    streams: &mut Option<&mut dyn AttachStreamSource>,
) -> Option<ResolvedAttach> {
    if let Some(data) = attach.data.as_ref() {
        return Some(ResolvedAttach::Bytes(data.clone()));
    }
    let src = streams.as_mut()?;
    match src.open_attach_stream(
        attach.source_path.as_deref(),
        attach.parent_nid,
        attach.attach_nid,
        &attach.filename,
    ) {
        Ok(Some(reader)) => Some(ResolvedAttach::Stream(reader)),
        Ok(None) | Err(_) => None,
    }
}

/// Successfully written attachment metadata used for the message attachment
/// table TC and MessageSize accounting.
struct WrittenAttach {
    nid: u64,
    bid_data: u64,
    bid_sub: u64,
    /// Bytes contributed to MessageSize (PC ± diverted binary / nested).
    size_contrib: u64,
    /// Actual attach size written into the attachment-table AttachSize column.
    attach_size: u32,
    method: i32,
    filename: String,
}

/// Write a CloudLink metadata/pointer attach row (0084 DoD-4b / 0092 named props).
///
/// - No `PidTagAttachDataBinary` (never invent payload bytes).
/// - Method: original web-ref/method when present, else `ATTACH_BY_WEB_REFERENCE`.
/// - Best-effort URL on `PidTagAttachLongPathname` (+ optional Pathname 0x3708).
/// - Allowlisted named props from [`NamedPropWritePlan`] when present.
/// - Always emits fail-severity `ATTACH_CLOUD_LINK` (payload not collected offline).
#[allow(clippy::too_many_arguments)] // named_prop_plan + ledger sink for 0092/0073
fn write_cloud_link_pointer_attach(
    layout: &mut Layout,
    msg: &WriteMessage,
    attach: &WriteAttachment,
    attach_index: u32,
    named_prop_plan: &crate::named_prop_map::NamedPropWritePlan,
    counters: &mut WriteCounters,
    attach_nid_counter: &mut u32,
    attach_event_sink: &mut Option<&mut dyn AttachEventSink>,
) -> Result<Option<WrittenAttach>> {
    use crate::named_prop_map::AllowlistedNamedProp;

    // Preserve empty filename honestly — do not invent "cloud-link".
    let filename = attach.filename.clone();
    let short = short_attach_filename(&filename);
    // Honest method for pointer rows (no binary): never advertise BY_VALUE (1)
    // without PidTagAttachDataBinary. Preserve reference / web-ref methods;
    // force ATTACH_BY_WEB_REFERENCE for by-value/unknown/zero.
    let method = match attach.attach_method {
        Some(m)
            if m == ATTACH_BY_WEB_REFERENCE
                || m == ATTACH_BY_REFERENCE
                || m == ATTACH_BY_REF_RESOLVE
                || m == ATTACH_BY_REF_ONLY =>
        {
            m
        }
        _ => ATTACH_BY_WEB_REFERENCE,
    };
    let size_i32 = i32::try_from(attach.size.min(i32::MAX as u32)).unwrap_or(i32::MAX);

    let mut props = vec![
        (
            PID_TAG_ATTACH_LONG_FILENAME,
            PcValue::String(filename.clone()),
        ),
        (PID_TAG_ATTACH_FILENAME, PcValue::String(short)),
        (PID_TAG_ATTACH_METHOD, PcValue::I32(method)),
        (PID_TAG_ATTACH_SIZE, PcValue::I32(size_i32)),
    ];
    if let Some(url) = attach
        .cloud_url
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        props.push((
            PID_TAG_ATTACH_LONG_PATHNAME,
            PcValue::String(url.to_string()),
        ));
        // Optional classic short pathname for older-client tolerance (0092).
        props.push((PID_TAG_ATTACH_PATHNAME, PcValue::String(url.to_string())));
        if let Some(npid) = named_prop_plan.npid(AllowlistedNamedProp::AttachmentUrl) {
            props.push((npid, PcValue::String(url.to_string())));
        }
    }
    if let Some(provider) = attach
        .cloud_provider
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if let Some(npid) = named_prop_plan.npid(AllowlistedNamedProp::AttachmentProviderType) {
            props.push((npid, PcValue::String(provider.to_string())));
        }
    }
    if let Some(perm) = attach.cloud_permission_type {
        if let Some(npid) = named_prop_plan.npid(AllowlistedNamedProp::AttachmentPermissionType) {
            props.push((npid, PcValue::I32(perm)));
        }
    }
    if let Some(mime) = &attach.mime {
        props.push((PID_TAG_ATTACH_MIME_TAG, PcValue::String(mime.clone())));
    }

    let attach_nid = next_attach_nid(attach_nid_counter);
    let mut heap = HeapBuilder::new(0x6C);
    let hid = build_pc_v2(&mut heap, &props)?;
    let pc_bytes = heap.finalize(hid);
    let pc_len = pc_bytes.len() as u64;
    let bid_data = layout.write_data_chain(pc_bytes)?;

    // Fail-severity cloud ledger (payload not collected); row still written.
    record_attach_event(
        counters,
        make_attach_event(
            msg,
            attach,
            attach_index,
            AttachmentFidelityKind::CloudLink,
            AttachEventSeverity::Fail,
        ),
        attach_event_sink,
    );
    // Count as written (pointer present) AND failed (payload missing) — fail
    // is already incremented by record_attach_event for Fail severity.
    counters.attachments_written += 1;

    Ok(Some(WrittenAttach {
        nid: attach_nid,
        bid_data,
        bid_sub: 0,
        size_contrib: pc_len,
        attach_size: size_i32 as u32,
        method,
        filename,
    }))
}

/// Build attach PC + data; returns written metadata for the attachment table.
///
/// MessageSize contribution (aligned with body logic):
/// - **Heap-inline** attach binary lives inside the attach PC → contribute
///   only `pc_len` (binary already counted inside the PC).
/// - **Subnode** attach binary is outside the PC → contribute
///   `pc_len + actual_len`.
/// - **Embedded** nested object is outside the attach PC →
///   `pc_len + nested_size`.
#[allow(clippy::too_many_arguments)] // streams/counters/locus needed for soft-fail fidelity
fn write_one_attachment(
    layout: &mut Layout,
    msg: &WriteMessage,
    attach: &WriteAttachment,
    attach_index: u32,
    depth: u32,
    max_depth: u32,
    include_bcc_recipients: bool,
    named_prop_plan: &crate::named_prop_map::NamedPropWritePlan,
    counters: &mut WriteCounters,
    attach_nid_counter: &mut u32,
    streams: &mut Option<&mut dyn AttachStreamSource>,
    attach_event_sink: &mut Option<&mut dyn AttachEventSink>,
) -> Result<Option<WrittenAttach>> {
    let method = attach.attach_method.unwrap_or(ATTACH_BY_VALUE);

    // 0084 CloudLink pointer preserve (anti-ghost): write metadata/pointer attach
    // row without inventing binary payload; still emit fail-severity ATTACH_CLOUD_LINK.
    if attach.is_cloud_link {
        return write_cloud_link_pointer_attach(
            layout,
            msg,
            attach,
            attach_index,
            named_prop_plan,
            counters,
            attach_nid_counter,
            attach_event_sink,
        );
    }

    // Non-cloud reference / OLE — skip soft (pre-0084 ghost path retained).
    if method != ATTACH_BY_VALUE && method != ATTACH_EMBEDDED_MSG {
        record_attach_event(
            counters,
            make_attach_event(
                msg,
                attach,
                attach_index,
                AttachmentFidelityKind::MethodUnsupported,
                AttachEventSeverity::Fail,
            ),
            attach_event_sink,
        );
        return Ok(None);
    }

    if method == ATTACH_EMBEDDED_MSG {
        if depth >= max_depth || attach.embedded_depth_limited {
            counters.embedded_depth_limit_hits += 1;
            record_attach_event(
                counters,
                make_attach_event(
                    msg,
                    attach,
                    attach_index,
                    AttachmentFidelityKind::DepthLimit,
                    AttachEventSeverity::Fail,
                ),
                attach_event_sink,
            );
            return Ok(None);
        }
        let Some(embedded) = attach.embedded_message.as_ref() else {
            // Not extractable — never invent nested content.
            counters.embedded_unparsed += 1;
            record_attach_event(
                counters,
                make_attach_event(
                    msg,
                    attach,
                    attach_index,
                    AttachmentFidelityKind::EmbeddedUnparsed,
                    AttachEventSeverity::Fail,
                ),
                attach_event_sink,
            );
            return Ok(None);
        };

        let attach_nid = next_attach_nid(attach_nid_counter);
        // Build nested message as a subnode object under the attach (not NBT).
        // Spec discovery: PidTagAttachDataObject as PtypObject 0x3701 / 0x000D
        // (MS-PST §2.4.6.2.2 + §2.3.3.5). Also keep subnode leaf link.
        let (nested_nid, nested_bid, nested_sub, nested_size) = build_embedded_message_object(
            layout,
            embedded,
            depth + 1,
            max_depth,
            include_bcc_recipients,
            named_prop_plan,
            counters,
            streams,
            attach_event_sink,
        )?;

        let attach_sub_entries = vec![(nested_nid, nested_bid, nested_sub)];
        let attach_sub_bid = layout.add_subnode_leaf(&attach_sub_entries)?;

        // Preserve source filename (including empty) — do not invent "Embedded Message"
        // (QC attachment multiset / 0094 honesty).
        let filename = attach.filename.clone();
        let short = short_attach_filename(&filename);
        let size_i32 = i32::try_from(nested_size.min(i32::MAX as u64)).unwrap_or(i32::MAX);
        let attach_size = size_i32 as u32;
        let object_size = u32::try_from(nested_size.min(u64::from(u32::MAX))).unwrap_or(u32::MAX);

        let mut props = vec![
            (PID_TAG_ATTACH_METHOD, PcValue::I32(ATTACH_EMBEDDED_MSG)),
            (PID_TAG_ATTACH_SIZE, PcValue::I32(size_i32)),
            // PidTagAttachDataObject (same id as binary; PtypObject not PtypBinary).
            (
                PID_TAG_ATTACH_DATA_BINARY,
                PcValue::Object {
                    nid: nested_nid,
                    size: object_size,
                },
            ),
        ];
        if !filename.is_empty() {
            props.push((
                PID_TAG_ATTACH_LONG_FILENAME,
                PcValue::String(filename.clone()),
            ));
            props.push((PID_TAG_ATTACH_FILENAME, PcValue::String(short)));
        }
        if let Some(mime) = &attach.mime {
            props.push((PID_TAG_ATTACH_MIME_TAG, PcValue::String(mime.clone())));
        }

        let mut heap = HeapBuilder::new(0x6C);
        let hid = build_pc_v2(&mut heap, &props)?;
        let pc_bytes = heap.finalize(hid);
        let pc_len = pc_bytes.len() as u64;
        let bid_data = layout.write_data_chain(pc_bytes)?;

        counters.attachments_written += 1;
        counters.embedded_messages_written += 1;
        return Ok(Some(WrittenAttach {
            nid: attach_nid,
            bid_data,
            bid_sub: attach_sub_bid,
            size_contrib: pc_len + nested_size,
            attach_size,
            method: ATTACH_EMBEDDED_MSG,
            filename,
        }));
    }

    // ATTACH_BY_VALUE — resolve without inventing.
    let Some(payload) = resolve_attach_payload(attach, streams) else {
        record_attach_event(
            counters,
            make_attach_event(
                msg,
                attach,
                attach_index,
                AttachmentFidelityKind::StreamOpenFailed,
                AttachEventSeverity::Fail,
            ),
            attach_event_sink,
        );
        return Ok(None);
    };

    let attach_nid = next_attach_nid(attach_nid_counter);
    let filename = if attach.filename.is_empty() {
        "attachment.bin".to_string()
    } else {
        attach.filename.clone()
    };
    let short = short_attach_filename(&filename);

    let mut attach_sub_entries: Vec<(u64, u64, u64)> = Vec::new();
    let mut body_counter = 0u32;

    // Write binary: heap-inline when small `Bytes`; otherwise subnode data chain
    // (chunked from `Stream` without a full multi-GB Vec).
    let (actual_len, diverted, data_prop, stream_crc_suspect) = match payload {
        ResolvedAttach::Bytes(data) => {
            let actual_len = data.len() as u64;
            if data.len() > MAX_HEAP_VALUE_SIZE {
                let sub_nid = next_subnode_nid(&mut body_counter);
                let bid = layout.write_data_chain(data)?;
                attach_sub_entries.push((sub_nid, bid, 0));
                (actual_len, true, PcValue::SubnodeBinary(sub_nid), false)
            } else {
                (actual_len, false, PcValue::Binary(data), false)
            }
        }
        ResolvedAttach::Stream(mut reader) => {
            // Chunked chain path (closes D-0069-stream-buffer): never assemble
            // a full attach Vec. Soft-fail mid-stream skips the attach.
            let sub_nid = next_subnode_nid(&mut body_counter);
            let (bid, total_len) = match layout.write_data_chain_from_reader(&mut reader) {
                Ok(v) => v,
                Err(_) => {
                    // WriterError does not yet distinguish CRC/block/truncated on
                    // this path — map to STREAM_READ_FAILED (0074 may refine).
                    record_attach_event(
                        counters,
                        make_attach_event(
                            msg,
                            attach,
                            attach_index,
                            AttachmentFidelityKind::StreamReadFailed,
                            AttachEventSeverity::Fail,
                        ),
                        attach_event_sink,
                    );
                    return Ok(None);
                }
            };
            // 0077: warning-only CRC that still returned Ok(bytes) must taint.
            // Check after full stream consume; flag is set by reader wrapper.
            let crc_hit = reader.crc_suspect();
            if total_len == 0 {
                // Valid zero-byte attach via stream.
                (0, false, PcValue::Binary(Vec::new()), crc_hit)
            } else if total_len as usize <= MAX_HEAP_VALUE_SIZE && bid != 0 {
                // Small stream result still lives as a data chain (subnode).
                // Prefer subnode for stream path to avoid re-buffering.
                attach_sub_entries.push((sub_nid, bid, 0));
                (total_len, true, PcValue::SubnodeBinary(sub_nid), crc_hit)
            } else {
                attach_sub_entries.push((sub_nid, bid, 0));
                (total_len, true, PcValue::SubnodeBinary(sub_nid), crc_hit)
            }
        }
    };

    // Successful write with late CRC taint: fidelity event only (not a fail).
    if stream_crc_suspect {
        record_attach_event(
            counters,
            make_attach_event(
                msg,
                attach,
                attach_index,
                AttachmentFidelityKind::StreamCrc,
                AttachEventSeverity::Info,
            ),
            attach_event_sink,
        );
    }

    let size_i32 = i32::try_from(actual_len.min(i32::MAX as u64)).unwrap_or(i32::MAX);
    let attach_size = size_i32 as u32;

    let mut props = vec![
        (
            PID_TAG_ATTACH_LONG_FILENAME,
            PcValue::String(filename.clone()),
        ),
        (PID_TAG_ATTACH_FILENAME, PcValue::String(short)),
        (PID_TAG_ATTACH_METHOD, PcValue::I32(ATTACH_BY_VALUE)),
        (PID_TAG_ATTACH_SIZE, PcValue::I32(size_i32)),
        (PID_TAG_ATTACH_DATA_BINARY, data_prop),
    ];
    if let Some(mime) = &attach.mime {
        props.push((PID_TAG_ATTACH_MIME_TAG, PcValue::String(mime.clone())));
    }

    let mut heap = HeapBuilder::new(0x6C);
    let hid = build_pc_v2(&mut heap, &props)?;
    let pc_bytes = heap.finalize(hid);
    let pc_len = pc_bytes.len() as u64;
    let bid_data = layout.write_data_chain(pc_bytes)?;
    let bid_sub = if attach_sub_entries.is_empty() {
        0
    } else {
        layout.add_subnode_leaf(&attach_sub_entries)?
    };

    counters.attachments_written += 1;
    let contrib = if diverted {
        pc_len + actual_len
    } else {
        pc_len
    };
    Ok(Some(WrittenAttach {
        nid: attach_nid,
        bid_data,
        bid_sub,
        size_contrib: contrib,
        attach_size,
        method: ATTACH_BY_VALUE,
        filename,
    }))
}

/// Nested message object stored only as a subnode (not a top-level NBT entry).
/// Returns `(nid, bid_data, bid_sub, size_contrib)`.
#[allow(clippy::too_many_arguments)] // include_bcc + streams/counters for nested fidelity
fn build_embedded_message_object(
    layout: &mut Layout,
    msg: &WriteMessage,
    depth: u32,
    max_depth: u32,
    include_bcc_recipients: bool,
    named_prop_plan: &crate::named_prop_map::NamedPropWritePlan,
    counters: &mut WriteCounters,
    streams: &mut Option<&mut dyn AttachStreamSource>,
    attach_event_sink: &mut Option<&mut dyn AttachEventSink>,
) -> Result<(u64, u64, u64, u64)> {
    // Allocate a message-type NID for use as a subnode key only (not an NBT entry).
    let msg_nid = layout.alloc_nid(NID_TYPE_NORMAL_MESSAGE);

    let (heap_bytes, sub_bid, size_contrib) = build_message_payload(
        layout,
        msg,
        max_depth,
        include_bcc_recipients,
        named_prop_plan,
        counters,
        depth,
        streams,
        attach_event_sink,
    )?;
    let bid_data = layout.write_data_chain(heap_bytes)?;
    Ok((msg_nid, bid_data, sub_bid, size_contrib))
}

/// Shared body/attach/recipient property builder for top-level and embedded messages.
/// Returns `(pc_heap_bytes, bid_sub, size_without_message_size_prop)`.
#[allow(clippy::too_many_arguments)] // include_bcc + streams/counters for fidelity path
fn build_message_payload(
    layout: &mut Layout,
    msg: &WriteMessage,
    max_depth: u32,
    include_bcc_recipients: bool,
    named_prop_plan: &crate::named_prop_map::NamedPropWritePlan,
    counters: &mut WriteCounters,
    depth: u32,
    streams: &mut Option<&mut dyn AttachStreamSource>,
    attach_event_sink: &mut Option<&mut dyn AttachEventSink>,
) -> Result<(Vec<u8>, u64, u64)> {
    let plain_text: Option<&str> = if msg.body_unavailable {
        None
    } else {
        msg.body_plain.as_deref()
    };
    let html_bytes: Option<&[u8]> = if msg.body_unavailable {
        None
    } else {
        msg.body_html.as_deref().filter(|b| !b.is_empty())
    };

    let mut subnode_entries: Vec<(u64, u64, u64)> = Vec::new();
    let mut subnode_counter = 0u32;
    let mut written_content_bytes: u64 = 0;
    let mut attach_nid_counter = 0u32;
    let mut written_attaches: Vec<WrittenAttach> = Vec::new();

    let mut props: Vec<(u16, PcValue)> = Vec::new();
    // Per-value diversion vs MAX_HEAP_VALUE_SIZE; cumulative escalate happens at
    // the MessageSize probe (multiple ~2 KiB helpers can still overflow 8176).
    {
        let mut push_string_prop = |pid: u16, s: &str| -> Result<()> {
            let bytes = utf16le_bytes(s);
            if bytes.len() > MAX_HEAP_VALUE_SIZE {
                written_content_bytes += bytes.len() as u64;
                let sub_nid = next_subnode_nid(&mut subnode_counter);
                let bid_data = layout.write_data_chain(bytes)?;
                subnode_entries.push((sub_nid, bid_data, 0));
                props.push((pid, PcValue::SubnodeString(sub_nid)));
            } else {
                props.push((pid, PcValue::String(s.to_string())));
            }
            Ok(())
        };
        if let Some(mid) = &msg.message_id {
            push_string_prop(PID_TAG_INTERNET_MESSAGE_ID, mid)?;
        }
        push_string_prop(PID_TAG_SUBJECT, &msg.subject)?;
        if let Some(sender) = &msg.sender {
            push_string_prop(PID_TAG_SENDER_EMAIL_ADDRESS, sender)?;
        }
        if let Some(display_to) = &msg.display_to {
            push_string_prop(PID_TAG_DISPLAY_TO, display_to)?;
        }
        if let Some(display_cc) = &msg.display_cc {
            if !display_cc.trim().is_empty() {
                push_string_prop(PID_TAG_DISPLAY_CC, display_cc)?;
            }
        }
        // PidTagDisplayBcc only when opt-in BCC disclosure is enabled (0082 §2.5 rule 5).
        if include_bcc_recipients {
            if let Some(display_bcc) = &msg.display_bcc {
                if !display_bcc.trim().is_empty() {
                    push_string_prop(PID_TAG_DISPLAY_BCC, display_bcc)?;
                }
            }
        }
        let message_class = msg.message_class.as_deref().unwrap_or("IPM.Note");
        push_string_prop(PID_TAG_MESSAGE_CLASS, message_class)?;
    }
    if let Some(submit_time) = msg.submit_time {
        props.push((PID_TAG_CLIENT_SUBMIT_TIME, PcValue::Time(submit_time)));
    }
    if let Some(submit_time) = msg.submit_time {
        props.push((PID_TAG_CREATION_TIME, PcValue::Time(submit_time)));
        props.push((PID_TAG_LAST_MODIFICATION_TIME, PcValue::Time(submit_time)));
    }

    if let Some(plain) = plain_text {
        let bytes = utf16le_bytes(plain);
        if bytes.len() > MAX_HEAP_VALUE_SIZE {
            written_content_bytes += bytes.len() as u64;
            let sub_nid = next_subnode_nid(&mut subnode_counter);
            let bid_data = layout.write_data_chain(bytes)?;
            subnode_entries.push((sub_nid, bid_data, 0));
            props.push((PID_TAG_BODY, PcValue::SubnodeString(sub_nid)));
        } else {
            props.push((PID_TAG_BODY, PcValue::String(plain.to_string())));
        }
    }
    if let Some(html) = html_bytes {
        if html.len() > MAX_HEAP_VALUE_SIZE {
            written_content_bytes += html.len() as u64;
            let sub_nid = next_subnode_nid(&mut subnode_counter);
            let bid_data = layout.write_data_chain(html.to_vec())?;
            subnode_entries.push((sub_nid, bid_data, 0));
            props.push((PID_TAG_BODY_HTML, PcValue::SubnodeBinary(sub_nid)));
        } else {
            props.push((PID_TAG_BODY_HTML, PcValue::Binary(html.to_vec())));
        }
    }

    if html_bytes.is_some() {
        props.push((PID_TAG_NATIVE_BODY, PcValue::I32(3)));
        props.push((PID_TAG_MESSAGE_EDITOR_FORMAT, PcValue::I32(2)));
        props.push((PID_TAG_INTERNET_CODEPAGE, PcValue::I32(65001)));
    } else if plain_text.is_some() {
        props.push((PID_TAG_NATIVE_BODY, PcValue::I32(1)));
        props.push((PID_TAG_MESSAGE_EDITOR_FORMAT, PcValue::I32(1)));
        props.push((PID_TAG_INTERNET_CODEPAGE, PcValue::I32(65001)));
    }

    // Attachments: embeds always write attaches (parents_only is only applied
    // at top level in `build_message_node`).
    for (attach_index, attach) in msg.attachments.iter().enumerate() {
        if let Some(written) = write_one_attachment(
            layout,
            msg,
            attach,
            attach_index as u32,
            depth,
            max_depth,
            include_bcc_recipients,
            named_prop_plan,
            counters,
            &mut attach_nid_counter,
            streams,
            attach_event_sink,
        )? {
            subnode_entries.push((written.nid, written.bid_data, written.bid_sub));
            written_content_bytes += written.size_contrib;
            written_attaches.push(written);
        }
    }

    let has_attaches = !written_attaches.is_empty();
    // Preserve readable source flags when present (0094); default MSGFLAG_READ.
    let mut flags = msg.message_flags.map(|f| f as i32).unwrap_or(MSGFLAG_READ);
    if has_attaches {
        flags |= MSGFLAG_HASATTACH;
        // Attachment table TC at fixed NID 0x671 — full MS-PST column schema
        // + RowIndex BTH (key = attach NID, value = 0-based row index).
        let table_rows: Vec<(u64, u32, i32, String)> = written_attaches
            .iter()
            .map(|a| (a.nid, a.attach_size, a.method, a.filename.clone()))
            .collect();
        let table_heap = {
            let mut heap = HeapBuilder::new(0xBC);
            let row_refs: Vec<(u64, u32, i32, &str)> = table_rows
                .iter()
                .map(|(nid, size, method, name)| (*nid, *size, *method, name.as_str()))
                .collect();
            let (hid, _heap_after) = build_attachment_table_tc(&mut heap, &row_refs)?;
            heap.finalize(hid)
        };
        let table_len = table_heap.len() as u64;
        let table_bid = layout.write_data_chain(table_heap)?;
        subnode_entries.push((NID_ATTACHMENT_TABLE, table_bid, 0));
        // Real attachment-table heap size (not a fabricated constant).
        written_content_bytes += table_len;
    }

    // Recipient table TC at fixed NID 0x692 — always present (MS-PST MUST),
    // including zero rows. BCC rows filtered unless include_bcc_recipients.
    // Strategy A (0100): all included rows; row-matrix subnode + multi-page HN.
    let recip_filtered: Vec<&WriteRecipient> = msg
        .recipients
        .iter()
        .filter(|r| include_bcc_recipients || !r.recipient_type.is_bcc())
        .collect();
    let recip_ordered = order_recipients_for_tc(&recip_filtered);
    let recip_built = build_recipient_table_strategy_a(layout, &recip_ordered)?;
    let recip_table_len = recip_built.heap.len() as u64;
    let recip_table_bid = layout.write_data_chain(recip_built.heap)?;
    subnode_entries.push((
        NID_RECIPIENT_TABLE,
        recip_table_bid,
        recip_built.table_bid_sub,
    ));
    written_content_bytes += recip_table_len.saturating_add(recip_built.extra_content_bytes);

    props.push((PID_TAG_HAS_ATTACHMENTS, PcValue::Bool(has_attaches)));
    props.push((PID_TAG_MESSAGE_FLAGS, PcValue::I32(flags)));

    // MessageSize probe with placeholder so the final PC (same props + real size)
    // cannot tip the single-page budget. On overflow, escalate largest remaining
    // inline helpers to subnodes and re-probe (0093 cumulative budget).
    props.push((PID_TAG_MESSAGE_SIZE, PcValue::I32(0)));
    let message_size = loop {
        let mut probe_heap = HeapBuilder::new(0x6C);
        match build_pc_v2(&mut probe_heap, &props) {
            Ok(probe_hid) => {
                let probe_bytes = probe_heap.finalize(probe_hid);
                let message_size_u64 = probe_bytes.len() as u64 + written_content_bytes;
                break i32::try_from(message_size_u64).map_err(|_| {
                    WriterError::BodyTooLarge(format!(
                        "computed message size {message_size_u64} bytes exceeds \
                         PidTagMessageSize's PT_LONG (MS-OXPROPS) range ({} bytes max) — \
                         refusing to silently clamp a size that would misrepresent what \
                         was written",
                        i32::MAX
                    ))
                })?;
            }
            Err(e) if is_heap_page_overflow(&e) => {
                let diverted = escalate_largest_inline_helper(
                    &mut props,
                    layout,
                    &mut subnode_counter,
                    &mut subnode_entries,
                    &mut written_content_bytes,
                )?;
                if !diverted {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    };
    if let Some((_, v)) = props.iter_mut().find(|(p, _)| *p == PID_TAG_MESSAGE_SIZE) {
        *v = PcValue::I32(message_size);
    }

    let mut heap = HeapBuilder::new(0x6C);
    let hid = build_pc_v2(&mut heap, &props)?;
    let msg_heap_bytes = heap.finalize(hid);

    let sub_bid = if subnode_entries.is_empty() {
        0
    } else {
        layout.add_subnode_leaf(&subnode_entries)?
    };

    Ok((msg_heap_bytes, sub_bid, message_size as u64))
}

#[allow(clippy::too_many_arguments)] // streams + attach_event_sink for 0073 fidelity
fn build_message_node(
    layout: &mut Layout,
    msg: &WriteMessage,
    parent_nid: u64,
    opts: &WritePstOpts,
    counters: &mut WriteCounters,
    depth: u32,
    streams: &mut Option<&mut dyn AttachStreamSource>,
    attach_event_sink: &mut Option<&mut dyn AttachEventSink>,
) -> Result<u64> {
    let msg_nid = layout.alloc_nid(NID_TYPE_NORMAL_MESSAGE);
    let max_depth = opts.embedded_depth_limit();

    // list_attachments / attach-meta failed at materialize: one synthetic fail event.
    // Must run before parents_only omit so histogram includes MetaFailed even when
    // the attach list is empty (or would be emptied by policy).
    if msg.attach_list_failed {
        let synthetic = WriteAttachment {
            source_path: msg.source_path.clone(),
            parent_nid: msg.source_msg_nid,
            attach_method: None,
            ..WriteAttachment::default()
        };
        record_attach_event(
            counters,
            make_attach_event(
                msg,
                &synthetic,
                0,
                AttachmentFidelityKind::MetaFailed,
                AttachEventSeverity::Fail,
            ),
            attach_event_sink,
        );
    }
    // 0094: nested extract soft-skipped child attach rows — honesty without ghost attaches.
    if msg.attachments_incomplete {
        let synthetic = WriteAttachment {
            source_path: msg.source_path.clone(),
            parent_nid: msg.source_msg_nid,
            attach_method: None,
            filename: "(nested attach list incomplete)".into(),
            ..WriteAttachment::default()
        };
        record_attach_event(
            counters,
            make_attach_event(
                msg,
                &synthetic,
                0,
                AttachmentFidelityKind::MetaFailed,
                AttachEventSeverity::Fail,
            ),
            attach_event_sink,
        );
    }

    // parents_only: emit info omit events (not fail) and write with empty attach list.
    let owned: WriteMessage;
    let msg_ref: &WriteMessage = if opts.parents_only && !msg.attachments.is_empty() {
        for (i, attach) in msg.attachments.iter().enumerate() {
            counters.attachments_omitted_by_policy =
                counters.attachments_omitted_by_policy.saturating_add(1);
            record_attach_event(
                counters,
                make_attach_event(
                    msg,
                    attach,
                    i as u32,
                    AttachmentFidelityKind::OmittedByPolicy,
                    AttachEventSeverity::Info,
                ),
                attach_event_sink,
            );
        }
        owned = WriteMessage {
            attachments: Vec::new(),
            ..msg.clone()
        };
        &owned
    } else {
        msg
    };

    let (heap_bytes, sub_bid, _size) = build_message_payload(
        layout,
        msg_ref,
        max_depth,
        opts.include_bcc_recipients,
        &opts.named_prop_plan,
        counters,
        depth,
        streams,
        attach_event_sink,
    )?;
    layout.add_node_data(msg_nid, heap_bytes, parent_nid, sub_bid)?;
    Ok(msg_nid)
}

/// PST-local Folder EntryID, used for `PidTagIpmSubtreeEntryId` (§3.2),
/// `PidTagIpmWastebasketEntryId`, and `PidTagFinderEntryId` (§1 of the
/// round-9 verified MS-PST data) alike — generalized from the
/// IPM_SUBTREE-only `build_ipm_subtree_entry_id` (its original name) once a
/// second and third caller (Deleted Items, Search Root) needed the identical
/// shape for a different target folder NID.
///
/// Design decision: `pst-reader` does not parse/resolve EntryIDs at all (it
/// walks folders by NID directly), and Outlook/scanpst are not available in
/// this environment to independently verify EntryID acceptance — this is
/// therefore a documented, best-effort MS-OXCDATA-shaped structure, not one
/// verified against a real Outlook-opened PST:
///
/// `abFlags(4) = 0` + `ProviderUID(16)` (matches the store's own
/// `PidTagRecordKey`, `provider_uid`, byte-for-byte — a store-internal
/// EntryID's provider UID is conventionally the store's own unique record
/// key, so the EntryID genuinely identifies this specific store rather than
/// carrying an arbitrary value) + `folder_nid` encoded as a 4-byte LE value
/// (its "internal reference"). Total 24 bytes. Still not independently
/// verified against a real Outlook-opened PST in this environment — flagged
/// as a residual for operator scanpst/Outlook verification per spec
/// §3.9-7/8 — see final report.
fn build_folder_entry_id(folder_nid: u64, provider_uid: &[u8; 16]) -> Vec<u8> {
    let mut id = Vec::with_capacity(24);
    id.extend_from_slice(&0u32.to_le_bytes());
    id.extend_from_slice(provider_uid);
    id.extend_from_slice(&(folder_nid as u32).to_le_bytes());
    id
}

// ── Store PidTagRecordKey derivation (0087 — deterministic by default) ─────
//
// Preimage (normative, length-prefixed variable fields only):
//
//   preimage =
//     b"pst-dedup/store-record-key/v1\0"
//     || algo_version_u32_le          // 1
//     || volume_index_u32_le
//     || message_count_u64_le
//     || content_fingerprint          // 32 bytes
//
//   record_key_16 = SHA-256(preimage)[0..16]
//   if all zero: key[0] = 0x5A
//
// content_fingerprint:
//   - If store_key_material is Some:
//       SHA-256( b"pst-dedup/store-key-material/v1\0"
//                || material || volume_index_u32_le
//                || message_count_u64_le || volume_local_fingerprint )
//   - Else: volume_local_fingerprint only
//
// volume_local_fingerprint = SHA-256 over write-order messages:
//   for each msg:
//     b"msg\0"
//     || len_u32_le || utf8(internet_message_id)  // empty ok
//     || len_u32_le || utf8(subject)
//     || submit_time_filetime_i64_le              // None → 0
//     || len_u32_le || utf8(source_folder_path)
//
// Dest path, wall clock, and PID are never in the deterministic preimage.

/// Domain separator for the outer store-record-key preimage (0087 §2.6).
pub const STORE_RECORD_KEY_DOMAIN: &[u8] = b"pst-dedup/store-record-key/v1\0";
/// Domain separator when rebinding job-global `store_key_material` (0087 §2.6.1).
pub const STORE_KEY_MATERIAL_DOMAIN: &[u8] = b"pst-dedup/store-key-material/v1\0";
/// Domain separator for job-global winner-loci digests used as `store_key_material`.
pub const JOB_KEY_MATERIAL_DOMAIN: &[u8] = b"pst-dedup/job-key-material/v1\0";
/// Algorithm version embedded in the store-record-key preimage.
pub const STORE_RECORD_KEY_ALGO_VERSION: u32 = 1;

/// Pure 16-byte store RecordKey from volume index, message count, and a
/// 32-byte content fingerprint (0087 §2.6). Destination path is **not** an
/// input — path independence is intentional.
pub fn derive_store_record_key(
    volume_index: u32,
    message_count: u64,
    content_fingerprint: &[u8; 32],
) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(STORE_RECORD_KEY_DOMAIN);
    hasher.update(STORE_RECORD_KEY_ALGO_VERSION.to_le_bytes());
    hasher.update(volume_index.to_le_bytes());
    hasher.update(message_count.to_le_bytes());
    hasher.update(content_fingerprint);
    let digest = hasher.finalize();
    let mut key = [0u8; 16];
    key.copy_from_slice(&digest[..16]);
    apply_all_zero_guard(&mut key);
    key
}

/// Resolve `content_fingerprint` (32 bytes) from optional job material +
/// volume-local fingerprint (0087 §2.6.1).
pub fn resolve_content_fingerprint(
    store_key_material: Option<&[u8; 32]>,
    volume_index: u32,
    message_count: u64,
    volume_local_fingerprint: &[u8; 32],
) -> [u8; 32] {
    match store_key_material {
        None => *volume_local_fingerprint,
        Some(material) => {
            let mut hasher = Sha256::new();
            hasher.update(STORE_KEY_MATERIAL_DOMAIN);
            hasher.update(material);
            hasher.update(volume_index.to_le_bytes());
            hasher.update(message_count.to_le_bytes());
            hasher.update(volume_local_fingerprint);
            hasher.finalize().into()
        }
    }
}

/// SHA-256 volume-local fingerprint over ordered messages (write order).
/// Length-prefixed fields only — no null-terminated variable framing.
pub fn volume_local_fingerprint_from_messages<'a>(
    messages: impl IntoIterator<Item = &'a WriteMessage>,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for msg in messages {
        update_volume_local_fingerprint(&mut hasher, msg);
    }
    hasher.finalize().into()
}

/// Append one message's length-prefixed fingerprint record to `hasher`.
fn update_volume_local_fingerprint(hasher: &mut Sha256, msg: &WriteMessage) {
    let mid = msg.message_id.as_deref().unwrap_or("");
    let subject = msg.subject.as_str();
    let submit = msg.submit_time.unwrap_or(0);
    let folder = msg.source_folder_path.as_deref().unwrap_or("");
    hasher.update(b"msg\0");
    append_len_prefixed_utf8(hasher, mid);
    append_len_prefixed_utf8(hasher, subject);
    hasher.update(submit.to_le_bytes());
    append_len_prefixed_utf8(hasher, folder);
}

#[inline]
fn append_len_prefixed_utf8(hasher: &mut Sha256, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len() as u32;
    hasher.update(len.to_le_bytes());
    hasher.update(bytes);
}

/// Job-global 32-byte seed from ordered keep-set winner loci (source path,
/// folder path, nid). Used as [`WritePstOpts::store_key_material`] so each
/// volume key is bound to the whole export job (0087 §2.5).
pub fn job_store_key_material_from_loci<'a>(
    loci: impl IntoIterator<Item = (&'a str, &'a str, u64)>,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(JOB_KEY_MATERIAL_DOMAIN);
    for (source_path, folder_path, nid) in loci {
        hasher.update(b"win\0");
        append_len_prefixed_utf8(&mut hasher, source_path);
        append_len_prefixed_utf8(&mut hasher, folder_path);
        hasher.update(nid.to_le_bytes());
    }
    hasher.finalize().into()
}

#[inline]
fn apply_all_zero_guard(key: &mut [u8; 16]) {
    if key.iter().all(|&b| b == 0) {
        key[0] = 0x5A;
    }
}

/// Ephemeral (non-default) store key: wall-clock + pid + count. Escape hatch
/// only — not used for chain-of-custody reproducibility.
fn generate_ephemeral_store_record_key(message_count: u64) -> [u8; 16] {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();

    let mut seed_input = Vec::with_capacity(32);
    seed_input.extend_from_slice(&nanos.to_le_bytes());
    seed_input.extend_from_slice(&pid.to_le_bytes());
    seed_input.extend_from_slice(&message_count.to_le_bytes());

    let mut key = [0u8; 16];
    let salts: [u32; 4] = [0x5A17_0001, 0x5A17_0002, 0x5A17_0003, 0x5A17_0004];
    for (i, salt) in salts.into_iter().enumerate() {
        let mut salted = Vec::with_capacity(seed_input.len() + 4);
        salted.extend_from_slice(&salt.to_le_bytes());
        salted.extend_from_slice(&seed_input);
        let hash = crc32fast::hash(&salted);
        key[i * 4..i * 4 + 4].copy_from_slice(&hash.to_le_bytes());
    }
    apply_all_zero_guard(&mut key);
    key
}

// ── PC value encoding (Result-based; no unwrap/expect/assert) ───────────────

/// Property value for the production PC builder. Distinct from `crate::PropertyValue`
/// (used by the fixture path only) — adds `Binary`, `Object`, and subnode-reference variants.
#[derive(Debug, Clone)]
pub enum PcValue {
    I32(i32),
    Bool(bool),
    Time(i64),
    String(String),
    Binary(Vec<u8>),
    /// PtypObject (`0x000D`): 8-byte heap `{Nid:u32 LE, ulSize:u32 LE}` (MS-PST §2.3.3.5).
    Object {
        nid: u64,
        size: u32,
    },
    /// Value already stored in a subnode (see module docs); stores the raw NID
    /// as `dwValueHnid` with `PtypString`.
    SubnodeString(u64),
    /// As `SubnodeString`, with `PtypBinary`.
    SubnodeBinary(u64),
}

fn encode_pc_value(heap: &mut HeapBuilder, value: &PcValue) -> Result<Vec<u8>> {
    let mut r = Vec::with_capacity(6);
    match value {
        PcValue::I32(v) => {
            r.extend_from_slice(&PTYP_INTEGER_32.to_le_bytes());
            r.extend_from_slice(&v.to_le_bytes());
        }
        PcValue::Bool(v) => {
            r.extend_from_slice(&PTYP_BOOLEAN.to_le_bytes());
            r.extend_from_slice(&(*v as u32).to_le_bytes());
        }
        PcValue::Time(v) => {
            r.extend_from_slice(&PTYP_TIME.to_le_bytes());
            let hid = heap.try_alloc(&v.to_le_bytes())?;
            r.extend_from_slice(&hid.to_le_bytes());
        }
        PcValue::String(s) => {
            r.extend_from_slice(&PTYP_STRING.to_le_bytes());
            let utf16 = utf16le_bytes(s);
            let hid = heap.try_alloc(&utf16)?;
            r.extend_from_slice(&hid.to_le_bytes());
        }
        PcValue::Binary(b) => {
            r.extend_from_slice(&PTYP_BINARY.to_le_bytes());
            let hid = heap.try_alloc(b)?;
            r.extend_from_slice(&hid.to_le_bytes());
        }
        PcValue::Object { nid, size } => {
            r.extend_from_slice(&PTYP_OBJECT.to_le_bytes());
            let mut heap_val = [0u8; 8];
            heap_val[0..4].copy_from_slice(&(*nid as u32).to_le_bytes());
            heap_val[4..8].copy_from_slice(&size.to_le_bytes());
            let hid = heap.try_alloc(&heap_val)?;
            r.extend_from_slice(&hid.to_le_bytes());
        }
        PcValue::SubnodeString(nid) => {
            r.extend_from_slice(&PTYP_STRING.to_le_bytes());
            r.extend_from_slice(&(*nid as u32).to_le_bytes());
        }
        PcValue::SubnodeBinary(nid) => {
            r.extend_from_slice(&PTYP_BINARY.to_le_bytes());
            r.extend_from_slice(&(*nid as u32).to_le_bytes());
        }
    }
    Ok(r)
}

/// Build a Property Context (Result-based; production path).
pub fn build_pc_v2(heap: &mut HeapBuilder, properties: &[(u16, PcValue)]) -> Result<u32> {
    let mut records: Vec<(u16, Vec<u8>)> = Vec::with_capacity(properties.len());
    for (prop_id, value) in properties {
        records.push((*prop_id, encode_pc_value(heap, value)?));
    }
    build_bth_checked(heap, 2, 6, &mut records)
}

/// Result-based BTH builder mirroring `crate::build_bth` (fixture path keeps the
/// original, panic-free-but-unchecked version for its own use).
pub fn build_bth_checked(
    heap: &mut HeapBuilder,
    cb_key: u8,
    cb_ent: u8,
    records: &mut [(u16, Vec<u8>)],
) -> Result<u32> {
    records.sort_by_key(|r| r.0);

    let mut bth_data = vec![0xB5, cb_key, cb_ent, 0];
    bth_data.extend_from_slice(&0u32.to_le_bytes());
    let hid_root = heap.try_alloc(&bth_data)?;

    let mut leaf_data = Vec::new();
    for (key, data) in records.iter() {
        leaf_data.extend_from_slice(&key.to_le_bytes());
        leaf_data.extend_from_slice(data);
    }
    let hid_leaf = heap.try_alloc(&leaf_data)?;

    heap.patch_u32(hid_root, 4, hid_leaf)?;
    Ok(hid_root)
}

/// BTH builder with **u32 keys** (e.g. TC RowIndex: `cbKey=4`, `cbEnt=4`).
///
/// [`build_bth_checked`] only supports u16 keys (PC property IDs). The TC
/// RowIndex BTH maps `dwRowID` (4-byte attach/message NID) → `dwRowIndex`
/// (4-byte 0-based matrix index). `pst_reader::ltp::tc` requires `cbKey >= 4`
/// for row-id keys.
pub(crate) fn build_bth_u32_checked(
    heap: &mut impl HeapTryAlloc,
    cb_key: u8,
    cb_ent: u8,
    records: &mut [(u32, Vec<u8>)],
) -> Result<u32> {
    if cb_key < 4 {
        return Err(WriterError::Layout(format!(
            "build_bth_u32_checked: cb_key={cb_key} must be >= 4 for u32 row-id keys"
        )));
    }
    if cb_ent == 0 {
        return Err(WriterError::Layout(
            "build_bth_u32_checked: cb_ent must be non-zero".into(),
        ));
    }

    records.sort_by_key(|r| r.0);

    let mut bth_data = vec![0xB5, cb_key, cb_ent, 0];
    bth_data.extend_from_slice(&0u32.to_le_bytes());
    let hid_root = heap.try_alloc(&bth_data)?;

    let mut leaf_data = Vec::new();
    for (key, data) in records.iter() {
        // Write key as little-endian u32 (cb_key bytes; pad/truncate to cb_key).
        let key_bytes = key.to_le_bytes();
        if cb_key as usize <= key_bytes.len() {
            leaf_data.extend_from_slice(&key_bytes[..cb_key as usize]);
        } else {
            leaf_data.extend_from_slice(&key_bytes);
            leaf_data.resize(leaf_data.len() + (cb_key as usize - 4), 0);
        }
        if data.len() != cb_ent as usize {
            return Err(WriterError::Layout(format!(
                "build_bth_u32_checked: record data len {} != cb_ent {cb_ent}",
                data.len()
            )));
        }
        leaf_data.extend_from_slice(data);
    }
    let hid_leaf = heap.try_alloc(&leaf_data)?;

    heap.patch_u32(hid_root, 4, hid_leaf)?;
    Ok(hid_root)
}

/// Heap allocation surface shared by single-page [`HeapBuilder`] and
/// recipient-table [`PagedHeapBuilder`].
pub(crate) trait HeapTryAlloc {
    fn try_alloc(&mut self, bytes: &[u8]) -> Result<u32>;
    fn patch_u32(&mut self, hid: u32, field_offset: usize, value: u32) -> Result<()>;
}

impl HeapTryAlloc for HeapBuilder {
    fn try_alloc(&mut self, bytes: &[u8]) -> Result<u32> {
        HeapBuilder::try_alloc(self, bytes)
    }
    fn patch_u32(&mut self, hid: u32, field_offset: usize, value: u32) -> Result<()> {
        HeapBuilder::patch_u32(self, hid, field_offset, value)
    }
}

impl HeapTryAlloc for PagedHeapBuilder {
    fn try_alloc(&mut self, bytes: &[u8]) -> Result<u32> {
        PagedHeapBuilder::try_alloc(self, bytes)
    }
    fn patch_u32(&mut self, hid: u32, field_offset: usize, value: u32) -> Result<()> {
        PagedHeapBuilder::patch_u32(self, hid, field_offset, value)
    }
}

/// Build an MS-PST-conformant attachment table TC on `heap`.
///
/// Columns match [`ATTACHMENT_TABLE_COLUMNS`] / the NBT template at
/// [`NID_ATTACHMENT_TABLE_TEMPLATE`]. Each row carries AttachSize, Filename
/// (UTF-16LE heap string via HNID), AttachMethod, RenderingPosition
/// (`0xFFFFFFFF`), LtpRowId (= attach NID), LtpRowVer (= 1-based row index),
/// and a full existence bitmap.
///
/// **RowIndex BTH** (`hidRowIndex` at TCINFO offset 10): key = attach NID
/// (u32), value = 0-based row index (u32). `hnidRows` is patched at offset 14.
///
/// Returns `(hid_user_root, heap_bytes_after_build)` where the second value is
/// the heap data length after all allocations (pre-finalize). Callers that
/// need MessageSize should prefer `heap.finalize(hid).len()` for the final
/// on-disk heap size.
fn build_attachment_table_tc(
    heap: &mut HeapBuilder,
    rows: &[(u64, u32, i32, &str)],
) -> Result<(u32, usize)> {
    let (columns, total_row_width) = build_template_tc_columns(&ATTACHMENT_TABLE_COLUMNS);
    let ncols = columns.len();
    let bitmap_bytes = ncols.div_ceil(8);
    let row_width = total_row_width as usize;

    let mut row_matrix: Vec<u8> = Vec::with_capacity(rows.len() * row_width);
    let mut row_index_records: Vec<(u32, Vec<u8>)> = Vec::with_capacity(rows.len());

    for (i, (attach_nid, size, method, filename)) in rows.iter().enumerate() {
        let fname_hid = heap.try_alloc(&utf16le_bytes(filename))?;
        let mut row = vec![0u8; row_width];

        for col in &columns {
            let prop_id = col.0;
            let ib = col.2 as usize;
            let cb = col.3 as usize;
            let bytes: [u8; 4] = match prop_id {
                PID_TAG_ATTACH_SIZE => size.to_le_bytes(),
                PID_TAG_ATTACH_FILENAME => fname_hid.to_le_bytes(),
                PID_TAG_ATTACH_METHOD => (*method as u32).to_le_bytes(),
                PID_TAG_RENDERING_POSITION => 0xFFFF_FFFFu32.to_le_bytes(),
                PID_TAG_LTP_ROW_ID => (*attach_nid as u32).to_le_bytes(),
                PID_TAG_LTP_ROW_VER => ((i as u32) + 1).to_le_bytes(),
                _ => {
                    return Err(WriterError::Layout(format!(
                        "build_attachment_table_tc: unexpected column prop 0x{prop_id:04X}"
                    )));
                }
            };
            if ib + cb > row_width || cb > 4 {
                return Err(WriterError::Layout(format!(
                    "build_attachment_table_tc: column 0x{prop_id:04X} out of row bounds \
                     (ib={ib} cb={cb} row_width={row_width})"
                )));
            }
            row[ib..ib + cb].copy_from_slice(&bytes[..cb]);
        }

        // Existence bitmap at end of row — all present columns set.
        let bitmap_start = row_width - bitmap_bytes;
        for col in &columns {
            let bit = col.4 as usize;
            row[bitmap_start + bit / 8] |= 1u8 << (bit % 8);
        }

        row_matrix.extend_from_slice(&row);
        row_index_records.push((*attach_nid as u32, (i as u32).to_le_bytes().to_vec()));
    }

    // RowIndex BTH (required when there are rows so get_row_id works).
    let hid_row_index = if rows.is_empty() {
        0u32
    } else {
        build_bth_u32_checked(heap, 4, 4, &mut row_index_records)?
    };

    // TCINFO: bType + cCols + rgib[4*2] + hidRowIndex(4) + hnidRows(4) = 18
    // then TCOLDESCs. Patch hidRowIndex @ field offset 10, hnidRows @ 14.
    let mut tcinfo = Vec::new();
    tcinfo.push(0x7C);
    tcinfo.push(columns.len() as u8);
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&total_row_width.to_le_bytes());
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hidRowIndex placeholder
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hnidRows placeholder

    for col in &columns {
        tcinfo.extend_from_slice(&col.0.to_le_bytes());
        tcinfo.extend_from_slice(&col.1.to_le_bytes());
        tcinfo.extend_from_slice(&col.2.to_le_bytes());
        tcinfo.push(col.3);
        tcinfo.push(col.4);
    }

    let hid_tcinfo = heap.try_alloc(&tcinfo)?;
    let hid_rows = heap.try_alloc(&row_matrix)?;

    heap.patch_u32(hid_tcinfo, 10, hid_row_index)?;
    heap.patch_u32(hid_tcinfo, 14, hid_rows)?;

    Ok((hid_tcinfo, heap_data_len(heap)))
}

/// Result of Strategy A recipient-table build.
struct RecipientTableBuilt {
    heap: Vec<u8>,
    table_bid_sub: u64,
    extra_content_bytes: u64,
}

/// Allocate a TC cell value: HID on the (paged) heap, or cell NID when larger
/// than [`MAX_HEAP_VALUE_SIZE`] (new in this builder — 0100).
fn alloc_tc_value(
    heap: &mut PagedHeapBuilder,
    layout: &mut Layout,
    sub_counter: &mut u32,
    table_subs: &mut Vec<(u64, u64, u64)>,
    extra_bytes: &mut u64,
    bytes: &[u8],
) -> Result<u32> {
    if bytes.len() > MAX_HEAP_VALUE_SIZE {
        let nid = next_subnode_nid(sub_counter);
        let bid = layout.write_data_chain(bytes.to_vec())?;
        table_subs.push((nid, bid, 0));
        *extra_bytes = extra_bytes.saturating_add(bytes.len() as u64);
        return Ok(nid as u32);
    }
    heap.try_alloc(bytes)
}

/// Build an MS-PST-conformant recipient table TC (0082 / 0100 Strategy A).
///
/// Columns match [`RECIPIENT_TABLE_COLUMNS`] / the NBT template at
/// [`NID_RECIPIENT_TABLE_TEMPLATE`] (14 MUST + product `PidTagSmtpAddress`).
/// Non-empty tables store the row matrix as a subnode (`hnidRows` = NID) packed
/// with RowsPerBlock; empty tables use `hnidRows = 0` and `bid_sub = 0`.
fn build_recipient_table_strategy_a(
    layout: &mut Layout,
    rows: &[&WriteRecipient],
) -> Result<RecipientTableBuilt> {
    let (columns, total_row_width) = build_template_tc_columns(&RECIPIENT_TABLE_COLUMNS);
    let ncols = columns.len();
    let bitmap_bytes = ncols.div_ceil(8);
    let row_width = total_row_width as usize;
    if row_width == 0 {
        return Err(WriterError::Layout(
            "recipient TC row_width is 0; cannot pack row matrix".into(),
        ));
    }

    let mut heap = PagedHeapBuilder::new(0xBC);
    let mut table_subs: Vec<(u64, u64, u64)> = Vec::new();
    let mut extra_content_bytes = 0u64;
    let mut sub_counter = 0u32;

    let mut row_matrix: Vec<u8> = Vec::with_capacity(rows.len() * row_width);
    let mut row_index_records: Vec<(u32, Vec<u8>)> = Vec::with_capacity(rows.len());

    for (i, recip) in rows.iter().enumerate() {
        let row_id = (i as u32).saturating_add(1);
        let display = recip
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        let email = recip
            .email_address
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        let addr_type = resolve_recipient_address_type(recip);
        let smtp = recip
            .smtp_address
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let seven_bit = if !display.is_empty() {
            display
        } else if !email.is_empty() {
            email
        } else {
            smtp.unwrap_or("")
        };

        let record_key = synthesize_recipient_record_key(row_id, email, display, &addr_type);
        let entry_id = build_folder_entry_id(u64::from(row_id), &record_key);
        let search_key = synthesize_recipient_search_key(&addr_type, email, smtp);

        let display_hid = alloc_tc_value(
            &mut heap,
            layout,
            &mut sub_counter,
            &mut table_subs,
            &mut extra_content_bytes,
            &utf16le_bytes(display),
        )?;
        let addr_type_hid = alloc_tc_value(
            &mut heap,
            layout,
            &mut sub_counter,
            &mut table_subs,
            &mut extra_content_bytes,
            &utf16le_bytes(&addr_type),
        )?;
        let email_hid = alloc_tc_value(
            &mut heap,
            layout,
            &mut sub_counter,
            &mut table_subs,
            &mut extra_content_bytes,
            &utf16le_bytes(email),
        )?;
        let seven_hid = alloc_tc_value(
            &mut heap,
            layout,
            &mut sub_counter,
            &mut table_subs,
            &mut extra_content_bytes,
            &utf16le_bytes(seven_bit),
        )?;
        let record_key_hid = alloc_tc_value(
            &mut heap,
            layout,
            &mut sub_counter,
            &mut table_subs,
            &mut extra_content_bytes,
            &record_key,
        )?;
        let entry_id_hid = alloc_tc_value(
            &mut heap,
            layout,
            &mut sub_counter,
            &mut table_subs,
            &mut extra_content_bytes,
            &entry_id,
        )?;
        let search_key_hid = alloc_tc_value(
            &mut heap,
            layout,
            &mut sub_counter,
            &mut table_subs,
            &mut extra_content_bytes,
            &search_key,
        )?;
        let smtp_hid = match smtp {
            Some(s) => Some(alloc_tc_value(
                &mut heap,
                layout,
                &mut sub_counter,
                &mut table_subs,
                &mut extra_content_bytes,
                &utf16le_bytes(s),
            )?),
            None => None,
        };

        let mut row = vec![0u8; row_width];
        let mut present_bits: Vec<bool> = vec![true; ncols];

        for (col_idx, col) in columns.iter().enumerate() {
            let prop_id = col.0;
            let ib = col.2 as usize;
            let cb = col.3 as usize;
            match prop_id {
                PID_TAG_RECIPIENT_TYPE => {
                    let v = recip.recipient_type.to_mapi().to_le_bytes();
                    copy_col_bytes(&mut row, ib, cb, &v)?;
                }
                PID_TAG_RESPONSIBILITY => {
                    if cb < 1 {
                        return Err(WriterError::Layout(
                            "build_recipient_table_tc: Responsibility column width 0".into(),
                        ));
                    }
                    row[ib] = 1;
                }
                PID_TAG_RECORD_KEY => {
                    copy_col_bytes(&mut row, ib, cb, &record_key_hid.to_le_bytes())?;
                }
                PID_TAG_OBJECT_TYPE => {
                    copy_col_bytes(&mut row, ib, cb, &(MAPI_MAILUSER as u32).to_le_bytes())?;
                }
                PID_TAG_ENTRY_ID => {
                    copy_col_bytes(&mut row, ib, cb, &entry_id_hid.to_le_bytes())?;
                }
                PID_TAG_DISPLAY_NAME => {
                    copy_col_bytes(&mut row, ib, cb, &display_hid.to_le_bytes())?;
                }
                PID_TAG_ADDRESS_TYPE => {
                    copy_col_bytes(&mut row, ib, cb, &addr_type_hid.to_le_bytes())?;
                }
                PID_TAG_EMAIL_ADDRESS => {
                    copy_col_bytes(&mut row, ib, cb, &email_hid.to_le_bytes())?;
                }
                PID_TAG_SEARCH_KEY => {
                    copy_col_bytes(&mut row, ib, cb, &search_key_hid.to_le_bytes())?;
                }
                PID_TAG_DISPLAY_TYPE => {
                    copy_col_bytes(&mut row, ib, cb, &(DT_MAILUSER as u32).to_le_bytes())?;
                }
                PID_TAG_SMTP_ADDRESS => match smtp_hid {
                    Some(hid) => copy_col_bytes(&mut row, ib, cb, &hid.to_le_bytes())?,
                    None => {
                        present_bits[col_idx] = false;
                    }
                },
                PID_TAG_7BIT_DISPLAY_NAME => {
                    copy_col_bytes(&mut row, ib, cb, &seven_hid.to_le_bytes())?;
                }
                PID_TAG_SEND_RICH_INFO => {
                    if cb < 1 {
                        return Err(WriterError::Layout(
                            "build_recipient_table_tc: SendRichInfo column width 0".into(),
                        ));
                    }
                    row[ib] = 0;
                }
                PID_TAG_LTP_ROW_ID => {
                    copy_col_bytes(&mut row, ib, cb, &row_id.to_le_bytes())?;
                }
                PID_TAG_LTP_ROW_VER => {
                    let ver = (i as u32).saturating_add(1);
                    copy_col_bytes(&mut row, ib, cb, &ver.to_le_bytes())?;
                }
                _ => {
                    return Err(WriterError::Layout(format!(
                        "build_recipient_table_tc: unexpected column prop 0x{prop_id:04X}"
                    )));
                }
            }
        }

        let bitmap_start = row_width - bitmap_bytes;
        for (col_idx, col) in columns.iter().enumerate() {
            if present_bits[col_idx] {
                let bit = col.4 as usize;
                row[bitmap_start + bit / 8] |= 1u8 << (bit % 8);
            }
        }

        row_matrix.extend_from_slice(&row);
        row_index_records.push((row_id, (i as u32).to_le_bytes().to_vec()));
    }

    let hid_row_index = if rows.is_empty() {
        0u32
    } else {
        build_bth_u32_checked(&mut heap, 4, 4, &mut row_index_records)?
    };

    let mut tcinfo = Vec::new();
    tcinfo.push(0x7C);
    tcinfo.push(columns.len() as u8);
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&total_row_width.to_le_bytes());
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hidRowIndex placeholder
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hnidRows placeholder

    for col in &columns {
        tcinfo.extend_from_slice(&col.0.to_le_bytes());
        tcinfo.extend_from_slice(&col.1.to_le_bytes());
        tcinfo.extend_from_slice(&col.2.to_le_bytes());
        tcinfo.push(col.3);
        tcinfo.push(col.4);
    }

    let hid_tcinfo = heap.try_alloc(&tcinfo)?;
    heap.patch_u32(hid_tcinfo, 10, hid_row_index)?;

    if rows.is_empty() {
        heap.patch_u32(hid_tcinfo, 14, 0)?;
        return Ok(RecipientTableBuilt {
            heap: heap.finalize(hid_tcinfo),
            table_bid_sub: 0,
            extra_content_bytes: 0,
        });
    }

    let (matrix_bid, matrix_bytes) = layout.write_row_matrix_tree(&row_matrix, row_width)?;
    extra_content_bytes = extra_content_bytes.saturating_add(matrix_bytes);
    let matrix_nid = next_subnode_nid(&mut sub_counter);
    table_subs.insert(0, (matrix_nid, matrix_bid, 0));
    heap.patch_u32(hid_tcinfo, 14, matrix_nid as u32)?;

    let table_bid_sub = layout.add_subnode_leaf(&table_subs)?;
    Ok(RecipientTableBuilt {
        heap: heap.finalize(hid_tcinfo),
        table_bid_sub,
        extra_content_bytes,
    })
}

/// Copy up to 4 LE value bytes into a TC row cell, checking bounds.
fn copy_col_bytes(row: &mut [u8], ib: usize, cb: usize, bytes: &[u8]) -> Result<()> {
    if ib + cb > row.len() || cb > bytes.len() {
        return Err(WriterError::Layout(format!(
            "recipient TC column out of bounds (ib={ib} cb={cb} row_len={} val_len={})",
            row.len(),
            bytes.len()
        )));
    }
    row[ib..ib + cb].copy_from_slice(&bytes[..cb]);
    Ok(())
}

/// Resolve address type for a recipient row (caller value, else SMTP/EX heuristic).
fn resolve_recipient_address_type(recip: &WriteRecipient) -> String {
    if let Some(t) = recip
        .address_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return t.to_string();
    }
    if recip
        .smtp_address
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
    {
        return "SMTP".to_string();
    }
    if let Some(email) = recip
        .email_address
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if email.to_ascii_uppercase().contains("/O=") {
            return "EX".to_string();
        }
        if email.contains('@') {
            return "SMTP".to_string();
        }
    }
    String::new()
}

/// 16-byte synthetic RecordKey for a recipient row (crc-salted seed, no new deps).
fn synthesize_recipient_record_key(
    row_id: u32,
    email: &str,
    display: &str,
    addr_type: &str,
) -> [u8; 16] {
    let mut seed = Vec::with_capacity(64);
    seed.extend_from_slice(&row_id.to_le_bytes());
    seed.extend_from_slice(addr_type.as_bytes());
    seed.push(0);
    seed.extend_from_slice(email.as_bytes());
    seed.push(0);
    seed.extend_from_slice(display.as_bytes());

    let mut key = [0u8; 16];
    let salts: [u32; 4] = [0x0C15_0001, 0x0C15_0002, 0x0C15_0003, 0x0C15_0004];
    for (i, salt) in salts.into_iter().enumerate() {
        let mut salted = Vec::with_capacity(seed.len() + 4);
        salted.extend_from_slice(&salt.to_le_bytes());
        salted.extend_from_slice(&seed);
        let h = crc32fast::hash(&salted);
        key[i * 4..(i + 1) * 4].copy_from_slice(&h.to_le_bytes());
    }
    key
}

/// MAPI-shaped SearchKey: `TYPE:ADDRESS` uppercase ASCII bytes.
fn synthesize_recipient_search_key(addr_type: &str, email: &str, smtp: Option<&str>) -> Vec<u8> {
    let ty = if addr_type.is_empty() {
        "UNKNOWN"
    } else {
        addr_type
    };
    let addr = if let Some(s) = smtp {
        s
    } else if !email.is_empty() {
        email
    } else {
        ""
    };
    format!("{}:{}", ty.to_ascii_uppercase(), addr.to_ascii_uppercase()).into_bytes()
}

/// Current allocated heap data length (pre-finalize), for sizing probes.
fn heap_data_len(heap: &HeapBuilder) -> usize {
    heap.data.len()
}

/// Result-based inline TC builder mirroring `crate::build_tc_inline`.
///
/// `rgib[3]` (the total row width, used by `pst_reader::ltp::tc` as the row
/// stride when dividing up row data) is derived from `rows.first()`'s actual
/// length — correct for every call site in this file that passes real row
/// data, but degenerates to `0` for a table that is defined to always have
/// zero rows (there is no row to measure). The four fixed MS-PST "template
/// object" tables (§5 of the round-9 verified data) are always zero-row by
/// spec, yet still need a correct, non-degenerate row width in their TCINFO
/// header for a reader to parse the column schema without error — see
/// [`build_tc_inline_checked_sized`], which this function now delegates to.
pub fn build_tc_inline_checked(
    heap: &mut HeapBuilder,
    columns: &[(u16, u16, u16, u8, u8)],
    rows: &[Vec<u8>],
) -> Result<u32> {
    let total_row_width = rows.first().map(|r| r.len() as u16).unwrap_or(0);
    build_tc_inline_checked_sized(heap, columns, rows, total_row_width)
}

/// As [`build_tc_inline_checked`], but with an explicit `total_row_width`
/// (`TCINFO.rgib[3]`) instead of inferring it from `rows.first()`. Needed for
/// the four fixed MS-PST template-object tables (§5 of the round-9 verified
/// data), which are always zero-row (`rows` is always `&[]`) but must still
/// carry a correct row-width value derived from their real column schema —
/// see [`build_template_tc_columns`], which computes both the column
/// descriptors and this width together so they can never drift apart.
pub fn build_tc_inline_checked_sized(
    heap: &mut HeapBuilder,
    columns: &[(u16, u16, u16, u8, u8)],
    rows: &[Vec<u8>],
    total_row_width: u16,
) -> Result<u32> {
    let mut tcinfo = Vec::new();
    tcinfo.push(0x7C);
    tcinfo.push(columns.len() as u8);

    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&0u16.to_le_bytes());
    tcinfo.extend_from_slice(&total_row_width.to_le_bytes());
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hidRowIndex (none — v1 tables are tiny)
    tcinfo.extend_from_slice(&0u32.to_le_bytes()); // hnidRows placeholder, patched below

    for col in columns {
        tcinfo.extend_from_slice(&col.0.to_le_bytes());
        tcinfo.extend_from_slice(&col.1.to_le_bytes());
        tcinfo.extend_from_slice(&col.2.to_le_bytes());
        tcinfo.push(col.3);
        tcinfo.push(col.4);
    }

    let hid_tcinfo = heap.try_alloc(&tcinfo)?;

    let mut row_data = Vec::new();
    for row in rows {
        row_data.extend_from_slice(row);
    }
    let hid_rows = heap.try_alloc(&row_data)?;

    heap.patch_u32(hid_tcinfo, 14, hid_rows)?;
    Ok(hid_tcinfo)
}

// ── Fixed MS-PST "template object" column schemas (§5 of the round-9
// verified MS-PST data) ──────────────────────────────────────────────────

/// A TCOLDESC tuple: `(prop_id, prop_type, ib_data, cb_data, i_bit)`. Named
/// alias for the 5-tuple already used positionally throughout this file
/// (`build_tc_inline_checked` and friends) — introduced alongside
/// [`build_template_tc_columns`] so its `Vec<...>` return type stays a single
/// named type rather than a directly-nested 5-tuple (clippy
/// `type_complexity`).
type TcColumnTuple = (u16, u16, u16, u8, u8);

/// A TC column's storage width class, used only to compute correct
/// `ib_data`/`cb_data` byte-offset bookkeeping for the four always-empty
/// template tables below — this is deliberately a narrower, purpose-built
/// enum, not a general MAPI-type abstraction used elsewhere in this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TcColType {
    /// PtypInteger32 (0x0003) — 4 bytes inline.
    I32,
    /// PtypInteger64 (0x0014) — 8 bytes inline.
    I64,
    /// PtypBoolean (0x000B) — 1 byte inline.
    Bool,
    /// PtypTime (0x0040) — 8 bytes inline (FILETIME).
    Time,
    /// PtypString (0x001F) — variable-length; the row stores a 4-byte HNID
    /// reference (matches `PID_TAG_LTP_ROW_ID`'s existing inline-I32
    /// precedent for *width*, but this is the HNID-reference case that
    /// `pst_reader::ltp::tc::TableContext::get_row_string` expects: `cb_data
    /// == 4`, `prop_type == 0x001F`).
    StringRef,
    /// PtypBinary (0x0102) — variable-length; 4-byte HNID reference, same
    /// convention as `StringRef`.
    BinaryRef,
    /// PtypMultipleInteger32 (0x1003) — variable-length MAPI multi-value
    /// type. This repo's TC column model has no existing precedent for a
    /// genuine `PtypMultiple*` value; per the verified source data's own
    /// guidance this is conservatively modeled as a 4-byte HNID reference,
    /// identical in *width* to `StringRef`/`BinaryRef` — documented judgment
    /// call, see final report. Never exercised beyond column-width
    /// bookkeeping since this table always has zero rows.
    MultiI32Ref,
}

impl TcColType {
    /// Row-storage width in bytes (MS-PST §2.3.4.1 TCINFO row-layout
    /// convention: fixed-size types store their value inline at this width;
    /// variable-length types store a 4-byte HNID reference at this width).
    fn width(self) -> u8 {
        match self {
            TcColType::I64 | TcColType::Time => 8,
            TcColType::I32
            | TcColType::StringRef
            | TcColType::BinaryRef
            | TcColType::MultiI32Ref => 4,
            TcColType::Bool => 1,
        }
    }

    /// The `wPropType` (TCOLDESC `prop_type`) written for this column — the
    /// *real* MAPI type (e.g. `0x001F` for a string), even when its row
    /// storage is an HNID reference rather than the value itself.
    fn prop_type(self) -> u16 {
        match self {
            TcColType::I32 => PTYP_INTEGER_32,
            TcColType::I64 => PTYP_INTEGER_64,
            TcColType::Bool => PTYP_BOOLEAN,
            TcColType::Time => PTYP_TIME,
            TcColType::StringRef => PTYP_STRING,
            TcColType::BinaryRef => PTYP_BINARY,
            TcColType::MultiI32Ref => PTYP_MULTIPLE_INTEGER_32,
        }
    }
}

/// Build TCOLDESC column tuples `(prop_id, prop_type, ib_data, cb_data,
/// i_bit)` for a fixed-NID template table, plus the resulting total row
/// width (`TCINFO.rgib[3]`) — computed together so the two can never drift
/// apart (see [`build_tc_inline_checked_sized`]).
///
/// Groups columns widest-first (8-byte, then 4-byte, then 1-byte — none of
/// the four verified template schemas need a 2-byte group) per MS-PST
/// §2.3.4.1's TCINFO row-layout convention, computing running `ib_data`
/// offsets within each group, then appends the existence-bitmap tail
/// (`ceil(cCols/8)` bytes, MS-PST §2.3.4.1) to get the total row width. Every
/// column gets a real TCOLDESC even though these tables are always empty
/// (zero data rows) — the byte-width bookkeeping must still be correct for a
/// reader to parse the TCINFO header without error (the explicit reason this
/// helper exists, rather than reusing the existing single-column
/// `(PID_TAG_LTP_ROW_ID, PTYP_INTEGER_32, 0, 4, 0)` pattern used for the
/// per-folder hierarchy/contents/assoc-contents tables elsewhere in this
/// file, none of which has more than one column).
fn build_template_tc_columns(specs: &[(u16, TcColType)]) -> (Vec<TcColumnTuple>, u16) {
    let mut group8: Vec<&(u16, TcColType)> = Vec::new();
    let mut group4: Vec<&(u16, TcColType)> = Vec::new();
    let mut group1: Vec<&(u16, TcColType)> = Vec::new();
    for spec in specs {
        match spec.1.width() {
            8 => group8.push(spec),
            4 => group4.push(spec),
            _ => group1.push(spec),
        }
    }

    let mut columns = Vec::with_capacity(specs.len());
    let mut offset: u16 = 0;
    for (idx, (tag, ty)) in group8.into_iter().chain(group4).chain(group1).enumerate() {
        let width = ty.width();
        let i_bit = idx as u8;
        columns.push((*tag, ty.prop_type(), offset, width, i_bit));
        offset += width as u16;
    }

    let bitmap_bytes = (specs.len() as u16).div_ceil(8);
    let total_row_width = offset + bitmap_bytes;
    (columns, total_row_width)
}

/// 5a. Hierarchy Table Template (NID `0x60D`) column schema — verified from
/// https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/c08fb6cb-2d91-42e5-b70d-f3e4f9781a2a
const HIERARCHY_TABLE_TEMPLATE_COLUMNS: [(u16, TcColType); 13] = [
    (0x0E30, TcColType::I32),
    (0x0E33, TcColType::I64),
    (0x0E34, TcColType::BinaryRef),
    (0x0E38, TcColType::I32),
    (0x3001, TcColType::StringRef),
    (0x3602, TcColType::I32),
    (0x3603, TcColType::I32),
    (0x360A, TcColType::Bool),
    (0x3613, TcColType::BinaryRef),
    (0x6635, TcColType::I32),
    (0x6636, TcColType::I32),
    (0x67F2, TcColType::I32),
    (0x67F3, TcColType::I32),
];

/// 5b. Contents Table Template (NID `0x60E`) column schema — verified from
/// https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/f58e1ea9-b592-408d-b89e-53fd4cd6024b
const CONTENTS_TABLE_TEMPLATE_COLUMNS: [(u16, TcColType); 27] = [
    (0x0017, TcColType::I32),
    (0x001A, TcColType::StringRef),
    (0x0036, TcColType::I32),
    (0x0037, TcColType::StringRef),
    (0x0039, TcColType::Time),
    (0x0042, TcColType::StringRef),
    (0x0057, TcColType::Bool),
    (0x0058, TcColType::Bool),
    (0x0070, TcColType::StringRef),
    (0x0071, TcColType::BinaryRef),
    (0x0E03, TcColType::StringRef),
    (0x0E04, TcColType::StringRef),
    (0x0E06, TcColType::Time),
    (0x0E07, TcColType::I32),
    (0x0E08, TcColType::I32),
    (0x0E17, TcColType::I32),
    (0x0E30, TcColType::I32),
    (0x0E33, TcColType::I64),
    (0x0E34, TcColType::BinaryRef),
    (0x0E38, TcColType::I32),
    (0x0E3C, TcColType::BinaryRef),
    (0x0E3D, TcColType::BinaryRef),
    (0x1097, TcColType::I32),
    (0x3008, TcColType::Time),
    (0x65C6, TcColType::I32),
    (0x67F2, TcColType::I32),
    (0x67F3, TcColType::I32),
];

/// 5c. FAI Contents Table Template (NID `0x60F`) column schema — verified
/// from https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/b2e619a0-6a9c-4101-9dcb-340ac41cf308
const ASSOC_CONTENTS_TABLE_TEMPLATE_COLUMNS: [(u16, TcColType); 14] = [
    (0x001A, TcColType::StringRef),
    (0x0E07, TcColType::I32),
    (0x0E17, TcColType::I32),
    (0x3001, TcColType::StringRef),
    (0x67F2, TcColType::I32),
    (0x67F3, TcColType::I32),
    (0x6800, TcColType::StringRef),
    (0x6803, TcColType::Bool),
    (0x6805, TcColType::MultiI32Ref),
    (0x7003, TcColType::I32),
    (0x7004, TcColType::BinaryRef),
    (0x7005, TcColType::BinaryRef),
    (0x7006, TcColType::StringRef),
    (0x7007, TcColType::I32),
];

/// Attachment Table (template NID `0x671` + per-message subnode) column schema.
/// MS-PST attachment table minimum columns used by this writer.
const ATTACHMENT_TABLE_COLUMNS: [(u16, TcColType); 6] = [
    (PID_TAG_ATTACH_SIZE, TcColType::I32),           // 0x0E20
    (PID_TAG_ATTACH_FILENAME, TcColType::StringRef), // 0x3704
    (PID_TAG_ATTACH_METHOD, TcColType::I32),         // 0x3705
    (PID_TAG_RENDERING_POSITION, TcColType::I32),    // 0x370B
    (PID_TAG_LTP_ROW_ID, TcColType::I32),            // 0x67F2
    (PID_TAG_LTP_ROW_VER, TcColType::I32),           // 0x67F3
];

/// Recipient Table (template NID `0x692` + per-message subnode) column schema.
/// MS-PST Recipient Table Template 14 MUST columns + product `PidTagSmtpAddress`.
/// https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/bb069b2b-80ad-46d5-b86f-33487d16bf0c
const RECIPIENT_TABLE_COLUMNS: [(u16, TcColType); 15] = [
    (PID_TAG_RECIPIENT_TYPE, TcColType::I32),          // 0x0C15
    (PID_TAG_RESPONSIBILITY, TcColType::Bool),         // 0x0E0F
    (PID_TAG_RECORD_KEY, TcColType::BinaryRef),        // 0x0FF9
    (PID_TAG_OBJECT_TYPE, TcColType::I32),             // 0x0FFE
    (PID_TAG_ENTRY_ID, TcColType::BinaryRef),          // 0x0FFF
    (PID_TAG_DISPLAY_NAME, TcColType::StringRef),      // 0x3001
    (PID_TAG_ADDRESS_TYPE, TcColType::StringRef),      // 0x3002
    (PID_TAG_EMAIL_ADDRESS, TcColType::StringRef),     // 0x3003
    (PID_TAG_SEARCH_KEY, TcColType::BinaryRef),        // 0x300B
    (PID_TAG_DISPLAY_TYPE, TcColType::I32),            // 0x3900
    (PID_TAG_SMTP_ADDRESS, TcColType::StringRef),      // 0x39FE (product extra)
    (PID_TAG_7BIT_DISPLAY_NAME, TcColType::StringRef), // 0x39FF
    (PID_TAG_SEND_RICH_INFO, TcColType::Bool),         // 0x3A40
    (PID_TAG_LTP_ROW_ID, TcColType::I32),              // 0x67F2
    (PID_TAG_LTP_ROW_VER, TcColType::I32),             // 0x67F3
];

/// 5d. Search Folder Contents Table Template (NID `0x610`) column schema —
/// verified from https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-pst/cdcf9571-049f-47f5-b075-8374057134ec
/// (`0x0E07`/`0x0E17` appear twice in Microsoft's own published table; kept
/// once each here — a TC cannot have a duplicate column tag, so this is
/// treated as a documentation quirk on Microsoft's page, not replicated).
const SEARCH_CONTENTS_TABLE_TEMPLATE_COLUMNS: [(u16, TcColType); 18] = [
    (0x0017, TcColType::I32),
    (0x001A, TcColType::StringRef),
    (0x0036, TcColType::I32),
    (0x0E07, TcColType::I32),
    (0x0E17, TcColType::I32),
    (0x0037, TcColType::StringRef),
    (0x0042, TcColType::StringRef),
    (0x0057, TcColType::Bool),
    (0x0E03, TcColType::StringRef),
    (0x0E04, TcColType::StringRef),
    (0x0E05, TcColType::StringRef),
    (0x0E06, TcColType::Time),
    (0x0E08, TcColType::I32),
    (0x0E2A, TcColType::Bool),
    (0x3008, TcColType::Time),
    (0x67F1, TcColType::I32),
    (0x67F2, TcColType::I32),
    (0x67F3, TcColType::I32),
];

// ── Layout extensions: XBLOCK/XXBLOCK, subnodes, BTree planning ─────────────

/// Planned page BIDs for a full multi-level BTree, bottom-up
/// (`levels[0]` = leaf pages; `levels.last()` has exactly one page, the root).
struct TreePlan {
    ptype: u8,
    levels: Vec<Vec<u64>>,
    leaf_entry_size: usize,
    per_leaf_capacity: usize,
}

impl Layout {
    /// Write `data` as a single external block, an XBLOCK chain, or an XXBLOCK
    /// chain (MS-PST §2.2.2.8.3), returning the BID to use as a node's
    /// `bidData`. Returns the null BID (0) for empty data. Hard-fails (never
    /// silently truncates) when `data` exceeds documented XBLOCK/XXBLOCK
    /// capacity.
    pub fn write_data_chain(&mut self, data: Vec<u8>) -> Result<u64> {
        if data.is_empty() {
            return Ok(0);
        }
        if data.len() > i32::MAX as usize {
            return Err(WriterError::BodyTooLarge(format!(
                "{} bytes exceeds i32::MAX ({} bytes) — the largest value \
                 representable by PidTagMessageSize's PT_LONG (MS-OXPROPS) \
                 range, which every written body/html value must fit within",
                data.len(),
                i32::MAX
            )));
        }
        if data.len() <= MAX_BLOCK_DATA {
            let bid = self.alloc_bid(false);
            self.push_leaf_block(bid, data)?;
            return Ok(bid);
        }

        let total_len = data.len() as u32;
        let mut data_chunks: Vec<(u64, u32)> = Vec::new();
        for c in data.chunks(MAX_BLOCK_DATA) {
            let bid = self.alloc_bid(false);
            let len = c.len() as u32;
            self.push_leaf_block(bid, c.to_vec())?;
            data_chunks.push((bid, len));
        }

        if data_chunks.len() <= MAX_XBLOCK_ENTRIES {
            return self.build_xblock(&data_chunks);
        }

        let mut xblock_bids = Vec::new();
        for group in data_chunks.chunks(MAX_XBLOCK_ENTRIES) {
            xblock_bids.push(self.build_xblock(group)?);
        }
        if xblock_bids.len() > MAX_XBLOCK_ENTRIES {
            let max_bytes =
                (MAX_XBLOCK_ENTRIES as u64) * (MAX_XBLOCK_ENTRIES as u64) * (MAX_BLOCK_DATA as u64);
            return Err(WriterError::AllocationFailed(format!(
                "data requires {} XBLOCKs, exceeding one XXBLOCK's capacity of {} entries \
                 (v1's two-level XBLOCK/XXBLOCK scheme represents at most ~{max_bytes} bytes per value)",
                xblock_bids.len(),
                MAX_XBLOCK_ENTRIES
            )));
        }
        self.build_xxblock(&xblock_bids, total_len)
    }

    /// Chunked data-chain write from a [`Read`] without assembling a full
    /// multi-GB payload `Vec`. Reads into a fixed `STREAM_CHUNK_SIZE`
    /// (`MAX_BLOCK_DATA`) buffer only.
    ///
    /// When [`Layout::eager`] is set, each leaf chunk is place+written to the
    /// same-dir temp immediately (`on_disk = true`, empty `data` in `Layout`).
    ///
    /// **Transactional soft-fail (0070):** on mid-stream I/O error or size
    /// overflow, rolls back any leaf/XBLOCK entries created for this call
    /// (and truncates the eager temp cursor) so failed attaches leave no
    /// orphan BBT blocks in the finalized PST.
    ///
    /// Returns `(root_bid, total_bytes)`. Soft callers should map I/O errors
    /// to attach soft-fail as needed.
    pub fn write_data_chain_from_reader<R: Read>(&mut self, reader: &mut R) -> Result<(u64, u64)> {
        let checkpoint = self.stream_chain_checkpoint();
        match self.write_data_chain_from_reader_inner(reader) {
            Ok(v) => Ok(v),
            Err(e) => {
                self.rollback_stream_chain(checkpoint);
                Err(e)
            }
        }
    }

    /// Snapshot layout state so a failed stream chain can be rolled back.
    fn stream_chain_checkpoint(&self) -> StreamChainCheckpoint {
        StreamChainCheckpoint {
            blocks_len: self.blocks.len(),
            next_bid_counter: self.next_bid_counter,
            eager_cursor: self.eager.as_ref().map(|e| e.cursor),
            eager_amap_len: self.eager.as_ref().map(|e| e.amap_pages.len()),
        }
    }
}

/// Checkpoint for transactional `write_data_chain_from_reader` soft-fail rollback.
struct StreamChainCheckpoint {
    blocks_len: usize,
    next_bid_counter: u64,
    eager_cursor: Option<u64>,
    eager_amap_len: Option<usize>,
}

impl Layout {
    // (methods continue — rollback + inner live on Layout below)

    /// Drop blocks/BIDs added after `cp` and restore eager placement cursor.
    fn rollback_stream_chain(&mut self, cp: StreamChainCheckpoint) {
        // Remove bids for blocks we are about to drop (and any XBLOCK we added).
        if self.blocks.len() > cp.blocks_len {
            for block in self.blocks.drain(cp.blocks_len..) {
                self.used_bids.remove(&block.bid);
            }
        }
        // Restore bid counter so subsequent allocs don't permanently burn the
        // range (used_bids already cleaned for rolled-back bids).
        self.next_bid_counter = cp.next_bid_counter;

        if let Some(eager) = self.eager.as_mut() {
            if let Some(cursor) = cp.eager_cursor {
                eager.cursor = cursor;
                // Truncate temp so physical size and free region match cursor.
                // Ignore truncate errors — soft-fail still proceeds without orphans in BBT.
                let _ = eager.file.set_len(cursor);
                let _ = eager.file.seek(SeekFrom::Start(cursor));
            }
            if let Some(amap_len) = cp.eager_amap_len {
                if eager.amap_pages.len() > amap_len {
                    for page in eager.amap_pages.drain(amap_len..) {
                        eager.amap_stubs_written.remove(&page.offset);
                        self.used_bids.remove(&page.bid);
                    }
                }
            }
        }
    }

    fn write_data_chain_from_reader_inner<R: Read>(
        &mut self,
        reader: &mut R,
    ) -> Result<(u64, u64)> {
        let mut buf = vec![0u8; STREAM_CHUNK_SIZE];
        let mut chunks: Vec<(u64, u32)> = Vec::new();
        let mut total: u64 = 0;

        loop {
            let mut filled = 0usize;
            while filled < STREAM_CHUNK_SIZE {
                match reader.read(&mut buf[filled..]) {
                    Ok(0) => break,
                    Ok(n) => filled += n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(WriterError::Io(e)),
                }
            }
            if filled == 0 {
                break;
            }
            total = total.saturating_add(filled as u64);
            if total > i32::MAX as u64 {
                return Err(WriterError::BodyTooLarge(format!(
                    "{total} bytes exceeds i32::MAX ({} bytes) — PidTagMessageSize PT_LONG range",
                    i32::MAX
                )));
            }
            let bid = self.alloc_bid(false);
            // Spill leaf when eager is attached; otherwise keep in RAM.
            self.push_leaf_block(bid, buf[..filled].to_vec())?;
            chunks.push((bid, filled as u32));
            if filled < STREAM_CHUNK_SIZE {
                break;
            }
        }

        if chunks.is_empty() {
            return Ok((0, 0));
        }
        if chunks.len() == 1 {
            return Ok((chunks[0].0, total));
        }
        if chunks.len() <= MAX_XBLOCK_ENTRIES {
            let bid = self.build_xblock(&chunks)?;
            return Ok((bid, total));
        }
        let mut xblock_bids = Vec::new();
        for group in chunks.chunks(MAX_XBLOCK_ENTRIES) {
            xblock_bids.push(self.build_xblock(group)?);
        }
        if xblock_bids.len() > MAX_XBLOCK_ENTRIES {
            let max_bytes =
                (MAX_XBLOCK_ENTRIES as u64) * (MAX_XBLOCK_ENTRIES as u64) * (MAX_BLOCK_DATA as u64);
            return Err(WriterError::AllocationFailed(format!(
                "streamed data requires {} XBLOCKs, exceeding XXBLOCK capacity (max ~{max_bytes} bytes)",
                xblock_bids.len()
            )));
        }
        let total_u32 = total as u32;
        let bid = self.build_xxblock(&xblock_bids, total_u32)?;
        Ok((bid, total))
    }

    fn build_xblock(&mut self, chunks: &[(u64, u32)]) -> Result<u64> {
        let c_entries = chunks.len() as u16;
        let lcb_total: u32 = chunks.iter().map(|(_, l)| *l).sum();
        let mut payload = Vec::with_capacity(8 + chunks.len() * 8);
        payload.push(0x01); // btype
        payload.push(0x01); // cLevel = 1 (XBLOCK: children are data blocks)
        payload.extend_from_slice(&c_entries.to_le_bytes());
        payload.extend_from_slice(&lcb_total.to_le_bytes());
        for (bid, _) in chunks {
            payload.extend_from_slice(&bid.to_le_bytes());
        }
        let bid = self.alloc_bid(true);
        self.blocks.push(BlockEntry::in_memory(bid, payload));
        Ok(bid)
    }

    fn build_xxblock(&mut self, xblock_bids: &[u64], total_len: u32) -> Result<u64> {
        let mut payload = Vec::with_capacity(8 + xblock_bids.len() * 8);
        payload.push(0x01); // btype
        payload.push(0x02); // cLevel = 2 (XXBLOCK: children are XBLOCKs)
        payload.extend_from_slice(&(xblock_bids.len() as u16).to_le_bytes());
        payload.extend_from_slice(&total_len.to_le_bytes());
        for bid in xblock_bids {
            payload.extend_from_slice(&bid.to_le_bytes());
        }
        let bid = self.alloc_bid(true);
        self.blocks.push(BlockEntry::in_memory(bid, payload));
        Ok(bid)
    }

    /// Add a top-level node whose data may exceed one block (via
    /// `write_data_chain`). `sub_bid` is the node's subnode-BTree root BID (0 if
    /// none). Never reachable is the fixture path's `assert!`-based `add_node`.
    pub fn add_node_data(
        &mut self,
        nid: u64,
        data: Vec<u8>,
        nid_parent: u64,
        sub_bid: u64,
    ) -> Result<u64> {
        let bid_data = self.write_data_chain(data)?;
        if !self.used_nids.insert(nid) {
            return Err(WriterError::Layout(format!(
                "duplicate NBT nid 0x{nid:X} (reserved template collision or double insert)"
            )));
        }
        self.nodes.push(NodeEntry {
            nid,
            bid_data,
            bid_sub: sub_bid,
            nid_parent,
        });
        Ok(bid_data)
    }

    /// Pack a TC row matrix into a data tree with integral rows per 8176-byte
    /// leaf (MS-PST §2.3.4.4). Non-last leaves are padded to `MAX_BLOCK_DATA`;
    /// the last leaf is only the remaining rows (dead space is not stored as
    /// extra rows). Returns `(root_bid, total_payload_bytes)`.
    fn write_row_matrix_tree(&mut self, row_matrix: &[u8], row_width: usize) -> Result<(u64, u64)> {
        if row_matrix.is_empty() {
            return Ok((0, 0));
        }
        let row_count = row_matrix
            .len()
            .checked_div(row_width)
            .ok_or_else(|| WriterError::Layout("row_width is 0; cannot pack row matrix".into()))?;
        if !row_matrix.len().is_multiple_of(row_width) {
            return Err(WriterError::Layout(format!(
                "row matrix length {} is not a multiple of row_width {row_width}",
                row_matrix.len()
            )));
        }
        let rows_per_block = MAX_BLOCK_DATA
            .checked_div(row_width)
            .ok_or_else(|| WriterError::Layout("row_width is 0; cannot pack row matrix".into()))?;
        if rows_per_block == 0 {
            return Err(WriterError::Layout(format!(
                "row_width {row_width} exceeds MAX_BLOCK_DATA {MAX_BLOCK_DATA}"
            )));
        }
        let mut chunks: Vec<(u64, u32)> = Vec::new();
        let mut i = 0usize;
        while i < row_count {
            let remaining = row_count - i;
            let take = remaining.min(rows_per_block);
            let is_last = i + take == row_count;
            let mut payload = row_matrix[i * row_width..(i + take) * row_width].to_vec();
            if !is_last {
                payload.resize(MAX_BLOCK_DATA, 0);
            }
            let bid = self.alloc_bid(false);
            let len = payload.len() as u32;
            self.push_leaf_block(bid, payload)?;
            chunks.push((bid, len));
            i += take;
        }
        let total: u64 = chunks.iter().map(|(_, l)| u64::from(*l)).sum();
        if chunks.len() == 1 {
            return Ok((chunks[0].0, total));
        }
        if chunks.len() <= MAX_XBLOCK_ENTRIES {
            let bid = self.build_xblock(&chunks)?;
            return Ok((bid, total));
        }
        let mut xblock_bids = Vec::new();
        for group in chunks.chunks(MAX_XBLOCK_ENTRIES) {
            xblock_bids.push(self.build_xblock(group)?);
        }
        if xblock_bids.len() > MAX_XBLOCK_ENTRIES {
            return Err(WriterError::AllocationFailed(format!(
                "row matrix requires {} XBLOCKs, exceeding one XXBLOCK capacity",
                xblock_bids.len()
            )));
        }
        let bid = self.build_xxblock(&xblock_bids, total as u32)?;
        Ok((bid, total))
    }

    /// Build a single-block SLBLOCK subnode leaf listing `entries` (nid,
    /// bidData, bidSub).
    ///
    /// Used for large body/HTML diversions, the attachment table
    /// (`NID_ATTACHMENT_TABLE`), attach objects (type `0x05`), attach data
    /// diversions, and nested embedded-message objects under attach subnodes
    /// (track 0069). One SLBLOCK always suffices for current scale; returns a
    /// typed error rather than silently dropping entries if capacity is
    /// exceeded (multi-level SI hierarchy remains a future concern).
    pub fn add_subnode_leaf(&mut self, entries: &[(u64, u64, u64)]) -> Result<u64> {
        let mut payload = Vec::with_capacity(8 + entries.len() * 24);
        payload.push(0x02); // btype (subnode block)
        payload.push(0x00); // cLevel = 0 (SLBLOCK: leaf)
        payload.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes()); // reserved
        for (nid, bid_data, bid_sub) in entries {
            payload.extend_from_slice(&nid.to_le_bytes());
            payload.extend_from_slice(&bid_data.to_le_bytes());
            payload.extend_from_slice(&bid_sub.to_le_bytes());
        }
        if payload.len() > MAX_BLOCK_DATA {
            return Err(WriterError::Layout(format!(
                "{} subnode entries exceed v1's single-SLBLOCK capacity",
                entries.len()
            )));
        }
        let bid = self.alloc_bid(true);
        self.blocks.push(BlockEntry::in_memory(bid, payload));
        Ok(bid)
    }

    /// Reserve pages for a full multi-level BTree over `entry_count` leaf
    /// entries. Content is filled in later (see `write_nbt`/`write_bbt`) once
    /// real file offsets are known.
    fn plan_tree(&mut self, ptype: u8, leaf_entry_size: usize, entry_count: usize) -> TreePlan {
        let per_leaf = (488usize / leaf_entry_size).max(1);
        let leaf_count = entry_count.div_ceil(per_leaf).max(1);
        let mut levels: Vec<Vec<u64>> =
            vec![(0..leaf_count).map(|_| self.reserve_page(ptype)).collect()];

        while levels.last().map(|l| l.len()).unwrap_or(0) > 1 {
            let prev_len = levels.last().map(|l| l.len()).unwrap_or(0);
            let next_count = prev_len.div_ceil(INTERMEDIATE_ENTRIES_PER_PAGE).max(1);
            let next: Vec<u64> = (0..next_count).map(|_| self.reserve_page(ptype)).collect();
            levels.push(next);
        }

        TreePlan {
            ptype,
            levels,
            leaf_entry_size,
            per_leaf_capacity: per_leaf,
        }
    }
}

// ── CRC / wSig ────────────────────────────────────────────────────────────

/// MS-PST §2.2.2.7.1 page signature. `pst-reader` does not validate this value
/// (see `pst_reader::ndb::page`), but real Outlook/scanpst do — implemented
/// here as a best-effort, widely-cross-referenced XOR-fold of the page's file
/// offset and BID rather than left as a placeholder. Not independently
/// verified against a real Outlook-opened PST in this environment (scanpst is
/// unavailable here) — flagged as a residual, see final report.
fn compute_page_sig(ib: u64, bid: u64) -> u16 {
    let ib32 = ib as u32;
    let bid_lo = (bid & 0xFFFF_FFFF) as u32;
    let bid_hi = (bid >> 32) as u32;
    let value = ib32 ^ bid_lo ^ bid_hi;
    ((value >> 16) ^ (value & 0xFFFF)) as u16
}

fn page_offset_map(layout: &Layout) -> HashMap<u64, u64> {
    layout.pages.iter().map(|p| (p.bid, p.offset)).collect()
}

// ── Page writers ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn write_bt_page<W: Write + Seek>(
    writer: &mut W,
    offset: u64,
    bid: u64,
    ptype: u8,
    c_level: u8,
    entries_region: &[u8],
    c_entries: u8,
    entry_size: usize,
) -> Result<()> {
    let mut page = vec![0u8; PAGE_SIZE as usize];
    let n = entries_region.len().min(488);
    page[..n].copy_from_slice(&entries_region[..n]);

    let c_ent_max = (488 / entry_size.max(1)).min(255) as u8;
    page[488] = c_entries;
    page[489] = c_ent_max;
    page[490] = 8; // cbEntKey
    page[491] = c_level;
    page[492..496].fill(0);

    let trailer_offset = PAGE_SIZE as usize - 16;
    page[trailer_offset] = ptype;
    page[trailer_offset + 1] = ptype;
    let sig = compute_page_sig(offset, bid);
    page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&sig.to_le_bytes());
    let crc = crc32fast::hash(&page[..trailer_offset]);
    page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
    page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&bid.to_le_bytes());

    writer.seek(SeekFrom::Start(offset))?;
    writer.write_all(&page)?;
    Ok(())
}

/// Write every AMap page at its mandated absolute offset.
/// Content: 496 bytes of `0xFF` free bits (v1 free accounting approximate).
fn write_all_amap_pages_v1<W: Write + Seek>(writer: &mut W, layout: &Layout) -> Result<()> {
    let amaps: Vec<_> = layout
        .pages
        .iter()
        .filter(|p| p.ptype == PTYPE_AMAP)
        .copied()
        .collect();
    if amaps.is_empty() {
        return Err(WriterError::Layout("missing AMap page".to_string()));
    }
    for amap_page in amaps {
        let mut page = vec![0u8; PAGE_SIZE as usize];
        page[..496].fill(0xFF);

        let trailer_offset = PAGE_SIZE as usize - 16;
        page[trailer_offset] = PTYPE_AMAP;
        page[trailer_offset + 1] = PTYPE_AMAP;
        let sig = compute_page_sig(amap_page.offset, amap_page.bid);
        page[trailer_offset + 2..trailer_offset + 4].copy_from_slice(&sig.to_le_bytes());
        let crc = crc32fast::hash(&page[..trailer_offset]);
        page[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&crc.to_le_bytes());
        page[trailer_offset + 8..trailer_offset + 16].copy_from_slice(&amap_page.bid.to_le_bytes());

        writer.seek(SeekFrom::Start(amap_page.offset))?;
        writer.write_all(&page)?;
    }
    Ok(())
}

fn encode_nbt_leaf(n: &NodeEntry) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[0..8].copy_from_slice(&n.nid.to_le_bytes());
    e[8..16].copy_from_slice(&n.bid_data.to_le_bytes());
    e[16..24].copy_from_slice(&n.bid_sub.to_le_bytes());
    e[24..28].copy_from_slice(&(n.nid_parent as u32).to_le_bytes());
    e
}

fn encode_bbt_leaf(b: &BlockEntry) -> [u8; 24] {
    let mut e = [0u8; 24];
    e[0..8].copy_from_slice(&b.bid.to_le_bytes());
    e[8..16].copy_from_slice(&b.offset.to_le_bytes());
    e[16..18].copy_from_slice(&(b.payload_len() as u16).to_le_bytes());
    e[18..20].copy_from_slice(&1u16.to_le_bytes()); // cRef
    e
}

/// Write every level of a planned BTree (leaf pages then each intermediate
/// level up to the single root), using real sorted ascending keys (the true
/// NID/BID of the minimum entry in each child subtree) — not a placeholder.
fn write_tree<W: Write + Seek>(
    writer: &mut W,
    plan: &TreePlan,
    page_offsets: &HashMap<u64, u64>,
    leaf_min_keys: &[u64],
    leaf_pages: &[(Vec<u8>, u8)],
) -> Result<()> {
    for (i, bid) in plan.levels[0].iter().enumerate() {
        let offset = *page_offsets
            .get(bid)
            .ok_or_else(|| WriterError::Layout("missing leaf page offset".to_string()))?;
        let (region, c_entries) = &leaf_pages[i];
        write_bt_page(
            writer,
            offset,
            *bid,
            plan.ptype,
            0,
            region,
            *c_entries,
            plan.leaf_entry_size,
        )?;
    }

    let mut prev_bids = plan.levels[0].clone();
    let mut prev_min_keys: Vec<u64> = leaf_min_keys.to_vec();

    for (level_idx, level_bids) in plan.levels.iter().enumerate().skip(1) {
        let mut new_min_keys = Vec::with_capacity(level_bids.len());
        let mut child_idx = 0usize;
        for bid in level_bids {
            let end = (child_idx + INTERMEDIATE_ENTRIES_PER_PAGE).min(prev_bids.len());
            if child_idx >= end {
                break;
            }
            let mut region = Vec::with_capacity((end - child_idx) * INTERMEDIATE_ENTRY_SIZE);
            for k in child_idx..end {
                let child_bid = prev_bids[k];
                let child_offset = *page_offsets
                    .get(&child_bid)
                    .ok_or_else(|| WriterError::Layout("missing child page offset".to_string()))?;
                region.extend_from_slice(&prev_min_keys[k].to_le_bytes());
                region.extend_from_slice(&child_bid.to_le_bytes());
                region.extend_from_slice(&child_offset.to_le_bytes());
            }
            new_min_keys.push(prev_min_keys[child_idx]);
            let offset = *page_offsets.get(bid).ok_or_else(|| {
                WriterError::Layout("missing intermediate page offset".to_string())
            })?;
            write_bt_page(
                writer,
                offset,
                *bid,
                plan.ptype,
                level_idx as u8,
                &region,
                (end - child_idx) as u8,
                INTERMEDIATE_ENTRY_SIZE,
            )?;
            child_idx = end;
        }
        prev_bids = level_bids.clone();
        prev_min_keys = new_min_keys;
    }

    Ok(())
}

fn write_nbt<W: Write + Seek>(
    writer: &mut W,
    layout: &Layout,
    plan: &TreePlan,
    page_offsets: &HashMap<u64, u64>,
) -> Result<()> {
    let mut sorted: Vec<&NodeEntry> = layout.nodes.iter().collect();
    sorted.sort_by_key(|n| n.nid);
    for w in sorted.windows(2) {
        if w[0].nid == w[1].nid {
            return Err(WriterError::Layout(format!(
                "duplicate NBT nid 0x{:X}",
                w[0].nid
            )));
        }
    }

    let mut leaf_pages = Vec::new();
    let mut min_keys = Vec::new();
    for chunk in sorted.chunks(plan.per_leaf_capacity) {
        let mut region = Vec::with_capacity(chunk.len() * NBT_LEAF_ENTRY_SIZE);
        for n in chunk {
            region.extend_from_slice(&encode_nbt_leaf(n));
        }
        min_keys.push(chunk[0].nid);
        leaf_pages.push((region, chunk.len() as u8));
    }
    if leaf_pages.is_empty() {
        leaf_pages.push((Vec::new(), 0));
        min_keys.push(0);
    }

    write_tree(writer, plan, page_offsets, &min_keys, &leaf_pages)
}

fn write_bbt<W: Write + Seek>(
    writer: &mut W,
    layout: &Layout,
    plan: &TreePlan,
    page_offsets: &HashMap<u64, u64>,
) -> Result<()> {
    let mut sorted: Vec<&BlockEntry> = layout.blocks.iter().collect();
    sorted.sort_by_key(|b| b.bid);

    let mut leaf_pages = Vec::new();
    let mut min_keys = Vec::new();
    for chunk in sorted.chunks(plan.per_leaf_capacity) {
        let mut region = Vec::with_capacity(chunk.len() * BBT_LEAF_ENTRY_SIZE);
        for b in chunk {
            region.extend_from_slice(&encode_bbt_leaf(b));
        }
        min_keys.push(chunk[0].bid);
        leaf_pages.push((region, chunk.len() as u8));
    }
    if leaf_pages.is_empty() {
        leaf_pages.push((Vec::new(), 0));
        min_keys.push(0);
    }

    write_tree(writer, plan, page_offsets, &min_keys, &leaf_pages)
}

/// Production header writer — unlike `crate::write_header` (fixture path, which
/// locates the NBT/BBT/AMap root pages by linear `.find()` + `.unwrap()` and
/// only works for single-page trees), this takes the real multi-level tree
/// plans and never panics.
fn write_header_v1<W: Write>(
    writer: &mut W,
    layout: &Layout,
    nbt_plan: &TreePlan,
    bbt_plan: &TreePlan,
) -> Result<()> {
    let root_nbt_bid = *nbt_plan
        .levels
        .last()
        .and_then(|l| l.first())
        .ok_or_else(|| WriterError::Layout("empty NBT plan".to_string()))?;
    let root_bbt_bid = *bbt_plan
        .levels
        .last()
        .and_then(|l| l.first())
        .ok_or_else(|| WriterError::Layout("empty BBT plan".to_string()))?;

    let offsets = page_offset_map(layout);
    let nbt_offset = *offsets
        .get(&root_nbt_bid)
        .ok_or_else(|| WriterError::Layout("missing NBT root offset".to_string()))?;
    let bbt_offset = *offsets
        .get(&root_bbt_bid)
        .ok_or_else(|| WriterError::Layout("missing BBT root offset".to_string()))?;
    let amap_last = layout
        .ib_amap_last()
        .ok_or_else(|| WriterError::Layout("missing AMap page (ibAMapLast)".to_string()))?;

    let file_size = layout.file_size();
    let next_bid = layout.next_bid_counter;

    let mut buf = Vec::new();
    buf.write_u32::<LittleEndian>(PST_MAGIC)?;
    buf.write_u32::<LittleEndian>(0)?; // dwCRCPartial
    buf.write_u16::<LittleEndian>(CLIENT_MAGIC)?;
    buf.write_u16::<LittleEndian>(UNICODE_VERSION)?;
    buf.write_u16::<LittleEndian>(0x0036)?; // wVerClient
    buf.write_all(&[0x01, 0x01])?; // bPlatformCreate, bPlatformAccess
    buf.write_u32::<LittleEndian>(0)?;
    buf.write_u32::<LittleEndian>(0)?;
    buf.write_u64::<LittleEndian>(0)?; // bidUnused
    buf.write_u64::<LittleEndian>(next_bid)?; // bidNextP
    buf.write_u32::<LittleEndian>(1)?; // dwUnique
    buf.write_all(&[0u8; 128])?; // rgnid
    buf.write_u64::<LittleEndian>(0)?; // qwUnused

    // ROOT (72 bytes)
    buf.write_u32::<LittleEndian>(0)?;
    buf.write_u64::<LittleEndian>(file_size)?;
    buf.write_u64::<LittleEndian>(amap_last)?;
    buf.write_u64::<LittleEndian>(0)?; // cbAMapFree
    buf.write_u64::<LittleEndian>(0)?; // cbPMapFree
    buf.write_u64::<LittleEndian>(root_nbt_bid)?;
    buf.write_u64::<LittleEndian>(nbt_offset)?;
    buf.write_u64::<LittleEndian>(root_bbt_bid)?;
    buf.write_u64::<LittleEndian>(bbt_offset)?;
    // MS-PST §2.2.2.5 ROOT (Unicode, 72 bytes total): fAMapValid (1) +
    // bReserved (1) + wReserved (2) = 4 bytes, matching
    // `pst_reader::header::PstHeader::read_root` exactly (see that module's
    // comment: the old 8-byte padding here was the same already-fixed-on-read
    // bug, copied verbatim from the pre-existing fixture `write_header`).
    buf.write_u8(1)?; // fAMapValid
    buf.write_all(&[0u8; 3])?; // bReserved (1) + wReserved (2)

    buf.write_u32::<LittleEndian>(0)?; // dwAlign — ends at 0x100
                                       // rgbFM (128) + rgbFP (128) = 256 bytes, ending at 0x200 — matching
                                       // `pst_reader::header::PstHeader::read` exactly (the old 508-byte skip
                                       // here was the corresponding already-fixed-on-read bug).
    buf.write_all(&[0u8; 256])?; // rgbFM + rgbFP
    buf.write_u8(0x80)?; // bSentinel (offset 0x200)
    buf.write_u8(0)?; // bCryptMethod = none (offset 0x201)
    buf.write_u16::<LittleEndian>(0)?; // rgbReserved (offset 0x202)
    buf.write_u64::<LittleEndian>(next_bid)?; // bidNextB (offset 0x204)

    let padding = (HEADER_SIZE as usize).saturating_sub(buf.len());
    buf.resize(buf.len() + padding, 0);
    writer.write_all(&buf)?;
    Ok(())
}

// ── Unit tests (verification gate: XBLOCK encode/decode symmetry) ──────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_data_chain_small_uses_single_external_block() {
        let mut layout = Layout::new();
        let bid = layout.write_data_chain(vec![1, 2, 3, 4, 5]).expect("chain");
        assert_eq!(bid & 0x02, 0, "small data should use an external block");
        assert_eq!(layout.blocks.len(), 1);
        assert_eq!(layout.blocks[0].data, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn write_data_chain_empty_returns_null_bid() {
        let mut layout = Layout::new();
        let bid = layout.write_data_chain(Vec::new()).expect("chain");
        assert_eq!(bid, 0);
        assert_eq!(layout.blocks.len(), 0);
    }

    #[test]
    fn write_data_chain_multiblock_builds_xblock() {
        let mut layout = Layout::new();
        let data = vec![7u8; MAX_BLOCK_DATA * 3 + 10];
        let bid = layout.write_data_chain(data.clone()).expect("chain");
        assert_eq!(bid & 0x02, 0x02, "multi-block data returns an internal bid");

        // 3 full external chunks + 1 partial + 1 XBLOCK = 5 blocks.
        assert_eq!(layout.blocks.len(), 5);

        let xblock = layout
            .blocks
            .iter()
            .find(|b| b.bid == bid)
            .expect("xblock present");
        assert_eq!(xblock.data[0], 0x01, "btype");
        assert_eq!(xblock.data[1], 0x01, "cLevel = XBLOCK");
        let c_entries = u16::from_le_bytes([xblock.data[2], xblock.data[3]]);
        assert_eq!(c_entries, 4);
        let lcb_total = u32::from_le_bytes([
            xblock.data[4],
            xblock.data[5],
            xblock.data[6],
            xblock.data[7],
        ]);
        assert_eq!(lcb_total as usize, data.len());
    }

    /// PidTagMessageSize (MAPI 0x0E08) is a PtypInteger32 / PT_LONG property
    /// (MS-OXPROPS) — representable range `0..=i32::MAX`. `write_data_chain`
    /// must refuse anything larger than that with a hard `BodyTooLarge`
    /// error, not silently clamp/accept it, even though XBLOCK/XXBLOCK's own
    /// `lcbTotal` (a `u32`) could structurally describe a larger value. This
    /// is the boundary check itself, so it must fail before any XBLOCK/XXBLOCK
    /// chunking work — only the length matters, so a zero-filled `Vec` (cheap
    /// to allocate; no per-byte work needed) is enough to prove it without
    /// actually building/writing a multi-gigabyte chain.
    #[test]
    fn write_data_chain_rejects_data_larger_than_i32_max() {
        let mut layout = Layout::new();
        let data = vec![0u8; i32::MAX as usize + 1];
        let err = layout
            .write_data_chain(data)
            .expect_err("data larger than i32::MAX must be refused, not silently accepted");
        assert!(
            matches!(err, WriterError::BodyTooLarge(_)),
            "expected BodyTooLarge, got {err:?}"
        );
        assert_eq!(
            layout.blocks.len(),
            0,
            "no blocks should be written when the size ceiling is exceeded"
        );
    }

    #[test]
    fn eager_spill_leaves_on_disk_with_empty_data() {
        let dir = std::env::temp_dir();
        let tmp = dir.join(format!("pst_writer_eager_unit_{}.tmp", std::process::id()));
        let _ = fs::remove_file(&tmp);
        let mut layout = Layout::new();
        layout.attach_eager(crate::EagerWriteCtx::create(&tmp).expect("eager create"));

        // Multi-block payload so we get several leaf blocks + XBLOCK.
        let data = vec![0xCDu8; MAX_BLOCK_DATA * 2 + 100];
        let bid = layout.write_data_chain(data).expect("chain");
        assert_ne!(bid, 0);

        let leaf_on_disk: Vec<_> = layout.blocks.iter().filter(|b| b.on_disk).collect();
        assert!(
            leaf_on_disk.len() >= 2,
            "expected ≥2 on_disk leaves, got {}",
            leaf_on_disk.len()
        );
        for b in &leaf_on_disk {
            assert!(b.data.is_empty(), "on_disk leaf must clear data");
            assert!(b.offset >= HEADER_SIZE);
            assert!(b.len > 0);
        }
        // XBLOCK internal block stays in memory (not on_disk).
        let xblock = layout.blocks.iter().find(|b| b.bid == bid).expect("xblock");
        assert!(!xblock.on_disk);
        assert!(!xblock.data.is_empty());

        // Physical size grew past header.
        assert!(layout.current_physical_size() > HEADER_SIZE);

        drop(layout.take_eager());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn eager_stream_reader_spills_leaves_on_disk() {
        let dir = std::env::temp_dir();
        let tmp = dir.join(format!(
            "pst_writer_eager_stream_{}.tmp",
            std::process::id()
        ));
        let _ = fs::remove_file(&tmp);
        let mut layout = Layout::new();
        layout.attach_eager(crate::EagerWriteCtx::create(&tmp).expect("eager create"));

        let payload = vec![0x11u8; MAX_BLOCK_DATA + 50];
        let mut reader = Cursor::new(payload.clone());
        let (bid, total) = layout
            .write_data_chain_from_reader(&mut reader)
            .expect("stream chain");
        assert_eq!(total, payload.len() as u64);
        assert_ne!(bid, 0);

        let leaves: Vec<_> = layout.blocks.iter().filter(|b| b.on_disk).collect();
        assert!(!leaves.is_empty());
        for b in leaves {
            assert!(b.data.is_empty());
            assert!(b.on_disk);
        }

        drop(layout.take_eager());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn incremental_folder_plan_matches_collect_single_source() {
        let opts = WritePstOpts {
            folder_layout: FolderLayoutPolicy::PreservePaths {
                multi_source_prefix: true,
            },
            ..WritePstOpts::default()
        };
        let messages = vec![
            WriteMessage {
                source_path: Some(r"C:\data\mail.pst".into()),
                source_folder_path: Some("Inbox".into()),
                subject: "a".into(),
                ..WriteMessage::default()
            },
            WriteMessage {
                source_path: Some(r"C:\data\mail.pst".into()),
                source_folder_path: Some("Inbox/Work".into()),
                subject: "b".into(),
                ..WriteMessage::default()
            },
            WriteMessage {
                source_path: Some(r"C:\data\mail.pst".into()),
                source_folder_path: None,
                subject: "c".into(),
                ..WriteMessage::default()
            },
        ];

        let mut collect = plan_folder_tree(&messages, &opts);
        let mut layout_c = Layout::new();
        allocate_folder_nids(&mut layout_c, &mut collect);

        let mut layout_i = Layout::new();
        let mut inc = IncrementalFolderPlan::start(&mut layout_i, &opts);
        for (i, m) in messages.iter().enumerate() {
            let _ = inc.assign_message(&mut layout_i, m, &opts, i);
        }
        let inc = inc.into_folder_plan();

        assert_eq!(inc.folders_created, collect.folders_created);
        assert_eq!(inc.folder_paths_residual, collect.folder_paths_residual);
        assert_eq!(inc.folder_paths_degraded, collect.folder_paths_degraded);
        assert_eq!(inc.message_folder.len(), collect.message_folder.len());
        // Same tree shape: residual + Inbox (+ Work under Inbox); single source → no prefix.
        assert_eq!(inc.roots.len(), collect.roots.len());
        trim_folder_plan_to_written(&mut collect, 3);
        assert_eq!(collect.message_folder.len(), 3);
    }

    #[test]
    fn incremental_multi_source_prefix_after_second_source() {
        // Residual: first message (only one source seen) has no prefix; after
        // second source appears, subsequent messages get unique_source_prefixes.
        let opts = WritePstOpts {
            folder_layout: FolderLayoutPolicy::PreservePaths {
                multi_source_prefix: true,
            },
            ..WritePstOpts::default()
        };
        let m0 = WriteMessage {
            source_path: Some(r"C:\a\one.pst".into()),
            source_folder_path: Some("Inbox".into()),
            subject: "first".into(),
            ..WriteMessage::default()
        };
        let m1 = WriteMessage {
            source_path: Some(r"C:\b\two.pst".into()),
            source_folder_path: Some("Inbox".into()),
            subject: "second".into(),
            ..WriteMessage::default()
        };
        let m2 = WriteMessage {
            source_path: Some(r"C:\a\one.pst".into()),
            source_folder_path: Some("Sent".into()),
            subject: "third".into(),
            ..WriteMessage::default()
        };

        let mut layout = Layout::new();
        let mut plan = IncrementalFolderPlan::start(&mut layout, &opts);
        plan.assign_message(&mut layout, &m0, &opts, 0);
        // Only one source so far → no multi-source prefixes yet.
        assert!(plan.prefix_map.is_empty());
        // m0 under top-level "Inbox" (no source prefix).
        assert!(
            plan.roots.iter().any(|r| r.display_name == "Inbox"),
            "first message routes without source prefix"
        );

        plan.assign_message(&mut layout, &m1, &opts, 1);
        assert_eq!(plan.prefix_map.len(), 2);
        plan.assign_message(&mut layout, &m2, &opts, 2);
        // m2 from one.pst should sit under source prefix + Sent.
        let one_prefix = plan.prefix_map.get(r"C:\a\one.pst").cloned().unwrap();
        fn find_path(nodes: &[PlannedFolder], segs: &[&str]) -> bool {
            if segs.is_empty() {
                return true;
            }
            nodes
                .iter()
                .any(|n| n.display_name == segs[0] && find_path(&n.children, &segs[1..]))
        }
        assert!(
            find_path(&plan.roots, &[&one_prefix, "Sent"]),
            "after second source, later msgs get prefix; roots={:?}",
            plan.roots
                .iter()
                .map(|r| r.display_name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn flat_policy_routes_all_to_residual() {
        let opts = WritePstOpts {
            folder_layout: FolderLayoutPolicy::Flat {
                folder_display_name: "All Mail".into(),
            },
            ..WritePstOpts::default()
        };
        let mut layout = Layout::new();
        let mut plan = IncrementalFolderPlan::start(&mut layout, &opts);
        let msg = WriteMessage {
            source_folder_path: Some("Inbox/Deep".into()),
            subject: "x".into(),
            ..WriteMessage::default()
        };
        let nid = plan.assign_message(&mut layout, &msg, &opts, 0);
        assert_eq!(plan.roots.len(), 1);
        assert_eq!(plan.roots[0].display_name, "All Mail");
        assert_eq!(nid, plan.roots[0].nid);
        assert_eq!(plan.folders_created, 1);
    }

    #[test]
    fn normalize_folder_path_key_strips_leading_aliases_and_sanitizes() {
        assert_eq!(
            normalize_folder_path_key("Root/Top of Personal Folders/Inbox"),
            "inbox"
        );
        assert_eq!(
            normalize_folder_path_key(
                "Top of Personal Folders/Mailbox - Doe, John/Top of Information Store/Inbox"
            ),
            "mailbox - doe, john/top of information store/inbox"
        );
        // Later user folder named like a sentinel is preserved.
        assert_eq!(
            normalize_folder_path_key("Root/Top of Personal Folders/Inbox/Top of Personal Folders"),
            "inbox/top of personal folders"
        );
        // Sanitize quotes / asterisks / trailing dots (writer parity).
        assert_eq!(
            normalize_folder_path_key(r#"Inbox/Anthony "Tony" Randall"#),
            "inbox/anthony _tony_ randall"
        );
        assert_eq!(
            normalize_folder_path_key("Inbox/1. tony s."),
            "inbox/1. tony s"
        );
        assert_eq!(
            normalize_folder_path_key("Inbox/** my team"),
            "inbox/__ my team"
        );
        assert_eq!(normalize_folder_path_key(".."), "");
        assert_eq!(normalize_folder_path_key(""), "");
        let over_depth: String = (0..=MAX_FOLDER_DEPTH)
            .map(|i| format!("seg{i}"))
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(normalize_folder_path_key(&over_depth), "");
    }

    #[test]
    fn folder_path_qc_expected_key_mirrors_residual_unique_mail() {
        assert_eq!(folder_path_qc_expected_key(""), "unique mail");
        assert_eq!(folder_path_qc_expected_key("   "), "unique mail");
        assert_eq!(folder_path_qc_expected_key(".."), "unique mail");
        assert_eq!(folder_path_qc_expected_key("Inbox/../Sent"), "unique mail");
        assert_eq!(
            folder_path_qc_expected_key("Root/Top of Personal Folders"),
            "unique mail"
        );
        let over_depth: String = (0..=MAX_FOLDER_DEPTH)
            .map(|i| format!("seg{i}"))
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(folder_path_qc_expected_key(&over_depth), "unique mail");
        assert_eq!(
            folder_path_qc_expected_key("Root/Top of Personal Folders/Inbox"),
            "inbox"
        );
        assert_eq!(
            folder_path_qc_expected_key(r#"Inbox/Anthony "Tony" Randall"#),
            "inbox/anthony _tony_ randall"
        );
    }

    #[test]
    fn parse_folder_path_strips_leading_aliases_only() {
        match parse_folder_path("Root/Top of Personal Folders/Inbox") {
            PathParseOutcome::Segments { segs, .. } => {
                assert_eq!(segs, vec!["Inbox".to_string()]);
            }
            other => panic!("expected segments, got {other:?}"),
        }
        match parse_folder_path("Root/Top of Personal Folders/Inbox/Top of Personal Folders/Nested")
        {
            PathParseOutcome::Segments { segs, .. } => {
                assert_eq!(
                    segs,
                    vec![
                        "Inbox".to_string(),
                        "Top of Personal Folders".to_string(),
                        "Nested".to_string()
                    ]
                );
            }
            other => panic!("expected segments, got {other:?}"),
        }
        // Non-sentinel mailbox root preserved.
        match parse_folder_path("Mailbox - Doe, John/Inbox") {
            PathParseOutcome::Segments { segs, .. } => {
                assert_eq!(
                    segs,
                    vec!["Mailbox - Doe, John".to_string(), "Inbox".to_string()]
                );
            }
            other => panic!("expected segments, got {other:?}"),
        }
    }

    #[test]
    fn preserve_start_has_no_residual_until_needed() {
        let opts = WritePstOpts {
            folder_layout: FolderLayoutPolicy::PreservePaths {
                multi_source_prefix: true,
            },
            ..WritePstOpts::default()
        };
        let mut layout = Layout::new();
        let plan = IncrementalFolderPlan::start(&mut layout, &opts);
        assert!(plan.roots.is_empty());
        assert_eq!(plan.folders_created, 0);
    }

    #[test]
    fn known_source_paths_preseed_prefixes_from_message_one() {
        let opts = WritePstOpts {
            folder_layout: FolderLayoutPolicy::PreservePaths {
                multi_source_prefix: true,
            },
            known_source_paths: vec![r"C:\a\one.pst".into(), r"C:\b\two.pst".into()],
            ..WritePstOpts::default()
        };
        let m0 = WriteMessage {
            source_path: Some(r"C:\a\one.pst".into()),
            source_folder_path: Some("Root/Top of Personal Folders/Inbox".into()),
            subject: "first".into(),
            ..WriteMessage::default()
        };
        let mut layout = Layout::new();
        let mut plan = IncrementalFolderPlan::start(&mut layout, &opts);
        assert_eq!(plan.prefix_map.len(), 2, "pre-seed must populate prefixes");
        plan.assign_message(&mut layout, &m0, &opts, 0);
        let one_prefix = plan.prefix_map.get(r"C:\a\one.pst").cloned().unwrap();
        fn find_path(nodes: &[PlannedFolder], segs: &[&str]) -> bool {
            if segs.is_empty() {
                return true;
            }
            nodes
                .iter()
                .any(|n| n.display_name == segs[0] && find_path(&n.children, &segs[1..]))
        }
        assert!(
            find_path(&plan.roots, &[&one_prefix, "Inbox"]),
            "message 1 must already be under source prefix; roots={:?}",
            plan.roots
                .iter()
                .map(|r| r.display_name.as_str())
                .collect::<Vec<_>>()
        );
        // No doubled ToPF under the prefix.
        assert!(
            !find_path(
                &plan.roots,
                &[&one_prefix, "Top of Personal Folders", "Inbox"]
            ),
            "leading ToPF alias must be stripped"
        );
    }

    /// 0079: concurrent SHA-256 + MD5 over shared 1 MiB buffers equals sequential digests.
    #[test]
    fn concurrent_hash_file_hex_matches_sequential() {
        use md5::Md5;
        use sha2::{Digest, Sha256};

        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "pst_writer_hash_unit_{}_{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        // Multi-chunk payload (>1 MiB buffer) + known short tail.
        let mut content = vec![0xABu8; 1024 * 1024 + 4096];
        content.extend_from_slice(b"0079-hash-known-tail");
        fs::write(&path, &content).expect("write temp");

        let (sha_conc, md5_conc) = hash_file_hex(&path).expect("concurrent hash");

        let mut sha = Sha256::new();
        sha.update(&content);
        let mut md5 = Md5::new();
        md5.update(&content);
        let sha_seq = digest_to_hex(sha.finalize());
        let md5_seq = digest_to_hex(md5.finalize());

        let _ = fs::remove_file(&path);
        assert_eq!(sha_conc, sha_seq, "SHA-256 concurrent == sequential");
        assert_eq!(md5_conc, md5_seq, "MD5 concurrent == sequential");
    }

    /// Pre-0079 shape: sequential SHA+MD5 over the same 1 MiB buffer (no concurrent scope).
    fn hash_file_hex_sequential_1mib(path: &Path) -> Result<(String, String)> {
        use md5::Md5;
        use sha2::{Digest, Sha256};
        let mut file = File::open(path)?;
        let mut sha = Sha256::new();
        let mut md5 = Md5::new();
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];
            sha.update(chunk);
            md5.update(chunk);
        }
        Ok((digest_to_hex(sha.finalize()), digest_to_hex(md5.finalize())))
    }

    /// 0079 Phase 5 measurement: sequential vs concurrent digests on a multi-MiB file.
    ///
    /// Records both walls (eprintln for baseline capture) and asserts digests match.
    /// This is the isolatable constant-factor measurement parent PhaseTimings cannot
    /// provide (parent had no phase timers).
    #[test]
    fn concurrent_vs_sequential_hash_timing_32mib() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "pst_writer_hash_timing_{}_{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        // 32 MiB: large enough that dual digests are measurable, small enough for CI.
        let size = 32 * 1024 * 1024;
        let mut content = vec![0u8; size];
        for (i, b) in content.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        fs::write(&path, &content).expect("write 32MiB");

        // Warm once so cold-cache noise is reduced.
        let _ = hash_file_hex_sequential_1mib(&path).expect("warm seq");
        let _ = hash_file_hex(&path).expect("warm conc");

        let t0 = std::time::Instant::now();
        let (sha_s, md5_s) = hash_file_hex_sequential_1mib(&path).expect("seq");
        let seq_ms = t0.elapsed().as_millis();

        let t1 = std::time::Instant::now();
        let (sha_c, md5_c) = hash_file_hex(&path).expect("conc");
        let conc_ms = t1.elapsed().as_millis();

        let _ = fs::remove_file(&path);
        assert_eq!(sha_s, sha_c, "SHA-256 seq == concurrent");
        assert_eq!(md5_s, md5_c, "MD5 seq == concurrent");
        // Evidence for baseline.md / review.md (captured with --nocapture).
        eprintln!("0079_hash_timing size_mib=32 sequential_ms={seq_ms} concurrent_ms={conc_ms}");
        // Sanity: both paths finish; do not assert concurrent is always faster
        // (thread spawn can lose on small/CPU-noisy hosts). Equality of digests
        // is the correctness gate; timing is the measurement artifact.
        assert!(seq_ms < 60_000, "seq hash should finish under 60s on 32MiB");
        assert!(
            conc_ms < 60_000,
            "conc hash should finish under 60s on 32MiB"
        );
    }

    /// 0080 §3.11: PidTagDisplayCc is written; non-empty display_bcc increments dropped.
    #[test]
    fn display_cc_written_and_bcc_counted_dropped() {
        use dedup_engine::integrity::RecoverableIntegrity;
        use dedup_engine::keepset::{CanonicalMessage, MessageLocus};

        let mut canonical = CanonicalMessage {
            locus: MessageLocus {
                source_path: "C:/fake/source.pst".into(),
                source_pst: "source.pst".into(),
                folder_path: "Inbox".into(),
                nid: 1,
                is_orphaned: false,
            },
            message_id: Some("<cc@example.com>".into()),
            subject: Some("CC test".into()),
            sender: Some("alice@example.com".into()),
            display_to: Some("bob@example.com".into()),
            display_cc: Some("carol@example.com".into()),
            display_bcc: Some("secret@example.com".into()),
            recipients: Vec::new(),
            message_flags: None,
            submit_time: Some(0x01D5B035EDA780_i64),
            size: None,
            message_class: None,
            body_plain: Some("hi".into()),
            body_html: None,
            attachments: Vec::new(),
            fidelity: RecoverableIntegrity::clean(),
            message_id_norm: None,
            content_hash: [0u8; 32],
            edrm_mih_hex: None,
            body_incomplete: false,
            body_unavailable: false,
        };
        let (write_msg, dropped) = from_canonical_message(&canonical);
        assert_eq!(dropped, 1, "non-empty BCC counts as dropped");
        assert_eq!(write_msg.display_cc.as_deref(), Some("carol@example.com"));

        // Empty BCC does not count.
        canonical.display_bcc = Some("   ".into());
        let (_, dropped0) = from_canonical_message(&canonical);
        assert_eq!(dropped0, 0);

        let path =
            std::env::temp_dir().join(format!("pst_writer_display_cc_{}.pst", std::process::id()));
        let _ = fs::remove_file(&path);
        write_unicode_pst(&path, vec![write_msg], &[], &WritePstOpts::default()).expect("write");

        let mut pst = pst_reader::PstFile::open(&path).expect("open");
        let folders = pst.folders().expect("folders");
        let nid = folders
            .iter()
            .find(|f| !f.message_nids.is_empty())
            .expect("folder with msg")
            .message_nids[0];
        let extract = pst.read_message_extract(nid).expect("extract");
        assert_eq!(
            extract.display_cc.as_deref(),
            Some("carol@example.com"),
            "PidTagDisplayCc must round-trip"
        );
        // BCC not written.
        assert!(
            extract
                .display_bcc
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty(),
            "BCC must not be written"
        );
        let _ = fs::remove_file(&path);
    }

    // ── 0087 store RecordKey pure derivation ────────────────────────────────

    fn hex16(key: &[u8; 16]) -> String {
        key.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn store_record_key_golden_empty_volume() {
        // Empty message list → SHA-256("") as volume_local.
        let volume_local = volume_local_fingerprint_from_messages(std::iter::empty());
        let expected_empty_sha: [u8; 32] = {
            let d = Sha256::digest([]);
            let mut out = [0u8; 32];
            out.copy_from_slice(&d);
            out
        };
        assert_eq!(volume_local, expected_empty_sha);

        let content = resolve_content_fingerprint(None, 0, 0, &volume_local);
        assert_eq!(content, volume_local);
        let key = derive_store_record_key(0, 0, &content);

        // Independent re-derivation of the §2.6 preimage.
        let mut hasher = Sha256::new();
        hasher.update(STORE_RECORD_KEY_DOMAIN);
        hasher.update(STORE_RECORD_KEY_ALGO_VERSION.to_le_bytes());
        hasher.update(0u32.to_le_bytes());
        hasher.update(0u64.to_le_bytes());
        hasher.update(volume_local);
        let digest = hasher.finalize();
        let mut expected = [0u8; 16];
        expected.copy_from_slice(&digest[..16]);
        apply_all_zero_guard(&mut expected);
        assert_eq!(key, expected);
        assert_ne!(key, [0u8; 16]);

        // Locked golden hex for empty volume / algo v1 / volume_index 0.
        // Re-pin only when intentionally changing the preimage formula.
        assert_eq!(hex16(&key), "b0a1ca291fa355c1ebb3f9ec13a7b879");
    }

    #[test]
    fn store_record_key_differs_by_content_and_volume_index() {
        let m1 = WriteMessage {
            message_id: Some("<a@ex.com>".into()),
            subject: "one".into(),
            submit_time: Some(100),
            source_folder_path: Some("Inbox".into()),
            ..WriteMessage::default()
        };
        let m2 = WriteMessage {
            message_id: Some("<b@ex.com>".into()),
            subject: "two".into(),
            submit_time: Some(200),
            source_folder_path: Some("Sent".into()),
            ..WriteMessage::default()
        };
        let fp1 = volume_local_fingerprint_from_messages([&m1]);
        let fp2 = volume_local_fingerprint_from_messages([&m2]);
        assert_ne!(fp1, fp2);

        let k1 = derive_store_record_key(0, 1, &resolve_content_fingerprint(None, 0, 1, &fp1));
        let k2 = derive_store_record_key(0, 1, &resolve_content_fingerprint(None, 0, 1, &fp2));
        assert_ne!(k1, k2, "different content → different keys");

        let k_v0 = derive_store_record_key(0, 1, &resolve_content_fingerprint(None, 0, 1, &fp1));
        let k_v1 = derive_store_record_key(1, 1, &resolve_content_fingerprint(None, 1, 1, &fp1));
        assert_ne!(k_v0, k_v1, "volume_index 0 vs 1 → different keys");
    }

    #[test]
    fn store_record_key_seed_material_changes_fingerprint() {
        let m = WriteMessage {
            message_id: Some("<s@ex.com>".into()),
            subject: "seed".into(),
            ..WriteMessage::default()
        };
        let local = volume_local_fingerprint_from_messages([&m]);
        let seed_a = [0x11u8; 32];
        let seed_b = [0x22u8; 32];
        let c_a = resolve_content_fingerprint(Some(&seed_a), 0, 1, &local);
        let c_b = resolve_content_fingerprint(Some(&seed_b), 0, 1, &local);
        let c_none = resolve_content_fingerprint(None, 0, 1, &local);
        assert_ne!(c_a, c_b);
        assert_ne!(c_a, c_none);
        let k_a = derive_store_record_key(0, 1, &c_a);
        let k_a2 = derive_store_record_key(0, 1, &c_a);
        assert_eq!(k_a, k_a2, "same seed + volume → same key");
        assert_ne!(
            derive_store_record_key(0, 1, &c_b),
            k_a,
            "different seeds → different keys"
        );
    }

    #[test]
    fn store_record_key_length_prefix_boundary_no_collision() {
        // Under naive concat mid||subject both yield "ab". Length-prefix must
        // distinguish (mid="ab", subject="") from (mid="a", subject="b").
        let a = WriteMessage {
            message_id: Some("ab".into()),
            subject: String::new(),
            ..WriteMessage::default()
        };
        let b = WriteMessage {
            message_id: Some("a".into()),
            subject: "b".into(),
            ..WriteMessage::default()
        };
        let naive_a = format!(
            "{}{}",
            a.message_id.as_deref().unwrap_or(""),
            a.subject.as_str()
        );
        let naive_b = format!(
            "{}{}",
            b.message_id.as_deref().unwrap_or(""),
            b.subject.as_str()
        );
        assert_eq!(naive_a, naive_b, "setup: naive concat collides");
        let fa = volume_local_fingerprint_from_messages([&a]);
        let fb = volume_local_fingerprint_from_messages([&b]);
        assert_ne!(
            fa, fb,
            "length-prefixed volume_local must not collide on boundary case"
        );
        let ka = derive_store_record_key(0, 1, &fa);
        let kb = derive_store_record_key(0, 1, &fb);
        assert_ne!(ka, kb);
    }

    #[test]
    fn store_record_key_all_zero_guard() {
        // Force all-zero truncated digest path via direct guard unit.
        let mut key = [0u8; 16];
        apply_all_zero_guard(&mut key);
        assert_eq!(key[0], 0x5A);
        assert!(key[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn job_store_key_material_from_loci_stable() {
        let a = job_store_key_material_from_loci([("C:\\a.pst", "Inbox", 42u64)]);
        let b = job_store_key_material_from_loci([("C:\\a.pst", "Inbox", 42u64)]);
        let c = job_store_key_material_from_loci([("C:\\a.pst", "Inbox", 43u64)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
