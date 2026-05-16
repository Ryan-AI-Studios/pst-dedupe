//! BTree-on-Heap (BTH) — MS-PST §2.3.2
//!
//! A B-tree stored inside a Heap-on-Node. Used by Property Context (PC) and
//! Table Context (TC) for indexed lookups.

use super::hn::{Heap, Hid};
use crate::error::{PstError, Result};
use byteorder::{ByteOrder, LittleEndian};

/// BTH header (BTHHEADER).
#[derive(Debug)]
pub struct BthHeader {
    /// Type marker (must be 0xB5).
    pub b_type: u8,
    /// Key size in bytes (2, 4, 8, or 16).
    pub cb_key: u8,
    /// Data size per entry in bytes.
    pub cb_ent: u8,
    /// Number of index levels (0 = all data in leaf records).
    pub b_idx_levels: u8,
    /// HID of the root of the BTH (0 if empty).
    pub hid_root: Hid,
}

/// A single BTH leaf record (key + data).
#[derive(Debug, Clone)]
pub struct BthRecord {
    pub key: Vec<u8>,
    pub data: Vec<u8>,
}

/// Parse the BTH header from an HID in the heap.
pub fn read_bth_header(heap: &Heap, hid: Hid) -> Result<BthHeader> {
    let data = heap.get(hid)?;
    if data.len() < 8 {
        return Err(PstError::DataTruncated {
            needed: 8,
            available: data.len(),
        });
    }

    let b_type = data[0];
    if b_type != 0xB5 {
        return Err(PstError::InvalidBthType(b_type));
    }

    Ok(BthHeader {
        b_type,
        cb_key: data[1],
        cb_ent: data[2],
        b_idx_levels: data[3],
        hid_root: Hid(LittleEndian::read_u32(&data[4..8])),
    })
}

/// Collect all leaf records from a BTH.
///
/// Traverses intermediate levels if present, returns all key-data pairs.
pub fn collect_records(heap: &Heap, header: &BthHeader) -> Result<Vec<BthRecord>> {
    if header.hid_root.is_null() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    collect_level(
        heap,
        header,
        header.hid_root,
        header.b_idx_levels,
        &mut records,
    )?;
    Ok(records)
}

fn collect_level(
    heap: &Heap,
    header: &BthHeader,
    hid: Hid,
    level: u8,
    records: &mut Vec<BthRecord>,
) -> Result<()> {
    let data = heap.get(hid)?;

    if level == 0 {
        // Leaf level: records are key(cb_key) + data(cb_ent)
        let record_size = header.cb_key as usize + header.cb_ent as usize;
        if record_size == 0 {
            return Ok(());
        }

        let count = data.len() / record_size;
        for i in 0..count {
            let offset = i * record_size;
            if offset + record_size > data.len() {
                break;
            }

            let key = data[offset..offset + header.cb_key as usize].to_vec();
            let value = data[offset + header.cb_key as usize..offset + record_size].to_vec();

            records.push(BthRecord { key, data: value });
        }
    } else {
        // Intermediate level: records are key(cb_key) + hidChild(4)
        let record_size = header.cb_key as usize + 4;
        if record_size == 0 {
            return Ok(());
        }

        let count = data.len() / record_size;
        for i in 0..count {
            let offset = i * record_size;
            if offset + record_size > data.len() {
                break;
            }

            let hid_child_offset = offset + header.cb_key as usize;
            let hid_child = Hid(LittleEndian::read_u32(
                &data[hid_child_offset..hid_child_offset + 4],
            ));

            collect_level(heap, header, hid_child, level - 1, records)?;
        }
    }

    Ok(())
}

/// Look up a single key in the BTH, returning the data portion if found.
pub fn lookup(heap: &Heap, header: &BthHeader, search_key: &[u8]) -> Result<Option<Vec<u8>>> {
    if header.hid_root.is_null() {
        return Ok(None);
    }

    lookup_level(
        heap,
        header,
        header.hid_root,
        header.b_idx_levels,
        search_key,
    )
}

fn lookup_level(
    heap: &Heap,
    header: &BthHeader,
    hid: Hid,
    level: u8,
    search_key: &[u8],
) -> Result<Option<Vec<u8>>> {
    let data = heap.get(hid)?;

    if level == 0 {
        // Leaf: linear scan for matching key
        let record_size = header.cb_key as usize + header.cb_ent as usize;
        if record_size == 0 {
            return Ok(None);
        }

        let count = data.len() / record_size;
        for i in 0..count {
            let offset = i * record_size;
            let key = &data[offset..offset + header.cb_key as usize];
            if key == search_key {
                let value_start = offset + header.cb_key as usize;
                return Ok(Some(
                    data[value_start..value_start + header.cb_ent as usize].to_vec(),
                ));
            }
        }
        Ok(None)
    } else {
        // Intermediate: find the correct child (last key <= search_key)
        let record_size = header.cb_key as usize + 4;
        if record_size == 0 {
            return Ok(None);
        }

        let count = data.len() / record_size;
        let mut best_hid: Option<Hid> = None;

        for i in 0..count {
            let offset = i * record_size;
            let key = &data[offset..offset + header.cb_key as usize];
            let hid_child_offset = offset + header.cb_key as usize;
            let child_hid = Hid(LittleEndian::read_u32(
                &data[hid_child_offset..hid_child_offset + 4],
            ));

            if key <= search_key {
                best_hid = Some(child_hid);
            } else {
                break;
            }
        }

        match best_hid {
            Some(child) => lookup_level(heap, header, child, level - 1, search_key),
            None => Ok(None),
        }
    }
}
