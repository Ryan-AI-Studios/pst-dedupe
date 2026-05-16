//! Property Context (PC) — MS-PST §2.3.3
//!
//! A PC is a BTH-based key-value store mapping MAPI property tags to typed values.
//! The BTH uses 2-byte keys (property ID) and 6-byte data (type + value/HID).

use super::bth::{self, BthRecord};
use super::hn::{Heap, Hid};
use crate::crypto::CryptMethod;
use crate::error::{PstError, Result};
use crate::ndb::block;
use crate::ndb::btree::{BbtIndex, NbtIndex};
use crate::ndb::NodeId;
use byteorder::{ByteOrder, LittleEndian};

use std::io::{Read, Seek};

/// Maximum data block payload (used to determine single-block HN).
const MAX_BLOCK_DATA: usize = 8176;

/// A property value extracted from a PC.
#[derive(Debug, Clone)]
pub enum PropertyValue {
    I16(i16),
    I32(i32),
    I64(i64),
    Bool(bool),
    /// UTF-16LE decoded string.
    String(String),
    /// FILETIME as raw i64 (100ns intervals since 1601-01-01).
    Time(i64),
    /// Raw binary data.
    Binary(Vec<u8>),
    /// Multiple strings.
    MultiString(Vec<String>),
    /// Multiple binary values.
    MultiBinary(Vec<Vec<u8>>),
}

impl PropertyValue {
    /// Try to get as string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropertyValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get as i32.
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            PropertyValue::I32(v) => Some(*v),
            _ => None,
        }
    }

    /// Try to get as i64.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PropertyValue::I64(v) => Some(*v),
            _ => None,
        }
    }

    /// Try to get as time (FILETIME).
    pub fn as_time(&self) -> Option<i64> {
        match self {
            PropertyValue::Time(v) => Some(*v),
            _ => None,
        }
    }

    /// Try to get as bool.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropertyValue::Bool(v) => Some(*v),
            _ => None,
        }
    }
}

/// A loaded Property Context — provides typed property access.
pub struct PropContext {
    heap: Heap,
    records: Vec<BthRecord>,
}

impl PropContext {
    /// Load a PC from a node's data.
    ///
    /// `data` is the complete decrypted node data (from NDB).
    pub fn load(data: Vec<u8>) -> Result<Self> {
        let block_size = if data.len() <= MAX_BLOCK_DATA {
            data.len()
        } else {
            MAX_BLOCK_DATA
        };

        let heap = Heap::parse(data, block_size)?;
        let bth_header = bth::read_bth_header(&heap, heap.header.hid_user_root)?;
        let records = bth::collect_records(&heap, &bth_header)?;

        Ok(Self { heap, records })
    }

    /// Look up a property by tag (property ID, u16).
    ///
    /// Returns the typed value if found, or None.
    pub fn get(&self, prop_id: u16) -> Result<Option<PropertyValue>> {
        let key = prop_id.to_le_bytes();

        // Find the record
        let record = match self.records.iter().find(|r| r.key == key) {
            Some(r) => r,
            None => return Ok(None),
        };

        // Record data is: wPropType(2) + dwValueHnid(4)
        if record.data.len() < 6 {
            return Ok(None);
        }

        let prop_type = LittleEndian::read_u16(&record.data[0..2]);
        let value_hnid = LittleEndian::read_u32(&record.data[2..6]);

        self.resolve_value(prop_type, value_hnid)
    }

    /// Get a string property, returning None if missing or wrong type.
    pub fn get_string(&self, prop_id: u16) -> Result<Option<String>> {
        match self.get(prop_id)? {
            Some(PropertyValue::String(s)) => Ok(Some(s)),
            _ => Ok(None),
        }
    }

    /// Get an i32 property.
    pub fn get_i32(&self, prop_id: u16) -> Result<Option<i32>> {
        match self.get(prop_id)? {
            Some(PropertyValue::I32(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Get a boolean property.
    pub fn get_bool(&self, prop_id: u16) -> Result<Option<bool>> {
        match self.get(prop_id)? {
            Some(PropertyValue::Bool(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Get a time property (FILETIME).
    pub fn get_time(&self, prop_id: u16) -> Result<Option<i64>> {
        match self.get(prop_id)? {
            Some(PropertyValue::Time(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    /// Resolve a property value from its type and HNID.
    fn resolve_value(&self, prop_type: u16, value_hnid: u32) -> Result<Option<PropertyValue>> {
        match prop_type {
            // Fixed-size types — value is inline in dwValueHnid
            0x0002 => {
                // PtypInteger16
                Ok(Some(PropertyValue::I16(value_hnid as i16)))
            }
            0x0003 => {
                // PtypInteger32
                Ok(Some(PropertyValue::I32(value_hnid as i32)))
            }
            0x000B => {
                // PtypBoolean
                Ok(Some(PropertyValue::Bool(value_hnid != 0)))
            }

            // Variable-size: dwValueHnid is an HID into the heap
            0x001F => {
                // PtypString (UTF-16LE)
                let hid = Hid(value_hnid);
                if hid.is_null() {
                    return Ok(Some(PropertyValue::String(String::new())));
                }
                let bytes = self.heap.get(hid)?;
                let s = decode_utf16le(bytes)?;
                Ok(Some(PropertyValue::String(s)))
            }

            0x0014 => {
                // PtypInteger64
                // 8 bytes — stored in heap
                let hid = Hid(value_hnid);
                let bytes = self.heap.get(hid)?;
                if bytes.len() >= 8 {
                    Ok(Some(PropertyValue::I64(LittleEndian::read_i64(bytes))))
                } else {
                    // May be inline as 4 bytes if fits
                    Ok(Some(PropertyValue::I64(value_hnid as i64)))
                }
            }

            0x0040 => {
                // PtypTime (FILETIME, 8 bytes)
                let hid = Hid(value_hnid);
                let bytes = self.heap.get(hid)?;
                if bytes.len() >= 8 {
                    Ok(Some(PropertyValue::Time(LittleEndian::read_i64(bytes))))
                } else {
                    Ok(None)
                }
            }

            0x0102 => {
                // PtypBinary
                let hid = Hid(value_hnid);
                if hid.is_null() {
                    return Ok(Some(PropertyValue::Binary(Vec::new())));
                }
                let bytes = self.heap.get(hid)?;
                Ok(Some(PropertyValue::Binary(bytes.to_vec())))
            }

            _ => {
                // Unknown or unimplemented type — return None rather than error
                tracing::debug!("Unhandled property type: 0x{:04X}", prop_type);
                Ok(None)
            }
        }
    }
}

/// Load a Property Context from a node, reading its data from NDB.
pub fn load_pc<R: Read + Seek>(
    reader: &mut R,
    nbt: &NbtIndex,
    bbt: &BbtIndex,
    nid: NodeId,
    crypt: CryptMethod,
) -> Result<PropContext> {
    let data = block::read_block_data(
        reader,
        bbt,
        nbt.get(nid).ok_or(PstError::NodeNotFound(nid.0))?.bid_data,
        crypt,
    )?;

    PropContext::load(data)
}

/// Decode a UTF-16LE byte slice to a Rust String.
pub fn decode_utf16le(bytes: &[u8]) -> Result<String> {
    if !bytes.len().is_multiple_of(2) {
        // Odd byte count — truncate last byte
        let adjusted = &bytes[..bytes.len() - 1];
        return decode_utf16le(adjusted);
    }

    let u16_iter = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]));

    String::from_utf16(&u16_iter.collect::<Vec<_>>()).map_err(|_| PstError::InvalidUtf16)
}
