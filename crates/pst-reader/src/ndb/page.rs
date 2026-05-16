//! NDB Page reading (MS-PST §2.2.2.7).
//!
//! All pages are 512 bytes. Each ends with a 16-byte page trailer (Unicode).
//! Pages are used for B-tree nodes (NBT, BBT) and allocation maps.

use std::io::{Read, Seek, SeekFrom};
use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::{PstError, Result};

/// Size of a page in bytes.
pub const PAGE_SIZE: usize = 512;

/// Size of the Unicode page trailer.
pub const PAGE_TRAILER_SIZE: usize = 16;

/// Usable data bytes per page (before trailer).
pub const PAGE_DATA_SIZE: usize = PAGE_SIZE - PAGE_TRAILER_SIZE;

/// Page types (ptype field in trailer).
pub mod ptype {
    pub const BBT: u8 = 0x80;
    pub const NBT: u8 = 0x81;
    pub const FMAP: u8 = 0x82;
    pub const PMAP: u8 = 0x83;
    pub const AMAP: u8 = 0x84;
    pub const FPMAP: u8 = 0x85;
    pub const DLIST: u8 = 0x86;
}

/// A raw 512-byte page read from disk.
pub struct RawPage {
    /// Full 512 bytes.
    pub data: [u8; PAGE_SIZE],
}

/// Parsed page trailer (last 16 bytes of a Unicode page).
#[derive(Debug)]
pub struct PageTrailer {
    pub ptype: u8,
    pub ptype_repeat: u8,
    pub w_sig: u16,
    pub dw_crc: u32,
    pub bid: u64,
}

/// Parsed BTree page header (from the data portion).
#[derive(Debug)]
pub struct BtPageHeader {
    /// Number of entries in this page.
    pub c_entries: u8,
    /// Maximum entries this page can hold.
    pub c_ent_max: u8,
    /// Key size in bytes.
    pub cb_ent_key: u8,
    /// B-tree level: 0 = leaf, >0 = intermediate.
    pub c_level: u8,
}

impl RawPage {
    /// Read a page at the given absolute file offset.
    pub fn read_at<R: Read + Seek>(reader: &mut R, offset: u64) -> Result<Self> {
        reader.seek(SeekFrom::Start(offset))?;
        let mut data = [0u8; PAGE_SIZE];
        reader.read_exact(&mut data)?;
        Ok(Self { data })
    }

    /// Parse the page trailer (last 16 bytes).
    pub fn trailer(&self) -> PageTrailer {
        let t = &self.data[PAGE_DATA_SIZE..];
        PageTrailer {
            ptype: t[0],
            ptype_repeat: t[1],
            w_sig: u16::from_le_bytes([t[2], t[3]]),
            dw_crc: u32::from_le_bytes([t[4], t[5], t[6], t[7]]),
            bid: u64::from_le_bytes([t[8], t[9], t[10], t[11], t[12], t[13], t[14], t[15]]),
        }
    }

    /// Validate the page trailer.
    pub fn validate(&self, expected_ptype: u8) -> Result<()> {
        let trailer = self.trailer();

        if trailer.ptype != expected_ptype {
            return Err(PstError::InvalidPageType {
                expected: expected_ptype,
                actual: trailer.ptype,
            });
        }

        if trailer.ptype != trailer.ptype_repeat {
            return Err(PstError::PageTypeMismatch {
                ptype: trailer.ptype,
                ptype_repeat: trailer.ptype_repeat,
            });
        }

        // CRC validation can be added in Phase 6 (hardening)
        Ok(())
    }

    /// Parse the BTree page header from bytes 488..492.
    pub fn bt_header(&self) -> BtPageHeader {
        BtPageHeader {
            c_entries: self.data[488],
            c_ent_max: self.data[489],
            cb_ent_key: self.data[490],
            c_level: self.data[491],
        }
    }

    /// Get the entries region (bytes 0..488).
    pub fn entries_data(&self) -> &[u8] {
        &self.data[..488]
    }
}
