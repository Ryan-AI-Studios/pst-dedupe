//! B-tree traversal for NBT (Node BTree) and BBT (Block BTree).
//!
//! Both trees are stored as multi-level B-trees of 512-byte pages.
//! We traverse them fully on file open and build in-memory indexes
//! (HashMap-based) for O(1) lookups during message processing.

use byteorder::{ByteOrder, LittleEndian};
use std::collections::HashMap;
use std::io::{Read, Seek};

use super::nid::NodeId;
use super::page::{self, RawPage};
use super::BlockId;
use crate::error::Result;
use crate::header::{Bref, PstHeader};

/// NBT leaf entry (NBTENTRY, §2.2.2.7.7.4) — 32 bytes for Unicode.
#[derive(Debug, Clone)]
pub struct NbtEntry {
    /// Node ID (key).
    pub nid: NodeId,
    /// BID of the node's data block (or XBLOCK/XXBLOCK root).
    pub bid_data: BlockId,
    /// BID of the node's subnode BTree block (0 if none).
    pub bid_sub: BlockId,
    /// Parent node ID.
    pub nid_parent: u32,
}

/// BBT leaf entry (BBTENTRY, §2.2.2.7.7.3) — 24 bytes for Unicode.
#[derive(Debug, Clone)]
pub struct BbtEntry {
    /// Block reference: BID + absolute file offset.
    pub bref: Bref,
    /// Size of data in the block (bytes, before decryption).
    pub cb: u16,
    /// Reference count.
    pub c_ref: u16,
}

/// In-memory Node BTree index.
#[derive(Debug, Clone)]
pub struct NbtIndex {
    entries: HashMap<u64, NbtEntry>,
}

/// In-memory Block BTree index.
#[derive(Debug, Clone)]
pub struct BbtIndex {
    entries: HashMap<u64, BbtEntry>,
}

impl NbtIndex {
    /// Build the index by traversing the entire NBT from the root page.
    pub fn build<R: Read + Seek>(reader: &mut R, header: &PstHeader) -> Result<Self> {
        let mut entries = HashMap::new();
        let root_offset = header.root.bref_nbt.ib;

        if root_offset == 0 {
            return Ok(Self { entries });
        }

        Self::traverse(reader, root_offset, &mut entries)?;
        Ok(Self { entries })
    }

    fn traverse<R: Read + Seek>(
        reader: &mut R,
        page_offset: u64,
        entries: &mut HashMap<u64, NbtEntry>,
    ) -> Result<()> {
        let page = RawPage::read_at(reader, page_offset)?;
        page.validate(page::ptype::NBT)?;

        let hdr = page.bt_header();
        let data = page.entries_data();

        if hdr.c_level == 0 {
            // Leaf level — parse NBTENTRY records (32 bytes each for Unicode)
            for i in 0..hdr.c_entries as usize {
                let offset = i * 32;
                if offset + 32 > data.len() {
                    break;
                }
                let entry_data = &data[offset..offset + 32];

                let nid = LittleEndian::read_u64(&entry_data[0..8]);
                let bid_data = LittleEndian::read_u64(&entry_data[8..16]);
                let bid_sub = LittleEndian::read_u64(&entry_data[16..24]);
                let nid_parent = LittleEndian::read_u32(&entry_data[24..28]);

                entries.insert(
                    nid,
                    NbtEntry {
                        nid: NodeId(nid),
                        bid_data: BlockId(bid_data),
                        bid_sub: BlockId(bid_sub),
                        nid_parent,
                    },
                );
            }
        } else {
            // Intermediate level — entries are key(8) + BREF(16) = 24 bytes
            for i in 0..hdr.c_entries as usize {
                let offset = i * 24;
                if offset + 24 > data.len() {
                    break;
                }
                let entry_data = &data[offset..offset + 24];

                // key is NID (8 bytes), then BREF: bid(8) + ib(8)
                let _key = LittleEndian::read_u64(&entry_data[0..8]);
                let _child_bid = LittleEndian::read_u64(&entry_data[8..16]);
                let child_ib = LittleEndian::read_u64(&entry_data[16..24]);

                Self::traverse(reader, child_ib, entries)?;
            }
        }

        Ok(())
    }

