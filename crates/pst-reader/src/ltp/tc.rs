//! Table Context (TC) — MS-PST §2.3.4
//!
//! A TC is a table (rows × columns) built on an HN + subnode BTree.
//! Used for folder hierarchy tables, contents tables, and attachment tables.

use super::bth;
use super::hn::{Heap, Hid};
use super::pc::decode_utf16le;
use crate::crypto::CryptMethod;
use crate::error::{PstError, Result};
use crate::ndb::block;
use crate::ndb::btree::{BbtIndex, NbtIndex};
use crate::ndb::BlockId;
use crate::ndb::NodeId;
use byteorder::{ByteOrder, LittleEndian};

use std::collections::HashMap;
use std::io::{Read, Seek};

const MAX_BLOCK_DATA: usize = 8176;

/// Resolves a table-subnode NID to that entry's data-tree bytes (one NID only).
///
/// Used by [`TableContext::load_with_resolver`] for `hnidRows` and cell HNIDs
/// that are NIDs (`nidType != 0`). Callers must not pre-concatenate sibling
/// SLENTRYs — look up the NID named by TCINFO / the cell.
pub trait SubnodeResolver {
    fn resolve_subnode(&mut self, nid: u32) -> Result<Vec<u8>>;
}

/// NDB-backed resolver: `find_subnode_entry` for `nid` under `bid_sub`, then
/// `read_block_data` on that entry's `bid_data`.
pub struct BlockSubnodeResolver<'a, R: Read + Seek> {
    reader: &'a mut R,
    bbt: &'a BbtIndex,
    bid_sub: BlockId,
    crypt: CryptMethod,
}

impl<'a, R: Read + Seek> BlockSubnodeResolver<'a, R> {
    pub fn new(reader: &'a mut R, bbt: &'a BbtIndex, bid_sub: BlockId, crypt: CryptMethod) -> Self {
        Self {
            reader,
            bbt,
            bid_sub,
            crypt,
        }
    }
}

impl<R: Read + Seek> SubnodeResolver for BlockSubnodeResolver<'_, R> {
    fn resolve_subnode(&mut self, nid: u32) -> Result<Vec<u8>> {
        if self.bid_sub.is_null() {
            return Err(PstError::SubnodeNotFound(u64::from(nid)));
        }
        let entry =
            block::find_subnode_entry(self.reader, self.bbt, self.bid_sub, NodeId(u64::from(nid)))?;
        let Some(entry) = entry else {
            return Err(PstError::SubnodeNotFound(u64::from(nid)));
        };
        block::read_block_data(self.reader, self.bbt, entry.bid_data, self.crypt)
    }
}

/// Column descriptor (TCOLDESC, 8 bytes).
#[derive(Debug, Clone)]
pub struct TcColumnDesc {
    /// MAPI property tag (property ID).
    pub prop_id: u16,
    /// Property type.
    pub prop_type: u16,
    /// Offset of this column's data within the row.
    pub ib_data: u16,
    /// Size of this column's data in bytes.
    pub cb_data: u8,
    /// Bit index for the cell existence bitmap.
    pub i_bit: u8,
}

/// Parsed TC info (TCINFO header).
#[derive(Debug)]
pub struct TcInfo {
    /// Number of columns.
    pub c_cols: u8,
    /// Offsets for 4-byte, 8-byte, and variable-size column groups.
    pub rgib: [u16; 4],
    /// HID of the row index BTH.
    pub hid_row_index: Hid,
    /// HID or NID containing row data.
    pub hnid_rows: u32,
    /// Column descriptors.
    pub columns: Vec<TcColumnDesc>,
}

/// A loaded Table Context.
pub struct TableContext {
    heap: Heap,
    info: TcInfo,
    /// Assembled row data (from HN inline or subnode BTree).
    row_data: Vec<u8>,
    /// Size of each row in bytes (rgib[3] from TcInfo — the total row width).
    row_size: usize,
    /// Row count.
    row_count: usize,
    /// RowID for each matrix index (from the RowIndex BTH). 0 if unknown.
    ///
    /// For hierarchy/contents tables the RowID is the child folder/message NID
    /// (MS-PST §2.3.4.3 / §2.4.4).
    row_ids: Vec<u32>,
    /// Cell values stored as subnode NIDs (`nidType != 0`), keyed by the HNID
    /// written in the row slot.
    cell_subnodes: HashMap<u32, Vec<u8>>,
}

