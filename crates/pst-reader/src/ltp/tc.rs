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
use crate::ndb::NodeId;
use byteorder::{ByteOrder, LittleEndian};

use std::io::{Read, Seek};

const MAX_BLOCK_DATA: usize = 8176;

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
}

impl TableContext {
    /// Load a TC from node data and optional subnode data.
    ///
    /// `data`: the node's main data (decrypted).
    /// `subnode_rows`: if the TC stores rows in a subnode, provide the assembled data.
    pub fn load(data: Vec<u8>, subnode_rows: Option<Vec<u8>>) -> Result<Self> {
        let block_size = if data.len() <= MAX_BLOCK_DATA {
            data.len()
        } else {
            MAX_BLOCK_DATA
        };

        let heap = Heap::parse(data, block_size)?;

        // Parse TCINFO from hidUserRoot
        let tc_data = heap.get(heap.header.hid_user_root)?;
        let info = Self::parse_tc_info(tc_data)?;

        let row_size = info.rgib[3] as usize; // TPI_TRAILER — total row width

        // Get row data
        let (row_data, row_count) = if let Some(sub_data) = subnode_rows {
            let count = sub_data.len().checked_div(row_size).unwrap_or(0);
            (sub_data, count)
        } else if info.hnid_rows != 0 {
            // Rows might be inline in the HN
            let hid = Hid(info.hnid_rows);
            if !hid.is_null() && hid.hid_type() == 0 {
                let inline_data = heap.get(hid)?.to_vec();
                let count = inline_data.len().checked_div(row_size).unwrap_or(0);
                (inline_data, count)
            } else {
                (Vec::new(), 0)
            }
        } else {
            (Vec::new(), 0)
        };

        let row_ids = Self::load_row_ids(&heap, &info, row_count)?;

        Ok(Self {
            heap,
            info,
            row_data,
            row_size,
            row_count,
            row_ids,
        })
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
            let bytes = self.heap.get(hid)?;
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

/// Load a Table Context from a node, reading data from NDB.
///
/// This handles both inline row data and subnode-based row data.
pub fn load_tc<R: Read + Seek>(
    reader: &mut R,
    nbt: &NbtIndex,
    bbt: &BbtIndex,
    nid: NodeId,
    crypt: CryptMethod,
) -> Result<TableContext> {
    let nbt_entry = nbt.get(nid).ok_or(PstError::NodeNotFound(nid.0))?;

    // Read main node data
    let data = block::read_block_data(reader, bbt, nbt_entry.bid_data, crypt)?;

    // Check if we need subnode data for rows
    // We first parse the TC header to check hnidRows
    // If hnidRows looks like a subnode NID (nidType != 0), read from subnode BTree
    let subnode_rows = if !nbt_entry.bid_sub.is_null() {
        // The TC may store rows in the subnode BTree
        // We'll try to collect all subnode entries and assemble their data
        let entries = block::list_subnode_entries(reader, bbt, nbt_entry.bid_sub)?;
        if !entries.is_empty() {
            let mut all_rows = Vec::new();
            for entry in &entries {
                let entry_data = block::read_block_data(reader, bbt, entry.bid_data, crypt)?;
                all_rows.extend_from_slice(&entry_data);
            }
            Some(all_rows)
        } else {
            None
        }
    } else {
        None
    };

    TableContext::load(data, subnode_rows)
}
