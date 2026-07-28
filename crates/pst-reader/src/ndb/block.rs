//! Data block reading, multi-block assembly, and subnode BTree traversal.
//!
//! Data blocks are the actual storage units for node data. They are up to 8192 bytes
//! (8176 payload + 16 trailer for Unicode). Larger data is split across XBLOCK or
//! XXBLOCK chains.

use byteorder::{ByteOrder, LittleEndian};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};

use super::btree::BbtIndex;
use super::nid::NodeId;
use crate::crypto::{self, CryptMethod};
use crate::error::{PstError, Result};

/// Hard cap on XBLOCK/XXBLOCK assembly size (adversarial `lcbTotal` OOM guard).
pub const MAX_XBLOCK_ASSEMBLE: usize = 64 * 1024 * 1024;

/// Max recursion depth for subnode BTree walks (SL/SI blocks).
pub const MAX_SUBNODE_DEPTH: u32 = 32;

/// Record a visit to a subnode block; fail closed on cycle or excessive depth.
///
/// Mirrors [`super::btree::enter_btree_page`] for SL/SI walks. Unit-tested
/// without full page fixtures; production paths call this before reading.
pub(crate) fn enter_subnode_block(
    block_id: u64,
    depth: u32,
    visited: &mut HashSet<u64>,
) -> Result<()> {
    if depth > MAX_SUBNODE_DEPTH {
        return Err(PstError::ResourceLimit(format!(
            "subnode depth {depth} exceeds max {MAX_SUBNODE_DEPTH}"
        )));
    }
    if !visited.insert(block_id) {
        return Err(PstError::BtreeCycle {
            page_offset: block_id,
        });
    }
    Ok(())
}

/// A Block ID — references a data or internal block in the BBT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u64);

impl BlockId {
    /// Whether this is an internal block (XBLOCK, XXBLOCK, SLBLOCK, SIBLOCK).
    /// Bit 1 (second-lowest bit) indicates internal.
    pub fn is_internal(self) -> bool {
        self.0 & 0x02 != 0
    }

    /// Whether this BID is null (no block).
    pub fn is_null(self) -> bool {
        self.0 == 0
    }
}

/// Block trailer size (Unicode): 16 bytes.
const BLOCK_TRAILER_SIZE: usize = 16;

/// Parse and validate a block trailer, returning the BID.
/// The CRC covers the block data (everything before the trailer).
fn validate_block_trailer(raw: &[u8], data_len: usize) -> Result<BlockId> {
    let trailer_start = align64(data_len);
    if raw.len() < trailer_start + BLOCK_TRAILER_SIZE {
        return Err(PstError::DataTruncated {
            needed: trailer_start + BLOCK_TRAILER_SIZE,
            available: raw.len(),
        });
    }
    let trailer = &raw[trailer_start..trailer_start + BLOCK_TRAILER_SIZE];
    let stored_crc = LittleEndian::read_u32(&trailer[0..4]);
    let bid = LittleEndian::read_u64(&trailer[4..12]);

    // 0077: count + rate-limit via integrity_telemetry (still non-fatal).
    let computed = crc32fast::hash(&raw[..data_len]);
    if computed != stored_crc {
        crate::integrity_telemetry::note_block_crc(bid, computed, stored_crc);
    }

    Ok(BlockId(bid))
}

/// Read a raw block from disk, validate its CRC and BID consistency.
fn read_raw_block<R: Read + Seek>(reader: &mut R, bbt: &BbtIndex, bid: BlockId) -> Result<Vec<u8>> {
    let bbt_entry = bbt.get(bid).ok_or(PstError::BlockNotFound(bid.0))?;
    reader.seek(SeekFrom::Start(bbt_entry.bref.ib))?;
    let raw_size = align64(bbt_entry.cb as usize) + BLOCK_TRAILER_SIZE;
    let mut raw = vec![0u8; raw_size];
    reader.read_exact(&mut raw)?;

    crate::integrity_telemetry::note_block_read();
    let trailer_bid = validate_block_trailer(&raw, bbt_entry.cb as usize)?;
    if trailer_bid != bid {
        crate::integrity_telemetry::note_block_bid_mismatch(
            bbt_entry.bref.ib,
            bid.0,
            trailer_bid.0,
        );
    }

    Ok(raw)
}