impl TableContext {
    /// Load a TC whose row matrix (if any) lives inline on the heap (HID
    /// `hnidRows`). For NID-backed matrices / cell HNIDs use
    /// [`Self::load_with_resolver`].
    pub fn load(data: Vec<u8>) -> Result<Self> {
        load_inner(data, None)
    }

    /// Load a TC, resolving `hnidRows` / cell HNIDs that are NIDs via `resolver`.
    ///
    /// The resolver must look up **that NID only** (no sibling SLENTRY concat).
    pub fn load_with_resolver<R: SubnodeResolver>(data: Vec<u8>, resolver: &mut R) -> Result<Self> {
        load_inner(data, Some(resolver))
    }

    /// Build matrix-index → RowID map from the TC RowIndex BTH.
    fn load_row_ids(heap: &Heap, info: &TcInfo, row_count: usize) -> Result<Vec<u32>> {
        let mut row_ids = vec![0u32; row_count];
        if info.hid_row_index.is_null() || row_count == 0 {
            return Ok(row_ids);
        }

        let bth_header = match bth::read_bth_header(heap, info.hid_row_index) {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("TC RowIndex BTH header unreadable: {e}");
                return Ok(row_ids);
            }
        };

        let records = match bth::collect_records(heap, &bth_header) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("TC RowIndex BTH traversal failed: {e}");
                return Ok(row_ids);
            }
        };

        for rec in records {
            if rec.key.len() < 4 {
                continue;
            }
            let row_id = LittleEndian::read_u32(&rec.key[0..4]);
            let row_index = match rec.data.len() {
                n if n >= 4 => LittleEndian::read_u32(&rec.data[0..4]) as usize,
                2 => LittleEndian::read_u16(&rec.data[0..2]) as usize,
                1 => rec.data[0] as usize,
                _ => continue,
            };
            if row_index < row_ids.len() {
                row_ids[row_index] = row_id;
            }
        }

        Ok(row_ids)
    }

    fn parse_tc_info(data: &[u8]) -> Result<TcInfo> {
        // TCINFO: bType(1) + cCols(1) + rgib[4](8) + hidRowIndex(4) + hnidRows(4) = 18 bytes
        // Then cCols × TCOLDESC (8 bytes each)
        if data.len() < 18 {
            return Err(PstError::DataTruncated {
                needed: 18,
                available: data.len(),
            });
        }

        let _b_type = data[0]; // 0x7C for TC
        let c_cols = data[1];
        let rgib = [
            LittleEndian::read_u16(&data[2..4]),
            LittleEndian::read_u16(&data[4..6]),
            LittleEndian::read_u16(&data[6..8]),
            LittleEndian::read_u16(&data[8..10]),
        ];
        let hid_row_index = Hid(LittleEndian::read_u32(&data[10..14]));
        let hnid_rows = LittleEndian::read_u32(&data[14..18]);

        let mut columns = Vec::with_capacity(c_cols as usize);
        for i in 0..c_cols as usize {
            let col_offset = 18 + i * 8;
            if col_offset + 8 > data.len() {
                break;
            }
            let col_data = &data[col_offset..col_offset + 8];
            columns.push(TcColumnDesc {
                prop_id: LittleEndian::read_u16(&col_data[0..2]),
                prop_type: LittleEndian::read_u16(&col_data[2..4]),
                ib_data: LittleEndian::read_u16(&col_data[4..6]),
                cb_data: col_data[6],
                i_bit: col_data[7],
            });
        }

        Ok(TcInfo {
            c_cols,
            rgib,
            hid_row_index,
            hnid_rows,
            columns,
        })
    }

    /// Number of rows in the table.
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// Get column descriptors.
    pub fn columns(&self) -> &[TcColumnDesc] {
        &self.info.columns
    }

    /// RowID for a matrix index (from the RowIndex BTH).
    ///
    /// For folder hierarchy and contents tables this is the NID of the child
    /// object. Returns `None` when the RowIndex entry is missing or zero.
    pub fn get_row_id(&self, row_index: usize) -> Option<u32> {
        let id = *self.row_ids.get(row_index)?;
        if id == 0 {
            None
        } else {
            Some(id)
        }
    }

    /// Read a 4-byte value from a specific row and column.
    pub fn get_row_u32(&self, row_index: usize, prop_id: u16) -> Option<u32> {
        let col = self.info.columns.iter().find(|c| c.prop_id == prop_id)?;
        let row_start = row_index * self.row_size;
        let data_offset = row_start + col.ib_data as usize;
        let data_end = data_offset + col.cb_data as usize;

        if data_end > self.row_data.len() {
            return None;
        }

        match col.cb_data {
            4 => Some(LittleEndian::read_u32(
                &self.row_data[data_offset..data_end],
            )),
            2 => Some(LittleEndian::read_u16(&self.row_data[data_offset..data_end]) as u32),
            1 => Some(self.row_data[data_offset] as u32),
            _ => None,
        }
    }

    /// Read a u64 value from a row (for 8-byte columns).
    pub fn get_row_u64(&self, row_index: usize, prop_id: u16) -> Option<u64> {
        let col = self.info.columns.iter().find(|c| c.prop_id == prop_id)?;
        let row_start = row_index * self.row_size;
        let data_offset = row_start + col.ib_data as usize;

        if col.cb_data != 8 || data_offset + 8 > self.row_data.len() {
            return None;
        }

        Some(LittleEndian::read_u64(
            &self.row_data[data_offset..data_offset + 8],
        ))
    }

    /// Read a variable-length value (string or binary) from a row.
    ///
    /// For variable-size columns, the row stores an HNID (4 bytes) pointing to
    /// the actual data in the HN or subnode.
    pub fn get_row_string(&self, row_index: usize, prop_id: u16) -> Result<Option<String>> {
        let col = self.info.columns.iter().find(|c| c.prop_id == prop_id);
        let col = match col {
            Some(c) => c,
            None => return Ok(None),
        };

        if col.prop_type != 0x001F {
            return Ok(None);
        }

        let row_start = row_index * self.row_size;
        let data_offset = row_start + col.ib_data as usize;

        if col.cb_data == 4 && data_offset + 4 <= self.row_data.len() {
            let hnid = LittleEndian::read_u32(&self.row_data[data_offset..data_offset + 4]);
            if hnid == 0 {
                return Ok(Some(String::new()));
            }
            let hid = Hid(hnid);
            let bytes = if hid.hid_type() == 0 {
                self.heap.get(hid)?
            } else {
                match self.cell_subnodes.get(&hnid) {
                    Some(b) => b.as_slice(),
                    None => {
                        return Err(PstError::SubnodeNotFound(u64::from(hnid)));
                    }
                }
            };
            let s = decode_utf16le(bytes)?;
            Ok(Some(s))
        } else {
            Ok(None)
        }
    }

    /// Access the TcInfo.
    pub fn info(&self) -> &TcInfo {
        &self.info
    }
}

