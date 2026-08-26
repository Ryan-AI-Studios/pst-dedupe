//! Named Property Lookup Map (NPMAP) — MS-PST §2.4.7 / Name-to-ID-Map.
//!
//! Parses the store-level PC at [`NID_NAME_TO_ID_MAP`](crate::ndb::nid::NID_NAME_TO_ID_MAP)
//! (`0x61`) so callers can resolve allowlisted `(GUID, name|LID)` keys to NPIDs
//! (`0x8000 + wPropIdx`).
//!
//! **Degrade policy (0084):** corrupt or missing maps yield an empty
//! [`NameIdMap`]; callers must not hard-fail PST open solely for NPMAP failure.
//!
//! Spec anchors (access date 2026-07-29):
//! - MS-PST Named Property Lookup Map
//! - MS-OXPROPS `PidNameAttachmentProviderType` / PSETID_Attachment

use std::collections::HashMap;

use byteorder::{ByteOrder, LittleEndian};

use crate::error::Result;
use crate::ltp::pc::{self, PropContext};
use crate::ndb::nid;
use crate::PstFile;

/// Property IDs on the Name-to-ID-Map PC that hold the three streams (MS-PST).
const PID_NAMEID_GUID_STREAM: u16 = 0x0002;
const PID_NAMEID_ENTRY_STREAM: u16 = 0x0003;
const PID_NAMEID_STRING_STREAM: u16 = 0x0004;

/// NPID base: named property IDs start at `0x8000`.
pub const NPID_BASE: u16 = 0x8000;

/// PS_MAPI — used when NAMEID `wGuid` index is **1** (MS-PST / MS-OXMSG).
pub const PS_MAPI: [u8; 16] = [
    0x28, 0x03, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// PS_PUBLIC_STRINGS — used when NAMEID `wGuid` index is **2** (MS-PST / MS-OXMSG).
pub const PS_PUBLIC_STRINGS: [u8; 16] = [
    0x29, 0x03, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// PSETID_Attachment — MS-OXPROPS property set for modern/cloud attach props.
/// `{96357F7F-59E1-47D0-99A7-46515C183B54}` (mixed-endian MS GUID layout).
pub const PSETID_ATTACHMENT: [u8; 16] = [
    0x7F, 0x5F, 0x35, 0x96, // Data1 0x96357F7F LE
    0xE1, 0x59, // Data2 0x59E1 LE
    0xD0, 0x47, // Data3 0x47D0 LE
    0x99, 0xA7, 0x46, 0x51, 0x5C, 0x18, 0x3B, 0x54, // Data4
];

/// Public string name for `PidNameAttachmentProviderType` (not a numeric LID).
pub const NAME_ATTACHMENT_PROVIDER_TYPE: &str = "AttachmentProviderType";

/// Public string name for `PidNameAttachmentPermissionType` (MS-OXCMSG §2.2.2.28).
pub const NAME_ATTACHMENT_PERMISSION_TYPE: &str = "AttachmentPermissionType";

/// Documented MS-OXCMSG PermissionType values (fixtures only — not a reject list).
pub const PERMISSION_NONE: i32 = 0;
pub const PERMISSION_VIEW: i32 = 1;
pub const PERMISSION_EDIT: i32 = 2;

/// Identifier half of a named property key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NamedPropId {
    /// 16-bit numerical LID (dispid).
    Lid(u16),
    /// String name (case-sensitive per MS-PST string stream).
    Name(String),
}

/// Full named-property key: property-set GUID + name or LID.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NamedPropKey {
    /// Property-set GUID in MS mixed-endian 16-byte form.
    pub guid: [u8; 16],
    pub kind: NamedPropId,
}

/// Parsed Name-to-ID-Map: resolve `(GUID, name|LID) → NPID` and reverse.
#[derive(Debug, Clone, Default)]
pub struct NameIdMap {
    forward: HashMap<NamedPropKey, u16>,
    reverse: HashMap<u16, NamedPropKey>,
    /// True when the map node was missing or parse degraded.
    pub degraded: bool,
}

impl NameIdMap {
    /// Empty map (missing/corrupt NPMAP or no entries).
    pub fn empty_degraded() -> Self {
        Self {
            forward: HashMap::new(),
            reverse: HashMap::new(),
            degraded: true,
        }
    }

    /// Empty map that parsed cleanly but had no entries.
    pub fn empty() -> Self {
        Self {
            forward: HashMap::new(),
            reverse: HashMap::new(),
            degraded: false,
        }
    }