/// Read all data for a BID, handling single blocks, XBLOCKs, and XXBLOCKs.
///
/// For external (non-internal) BIDs, reads and decrypts a single data block.
/// For internal BIDs, reads the XBLOCK/XXBLOCK structure and assembles all
/// referenced data blocks.
pub fn read_block_data<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    bid: BlockId,
    crypt: CryptMethod,
) -> Result<Vec<u8>> {
    if bid.is_null() {
        return Ok(Vec::new());
    }

    let raw = read_raw_block(reader, bbt, bid)?;
    let bbt_entry = bbt.get(bid).ok_or(PstError::BlockNotFound(bid.0))?;
    let payload = &raw[..bbt_entry.cb as usize];

    if !bid.is_internal() {
        // External data block — decrypt and return
        let mut data = payload.to_vec();
        crypto::decrypt_block(&mut data, crypt, bid.0);
        Ok(data)
    } else {
        // Internal block — check type
        // XBLOCK/XXBLOCK: btype=0x01; SLBLOCK/SIBLOCK: btype=0x02
        // Fail closed on truncated headers (cb < 2) — never panic on crafted PST.
        if payload.len() < 2 {
            return Err(PstError::DataTruncated {
                needed: 2,
                available: payload.len(),
            });
        }

        let btype = payload[0];
        let clevel = payload[1];

        match (btype, clevel) {
            (0x01, 0x01) => {
                // XBLOCK — references data blocks directly
                read_xblock_data(reader, bbt, payload, crypt)
            }
            (0x01, 0x02) => {
                // XXBLOCK — references XBLOCKs
                read_xxblock_data(reader, bbt, payload, crypt)
            }
            _ => Err(PstError::InvalidBlockType {
                expected: 0x01,
                actual: btype,
            }),
        }
    }
}

/// Reject attacker-controlled `lcbTotal` before any preallocation.
pub(crate) fn check_xblock_assemble_limit(lcb_total: usize) -> Result<()> {
    if lcb_total > MAX_XBLOCK_ASSEMBLE {
        return Err(PstError::ResourceLimit(format!(
            "xblock/xxblock lcbTotal {lcb_total} exceeds max {MAX_XBLOCK_ASSEMBLE}"
        )));
    }
    Ok(())
}

/// Read and assemble data from an XBLOCK (§2.2.2.8.3.1).
///
/// Layout: btype(1) + cLevel(1) + cEntries(2) + lcbTotal(4) + rgBIDs(8*cEntries)
fn read_xblock_data<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    xblock_data: &[u8],
    crypt: CryptMethod,
) -> Result<Vec<u8>> {
    if xblock_data.len() < 8 {
        return Err(PstError::DataTruncated {
            needed: 8,
            available: xblock_data.len(),
        });
    }

    let c_entries = LittleEndian::read_u16(&xblock_data[2..4]) as usize;
    let lcb_total = LittleEndian::read_u32(&xblock_data[4..8]) as usize;
    check_xblock_assemble_limit(lcb_total)?;

    let mut result = Vec::with_capacity(lcb_total);

    for i in 0..c_entries {
        let bid_offset = 8 + i * 8;
        if bid_offset + 8 > xblock_data.len() {
            break;
        }
        let child_bid = BlockId(LittleEndian::read_u64(
            &xblock_data[bid_offset..bid_offset + 8],
        ));

        // Each child is an external data block — read, validate, and decrypt
        let raw = read_raw_block(reader, bbt, child_bid)?;
        let bbt_entry = bbt
            .get(child_bid)
            .ok_or(PstError::BlockNotFound(child_bid.0))?;
        let mut payload = raw[..bbt_entry.cb as usize].to_vec();
        crypto::decrypt_block(&mut payload, crypt, child_bid.0);
        if result.len().saturating_add(payload.len()) > MAX_XBLOCK_ASSEMBLE {
            return Err(PstError::ResourceLimit(format!(
                "xblock assembled size exceeds max {MAX_XBLOCK_ASSEMBLE}"
            )));
        }
        result.extend_from_slice(&payload);
    }

    Ok(result)
}