fn load_inner(
    data: Vec<u8>,
    mut resolver: Option<&mut dyn SubnodeResolver>,
) -> Result<TableContext> {
    let block_size = if data.len() <= MAX_BLOCK_DATA {
        data.len()
    } else {
        MAX_BLOCK_DATA
    };

    let heap = Heap::parse(data, block_size)?;
    let tc_data = heap.get(heap.header.hid_user_root)?;
    let info = TableContext::parse_tc_info(tc_data)?;
    let row_size = info.rgib[3] as usize;

    let (row_data, row_count) = if info.hnid_rows == 0 {
        (Vec::new(), 0)
    } else {
        let hid = Hid(info.hnid_rows);
        if hid.hid_type() == 0 {
            let inline_data = heap.get(hid)?.to_vec();
            let count = inline_data.len().checked_div(row_size).unwrap_or(0);
            (inline_data, count)
        } else {
            let resolver = resolver
                .as_mut()
                .ok_or(PstError::SubnodeNotFound(u64::from(info.hnid_rows)))?;
            let payload = resolver.resolve_subnode(info.hnid_rows)?;
            extract_rows_per_block(&payload, row_size)?
        }
    };

    let mut cell_subnodes = HashMap::new();
    if let Some(resolver) = resolver.as_mut() {
        collect_cell_subnodes(
            &info,
            &row_data,
            row_size,
            row_count,
            *resolver,
            &mut cell_subnodes,
        )?;
    }

    let row_ids = TableContext::load_row_ids(&heap, &info, row_count)?;

    Ok(TableContext {
        heap,
        info,
        row_data,
        row_size,
        row_count,
        row_ids,
        cell_subnodes,
    })
}

