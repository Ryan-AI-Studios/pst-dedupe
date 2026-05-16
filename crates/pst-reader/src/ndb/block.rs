//! Data block reading, multi-block assembly, and subnode BTree traversal.
//!
//! Data blocks are the actual storage units for node data. They are up to 8192 bytes
//! (8176 payload + 16 trailer for Unicode). Larger data is split across XBLOCK or
//! XXBLOCK chains.

use std::io::{Read, Seek, SeekFrom};
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt};

use crate::crypto::{self, CryptMethod};
use crate::error::{PstError, Result};
use super::btree::BbtIndex;
use super::nid::NodeId;

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

/// Maximum data payload per block (Unicode): 8192 - 16 (trailer) = 8176 bytes.
const MAX_BLOCK_DATA: usize = 8176;

/// Block trailer size (Unicode): 16 bytes.
const BLOCK_TRAILER_SIZE: usize = 16;

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

    let bbt_entry = bbt.get(bid)
        .ok_or(PstError::BlockNotFound(bid.0))?;

    // Read raw block from disk
    reader.seek(SeekFrom::Start(bbt_entry.bref.ib))?;

    // Block size on disk is cb rounded up to 64-byte alignment + trailer
    let raw_size = align64(bbt_entry.cb as usize) + BLOCK_TRAILER_SIZE;
    let mut raw = vec![0u8; raw_size];
    reader.read_exact(&mut raw)?;

    // Extract payload (first cb bytes)
    let payload = &raw[..bbt_entry.cb as usize];

    if !bid.is_internal() {
        // External data block — decrypt and return
        let mut data = payload.to_vec();
        crypto::decrypt_block(&mut data, crypt, bid.0);
        Ok(data)
    } else {
        // Internal block — check type
        // XBLOCK/XXBLOCK: btype=0x01; SLBLOCK/SIBLOCK: btype=0x02
        if payload.is_empty() {
            return Ok(Vec::new());
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
            _ => {
                Err(PstError::InvalidBlockType { expected: 0x01, actual: btype })
            }
        }
    }
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
        return Err(PstError::DataTruncated { needed: 8, available: xblock_data.len() });
    }

    let c_entries = LittleEndian::read_u16(&xblock_data[2..4]) as usize;
    let lcb_total = LittleEndian::read_u32(&xblock_data[4..8]) as usize;

    let mut result = Vec::with_capacity(lcb_total);

    for i in 0..c_entries {
        let bid_offset = 8 + i * 8;
        if bid_offset + 8 > xblock_data.len() {
            break;
        }
        let child_bid = BlockId(LittleEndian::read_u64(&xblock_data[bid_offset..bid_offset + 8]));

        // Each child is an external data block — read and decrypt
        let bbt_entry = bbt.get(child_bid)
            .ok_or(PstError::BlockNotFound(child_bid.0))?;

        reader.seek(SeekFrom::Start(bbt_entry.bref.ib))?;
        let raw_size = align64(bbt_entry.cb as usize) + BLOCK_TRAILER_SIZE;
        let mut raw = vec![0u8; raw_size];
        reader.read_exact(&mut raw)?;

        let mut payload = raw[..bbt_entry.cb as usize].to_vec();
        crypto::decrypt_block(&mut payload, crypt, child_bid.0);
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
        return Err(PstError::DataTruncated { needed: 8, available: xxblock_data.len() });
    }

    let c_entries = LittleEndian::read_u16(&xxblock_data[2..4]) as usize;
    let lcb_total = LittleEndian::read_u32(&xxblock_data[4..8]) as usize;

    let mut result = Vec::with_capacity(lcb_total);

    for i in 0..c_entries {
        let bid_offset = 8 + i * 8;
        if bid_offset + 8 > xxblock_data.len() {
            break;
        }
        let child_bid = BlockId(LittleEndian::read_u64(&xxblock_data[bid_offset..bid_offset + 8]));

        // Read the child XBLOCK
        let bbt_entry = bbt.get(child_bid)
            .ok_or(PstError::BlockNotFound(child_bid.0))?;

        reader.seek(SeekFrom::Start(bbt_entry.bref.ib))?;
        let raw_size = align64(bbt_entry.cb as usize) + BLOCK_TRAILER_SIZE;
        let mut raw = vec![0u8; raw_size];
        reader.read_exact(&mut raw)?;

        // XBLOCK data is not encrypted (internal block)
        let xblock_payload = &raw[..bbt_entry.cb as usize];
        let chunk = read_xblock_data(reader, bbt, xblock_payload, crypt)?;
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
    if sub_bid.is_null() {
        return Err(PstError::SubnodeNotFound(target_nid.0));
    }

    // Read the subnode block
    let bbt_entry = bbt.get(sub_bid)
        .ok_or(PstError::BlockNotFound(sub_bid.0))?;

    reader.seek(SeekFrom::Start(bbt_entry.bref.ib))?;
    let raw_size = align64(bbt_entry.cb as usize) + BLOCK_TRAILER_SIZE;
    let mut raw = vec![0u8; raw_size];
    reader.read_exact(&mut raw)?;

    let payload = &raw[..bbt_entry.cb as usize];
    if payload.len() < 8 {
        return Err(PstError::DataTruncated { needed: 8, available: payload.len() });
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
                let entry_bid_data = BlockId(LittleEndian::read_u64(&payload[offset + 8..offset + 16]));

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
                match read_subnode_data(reader, bbt, child_bid, target_nid, crypt) {
                    Ok(data) => return Ok(data),
                    Err(PstError::SubnodeNotFound(_)) => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(PstError::SubnodeNotFound(target_nid.0))
        }
        _ => {
            Err(PstError::InvalidBlockType { expected: 0x02, actual: btype })
        }
    }
}

/// List all entries in a subnode BTree (used by TC for row data iteration).
pub fn list_subnode_entries<R: Read + Seek>(
    reader: &mut R,
    bbt: &BbtIndex,
    sub_bid: BlockId,
) -> Result<Vec<SubnodeEntry>> {
    if sub_bid.is_null() {
        return Ok(Vec::new());
    }

    let bbt_entry = bbt.get(sub_bid)
        .ok_or(PstError::BlockNotFound(sub_bid.0))?;

    reader.seek(SeekFrom::Start(bbt_entry.bref.ib))?;
    let raw_size = align64(bbt_entry.cb as usize) + BLOCK_TRAILER_SIZE;
    let mut raw = vec![0u8; raw_size];
    reader.read_exact(&mut raw)?;

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
                let mut child_entries = list_subnode_entries(reader, bbt, child_bid)?;
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

/// Round up to 64-byte alignment.
fn align64(size: usize) -> usize {
    (size + 63) & !63
}