/// Read and assemble data from an XXBLOCK (§2.2.2.8.3.2).
///
/// Same layout as XBLOCK but each child BID points to an XBLOCK, not a data block.
fn read_xxblock_data<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    xxblock_data: &[u8],
    crypt: CryptMethod,
) -> Result<Vec<u8>> {
    if xxblock_data.len() < 8 {
        return Err(PstError::DataTruncated {
            needed: 8,
            available: xxblock_data.len(),
        });
    }

    let c_entries = LittleEndian::read_u16(&xxblock_data[2..4]) as usize;
    let lcb_total = LittleEndian::read_u32(&xxblock_data[4..8]) as usize;
    check_xblock_assemble_limit(lcb_total)?;

    let mut result = Vec::with_capacity(lcb_total);

    for i in 0..c_entries {
        let bid_offset = 8 + i * 8;
        if bid_offset + 8 > xxblock_data.len() {
            break;
        }
        let child_bid = BlockId(LittleEndian::read_u64(
            &xxblock_data[bid_offset..bid_offset + 8],
        ));

        // Read the child XBLOCK (internal — no decryption)
        let raw = read_raw_block(reader, bbt, child_bid)?;
        let bbt_entry = bbt
            .get(child_bid)
            .ok_or(PstError::BlockNotFound(child_bid.0))?;
        let xblock_payload = &raw[..bbt_entry.cb as usize];
        let chunk = read_xblock_data(reader, bbt, xblock_payload, crypt)?;
        if result.len().saturating_add(chunk.len()) > MAX_XBLOCK_ASSEMBLE {
            return Err(PstError::ResourceLimit(format!(
                "xxblock assembled size exceeds max {MAX_XBLOCK_ASSEMBLE}"
            )));
        }
        result.extend_from_slice(&chunk);
    }

    Ok(result)
}

/// Read data from a subnode BTree for a specific sub-NID.
///
/// The subnode BTree is stored as SLBLOCK (leaf) or SIBLOCK (intermediate) blocks.
pub fn read_subnode_data<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    sub_bid: BlockId,
    target_nid: NodeId,
    crypt: CryptMethod,
) -> Result<Vec<u8>> {
    let mut visited = HashSet::new();
    read_subnode_data_at(reader, bbt, sub_bid, target_nid, crypt, 0, &mut visited)
}