    /// Look up a node by NID.
    pub fn get(&self, nid: NodeId) -> Option<&NbtEntry> {
        self.entries.get(&nid.0)
    }

    /// Number of nodes in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate all entries.
    pub fn iter(&self) -> impl Iterator<Item = (&u64, &NbtEntry)> {
        self.entries.iter()
    }

    /// Test-only constructor: build an index directly from entries, bypassing
    /// real on-disk B-tree page parsing. Used by unit tests elsewhere in this
    /// crate (e.g. `ltp::pc`) that need a minimal synthetic NBT without
    /// constructing full MS-PST page/trailer bytes.
    #[cfg(test)]
    pub(crate) fn from_entries_for_test(entries: HashMap<u64, NbtEntry>) -> Self {
        Self { entries }
    }
}

impl BbtIndex {
    /// Build the index by traversing the entire BBT from the root page.
    pub fn build<R: Read + Seek>(reader: &mut R, header: &PstHeader) -> Result<Self> {
        let mut entries = HashMap::new();
        let root_offset = header.root.bref_bbt.ib;

        if root_offset == 0 {
            return Ok(Self { entries });
        }

        Self::traverse(reader, root_offset, &mut entries)?;
        Ok(Self { entries })
    }

    fn traverse<R: Read + Seek>(
        reader: &mut R,
        page_offset: u64,
        entries: &mut HashMap<u64, BbtEntry>,
    ) -> Result<()> {
        let page = RawPage::read_at(reader, page_offset)?;
        page.validate(page::ptype::BBT)?;

        let hdr = page.bt_header();
        let data = page.entries_data();

        if hdr.c_level == 0 {
            // Leaf level — parse BBTENTRY records (24 bytes each for Unicode)
            for i in 0..hdr.c_entries as usize {
                let offset = i * 24;
                if offset + 24 > data.len() {
                    break;
                }
                let entry_data = &data[offset..offset + 24];

                let bid = LittleEndian::read_u64(&entry_data[0..8]);
                let ib = LittleEndian::read_u64(&entry_data[8..16]);
                let cb = LittleEndian::read_u16(&entry_data[16..18]);
                let c_ref = LittleEndian::read_u16(&entry_data[18..20]);

                entries.insert(
                    bid,
                    BbtEntry {
                        bref: Bref { bid, ib },
                        cb,
                        c_ref,
                    },
                );
            }
        } else {
            // Intermediate level — entries are key(8) + BREF(16) = 24 bytes
            for i in 0..hdr.c_entries as usize {
                let offset = i * 24;
                if offset + 24 > data.len() {
                    break;
                }
                let entry_data = &data[offset..offset + 24];

                let _key = LittleEndian::read_u64(&entry_data[0..8]);
                let _child_bid = LittleEndian::read_u64(&entry_data[8..16]);
                let child_ib = LittleEndian::read_u64(&entry_data[16..24]);

                Self::traverse(reader, child_ib, entries)?;
            }
        }

        Ok(())
    }

    /// Look up a block by BID.
    pub fn get(&self, bid: BlockId) -> Option<&BbtEntry> {
        self.entries.get(&bid.0)
    }

    /// Number of blocks in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Test-only constructor: build an index directly from entries, bypassing
    /// real on-disk B-tree page parsing. Used by unit tests elsewhere in this
    /// crate (e.g. `ltp::pc`) that need a minimal synthetic BBT without
    /// constructing full MS-PST page/trailer bytes.
    #[cfg(test)]
    pub(crate) fn from_entries_for_test(entries: HashMap<u64, BbtEntry>) -> Self {
        Self { entries }
    }
}
