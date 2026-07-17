//! Heap-on-Node (HN) — MS-PST §2.3.1
//!
//! An HN treats a node's data (potentially spanning multiple blocks via XBLOCK)
//! as a heap with fixed-size allocation pages. Each allocation is addressed by
//! a Heap ID (HID).

use crate::error::{PstError, Result};
use byteorder::{ByteOrder, LittleEndian};

/// Heap ID — 4 bytes addressing an allocation within the HN.
///
/// ```text
/// Bits 0-4:   hidType (must be 0)
/// Bits 5-15:  hidIndex (1-based allocation index within the block)
/// Bits 16-31: hidBlockIndex (0-based index of the data block)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hid(pub u32);

impl Hid {
    pub fn hid_type(self) -> u8 {
        (self.0 & 0x1F) as u8
    }

    pub fn hid_index(self) -> u16 {
        ((self.0 >> 5) & 0x7FF) as u16
    }

    pub fn hid_block_index(self) -> u16 {
        ((self.0 >> 16) & 0xFFFF) as u16
    }

    pub fn is_null(self) -> bool {
        self.0 == 0
    }
}

/// Parsed HN header (HNHDR, at start of first data block).
#[derive(Debug)]
pub struct HnHeader {
    /// Offset to HN page map within this block.
    pub ib_hnpm: u16,
    /// Signature byte (must be 0xEC).
    pub b_sig: u8,
    /// Client signature: 0x6C = PC (via BTH), 0x7C = TC, 0xBC = BTH
    pub b_client_sig: u8,
    /// HID of the client's root structure (e.g., BTH header or TC info).
    pub hid_user_root: Hid,
    /// Fill level (4 bytes, can be ignored for read-only).
    pub rgb_fill_level: u32,
}

/// An HN heap — provides HID-based access to allocations within node data.
pub struct Heap {
    /// The complete node data (assembled from blocks).
    data: Vec<u8>,
    /// Size of each "block" within the data for HN page map purposes.
    /// For single-block nodes this is the whole data length.
    /// For multi-block nodes (XBLOCK), each block is up to 8176 bytes.
    block_size: usize,
    /// Parsed header.
    pub header: HnHeader,
}

impl Heap {
    /// Parse an HN heap from assembled node data.
    ///
    /// `data` is the complete decrypted node data (from NDB block reading).
    /// `block_size` is the size of individual blocks (8176 for multi-block, or data.len()
    /// for single-block nodes).
    pub fn parse(data: Vec<u8>, block_size: usize) -> Result<Self> {
        if data.len() < 12 {
            return Err(PstError::DataTruncated {
                needed: 12,
                available: data.len(),
            });
        }

        let ib_hnpm = LittleEndian::read_u16(&data[0..2]);
        let b_sig = data[2];
        let b_client_sig = data[3];
        let hid_user_root = Hid(LittleEndian::read_u32(&data[4..8]));
        let rgb_fill_level = LittleEndian::read_u32(&data[8..12]);

        if b_sig != 0xEC {
            return Err(PstError::InvalidHnSignature(b_sig));
        }

        let header = HnHeader {
            ib_hnpm,
            b_sig,
            b_client_sig,
            hid_user_root,
            rgb_fill_level,
        };

        Ok(Self {
            data,
            block_size,
            header,
        })
    }

    /// Resolve an HID to a byte slice within the heap.
    pub fn get(&self, hid: Hid) -> Result<&[u8]> {
        if hid.is_null() {
            return Ok(&[]);
        }

        if hid.hid_type() != 0 {
            return Err(PstError::InvalidHid(hid.0));
        }

        let block_index = hid.hid_block_index() as usize;
        let alloc_index = hid.hid_index() as usize;

        if alloc_index == 0 {
            return Err(PstError::InvalidHid(hid.0));
        }

        // Find the start of this block within the data
        let block_start = block_index * self.block_size;
        if block_start >= self.data.len() {
            return Err(PstError::InvalidHid(hid.0));
        }

        let block_end = std::cmp::min(block_start + self.block_size, self.data.len());
        let block_data = &self.data[block_start..block_end];

        // Find the HN page map for this block
        let hnpm_offset = if block_index == 0 {
            // First block: ib_hnpm is in the header
            self.header.ib_hnpm as usize
        } else {
            // Subsequent blocks: HNPAGEMAP is at offset 0..2 of the block
            // (HNPAGEHDR: ibHnpm at offset 0)
            if block_data.len() < 2 {
                return Err(PstError::DataTruncated {
                    needed: 2,
                    available: block_data.len(),
                });
            }
            LittleEndian::read_u16(&block_data[0..2]) as usize
        };

        if hnpm_offset + 2 > block_data.len() {
            return Err(PstError::DataTruncated {
                needed: hnpm_offset + 2,
                available: block_data.len(),
            });
        }

        // HNPAGEMAP (MS-PST §2.3.1.5): cAlloc(2) + cFree(2) + rgibAlloc[(cAlloc+1)×2]
        if hnpm_offset + 4 > block_data.len() {
            return Err(PstError::DataTruncated {
                needed: hnpm_offset + 4,
                available: block_data.len(),
            });
        }
        let c_alloc = LittleEndian::read_u16(&block_data[hnpm_offset..hnpm_offset + 2]) as usize;
        let _c_free = LittleEndian::read_u16(&block_data[hnpm_offset + 2..hnpm_offset + 4]) as usize;

        if alloc_index > c_alloc {
            return Err(PstError::InvalidHid(hid.0));
        }

        // rgibAlloc starts after cAlloc + cFree
        let rgib_start = hnpm_offset + 4;
        let offset_a = rgib_start + (alloc_index - 1) * 2;
        let offset_b = rgib_start + alloc_index * 2;

        if offset_b + 2 > block_data.len() {
            return Err(PstError::DataTruncated {
                needed: offset_b + 2,
                available: block_data.len(),
            });
        }

        let start = LittleEndian::read_u16(&block_data[offset_a..offset_a + 2]) as usize;
        let end = LittleEndian::read_u16(&block_data[offset_b..offset_b + 2]) as usize;

        if start > end || end > block_data.len() {
            return Err(PstError::DataTruncated {
                needed: end,
                available: block_data.len(),
            });
        }

        Ok(&block_data[start..end])
    }

    /// Get the complete node data (for TC row data access).
    pub fn raw_data(&self) -> &[u8] {
        &self.data
    }

    /// Block size for this heap.
    pub fn block_size(&self) -> usize {
        self.block_size
    }
}