fn read_subnode_data_at<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    sub_bid: BlockId,
    target_nid: NodeId,
    crypt: CryptMethod,
    depth: u32,
    visited: &mut HashSet<u64>,
) -> Result<Vec<u8>> {
    if sub_bid.is_null() {
        return Err(PstError::SubnodeNotFound(target_nid.0));
    }
    enter_subnode_block(sub_bid.0, depth, visited)?;

    // Read the subnode block
    let raw = read_raw_block(reader, bbt, sub_bid)?;
    let bbt_entry = bbt.get(sub_bid).ok_or(PstError::BlockNotFound(sub_bid.0))?;
    let payload = &raw[..bbt_entry.cb as usize];
    if payload.len() < 8 {
        return Err(PstError::DataTruncated {
            needed: 8,
            available: payload.len(),
        });
    }

    let btype = payload[0];
    let clevel = payload[1];
    let c_entries = LittleEndian::read_u16(&payload[2..4]) as usize;

    match (btype, clevel) {
        (0x02, 0x00) => {
            // SLBLOCK — leaf: entries are SLENTRY (24 bytes): nid(8) + bidData(8) + bidSub(8)
            for i in 0..c_entries {
                let offset = 8 + i * 24;
                if offset + 24 > payload.len() {
                    break;
                }
                let entry_nid = LittleEndian::read_u64(&payload[offset..offset + 8]);
                let entry_bid_data =
                    BlockId(LittleEndian::read_u64(&payload[offset + 8..offset + 16]));

                if entry_nid == target_nid.0 {
                    return read_block_data(reader, bbt, entry_bid_data, crypt);
                }
            }
            Err(PstError::SubnodeNotFound(target_nid.0))
        }
        (0x02, 0x01) => {
            // SIBLOCK — intermediate: entries are nid(8) + bid(8) = 16 bytes
            for i in 0..c_entries {
                let offset = 8 + i * 16;
                if offset + 16 > payload.len() {
                    break;
                }
                let _entry_nid = LittleEndian::read_u64(&payload[offset..offset + 8]);
                let child_bid = BlockId(LittleEndian::read_u64(&payload[offset + 8..offset + 16]));

                // Try this child — if the sub-NID is found, return
                match read_subnode_data_at(
                    reader,
                    bbt,
                    child_bid,
                    target_nid,
                    crypt,
                    depth + 1,
                    visited,
                ) {
                    Ok(data) => return Ok(data),
                    Err(PstError::SubnodeNotFound(_)) => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(PstError::SubnodeNotFound(target_nid.0))
        }
        _ => Err(PstError::InvalidBlockType {
            expected: 0x02,
            actual: btype,
        }),
    }
}

/// List all entries in a subnode BTree (used by TC for row data iteration).
pub fn list_subnode_entries<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    sub_bid: BlockId,
) -> Result<Vec<SubnodeEntry>> {
    let mut visited = HashSet::new();
    list_subnode_entries_at(reader, bbt, sub_bid, 0, &mut visited)
}

fn list_subnode_entries_at<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    sub_bid: BlockId,
    depth: u32,
    visited: &mut HashSet<u64>,
) -> Result<Vec<SubnodeEntry>> {
    if sub_bid.is_null() {
        return Ok(Vec::new());
    }
    enter_subnode_block(sub_bid.0, depth, visited)?;

    let raw = read_raw_block(reader, bbt, sub_bid)?;
    let bbt_entry = bbt.get(sub_bid).ok_or(PstError::BlockNotFound(sub_bid.0))?;
    let payload = &raw[..bbt_entry.cb as usize];
    if payload.len() < 8 {
        return Ok(Vec::new());
    }

    let btype = payload[0];
    let clevel = payload[1];
    let c_entries = LittleEndian::read_u16(&payload[2..4]) as usize;

    let mut results = Vec::new();

    match (btype, clevel) {
        (0x02, 0x00) => {
            for i in 0..c_entries {
                let offset = 8 + i * 24;
                if offset + 24 > payload.len() {
                    break;
                }
                results.push(SubnodeEntry {
                    nid: NodeId(LittleEndian::read_u64(&payload[offset..offset + 8])),
                    bid_data: BlockId(LittleEndian::read_u64(&payload[offset + 8..offset + 16])),
                    bid_sub: BlockId(LittleEndian::read_u64(&payload[offset + 16..offset + 24])),
                });
            }
        }
        (0x02, 0x01) => {
            for i in 0..c_entries {
                let offset = 8 + i * 16;
                if offset + 16 > payload.len() {
                    break;
                }
                let child_bid = BlockId(LittleEndian::read_u64(&payload[offset + 8..offset + 16]));
                let mut child_entries =
                    list_subnode_entries_at(reader, bbt, child_bid, depth + 1, visited)?;
                results.append(&mut child_entries);
            }
        }
        _ => {}
    }

    Ok(results)
}

/// A subnode BTree entry.
#[derive(Debug, Clone)]
pub struct SubnodeEntry {
    pub nid: NodeId,
    pub bid_data: BlockId,
    pub bid_sub: BlockId,
}

/// Collect external (leaf) data-block BIDs for a top-level data BID.
///
/// Used by attachment streaming so callers can read/decrypt one leaf block at a
/// time without assembling a multi-GB `Vec<u8>`.
pub fn collect_leaf_data_bids<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    bid: BlockId,
) -> Result<Vec<BlockId>> {
    let mut visited = HashSet::new();
    collect_leaf_data_bids_at(reader, bbt, bid, 0, &mut visited)
}