    /// Number of resolved named properties.
    pub fn len(&self) -> usize {
        self.forward.len()
    }

    /// True when no entries are present.
    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }

    /// Resolve `(GUID, name|LID)` to NPID (`0x8000 + wPropIdx`).
    pub fn resolve(&self, key: &NamedPropKey) -> Option<u16> {
        self.forward.get(key).copied()
    }

    /// Resolve by raw GUID bytes + string name.
    pub fn resolve_name(&self, guid: &[u8; 16], name: &str) -> Option<u16> {
        self.resolve(&NamedPropKey {
            guid: *guid,
            kind: NamedPropId::Name(name.to_string()),
        })
    }

    /// Resolve by raw GUID bytes + LID.
    pub fn resolve_lid(&self, guid: &[u8; 16], lid: u16) -> Option<u16> {
        self.resolve(&NamedPropKey {
            guid: *guid,
            kind: NamedPropId::Lid(lid),
        })
    }

    /// Reverse lookup NPID → key (debugging / tests).
    pub fn reverse(&self, npid: u16) -> Option<&NamedPropKey> {
        self.reverse.get(&npid)
    }

    /// Resolve `PidNameAttachmentProviderType` (PSETID_Attachment + name).
    pub fn attachment_provider_type_npid(&self) -> Option<u16> {
        self.resolve_name(&PSETID_ATTACHMENT, NAME_ATTACHMENT_PROVIDER_TYPE)
    }

    /// Resolve `PidNameAttachmentPermissionType` (PSETID_Attachment + name).
    pub fn attachment_permission_type_npid(&self) -> Option<u16> {
        self.resolve_name(&PSETID_ATTACHMENT, NAME_ATTACHMENT_PERMISSION_TYPE)
    }

    /// Parse from the three NPMAP streams (unit-test and production entry point).
    ///
    /// On structural corruption of individual entries, skips the bad entry and
    /// continues (partial map). Completely unusable streams still return a map
    /// (possibly empty) with `degraded = true` only when the caller marks it —
    /// this function sets `degraded = false` on successful walk.
    pub fn from_streams(guid_stream: &[u8], entry_stream: &[u8], string_stream: &[u8]) -> Self {
        let mut map = Self::empty();
        if entry_stream.is_empty() {
            return map;
        }
        // Each NAMEID is 8 bytes.
        let mut offset = 0usize;
        while offset + 8 <= entry_stream.len() {
            let chunk = &entry_stream[offset..offset + 8];
            offset += 8;
            match parse_nameid_entry(chunk, guid_stream, string_stream) {
                Ok(Some((key, npid))) => {
                    map.forward.insert(key.clone(), npid);
                    map.reverse.insert(npid, key);
                }
                Ok(None) => {
                    // Skipped unresolvable entry (bad string offset / GUID index).
                    map.degraded = true;
                }
                Err(_) => {
                    map.degraded = true;
                }
            }
        }
        // Trailing partial entry is structural noise.
        if offset < entry_stream.len() {
            map.degraded = true;
        }
        map
    }

    /// Parse from a loaded Name-to-ID-Map Property Context.
    pub fn from_prop_context(pc: &PropContext) -> Result<Self> {
        let guid = pc.get_binary(PID_NAMEID_GUID_STREAM)?.unwrap_or_default();
        let entry = pc.get_binary(PID_NAMEID_ENTRY_STREAM)?.unwrap_or_default();
        let string = pc.get_binary(PID_NAMEID_STRING_STREAM)?.unwrap_or_default();
        Ok(Self::from_streams(&guid, &entry, &string))
    }
}

/// Parse one 8-byte NAMEID record.
///
/// Layout (MS-PST NAMEID):
/// - `dwPropertyID` (u32 LE): LID when N=0; string-stream byte offset when N=1
/// - `wGuidN` (u16 LE): bit0 = N (1=string name); bits1–15 = wGuid index
/// - `wPropIdx` (u16 LE): NPID = `0x8000 + wPropIdx`
fn parse_nameid_entry(
    entry: &[u8],
    guid_stream: &[u8],
    string_stream: &[u8],
) -> Result<Option<(NamedPropKey, u16)>> {
    if entry.len() < 8 {
        return Ok(None);
    }
    let property_id = LittleEndian::read_u32(&entry[0..4]);
    let guid_n = LittleEndian::read_u16(&entry[4..6]);
    let prop_idx = LittleEndian::read_u16(&entry[6..8]);
    let is_string = (guid_n & 0x0001) != 0;
    let w_guid = guid_n >> 1;

    let guid = match resolve_guid_index(w_guid, guid_stream) {
        Some(g) => g,
        None => return Ok(None),
    };

    let kind = if is_string {
        match read_string_stream_name(string_stream, property_id as usize) {
            Some(name) => NamedPropId::Name(name),
            None => return Ok(None),
        }
    } else {
        // LID is the low 16 bits of dwPropertyID for numerical named props.
        NamedPropId::Lid(property_id as u16)
    };

    let npid = NPID_BASE.saturating_add(prop_idx);
    Ok(Some((NamedPropKey { guid, kind }, npid)))
}

