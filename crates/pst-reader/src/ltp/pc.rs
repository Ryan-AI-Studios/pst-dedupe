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

use std::collections::{HashMap, HashSet};
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
    /// Large PtypString/PtypBinary values that were too big for a single HN heap
    /// page and were moved to a subnode (MS-PST §2.3.3.3: an HNID whose low 5 bits
    /// are nonzero is a subnode NID, not a heap HID). Keyed by the raw NID (u32).
    ///
    /// Track 0068 production writer note: a spec-conformant PST writer MUST divert
    /// values larger than one heap page to a subnode (a single heap allocation
    /// cannot span multiple HN pages — this is inherent to the HN/HNPAGEMAP format,
    /// not a writer shortcut). Prior to this change, `resolve_value` treated any
    /// non-null HNID as a heap HID, so subnode-stored strings/binaries were either
    /// misread or silently dropped (falling through to `Ok(None)`). This was a
    /// genuine reader gap blocking round-trip verification of large bodies, fixed
    /// here rather than worked around in the writer.
    subnodes: HashMap<u32, Vec<u8>>,
}

impl PropContext {
    /// Load a PC from a node's data (no subnode-resolution context).
    ///
    /// `data` is the complete decrypted node data (from NDB). Large values stored
    /// via subnode will resolve to `None` when loaded this way — use
    /// [`Self::load_with_subnodes`] (or [`load_pc`], which wires it automatically)
    /// when the node may have subnode-stored properties.
    pub fn load(data: Vec<u8>) -> Result<Self> {
        Self::load_with_subnodes(data, HashMap::new())
    }