fn collect_leaf_data_bids_at<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    bid: BlockId,
    depth: u32,
    visited: &mut HashSet<u64>,
) -> Result<Vec<BlockId>> {
    if bid.is_null() {
        return Ok(Vec::new());
    }
    if !bid.is_internal() {
        return Ok(vec![bid]);
    }
    if depth > MAX_SUBNODE_DEPTH {
        return Err(PstError::ResourceLimit(format!(
            "xblock tree depth {depth} exceeds max {MAX_SUBNODE_DEPTH}"
        )));
    }
    if !visited.insert(bid.0) {
        return Err(PstError::BtreeCycle { page_offset: bid.0 });
    }

    let raw = read_raw_block(reader, bbt, bid)?;
    let bbt_entry = bbt.get(bid).ok_or(PstError::BlockNotFound(bid.0))?;
    let payload = &raw[..bbt_entry.cb as usize];
    if payload.len() < 8 {
        return Ok(Vec::new());
    }
    let btype = payload[0];
    let clevel = payload[1];
    let c_entries = LittleEndian::read_u16(&payload[2..4]) as usize;

    match (btype, clevel) {
        (0x01, 0x01) => {
            let mut leaves = Vec::with_capacity(c_entries);
            for i in 0..c_entries {
                let bid_offset = 8 + i * 8;
                if bid_offset + 8 > payload.len() {
                    break;
                }
                leaves.push(BlockId(LittleEndian::read_u64(
                    &payload[bid_offset..bid_offset + 8],
                )));
            }
            Ok(leaves)
        }
        (0x01, 0x02) => {
            let mut leaves = Vec::new();
            for i in 0..c_entries {
                let bid_offset = 8 + i * 8;
                if bid_offset + 8 > payload.len() {
                    break;
                }
                let child = BlockId(LittleEndian::read_u64(&payload[bid_offset..bid_offset + 8]));
                let mut child_leaves =
                    collect_leaf_data_bids_at(reader, bbt, child, depth + 1, visited)?;
                leaves.append(&mut child_leaves);
            }
            Ok(leaves)
        }
        _ => Err(PstError::InvalidBlockType {
            expected: 0x01,
            actual: btype,
        }),
    }
}

/// Read and decrypt a single external data block by BID.
pub fn read_leaf_block_data<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    bid: BlockId,
    crypt: CryptMethod,
) -> Result<Vec<u8>> {
    if bid.is_null() || bid.is_internal() {
        return Err(PstError::InvalidBlockType {
            expected: 0x00,
            actual: if bid.is_internal() { 0x01 } else { 0x00 },
        });
    }
    let raw = read_raw_block(reader, bbt, bid)?;
    let bbt_entry = bbt.get(bid).ok_or(PstError::BlockNotFound(bid.0))?;
    let mut data = raw[..bbt_entry.cb as usize].to_vec();
    crypto::decrypt_block(&mut data, crypt, bid.0);
    Ok(data)
}

/// Look up a subnode entry by NID under a subnode BTree root BID.
pub fn find_subnode_entry<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    sub_bid: BlockId,
    target_nid: NodeId,
) -> Result<Option<SubnodeEntry>> {
    let entries = list_subnode_entries(reader, bbt, sub_bid)?;
    Ok(entries.into_iter().find(|e| e.nid.0 == target_nid.0))
}