/// Resolve NAMEID `wGuid` (bits 1–15 of `wGuidN`) to a property-set GUID.
///
/// MS-PST / MS-OXMSG Index and Kind Information:
/// | Value | GUID |
/// | 0 | no GUID (`NAMEID_GUID_NONE`) — skip entry |
/// | 1 | `PS_MAPI` |
/// | 2 | `PS_PUBLIC_STRINGS` |
/// | ≥ 3 | GUID stream slot `value - 3` (byte offset `(value - 3) * 16`) |
fn resolve_guid_index(w_guid: u16, guid_stream: &[u8]) -> Option<[u8; 16]> {
    match w_guid {
        0 => None, // NAMEID_GUID_NONE
        1 => Some(PS_MAPI),
        2 => Some(PS_PUBLIC_STRINGS),
        n => {
            // Index into GUID stream: n - 3.
            let idx = (n as usize).checked_sub(3)?;
            let off = idx.checked_mul(16)?;
            if off + 16 > guid_stream.len() {
                return None;
            }
            let mut g = [0u8; 16];
            g.copy_from_slice(&guid_stream[off..off + 16]);
            Some(g)
        }
    }
}

/// String Stream entry: `ulLength` (u32 LE, UTF-16LE byte length) + data + pad to 4.
fn read_string_stream_name(string_stream: &[u8], offset: usize) -> Option<String> {
    if offset.checked_add(4)? > string_stream.len() {
        return None;
    }
    let len = LittleEndian::read_u32(&string_stream[offset..offset + 4]) as usize;
    let data_start = offset + 4;
    let data_end = data_start.checked_add(len)?;
    if data_end > string_stream.len() {
        return None;
    }
    let bytes = &string_stream[data_start..data_end];
    // Odd length → truncate last byte (defensive).
    let even = bytes.len() - (bytes.len() % 2);
    let u16s: Vec<u16> = bytes[..even]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    // Strip trailing NUL if present.
    let end = u16s.iter().position(|&c| c == 0).unwrap_or(u16s.len());
    String::from_utf16(&u16s[..end]).ok()
}

/// Build a minimal string-stream blob for tests / fixture helpers.
pub fn encode_string_stream_entry(name: &str) -> Vec<u8> {
    let utf16: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let mut out = Vec::with_capacity(4 + utf16.len() + 4);
    out.extend_from_slice(&(utf16.len() as u32).to_le_bytes());
    out.extend_from_slice(&utf16);
    // Pad to 4-byte boundary.
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// Build one 8-byte NAMEID entry for tests / fixture helpers.
pub fn encode_nameid_entry(
    property_id: u32,
    is_string: bool,
    w_guid: u16,
    prop_idx: u16,
) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0..4].copy_from_slice(&property_id.to_le_bytes());
    let guid_n = (w_guid << 1) | (if is_string { 1 } else { 0 });
    out[4..6].copy_from_slice(&guid_n.to_le_bytes());
    out[6..8].copy_from_slice(&prop_idx.to_le_bytes());
    out
}

impl PstFile {
    /// Load (and cache) the store Name-to-ID-Map.
    ///
    /// Missing node or parse failure → empty degraded map (never hard-fails open).
    pub fn name_id_map(&mut self) -> &NameIdMap {
        if self.name_id_map.is_none() {
            let map = self.load_name_id_map_uncached();
            self.name_id_map = Some(map);
        }
        // After load: Some. `get_or_insert_with` covers the impossible None without unwrap.
        self.name_id_map
            .get_or_insert_with(NameIdMap::empty_degraded)
    }

    /// Convenience: resolve allowlisted AttachmentProviderType NPID when map present.
    pub fn attachment_provider_type_npid(&mut self) -> Option<u16> {
        self.name_id_map().attachment_provider_type_npid()
    }