    /// Load a PC, additionally supplying pre-resolved subnode data for large
    /// (out-of-heap) PtypString/PtypBinary values, keyed by raw NID (u32).
    pub fn load_with_subnodes(data: Vec<u8>, subnodes: HashMap<u32, Vec<u8>>) -> Result<Self> {
        let block_size = if data.len() <= MAX_BLOCK_DATA {
            data.len()
        } else {
            MAX_BLOCK_DATA
        };

        let heap = Heap::parse(data, block_size)?;
        let bth_header = bth::read_bth_header(&heap, heap.header.hid_user_root)?;
        let records = bth::collect_records(&heap, &bth_header)?;

        Ok(Self {
            heap,
            records,
            subnodes,
        })
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

    /// Get a binary property (heap-resident PtypBinary only).
    ///
    /// When the property's `dwValueHnid` is a subnode NID rather than an HID,
    /// returns `None` — callers should resolve the subnode via NDB.
    pub fn get_binary(&self, prop_id: u16) -> Result<Option<Vec<u8>>> {
        match self.get(prop_id)? {
            Some(PropertyValue::Binary(b)) => Ok(Some(b)),
            _ => Ok(None),
        }
    }

    /// Raw `(prop_type, value_hnid)` for a property tag, if present.
    ///
    /// Used by attachment streaming to distinguish HID (heap) vs NID (subnode)
    /// storage of large binary properties.
    pub fn get_raw_hnid(&self, prop_id: u16) -> Option<(u16, u32)> {
        let key = prop_id.to_le_bytes();
        let record = self.records.iter().find(|r| r.key == key)?;
        if record.data.len() < 6 {
            return None;
        }
        let prop_type = LittleEndian::read_u16(&record.data[0..2]);
        let value_hnid = LittleEndian::read_u32(&record.data[2..6]);
        Some((prop_type, value_hnid))
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

            // Variable-size: dwValueHnid is an HID into the heap, or (when the
            // value didn't fit in one heap page) a subnode NID — see `subnodes`.
            0x001F => {
                // PtypString (UTF-16LE)
                let hid = Hid(value_hnid);
                if hid.is_null() {
                    return Ok(Some(PropertyValue::String(String::new())));
                }
                if hid.hid_type() != 0 {
                    // Subnode-stored large value, not a heap HID.
                    return match self.subnodes.get(&value_hnid) {
                        Some(bytes) => Ok(Some(PropertyValue::String(decode_utf16le(bytes)?))),
                        None => Ok(None),
                    };
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
                // PtypBinary — HID into heap when hidType == 0; subnode NID otherwise.
                let hid = Hid(value_hnid);
                if hid.is_null() {
                    return Ok(Some(PropertyValue::Binary(Vec::new())));
                }
                // Subnode NIDs have non-zero type bits (low 5); heap HIDs have type 0.
                if hid.hid_type() != 0 {
                    return match self.subnodes.get(&value_hnid) {
                        Some(bytes) => Ok(Some(PropertyValue::Binary(bytes.clone()))),
                        None => Ok(None),
                    };
                }
                let bytes = self.heap.get(hid)?;
                Ok(Some(PropertyValue::Binary(bytes.to_vec())))
            }

            // PtypString8 (often used for HTML body in older stores) — treat as Latin-1-ish bytes → lossy UTF-8
            0x001E => {
                let hid = Hid(value_hnid);
                if hid.is_null() {
                    return Ok(Some(PropertyValue::String(String::new())));
                }
                if hid.hid_type() != 0 {
                    return Ok(None);
                }
                let bytes = self.heap.get(hid)?;
                let s = String::from_utf8_lossy(bytes).into_owned();
                Ok(Some(PropertyValue::String(s)))
            }

            _ => {
                // Unknown or unimplemented type — return None rather than error
                tracing::debug!("Unhandled property type: 0x{:04X}", prop_type);
                Ok(None)
            }
        }
    }
}

/// Scan a node's own PC records for `PtypString`/`PtypBinary` values whose
/// `dwValueHnid` is a **subnode NID** rather than a heap HID (MS-PST §2.3.3.3:
/// a non-zero `hidType` on the low 5 bits means "this is a subnode reference,
/// not a heap allocation"). Returns the set of raw NIDs actually referenced
/// this way.
///
/// This is a lightweight first pass so [`load_pc`] can fetch **only** those
/// specific subnode entries afterward, instead of eagerly reading every entry
/// in the node's subnode BTree — which, for a message with attachments,
/// includes the attachment table subnode and every attachment's own top-level
/// subnode entry, none of which are referenced by this node's own PC records
/// at all. Parses the node's HN heap and BTH once; this duplicates a small
/// amount of work `PropContext::load_with_subnodes` does on the second,
/// targeted pass, but is far cheaper than materializing every subnode's block
/// data unconditionally (this repo's Core Mandate #7: bounded memory use over
/// a potentially huge PST).
fn referenced_subnode_nids(data: &[u8]) -> Result<HashSet<u32>> {
    let block_size = if data.len() <= MAX_BLOCK_DATA {
        data.len()
    } else {
        MAX_BLOCK_DATA
    };
    let heap = Heap::parse(data.to_vec(), block_size)?;
    let bth_header = bth::read_bth_header(&heap, heap.header.hid_user_root)?;
    let records = bth::collect_records(&heap, &bth_header)?;

    let mut needed = HashSet::new();
    for record in &records {
        if record.data.len() < 6 {
            continue;
        }
        let prop_type = LittleEndian::read_u16(&record.data[0..2]);
        let value_hnid = LittleEndian::read_u32(&record.data[2..6]);
        if matches!(prop_type, 0x001F | 0x0102) {
            let hid = Hid(value_hnid);
            if hid.hid_type() != 0 {
                needed.insert(value_hnid);
            }
        }
    }
    Ok(needed)
}

/// Load a Property Context from a node, reading its data from NDB.
///
/// When the node has a subnode BTree (`bid_sub`), only the subnode entries
/// actually referenced by this node's own `PtypString`/`PtypBinary` PC records
/// (typically 0, 1, or 2 — a large body and/or HTML body) are read and made
/// available to [`PropContext::resolve_value`]. Everything else in the
/// subnode BTree (e.g. an attachment table, or an attachment's own top-level
/// subnode entry) is left untouched — targeted via [`referenced_subnode_nids`]
/// rather than eagerly materializing every direct subnode entry, which used to
/// happen unconditionally here regardless of whether the caller ever queried a
/// subnode-stored value (see track 0068 P2 fix).
pub fn load_pc<R: Read + Seek>(
    reader: &mut R,
    nbt: &NbtIndex,
    bbt: &BbtIndex,
    nid: NodeId,
    crypt: CryptMethod,
) -> Result<PropContext> {
    let nbt_entry = nbt.get(nid).ok_or(PstError::NodeNotFound(nid.0))?;
    let data = block::read_block_data(reader, bbt, nbt_entry.bid_data, crypt)?;

    let mut subnodes = HashMap::new();
    if !nbt_entry.bid_sub.is_null() {
        let needed = referenced_subnode_nids(&data)?;
        if !needed.is_empty() {
            let entries = block::list_subnode_entries(reader, bbt, nbt_entry.bid_sub)?;
            for entry in entries {
                let raw_nid = entry.nid.0 as u32;
                if needed.contains(&raw_nid) {
                    let bytes = block::read_block_data(reader, bbt, entry.bid_data, crypt)?;
                    subnodes.insert(raw_nid, bytes);
                }
            }
        }
    }

    PropContext::load_with_subnodes(data, subnodes)
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

#[cfg(test)]
mod targeted_subnode_tests {
    use super::*;
    use crate::crypto::CryptMethod;
    use crate::header::Bref;
    use crate::ndb::btree::{BbtEntry, NbtEntry};
    use crate::ndb::BlockId;
    use std::io::Cursor;

    fn align64(n: usize) -> usize {
        (n + 63) & !63
    }

    /// Append `data` as a raw NDB block (payload + zero-padding to 64-byte
    /// alignment + a 16-byte trailer) to `buf`, returning its file offset.
    /// `read_raw_block`'s CRC/BID checks only `tracing::warn!` on mismatch
    /// rather than erroring (see `ndb::block::validate_block_trailer`), so an
    /// all-zero trailer is fine for test purposes.
    fn push_raw_block(buf: &mut Vec<u8>, data: &[u8]) -> u64 {
        let offset = buf.len() as u64;
        buf.extend_from_slice(data);
        let padded = align64(data.len());
        buf.resize(buf.len() + (padded - data.len()), 0);
        buf.extend_from_slice(&[0u8; 16]);
        offset
    }

    fn utf16le(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
    }

    /// Hand-build a minimal single-block PC (Property Context) HN heap:
    /// - `PID_TAG_SUBJECT` (0x0037): `PtypString` (0x001F), heap-resident.
    /// - `PID_TAG_BODY` (0x1000): `PtypBinary` (0x0102), whose `dwValueHnid`
    ///   is `subnode_body_nid` — a **subnode NID** (low 5 bits nonzero), not a
    ///   heap HID, per MS-PST §2.3.3.3.
    fn build_synthetic_pc_data(subnode_body_nid: u32) -> Vec<u8> {
        // Allocation layout (single block, so alloc offsets are absolute
        // into `data`): #1 Hid 0x20 = BTH header; #2 Hid 0x40 = BTH leaf
        // records; #3 Hid 0x60 = subject string "Test" (UTF-16LE).
        let bth_header = [0xB5u8, 0x02, 0x06, 0x00, 0x40, 0x00, 0x00, 0x00];

        let mut leaf_records = Vec::new();
        leaf_records.extend_from_slice(&0x0037u16.to_le_bytes()); // PID_TAG_SUBJECT
        leaf_records.extend_from_slice(&0x001Fu16.to_le_bytes()); // PtypString
        leaf_records.extend_from_slice(&0x60u32.to_le_bytes()); // heap Hid #3
        leaf_records.extend_from_slice(&0x1000u16.to_le_bytes()); // PID_TAG_BODY
        leaf_records.extend_from_slice(&0x0102u16.to_le_bytes()); // PtypBinary
        leaf_records.extend_from_slice(&subnode_body_nid.to_le_bytes()); // subnode NID

        let subject_bytes = utf16le("Test");

        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_le_bytes()); // ib_hnpm placeholder, patched below
        data.push(0xEC); // bSig
        data.push(0x6C); // bClientSig = PC (BTH)
        data.extend_from_slice(&0x20u32.to_le_bytes()); // hidUserRoot = alloc #1
        data.extend_from_slice(&0u32.to_le_bytes()); // rgbFillLevel

        data.extend_from_slice(&bth_header); // alloc #1 (Hid 0x20)
        data.extend_from_slice(&leaf_records); // alloc #2 (Hid 0x40)
        data.extend_from_slice(&subject_bytes); // alloc #3 (Hid 0x60)

        let ib_hnpm = data.len() as u16;
        data[0..2].copy_from_slice(&ib_hnpm.to_le_bytes());

        let alloc1_start = 12u16;
        let alloc2_start = alloc1_start + bth_header.len() as u16;
        let alloc3_start = alloc2_start + leaf_records.len() as u16;
        let alloc3_end = alloc3_start + subject_bytes.len() as u16;

        // HNPAGEMAP: cAlloc(2) + cFree(2) + rgibAlloc[(cAlloc+1) x u16].
        data.extend_from_slice(&3u16.to_le_bytes()); // cAlloc
        data.extend_from_slice(&0u16.to_le_bytes()); // cFree
        data.extend_from_slice(&alloc1_start.to_le_bytes());
        data.extend_from_slice(&alloc2_start.to_le_bytes());
        data.extend_from_slice(&alloc3_start.to_le_bytes());
        data.extend_from_slice(&alloc3_end.to_le_bytes());

        data
    }

    #[test]
    fn referenced_subnode_nids_finds_only_the_body_subnode() {
        const SUBNODE_BODY_NID: u32 = 0x1F; // low 5 bits nonzero -> subnode NID
        let data = build_synthetic_pc_data(SUBNODE_BODY_NID);

        let needed = referenced_subnode_nids(&data).expect("scan pc records");
        assert_eq!(
            needed,
            HashSet::from([SUBNODE_BODY_NID]),
            "only the PtypBinary body's subnode NID should be discovered — not \
             any other subnode entry that might exist in the node's subnode \
             BTree (e.g. an attachment table, or an attachment's own \
             top-level subnode entry), since neither is referenced by any of \
             this node's own PC records"
        );
    }

    #[test]
    fn load_pc_does_not_fetch_unreferenced_subnode_entries() {
        // Two entries share the node's subnode BTree: one referenced by the
        // PC's PtypBinary body property (must be fetched and resolvable),
        // and one shaped like an attachment's own top-level subnode entry
        // that is NOT referenced by any PC record (must never be fetched —
        // its data BID is deliberately absent from the BBT, so the whole
        // call fails if the old eager-fetch-everything code path ever reads
        // it).
        const SUBNODE_BODY_NID: u32 = 0x1F;
        const SUBNODE_ATTACH_NID: u32 = 0x3F;
        const MESSAGE_NID: u64 = 0x21_0021;
        const PC_DATA_BID: u64 = 0x10;
        const SLBLOCK_BID: u64 = 0x12;
        const BODY_DATA_BID: u64 = 0x14;
        const MISSING_ATTACH_DATA_BID: u64 = 0x9999; // intentionally never added to BBT

        let pc_data = build_synthetic_pc_data(SUBNODE_BODY_NID);
        let body_bytes = b"HELLO_SUBNODE_BODY_BYTES".to_vec();

        // SLBLOCK (subnode leaf, §2.4.4.3.1): btype(1)+cLevel(1)+cEntries(2)
        // +reserved(4), then SLENTRY(24 bytes each): nid(8)+bidData(8)+bidSub(8).
        let mut slblock = Vec::new();
        slblock.push(0x02);
        slblock.push(0x00);
        slblock.extend_from_slice(&2u16.to_le_bytes());
        slblock.extend_from_slice(&0u32.to_le_bytes());
        slblock.extend_from_slice(&(SUBNODE_BODY_NID as u64).to_le_bytes());
        slblock.extend_from_slice(&BODY_DATA_BID.to_le_bytes());
        slblock.extend_from_slice(&0u64.to_le_bytes());
        slblock.extend_from_slice(&(SUBNODE_ATTACH_NID as u64).to_le_bytes());
        slblock.extend_from_slice(&MISSING_ATTACH_DATA_BID.to_le_bytes());
        slblock.extend_from_slice(&0u64.to_le_bytes());

        let mut file = Vec::new();
        let pc_offset = push_raw_block(&mut file, &pc_data);
        let slblock_offset = push_raw_block(&mut file, &slblock);
        let body_offset = push_raw_block(&mut file, &body_bytes);

        let mut bbt_entries = HashMap::new();
        bbt_entries.insert(
            PC_DATA_BID,
            BbtEntry {
                bref: Bref {
                    bid: PC_DATA_BID,
                    ib: pc_offset,
                },
                cb: pc_data.len() as u16,
                c_ref: 1,
            },
        );
        bbt_entries.insert(
            SLBLOCK_BID,
            BbtEntry {
                bref: Bref {
                    bid: SLBLOCK_BID,
                    ib: slblock_offset,
                },
                cb: slblock.len() as u16,
                c_ref: 1,
            },
        );
        bbt_entries.insert(
            BODY_DATA_BID,
            BbtEntry {
                bref: Bref {
                    bid: BODY_DATA_BID,
                    ib: body_offset,
                },
                cb: body_bytes.len() as u16,
                c_ref: 1,
            },
        );
        // MISSING_ATTACH_DATA_BID deliberately has no BBT entry.
        let bbt = BbtIndex::from_entries_for_test(bbt_entries);

        let mut nbt_entries = HashMap::new();
        nbt_entries.insert(
            MESSAGE_NID,
            NbtEntry {
                nid: NodeId(MESSAGE_NID),
                bid_data: BlockId(PC_DATA_BID),
                bid_sub: BlockId(SLBLOCK_BID),
                nid_parent: 0,
            },
        );
        let nbt = NbtIndex::from_entries_for_test(nbt_entries);

        let mut reader = Cursor::new(file);
        let pc = load_pc(
            &mut reader,
            &nbt,
            &bbt,
            NodeId(MESSAGE_NID),
            CryptMethod::None,
        )
        .expect(
            "load_pc must succeed without ever fetching the unreferenced \
             attachment-shaped subnode entry (its data BID is absent from \
             the BBT and would error out if it were fetched)",
        );

        let body = pc
            .get_binary(0x1000)
            .expect("get body")
            .expect("body present");
        assert_eq!(
            body, body_bytes,
            "the referenced subnode body must still resolve correctly"
        );

        let subject = pc
            .get_string(0x0037)
            .expect("get subject")
            .expect("subject present");
        assert_eq!(subject, "Test");
    }
}