/// Round up to 64-byte alignment.
fn align64(size: usize) -> usize {
    (size + 63) & !63
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::CryptMethod;
    use std::collections::HashMap;
    use std::io::Cursor;

    #[test]
    fn internal_block_truncated_header_errors_not_panic() {
        // cb=1 → payload len 1; must not index payload[1] (panic).
        use super::super::btree::{BbtEntry, BbtIndex};
        use crate::header::Bref;

        // MS-PST: BID is internal when bit 1 (0x2) is set.
        let bid = BlockId(0x02);
        assert!(bid.is_internal(), "test BID must be classified internal");
        let mut file = vec![0x01u8]; // single-byte payload
        file.resize(64, 0);
        file.extend_from_slice(&[0u8; 16]);

        let mut bbt_entries = HashMap::new();
        bbt_entries.insert(
            bid.0,
            BbtEntry {
                bref: Bref { bid: bid.0, ib: 0 },
                cb: 1,
                c_ref: 1,
            },
        );
        let bbt = BbtIndex::from_entries_for_test(bbt_entries);
        let mut cursor = Cursor::new(file);
        let err = read_block_data(&mut cursor, &bbt, bid, CryptMethod::None)
            .expect_err("truncated internal header must error");
        assert!(matches!(
            err,
            PstError::DataTruncated {
                needed: 2,
                available: 1
            }
        ));
    }

    #[test]
    fn xblock_huge_lcb_total_rejected_before_alloc() {
        // Synthetic XBLOCK header: btype=1, clevel=1, cEntries=0, lcbTotal=u32::MAX
        let mut data = vec![0x01u8, 0x01, 0x00, 0x00];
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let bbt = BbtIndex::from_entries_for_test(HashMap::new());
        let err = read_xblock_data(&mut cursor, &bbt, &data, CryptMethod::None)
            .expect_err("must reject huge lcbTotal");
        assert!(matches!(err, PstError::ResourceLimit(_)));
    }

    #[test]
    fn xxblock_huge_lcb_total_rejected_before_alloc() {
        let mut data = vec![0x01u8, 0x02, 0x00, 0x00];
        data.extend_from_slice(&(MAX_XBLOCK_ASSEMBLE as u32 + 1).to_le_bytes());
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let bbt = BbtIndex::from_entries_for_test(HashMap::new());
        let err = read_xxblock_data(&mut cursor, &bbt, &data, CryptMethod::None)
            .expect_err("must reject huge lcbTotal");
        assert!(matches!(err, PstError::ResourceLimit(_)));
    }

    #[test]
    fn check_xblock_assemble_limit_boundary() {
        assert!(check_xblock_assemble_limit(MAX_XBLOCK_ASSEMBLE).is_ok());
        assert!(check_xblock_assemble_limit(MAX_XBLOCK_ASSEMBLE + 1).is_err());
        assert!(check_xblock_assemble_limit(0).is_ok());
    }

    #[test]
    fn enter_subnode_block_detects_cycle() {
        let mut visited = HashSet::new();
        enter_subnode_block(0x22, 0, &mut visited).expect("first visit");
        let err = enter_subnode_block(0x22, 1, &mut visited).expect_err("cycle");
        assert!(matches!(err, PstError::BtreeCycle { page_offset: 0x22 }));
    }

    #[test]
    fn enter_subnode_block_depth_limit() {
        let mut visited = HashSet::new();
        let err =
            enter_subnode_block(0x30, MAX_SUBNODE_DEPTH + 1, &mut visited).expect_err("depth");
        assert!(matches!(err, PstError::ResourceLimit(_)));
    }

    #[test]
    fn enter_subnode_block_allows_distinct_bids() {
        let mut visited = HashSet::new();
        for i in 0..8u64 {
            enter_subnode_block(0x100 + i, i as u32, &mut visited).expect("visit");
        }
        assert_eq!(visited.len(), 8);
    }

    /// Synthetic SIBLOCK that points at itself: public list path must fail closed.
    #[test]
    fn list_subnode_entries_detects_self_cycle() {
        use super::super::btree::{BbtEntry, BbtIndex};
        use crate::header::Bref;

        const SI_BID: u64 = 0x42; // internal bit not required for subnode walk

        // SIBLOCK: btype=0x02, clevel=0x01, cEntries=1, reserved=0,
        // entry: nid=1 + child bid = SI_BID (self-loop).
        let mut siblock = vec![0x02u8, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
        siblock.extend_from_slice(&1u64.to_le_bytes());
        siblock.extend_from_slice(&SI_BID.to_le_bytes());

        let mut file = Vec::new();
        let offset = file.len() as u64;
        file.extend_from_slice(&siblock);
        let padded = (siblock.len() + 63) & !63;
        file.resize(file.len() + (padded - siblock.len()), 0);
        file.extend_from_slice(&[0u8; 16]); // trailer (CRC/BID warn-only)

        let mut bbt_entries = HashMap::new();
        bbt_entries.insert(
            SI_BID,
            BbtEntry {
                bref: Bref {
                    bid: SI_BID,
                    ib: offset,
                },
                cb: siblock.len() as u16,
                c_ref: 1,
            },
        );
        let bbt = BbtIndex::from_entries_for_test(bbt_entries);
        let mut cursor = Cursor::new(file);
        let err = list_subnode_entries(&mut cursor, &bbt, BlockId(SI_BID))
            .expect_err("self-cycle must fail");
        assert!(matches!(
            err,
            PstError::BtreeCycle {
                page_offset: SI_BID
            }
        ));
    }
}