    /// Convenience: resolve allowlisted AttachmentPermissionType NPID when map present.
    pub fn attachment_permission_type_npid(&mut self) -> Option<u16> {
        self.name_id_map().attachment_permission_type_npid()
    }

    fn load_name_id_map_uncached(&mut self) -> NameIdMap {
        let crypt = self.header.crypt_method;
        match pc::load_pc(
            &mut self.reader,
            &self.nbt,
            &self.bbt,
            nid::NID_NAME_TO_ID_MAP,
            crypt,
        ) {
            Ok(pc) => match NameIdMap::from_prop_context(&pc) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("NPMAP prop-context parse failed (degrading): {e}");
                    NameIdMap::empty_degraded()
                }
            },
            Err(e) => {
                tracing::debug!("NPMAP node missing or unreadable (degrading): {e}");
                NameIdMap::empty_degraded()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_streams_yield_empty_map() {
        let m = NameIdMap::from_streams(&[], &[], &[]);
        assert!(m.is_empty());
        assert!(!m.degraded);
        assert!(m
            .resolve_name(&PSETID_ATTACHMENT, NAME_ATTACHMENT_PROVIDER_TYPE)
            .is_none());
    }

    #[test]
    fn string_named_attachment_provider_type_hit() {
        // GUID stream: one GUID at slot 0 → wGuid = 3
        // (0=none, 1=PS_MAPI, 2=PS_PUBLIC_STRINGS, ≥3 = stream[n-3]).
        let guid_stream = PSETID_ATTACHMENT.to_vec();
        let string_stream = encode_string_stream_entry(NAME_ATTACHMENT_PROVIDER_TYPE);
        // property_id = 0 (string offset 0), is_string, w_guid=3, prop_idx=0 → NPID 0x8000
        let entry = encode_nameid_entry(0, true, 3, 0);
        let m = NameIdMap::from_streams(&guid_stream, &entry, &string_stream);
        assert_eq!(m.len(), 1);
        assert!(!m.degraded);
        let npid = m
            .attachment_provider_type_npid()
            .expect("AttachmentProviderType must resolve");
        assert_eq!(npid, 0x8000);
        let rev = m.reverse(npid).expect("reverse");
        assert_eq!(rev.guid, PSETID_ATTACHMENT);
        assert_eq!(
            rev.kind,
            NamedPropId::Name(NAME_ATTACHMENT_PROVIDER_TYPE.to_string())
        );
    }

    #[test]
    fn string_named_attachment_permission_type_hit() {
        let guid_stream = PSETID_ATTACHMENT.to_vec();
        let string_stream = encode_string_stream_entry(NAME_ATTACHMENT_PERMISSION_TYPE);
        let entry = encode_nameid_entry(0, true, 3, 0);
        let m = NameIdMap::from_streams(&guid_stream, &entry, &string_stream);
        let npid = m
            .attachment_permission_type_npid()
            .expect("AttachmentPermissionType must resolve");
        assert_eq!(npid, 0x8000);
        let rev = m.reverse(npid).expect("reverse");
        assert_eq!(rev.guid, PSETID_ATTACHMENT);
        assert_eq!(
            rev.kind,
            NamedPropId::Name(NAME_ATTACHMENT_PERMISSION_TYPE.to_string())
        );
    }

    #[test]
    fn name_attachment_permission_type_bytes_and_psetid() {
        assert_eq!(
            NAME_ATTACHMENT_PERMISSION_TYPE.as_bytes(),
            b"AttachmentPermissionType"
        );
        assert_eq!(PERMISSION_NONE, 0);
        assert_eq!(PERMISSION_VIEW, 1);
        assert_eq!(PERMISSION_EDIT, 2);
        // Same PSETID as ProviderType (mixed-endian {96357F7F-59E1-47D0-99A7-46515C183B54}).
        assert_eq!(PSETID_ATTACHMENT[0], 0x7F);
        assert_eq!(PSETID_ATTACHMENT[3], 0x96);
        assert_eq!(
            &PSETID_ATTACHMENT[8..],
            &[0x99, 0xA7, 0x46, 0x51, 0x5C, 0x18, 0x3B, 0x54]
        );
    }

    #[test]
    fn lid_named_prop_resolves() {
        let guid_stream = PSETID_ATTACHMENT.to_vec();
        // LID 0x8501, numerical, first GUID stream entry (w_guid=3), prop_idx=5 → NPID 0x8005
        let entry = encode_nameid_entry(0x8501, false, 3, 5);
        let m = NameIdMap::from_streams(&guid_stream, &entry, &[]);
        assert_eq!(m.resolve_lid(&PSETID_ATTACHMENT, 0x8501), Some(0x8005));
        assert!(m.resolve_lid(&PSETID_ATTACHMENT, 0x0001).is_none());
    }

    #[test]
    fn unknown_name_misses() {
        let guid_stream = PSETID_ATTACHMENT.to_vec();
        let string_stream = encode_string_stream_entry(NAME_ATTACHMENT_PROVIDER_TYPE);
        let entry = encode_nameid_entry(0, true, 3, 0);
        let m = NameIdMap::from_streams(&guid_stream, &entry, &string_stream);
        assert!(m.resolve_name(&PSETID_ATTACHMENT, "NoSuchProp").is_none());
        assert!(m
            .resolve_name(&PS_MAPI, NAME_ATTACHMENT_PROVIDER_TYPE)
            .is_none());
    }

    #[test]
    fn corrupt_entry_degrades_without_panic() {
        // Short entry stream (3 bytes) → degraded, empty.
        let m = NameIdMap::from_streams(&[], &[0x01, 0x02, 0x03], &[]);
        assert!(m.is_empty());
        assert!(m.degraded);

        // Bad string offset on a full 8-byte entry (w_guid=3 → first stream GUID).
        let entry = encode_nameid_entry(0x00FF_FFFF, true, 3, 0);
        let m2 = NameIdMap::from_streams(&PSETID_ATTACHMENT, &entry, &[]);
        assert!(m2.is_empty());
        assert!(m2.degraded);
    }

    #[test]
    fn nameid_guid_none_skips_entry() {
        // w_guid=0 → NAMEID_GUID_NONE → skip (degraded partial).
        let entry = encode_nameid_entry(0x1234, false, 0, 0);
        let m = NameIdMap::from_streams(&[], &entry, &[]);
        assert!(m.is_empty());
        assert!(m.degraded);
    }

    #[test]
    fn ps_mapi_index() {
        // w_guid=1 → PS_MAPI (numerical LID), no GUID stream needed.
        let entry = encode_nameid_entry(0x001A, false, 1, 7);
        let m = NameIdMap::from_streams(&[], &entry, &[]);
        assert_eq!(m.resolve_lid(&PS_MAPI, 0x001A), Some(0x8007));
        assert!(!m.degraded);
    }

    #[test]
    fn ps_public_strings_index() {
        let string_stream = encode_string_stream_entry("Keywords");
        // w_guid=2 → PS_PUBLIC_STRINGS, no GUID stream needed
        let entry = encode_nameid_entry(0, true, 2, 3);
        let m = NameIdMap::from_streams(&[], &entry, &string_stream);
        assert_eq!(m.resolve_name(&PS_PUBLIC_STRINGS, "Keywords"), Some(0x8003));
        assert!(!m.degraded);
    }

    #[test]
    fn second_guid_stream_slot_uses_w_guid_4() {
        // Two GUIDs in stream: slot0 → w_guid=3, slot1 → w_guid=4.
        let mut guid_stream = PSETID_ATTACHMENT.to_vec();
        guid_stream.extend_from_slice(&PS_MAPI); // filler second GUID
        let entry = encode_nameid_entry(0x42, false, 4, 1);
        let m = NameIdMap::from_streams(&guid_stream, &entry, &[]);
        assert_eq!(m.resolve_lid(&PS_MAPI, 0x42), Some(0x8001));
    }

    #[test]
    fn psetid_attachment_bytes_match_ms_guid() {
        // Verify mixed-endian encoding of {96357F7F-59E1-47D0-99A7-46515C183B54}
        assert_eq!(PSETID_ATTACHMENT[0], 0x7F);
        assert_eq!(PSETID_ATTACHMENT[1], 0x5F);
        assert_eq!(PSETID_ATTACHMENT[2], 0x35);
        assert_eq!(PSETID_ATTACHMENT[3], 0x96);
        assert_eq!(PSETID_ATTACHMENT[4], 0xE1);
        assert_eq!(PSETID_ATTACHMENT[5], 0x59);
        assert_eq!(PSETID_ATTACHMENT[6], 0xD0);
        assert_eq!(PSETID_ATTACHMENT[7], 0x47);
        assert_eq!(
            &PSETID_ATTACHMENT[8..],
            &[0x99, 0xA7, 0x46, 0x51, 0x5C, 0x18, 0x3B, 0x54]
        );
    }
}