/// MS-PST §2.3.4.4: `RowsPerBlock = Floor(8176 / rgib[TCI_bm])`. Non-last
/// leaves are 8176-byte payloads; dead space is ignored; rows never span blocks.
fn extract_rows_per_block(payload: &[u8], row_size: usize) -> Result<(Vec<u8>, usize)> {
    let rows_per_block = MAX_BLOCK_DATA.checked_div(row_size).ok_or_else(|| {
        PstError::ResourceLimit("TC row width is 0; cannot unpack row matrix".into())
    })?;
    if rows_per_block == 0 {
        return Err(PstError::ResourceLimit(format!(
            "TC row width {row_size} exceeds block payload {MAX_BLOCK_DATA}"
        )));
    }
    let row_bytes_per_block = rows_per_block * row_size;
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset < payload.len() {
        let remaining = payload.len() - offset;
        if remaining > MAX_BLOCK_DATA {
            out.extend_from_slice(&payload[offset..offset + row_bytes_per_block]);
            offset += MAX_BLOCK_DATA;
        } else {
            let n = remaining.checked_div(row_size).unwrap_or(0);
            out.extend_from_slice(&payload[offset..offset + n * row_size]);
            break;
        }
    }
    let count = out.len().checked_div(row_size).unwrap_or(0);
    Ok((out, count))
}

