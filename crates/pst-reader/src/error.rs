use thiserror::Error;

pub type Result<T> = std::result::Result<T, PstError>;

#[derive(Debug, Error)]
pub enum PstError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid PST magic bytes: expected 0x2142444E (!BDN), got 0x{0:08X}")]
    InvalidMagic(u32),

    #[error("Unsupported ANSI PST (wVer={0}). Only Unicode PSTs (wVer >= 23) are supported.")]
    AnsiPstNotSupported(u16),

    #[error("Invalid client magic: expected 0x4D53 (SM), got 0x{0:04X}")]
    InvalidClientMagic(u16),

    #[error("Invalid page type: expected {expected}, got {actual}")]
    InvalidPageType { expected: u8, actual: u8 },

    #[error("Page type mismatch: ptype={ptype}, ptypeRepeat={ptype_repeat}")]
    PageTypeMismatch { ptype: u8, ptype_repeat: u8 },

    #[error("CRC mismatch: computed=0x{computed:08X}, stored=0x{stored:08X}")]
    CrcMismatch { computed: u32, stored: u32 },

    #[error("Node not found: NID=0x{0:08X}")]
    NodeNotFound(u64),

    #[error("Block not found: BID=0x{0:016X}")]
    BlockNotFound(u64),

    #[error("Node 0x{0:08X} has no subnode BTree")]
    NoSubnodeBTree(u64),

    #[error("Invalid block type: expected {expected}, got {actual}")]
    InvalidBlockType { expected: u8, actual: u8 },

    #[error("Heap-on-Node signature invalid: expected 0xEC, got 0x{0:02X}")]
    InvalidHnSignature(u8),

    #[error("BTree-on-Heap type invalid: expected 0xB5, got 0x{0:02X}")]
    InvalidBthType(u8),

    #[error("Invalid HID: 0x{0:08X}")]
    InvalidHid(u32),

    #[error("Property not found: tag=0x{0:04X}")]
    PropertyNotFound(u16),

    #[error("Property type mismatch: tag=0x{tag:04X}, expected {expected}, got {actual}")]
    PropertyTypeMismatch { tag: u16, expected: &'static str, actual: u16 },

    #[error("Data truncated: needed {needed} bytes, got {available}")]
    DataTruncated { needed: usize, available: usize },

    #[error("Subnode not found: NID=0x{0:08X}")]
    SubnodeNotFound(u64),

    #[error("Unsupported encryption method: {0}")]
    UnsupportedCryptMethod(u8),

    #[error("Invalid UTF-16 string data")]
    InvalidUtf16,
}
