//! B-tree traversal for NBT (Node BTree) and BBT (Block BTree).
//!
//! Both trees are stored as multi-level B-trees of 512-byte pages.
//! We traverse them fully on file open and build in-memory indexes
//! (HashMap-based) for O(1) lookups during message processing.

use byteorder::{ByteOrder, LittleEndian};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use super::nid::NodeId;
use super::page::{self, RawPage};
use super::BlockId;
use crate::error::{PstError, Result};
use crate::header::{Bref, PstHeader};

/// MS-PST NBT/BBT trees are shallow; deeper walks indicate corruption or cycles.
pub const MAX_BTREE_DEPTH: u32 = 32;

/// Record a visit to a B-tree page; fail closed on cycle or excessive depth.
///
/// Used by NBT/BBT traversal and unit-tested without full page fixtures.
pub(crate) fn enter_btree_page(
    page_offset: u64,
    depth: u32,
    visited: &mut HashSet<u64>,
) -> Result<()> {
    if depth > MAX_BTREE_DEPTH {
        return Err(PstError::ResourceLimit(format!(
            "B-tree depth {depth} exceeds max {MAX_BTREE_DEPTH} at offset 0x{page_offset:X}"
        )));
    }
    if !visited.insert(page_offset) {
        return Err(PstError::BtreeCycle { page_offset });
    }
    Ok(())
}

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

        let mut visited = HashSet::new();
        Self::traverse(reader, root_offset, 0, &mut visited, &mut entries)?;
        Ok(Self { entries })
    }

    fn traverse<R: Read + Seek>(
        reader: &mut R,
        page_offset: u64,
        depth: u32,
        visited: &mut HashSet<u64>,
        entries: &mut HashMap<u64, NbtEntry>,
    ) -> Result<()> {
        enter_btree_page(page_offset, depth, visited)?;

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

                Self::traverse(reader, child_ib, depth + 1, visited, entries)?;
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

        let mut visited = HashSet::new();
        Self::traverse(reader, root_offset, 0, &mut visited, &mut entries)?;
        Ok(Self { entries })
    }

    fn traverse<R: Read + Seek>(
        reader: &mut R,
        page_offset: u64,
        depth: u32,
        visited: &mut HashSet<u64>,
        entries: &mut HashMap<u64, BbtEntry>,
    ) -> Result<()> {
        enter_btree_page(page_offset, depth, visited)?;

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

                Self::traverse(reader, child_ib, depth + 1, visited, entries)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::CryptMethod;
    use crate::header::{Bref, PstHeader, RootStructure};
    use std::io::Cursor;

    #[test]
    fn enter_btree_page_detects_cycle() {
        let mut visited = HashSet::new();
        enter_btree_page(0x1000, 0, &mut visited).expect("first visit");
        let err = enter_btree_page(0x1000, 1, &mut visited).expect_err("cycle");
        assert!(matches!(
            err,
            PstError::BtreeCycle {
                page_offset: 0x1000
            }
        ));
    }

    #[test]
    fn enter_btree_page_depth_limit() {
        let mut visited = HashSet::new();
        let err = enter_btree_page(0x2000, MAX_BTREE_DEPTH + 1, &mut visited).expect_err("depth");
        assert!(matches!(err, PstError::ResourceLimit(_)));
    }

    #[test]
    fn enter_btree_page_allows_distinct_offsets() {
        let mut visited = HashSet::new();
        for i in 0..8u64 {
            enter_btree_page(0x1000 + i * 512, i as u32, &mut visited).expect("visit");
        }
        assert_eq!(visited.len(), 8);
    }

    /// Fabricate a 512-byte intermediate B-tree page whose sole child BREF
    /// points back at itself (cycle). CRC is warning-only in `validate`, so
    /// trailer CRC may be zero.
    fn synthetic_self_cycle_page(ptype: u8, page_offset: u64) -> [u8; page::PAGE_SIZE] {
        let mut data = [0u8; page::PAGE_SIZE];
        // Intermediate entry at index 0: key(8) + bid(8) + ib(8) = 24 bytes.
        // child_ib = page_offset → traverse re-enters the same page → BtreeCycle.
        data[16..24].copy_from_slice(&page_offset.to_le_bytes());
        // BT page header at 488..492
        data[488] = 1; // c_entries
        data[489] = 20; // c_ent_max (unused by parse)
        data[490] = 8; // cb_ent_key
        data[491] = 1; // c_level > 0 → intermediate
                       // Trailer at 496..512
        data[496] = ptype;
        data[497] = ptype; // ptype_repeat
                           // w_sig, dw_crc, bid left zero — CRC is warning-only
        data
    }

    fn header_with_roots(nbt_ib: u64, bbt_ib: u64) -> PstHeader {
        PstHeader {
            version: 23,
            ver_client: 0,
            crypt_method: CryptMethod::None,
            root: RootStructure {
                ib_file_eof: 1024,
                ib_amap_last: 0,
                cb_amap_free: 0,
                bref_nbt: Bref { bid: 1, ib: nbt_ib },
                bref_bbt: Bref { bid: 2, ib: bbt_ib },
                f_amap_valid: false,
            },
            bid_next_b: 0,
        }
    }

    /// Production path: `NbtIndex::build` must surface `BtreeCycle` (not hang).
    #[test]
    fn nbt_build_detects_self_cycle_page() {
        const ROOT_IB: u64 = 512;
        let page = synthetic_self_cycle_page(page::ptype::NBT, ROOT_IB);
        // File layout: 512 zero pad + one page at 512
        let mut file = vec![0u8; ROOT_IB as usize];
        file.extend_from_slice(&page);
        let mut cursor = Cursor::new(file);
        let header = header_with_roots(ROOT_IB, 0);
        let err = NbtIndex::build(&mut cursor, &header).expect_err("cycle");
        assert!(
            matches!(
                err,
                PstError::BtreeCycle {
                    page_offset: ROOT_IB
                }
            ),
            "expected BtreeCycle at {ROOT_IB:#x}, got {err:?}"
        );
    }

    /// Production path: `BbtIndex::build` must surface `BtreeCycle` (not hang).
    #[test]
    fn bbt_build_detects_self_cycle_page() {
        const ROOT_IB: u64 = 512;
        let page = synthetic_self_cycle_page(page::ptype::BBT, ROOT_IB);
        let mut file = vec![0u8; ROOT_IB as usize];
        file.extend_from_slice(&page);
        let mut cursor = Cursor::new(file);
        let header = header_with_roots(0, ROOT_IB);
        let err = BbtIndex::build(&mut cursor, &header).expect_err("cycle");
        assert!(
            matches!(
                err,
                PstError::BtreeCycle {
                    page_offset: ROOT_IB
                }
            ),
            "expected BtreeCycle at {ROOT_IB:#x}, got {err:?}"
        );
    }
}
