//! Node ID (NID) types and constants (MS-PST §2.2.2.1).
//!
//! A NID is a 4-byte value (in the NBT key, stored in 8 bytes for Unicode with upper
//! 4 bytes zeroed) with the low 5 bits indicating the node type:
//!
//! ```text
//! NID = (nidIndex << 5) | nidType
//! ```

/// A Node ID — the primary key for NDB lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u64);

impl NodeId {
    /// Extract the 5-bit node type.
    pub fn nid_type(self) -> NidType {
        NidType::from_raw((self.0 & 0x1F) as u8)
    }

    /// Extract the node index (bits 5+).
    pub fn nid_index(self) -> u32 {
        ((self.0 >> 5) & 0x07FF_FFFF) as u32
    }

    /// Construct a NID from type and index.
    pub fn new(nid_type: u8, nid_index: u32) -> Self {
        Self(((nid_index as u64) << 5) | (nid_type as u64))
    }

    /// Derive the hierarchy table NID for a folder NID.
    /// hierarchy NID type = 0x0D
    pub fn hierarchy_table(self) -> Self {
        Self((self.0 & !0x1F) | 0x0D)
    }

    /// Derive the contents table NID for a folder NID.
    /// contents NID type = 0x0E
    pub fn contents_table(self) -> Self {
        Self((self.0 & !0x1F) | 0x0E)
    }

    /// Derive the associated contents table NID.
    /// associated contents NID type = 0x0F
    pub fn associated_contents_table(self) -> Self {
        Self((self.0 & !0x1F) | 0x0F)
    }
}

/// Node types (low 5 bits of NID).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NidType {
    /// 0x00 — HID (Heap node)
    Hid,
    /// 0x01 — Internal node
    Internal,
    /// 0x02 — Normal Folder object
    NormalFolder,
    /// 0x03 — Search Folder object
    SearchFolder,
    /// 0x04 — Normal Message object
    NormalMessage,
    /// 0x05 — Attachment object
    Attachment,
    /// 0x06 — Search update queue
    SearchUpdateQueue,
    /// 0x07 — Search criteria object
    SearchCriteria,
    /// 0x08 — Associated message (FAI)
    AssocMessage,
    /// 0x0A — Contents table (internal)
    ContentsTableInternal,
    /// 0x0B — Receive folder table
    ReceiveFolderTable,
    /// 0x0C — Outgoing queue table
    OutgoingQueueTable,
    /// 0x0D — Hierarchy table
    HierarchyTable,
    /// 0x0E — Contents table
    ContentsTable,
    /// 0x0F — Associated contents table
    AssocContentsTable,
    /// 0x10 — Search contents table
    SearchContentsTable,
    /// 0x11 — Attachment table
    AttachmentTable,
    /// 0x12 — Recipient table
    RecipientTable,
    /// 0x13 — Search table index
    SearchTableIndex,
    /// 0x1F — LTP
    Ltp,
    /// Unknown type
    Unknown(u8),
}

impl NidType {
    pub fn from_raw(val: u8) -> Self {
        match val {
            0x00 => Self::Hid,
            0x01 => Self::Internal,
            0x02 => Self::NormalFolder,
            0x03 => Self::SearchFolder,
            0x04 => Self::NormalMessage,
            0x05 => Self::Attachment,
            0x06 => Self::SearchUpdateQueue,
            0x07 => Self::SearchCriteria,
            0x08 => Self::AssocMessage,
            0x0A => Self::ContentsTableInternal,
            0x0B => Self::ReceiveFolderTable,
            0x0C => Self::OutgoingQueueTable,
            0x0D => Self::HierarchyTable,
            0x0E => Self::ContentsTable,
            0x0F => Self::AssocContentsTable,
            0x10 => Self::SearchContentsTable,
            0x11 => Self::AttachmentTable,
            0x12 => Self::RecipientTable,
            0x13 => Self::SearchTableIndex,
            0x1F => Self::Ltp,
            other => Self::Unknown(other),
        }
    }
}

// ── Special NIDs (§2.4.1) ──────────────────────────────────────────────────

/// Message store root properties.
pub const NID_MESSAGE_STORE: NodeId = NodeId(0x21);

/// Named property map (PidTag → named property mapping).
pub const NID_NAME_TO_ID_MAP: NodeId = NodeId(0x61);

/// Root mailbox folder.
pub const NID_ROOT_FOLDER: NodeId = NodeId(0x122);

// ── MAPI Property Tags we care about ───────────────────────────────────────

/// PidTagDisplayName — folder/store display name (PtypString)
pub const PID_TAG_DISPLAY_NAME: u16 = 0x3001;

/// PidTagContentCount — number of messages in folder (PtypInteger32)
pub const PID_TAG_CONTENT_COUNT: u16 = 0x3602;

/// PidTagSubject (PtypString)
pub const PID_TAG_SUBJECT: u16 = 0x0037;

/// PidTagClientSubmitTime (PtypTime / FILETIME)
pub const PID_TAG_CLIENT_SUBMIT_TIME: u16 = 0x0039;

/// PidTagSenderEmailAddress (PtypString)
pub const PID_TAG_SENDER_EMAIL_ADDRESS: u16 = 0x0C1F;

/// PidTagSenderSmtpAddress (PtypString) — fallback sender
pub const PID_TAG_SENDER_SMTP_ADDRESS: u16 = 0x5D01;

/// PidTagInternetMessageId (PtypString) — primary dedup key
pub const PID_TAG_INTERNET_MESSAGE_ID: u16 = 0x1035;

/// PidTagBody (PtypString) — plain text body
pub const PID_TAG_BODY: u16 = 0x1000;

/// PidTagDisplayTo (PtypString) — formatted To recipients
pub const PID_TAG_DISPLAY_TO: u16 = 0x0E04;

/// PidTagMessageSize (PtypInteger32)
pub const PID_TAG_MESSAGE_SIZE: u16 = 0x0E08;

/// PidTagHasAttachments (PtypBoolean)
pub const PID_TAG_HAS_ATTACHMENTS: u16 = 0x0E1B;

/// PidTagAttachFilename (PtypString)
pub const PID_TAG_ATTACH_FILENAME: u16 = 0x3704;

/// PidTagAttachLongFilename (PtypString)
pub const PID_TAG_ATTACH_LONG_FILENAME: u16 = 0x3707;

/// PidTagAttachSize (PtypInteger32)
pub const PID_TAG_ATTACH_SIZE: u16 = 0x0E20;

/// PidTagNid — used in TC rows to reference child folder/message NIDs
pub const PID_TAG_LTP_ROW_ID: u16 = 0x67F2;