fn collect_cell_subnodes(
    info: &TcInfo,
    row_data: &[u8],
    row_size: usize,
    row_count: usize,
    resolver: &mut dyn SubnodeResolver,
    out: &mut HashMap<u32, Vec<u8>>,
) -> Result<()> {
    if row_size == 0 || row_count == 0 {
        return Ok(());
    }
    for row_index in 0..row_count {
        let row_start = row_index * row_size;
        for col in &info.columns {
            // Only PtypString / PtypString8 / PtypBinary store HNIDs. Integer
            // cells (RowID, RecipientType, …) can have nidType ≠ 0 by coincidence.
            if col.cb_data != 4 || !matches!(col.prop_type, 0x001F | 0x001E | 0x0102) {
                continue;
            }
            let data_offset = row_start + col.ib_data as usize;
            if data_offset + 4 > row_data.len() {
                continue;
            }
            let hnid = LittleEndian::read_u32(&row_data[data_offset..data_offset + 4]);
            if hnid == 0 {
                continue;
            }
            let hid = Hid(hnid);
            if hid.hid_type() == 0 || out.contains_key(&hnid) {
                continue;
            }
            match resolver.resolve_subnode(hnid) {
                Ok(bytes) => {
                    out.insert(hnid, bytes);
                }
                Err(PstError::SubnodeNotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

/// Load a Table Context from a node, reading data from NDB.
///
/// Row matrix and cell HNIDs that are NIDs are resolved via `bid_sub` (that NID
/// only — sibling SLENTRYs are not concatenated).
pub fn load_tc<R: Read + Seek>(
    reader: &mut R,
    nbt: &NbtIndex,
    bbt: &BbtIndex,
    nid: NodeId,
    crypt: CryptMethod,
) -> Result<TableContext> {
    let nbt_entry = nbt.get(nid).ok_or(PstError::NodeNotFound(nid.0))?;
    let data = block::read_block_data(reader, bbt, nbt_entry.bid_data, crypt)?;
    let mut resolver = BlockSubnodeResolver::new(reader, bbt, nbt_entry.bid_sub, crypt);
    TableContext::load_with_resolver(data, &mut resolver)
}

/// Load a TC from already-read heap bytes plus the table node's `bid_sub`.
pub fn load_from_table_bids<R: Read + Seek>(
    data: Vec<u8>,
    reader: &mut R,
    bbt: &BbtIndex,
    bid_sub: BlockId,
    crypt: CryptMethod,
) -> Result<TableContext> {
    let mut resolver = BlockSubnodeResolver::new(reader, bbt, bid_sub, crypt);
    TableContext::load_with_resolver(data, &mut resolver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MapResolver(HashMap<u32, Vec<u8>>);

    impl SubnodeResolver for MapResolver {
        fn resolve_subnode(&mut self, nid: u32) -> Result<Vec<u8>> {
            self.0
                .get(&nid)
                .cloned()
                .ok_or(PstError::SubnodeNotFound(u64::from(nid)))
        }
    }

    /// Minimal single-page HN: HNHDR + allocations + HNPAGEMAP (cAlloc, cFree, rgib).
    fn build_hn(client_sig: u8, allocs: &[Vec<u8>], user_root_idx: usize) -> Vec<u8> {
        let mut data = vec![0u8; 12];
        data[2] = 0xEC;
        data[3] = client_sig;
        let mut ends = Vec::with_capacity(allocs.len());
        for a in allocs {
            data.extend_from_slice(a);
            ends.push(data.len());
        }
        if data.len() % 2 == 1 {
            data.push(0);
        }
        let hnpm = data.len() as u16;
        let c_alloc = allocs.len() as u16;
        data.extend_from_slice(&c_alloc.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&12u16.to_le_bytes());
        for end in &ends {
            data.extend_from_slice(&(*end as u16).to_le_bytes());
        }
        data[0..2].copy_from_slice(&hnpm.to_le_bytes());
        let hid_user = ((user_root_idx as u32) + 1) << 5;
        data[4..8].copy_from_slice(&hid_user.to_le_bytes());
        data
    }

    fn hid(index: u32) -> u32 {
        index << 5
    }

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// One string column (0x3001) + 1-byte CEB. Row width 5.
    fn tcinfo(hnid_rows: u32) -> Vec<u8> {
        let mut t = Vec::new();
        t.push(0x7C);
        t.push(1);
        t.extend_from_slice(&0u16.to_le_bytes());
        t.extend_from_slice(&0u16.to_le_bytes());
        t.extend_from_slice(&0u16.to_le_bytes());
        t.extend_from_slice(&5u16.to_le_bytes()); // row width
        t.extend_from_slice(&0u32.to_le_bytes()); // hidRowIndex
        t.extend_from_slice(&hnid_rows.to_le_bytes());
        t.extend_from_slice(&0x3001u16.to_le_bytes());
        t.extend_from_slice(&0x001Fu16.to_le_bytes());
        t.extend_from_slice(&0u16.to_le_bytes());
        t.push(4);
        t.push(0);
        t
    }

    fn row(hnid: u32) -> Vec<u8> {
        let mut r = Vec::with_capacity(5);
        r.extend_from_slice(&hnid.to_le_bytes());
        r.push(0x01); // CEB bit 0
        r
    }

    #[test]
    fn hid_inline_row_matrix_and_hid_cells() {
        let name = utf16le("Alice");
        let matrix = row(hid(1));
        let info = tcinfo(hid(2));
        let heap = build_hn(0x7C, &[name, matrix, info], 2);
        let table = TableContext::load(heap).expect("load HID TC");
        assert_eq!(table.row_count(), 1);
        assert_eq!(
            table.get_row_string(0, 0x3001).expect("str").as_deref(),
            Some("Alice")
        );
    }

    #[test]
    fn extra_sibling_subnode_does_not_change_row_count() {
        let row_width = 5usize;
        let matrix_nid = 0x3F; // nidIndex=1, nidType=0x1F
        let sibling_nid = 0x5F;
        let mut matrix = Vec::new();
        matrix.extend_from_slice(&row(0));
        matrix.extend_from_slice(&row(0));
        let sibling = vec![0u8; row_width * 3]; // would add 3 fake rows if concatenated
        let info = tcinfo(matrix_nid);
        let heap = build_hn(0x7C, &[info], 0);
        let mut map = HashMap::new();
        map.insert(matrix_nid, matrix);
        map.insert(sibling_nid, sibling);
        let mut resolver = MapResolver(map);
        let table = TableContext::load_with_resolver(heap, &mut resolver).expect("load NID matrix");
        assert_eq!(table.row_count(), 2, "sibling SLENTRY must not become rows");
    }

    #[test]
    fn rows_per_block_ignores_dead_space() {
        // row_size=100 → RowsPerBlock = 81, dead = 8176 - 8100 = 76.
        let row_size = 100usize;
        let rpb = MAX_BLOCK_DATA / row_size;
        assert_eq!(rpb, 81);
        let mut leaf0 = vec![0xABu8; rpb * row_size];
        leaf0.resize(MAX_BLOCK_DATA, 0xEE); // dead space
        let last_rows = 3usize;
        let leaf1 = vec![0xCDu8; last_rows * row_size];
        let mut payload = leaf0;
        payload.extend_from_slice(&leaf1);
        let (rows, count) = extract_rows_per_block(&payload, row_size).expect("unpack");
        assert_eq!(count, rpb + last_rows);
        assert_eq!(rows.len(), count * row_size);
        assert!(
            rows.iter().all(|&b| b != 0xEE),
            "dead space must not appear"
        );
        let first_last_row = rpb * row_size;
        assert_eq!(rows[first_last_row], 0xCD);
        // Integer-dividing the concat can yield the same *count* (dead < row_size)
        // while still misaligning later rows — the dead byte is still in the stream.
        assert_eq!(payload[rpb * row_size], 0xEE);
    }

    #[test]
    fn get_row_string_resolves_cell_nid() {
        let cell_nid = 0x3F;
        let matrix_nid = 0x5F;
        let name = utf16le("NidName");
        let matrix = row(cell_nid);
        let info = tcinfo(matrix_nid);
        let heap = build_hn(0x7C, &[info], 0);
        let mut map = HashMap::new();
        map.insert(matrix_nid, matrix);
        map.insert(cell_nid, name);
        let mut resolver = MapResolver(map);
        let table = TableContext::load_with_resolver(heap, &mut resolver).expect("load");
        assert_eq!(table.row_count(), 1);
        assert_eq!(
            table.get_row_string(0, 0x3001).expect("str").as_deref(),
            Some("NidName")
        );
    }

    #[test]
    fn integer_column_is_not_resolved_as_cell_subnode() {
        // Two columns: Unicode HNID + PtypInteger32 whose value equals a sibling NID.
        let cell_nid: u32 = 0x3F;
        let matrix_nid: u32 = 0x5F;
        let decoy_nid: u32 = 0x7F; // would be loaded if integer cells were treated as HNIDs
        let name = utf16le("Keep");
        let mut matrix = Vec::new();
        matrix.extend_from_slice(&cell_nid.to_le_bytes());
        matrix.extend_from_slice(&decoy_nid.to_le_bytes());
        matrix.push(0x03); // CEB bits 0 and 1
        let mut info = Vec::new();
        info.push(0x7C);
        info.push(2);
        info.extend_from_slice(&0u16.to_le_bytes());
        info.extend_from_slice(&0u16.to_le_bytes());
        info.extend_from_slice(&0u16.to_le_bytes());
        info.extend_from_slice(&9u16.to_le_bytes());
        info.extend_from_slice(&0u32.to_le_bytes());
        info.extend_from_slice(&matrix_nid.to_le_bytes());
        info.extend_from_slice(&0x3001u16.to_le_bytes());
        info.extend_from_slice(&0x001Fu16.to_le_bytes());
        info.extend_from_slice(&0u16.to_le_bytes());
        info.push(4);
        info.push(0);
        info.extend_from_slice(&0x0C15u16.to_le_bytes());
        info.extend_from_slice(&0x0003u16.to_le_bytes());
        info.extend_from_slice(&4u16.to_le_bytes());
        info.push(4);
        info.push(1);
        let heap = build_hn(0x7C, &[info], 0);
        let mut map = HashMap::new();
        map.insert(matrix_nid, matrix);
        map.insert(cell_nid, name);
        map.insert(decoy_nid, utf16le("DECOY"));
        let mut resolver = MapResolver(map);
        let table = TableContext::load_with_resolver(heap, &mut resolver).expect("load");
        assert_eq!(table.row_count(), 1);
        assert_eq!(
            table.get_row_string(0, 0x3001).expect("str").as_deref(),
            Some("Keep")
        );
        assert!(
            !table.cell_subnodes.contains_key(&decoy_nid),
            "integer cell 0x{decoy_nid:X} must not be collected as a string/binary HNID"
        );
    }
}
