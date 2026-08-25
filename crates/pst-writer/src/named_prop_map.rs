//! Allowlisted Name-to-ID Map (NPMAP) builder — track 0092.
//!
//! Implements MS-PST §2.4.7 streams + hash buckets for the hard allowlist of
//! cloud attach named props (`PSETID_Attachment`). Emit only when a
//! [`NamedPropWritePlan`] is non-empty (emit-when-used).
//!
//! Spec anchors (access 2026-08-25):
//! - MS-PST §2.4.7 / §2.4.7.5 / §2.7.3.2 (BucketCount SHOULD 251)
//! - MS-PST §5.3 ComputeCRC (seed 0; weak CRC32 — not `crc32fast::hash`)
//! - MS-OXCMSG §2.2.2.9 `afByWebReference` / PidNameAttachmentProviderType

use std::collections::{BTreeMap, BTreeSet};

use pst_reader::{
    encode_nameid_entry, encode_string_stream_entry, NAME_ATTACHMENT_PROVIDER_TYPE, NPID_BASE,
    PSETID_ATTACHMENT,
};

use crate::production::{PcValue, WriteAttachment, WriteMessage};

/// PidTagNameidBucketCount (MS-PST §2.1.2).
const PID_NAMEID_BUCKET_COUNT: u16 = 0x0001;
const PID_NAMEID_GUID_STREAM: u16 = 0x0002;
const PID_NAMEID_ENTRY_STREAM: u16 = 0x0003;
const PID_NAMEID_STRING_STREAM: u16 = 0x0004;
const PID_NAMEID_BUCKET_BASE: u16 = 0x1000;

/// MS-PST SHOULD value for bucket count (also minimum-map requirement).
pub const NAMEID_BUCKET_COUNT: u32 = 251;

/// Public string names in the allowlist (sorted order = NPID assignment order).
pub const NAME_ATTACHMENT_PERMISSION_TYPE: &str = "AttachmentPermissionType";
pub const NAME_ATTACHMENT_URL: &str = "AttachmentUrl";

/// Allowlisted named property (0092 hard fence — no encyclopedia).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AllowlistedNamedProp {
    /// MAY: `AttachmentPermissionType` when source had it.
    AttachmentPermissionType,
    /// MUST: `AttachmentProviderType` when provider known on cloud pointer.
    AttachmentProviderType,
    /// MAY: `AttachmentUrl` when source URL present.
    AttachmentUrl,
}

impl AllowlistedNamedProp {
    /// Stable string name for the NPMAP string stream.
    pub fn name(self) -> &'static str {
        match self {
            Self::AttachmentPermissionType => NAME_ATTACHMENT_PERMISSION_TYPE,
            Self::AttachmentProviderType => NAME_ATTACHMENT_PROVIDER_TYPE,
            Self::AttachmentUrl => NAME_ATTACHMENT_URL,
        }
    }
}

/// Which allowlisted named props will be written on this export.
///
/// Empty plan → writer keeps the empty NPMAP stub (cloud-free golden digests).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamedPropWritePlan {
    used: BTreeSet<AllowlistedNamedProp>,
}

impl NamedPropWritePlan {
    /// Empty plan (no allowlisted named props).
    pub fn empty() -> Self {
        Self::default()
    }

    /// True when no allowlisted props will be written.
    pub fn is_empty(&self) -> bool {
        self.used.is_empty()
    }

    /// Number of distinct allowlisted props in the plan.
    pub fn len(&self) -> usize {
        self.used.len()
    }

    /// Insert one allowlisted prop.
    pub fn insert(&mut self, prop: AllowlistedNamedProp) {
        self.used.insert(prop);
    }

    /// Whether `prop` is in the plan.
    pub fn contains(&self, prop: AllowlistedNamedProp) -> bool {
        self.used.contains(&prop)
    }

    /// Iterate used props in sorted allowlist order.
    pub fn iter(&self) -> impl Iterator<Item = AllowlistedNamedProp> + '_ {
        self.used.iter().copied()
    }

    /// NPID for a used prop (`0x8000 + index` in sorted used order).
    pub fn npid(&self, prop: AllowlistedNamedProp) -> Option<u16> {
        self.used
            .iter()
            .position(|&p| p == prop)
            .and_then(|i| u16::try_from(i).ok())
            .map(|idx| NPID_BASE.saturating_add(idx))
    }

    /// Default max embedded depth for planning (matches `WritePstOpts` default 3).
    pub const DEFAULT_MAX_EMBEDDED_DEPTH: u32 = 3;

    /// Pre-scan message attach metadata (no payload bytes) for allowlisted props.
    ///
    /// Walks top-level and embedded messages recursively up to
    /// [`Self::DEFAULT_MAX_EMBEDDED_DEPTH`] (aligned with the production writer).
    pub fn scan_messages<'a>(messages: impl IntoIterator<Item = &'a WriteMessage>) -> Self {
        Self::scan_messages_with_depth(messages, Self::DEFAULT_MAX_EMBEDDED_DEPTH)
    }

    /// Pre-scan with an explicit embedded-depth cap (same semantics as writer).
    pub fn scan_messages_with_depth<'a>(
        messages: impl IntoIterator<Item = &'a WriteMessage>,
        max_embedded_depth: u32,
    ) -> Self {
        let mut plan = Self::empty();
        let max_depth = max_embedded_depth.clamp(1, 8);
        for msg in messages {
            scan_message_named_props_at_depth(msg, &mut plan, 0, max_depth);
        }
        plan
    }
}

fn scan_message_named_props_at_depth(
    msg: &WriteMessage,
    plan: &mut NamedPropWritePlan,
    depth: u32,
    max_depth: u32,
) {
    for att in &msg.attachments {
        scan_attachment_named_props(att, plan);
        // Match production writer: embedded cloud props below max_depth are not written.
        if depth >= max_depth {
            continue;
        }
        if let Some(embedded) = att.embedded_message.as_deref() {
            scan_message_named_props_at_depth(embedded, plan, depth + 1, max_depth);
        }
    }
}

fn scan_attachment_named_props(att: &WriteAttachment, plan: &mut NamedPropWritePlan) {
    if !att.is_cloud_link {
        return;
    }
    if att
        .cloud_provider
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        plan.insert(AllowlistedNamedProp::AttachmentProviderType);
    }
    if att
        .cloud_url
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
    {
        plan.insert(AllowlistedNamedProp::AttachmentUrl);
    }
    if att.cloud_permission_type.is_some() {
        plan.insert(AllowlistedNamedProp::AttachmentPermissionType);
    }
}

/// Build NPMAP PC properties for a non-empty plan.
///
/// Returns an empty vec when the plan is empty (caller should keep stub PC).
pub fn build_named_prop_map_pc(plan: &NamedPropWritePlan) -> Vec<(u16, PcValue)> {
    if plan.is_empty() {
        return Vec::new();
    }

    let mut string_stream = Vec::new();
    let mut entry_stream = Vec::new();
    let mut buckets: BTreeMap<u16, Vec<u8>> = BTreeMap::new();

    // GUID stream: one slot for PSETID_Attachment → wGuid index 3.
    let guid_stream = PSETID_ATTACHMENT.to_vec();
    let w_guid: u16 = 3;

    for (prop_idx, prop) in plan.iter().enumerate() {
        let prop_idx = prop_idx as u16;
        let name = prop.name();
        let string_offset = string_stream.len() as u32;
        string_stream.extend_from_slice(&encode_string_stream_entry(name));

        let entry = encode_nameid_entry(string_offset, true, w_guid, prop_idx);
        entry_stream.extend_from_slice(&entry);

        let w_guid_n = (w_guid << 1) | 1;
        // Bucket selection uses the bucket-form NAMEID: for N=1, dwPropertyID is
        // CRC32 of the UTF-16LE name (MS-PST §2.4.7.5) — Outlook lookup hashes CRC.
        let utf16: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        let crc = compute_crc_mspst(0, &utf16);
        let bucket = nameid_bucket_index(crc, w_guid_n, NAMEID_BUCKET_COUNT);
        let bucket_entry = encode_nameid_entry(crc, true, w_guid, prop_idx);
        buckets
            .entry(bucket)
            .or_default()
            .extend_from_slice(&bucket_entry);
    }

    let mut props: Vec<(u16, PcValue)> = Vec::with_capacity(4 + buckets.len());
    props.push((
        PID_NAMEID_BUCKET_COUNT,
        PcValue::I32(NAMEID_BUCKET_COUNT as i32),
    ));
    props.push((PID_NAMEID_GUID_STREAM, PcValue::Binary(guid_stream)));
    props.push((PID_NAMEID_ENTRY_STREAM, PcValue::Binary(entry_stream)));
    props.push((PID_NAMEID_STRING_STREAM, PcValue::Binary(string_stream)));
    for (bucket, bytes) in buckets {
        props.push((
            PID_NAMEID_BUCKET_BASE.saturating_add(bucket),
            PcValue::Binary(bytes),
        ));
    }
    props
}

/// Bucket selection from an Entry Stream NAMEID (MS-PST §2.4.7.5).
///
/// `pul[0] = dwPropertyID`, `pul[1] & 0xFFFF = wGuidN`.
fn nameid_bucket_index(dw_property_id: u32, w_guid_n: u16, bucket_count: u32) -> u16 {
    let hash = dw_property_id ^ u32::from(w_guid_n);
    (hash % bucket_count) as u16
}

/// MS-PST §5.3 ComputeCRC — seed 0, no final invert (weak CRC32).
///
/// Table is `CRC_TABLE_OFFSET32` from the MS-PST reference / outlook-pst-rs.
pub(crate) fn compute_crc_mspst(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc = CRC_TABLE_OFFSET32[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

// MS-PST §5.3 / outlook-pst-rs `CRC_TABLE_OFFSET32` (first CRC table only).
const CRC_TABLE_OFFSET32: [u32; 256] = [
    0x00000000, 0x77073096, 0xEE0E612C, 0x990951BA, 0x076DC419, 0x706AF48F, 0xE963A535, 0x9E6495A3,
    0x0EDB8832, 0x79DCB8A4, 0xE0D5E91E, 0x97D2D988, 0x09B64C2B, 0x7EB17CBD, 0xE7B82D07, 0x90BF1D91,
    0x1DB71064, 0x6AB020F2, 0xF3B97148, 0x84BE41DE, 0x1ADAD47D, 0x6DDDE4EB, 0xF4D4B551, 0x83D385C7,
    0x136C9856, 0x646BA8C0, 0xFD62F97A, 0x8A65C9EC, 0x14015C4F, 0x63066CD9, 0xFA0F3D63, 0x8D080DF5,
    0x3B6E20C8, 0x4C69105E, 0xD56041E4, 0xA2677172, 0x3C03E4D1, 0x4B04D447, 0xD20D85FD, 0xA50AB56B,
    0x35B5A8FA, 0x42B2986C, 0xDBBBC9D6, 0xACBCF940, 0x32D86CE3, 0x45DF5C75, 0xDCD60DCF, 0xABD13D59,
    0x26D930AC, 0x51DE003A, 0xC8D75180, 0xBFD06116, 0x21B4F4B5, 0x56B3C423, 0xCFBA9599, 0xB8BDA50F,
    0x2802B89E, 0x5F058808, 0xC60CD9B2, 0xB10BE924, 0x2F6F7C87, 0x58684C11, 0xC1611DAB, 0xB6662D3D,
    0x76DC4190, 0x01DB7106, 0x98D220BC, 0xEFD5102A, 0x71B18589, 0x06B6B51F, 0x9FBFE4A5, 0xE8B8D433,
    0x7807C9A2, 0x0F00F934, 0x9609A88E, 0xE10E9818, 0x7F6A0DBB, 0x086D3D2D, 0x91646C97, 0xE6635C01,
    0x6B6B51F4, 0x1C6C6162, 0x856530D8, 0xF262004E, 0x6C0695ED, 0x1B01A57B, 0x8208F4C1, 0xF50FC457,
    0x65B0D9C6, 0x12B7E950, 0x8BBEB8EA, 0xFCB9887C, 0x62DD1DDF, 0x15DA2D49, 0x8CD37CF3, 0xFBD44C65,
    0x4DB26158, 0x3AB551CE, 0xA3BC0074, 0xD4BB30E2, 0x4ADFA541, 0x3DD895D7, 0xA4D1C46D, 0xD3D6F4FB,
    0x4369E96A, 0x346ED9FC, 0xAD678846, 0xDA60B8D0, 0x44042D73, 0x33031DE5, 0xAA0A4C5F, 0xDD0D7CC9,
    0x5005713C, 0x270241AA, 0xBE0B1010, 0xC90C2086, 0x5768B525, 0x206F85B3, 0xB966D409, 0xCE61E49F,
    0x5EDEF90E, 0x29D9C998, 0xB0D09822, 0xC7D7A8B4, 0x59B33D17, 0x2EB40D81, 0xB7BD5C3B, 0xC0BA6CAD,
    0xEDB88320, 0x9ABFB3B6, 0x03B6E20C, 0x74B1D29A, 0xEAD54739, 0x9DD277AF, 0x04DB2615, 0x73DC1683,
    0xE3630B12, 0x94643B84, 0x0D6D6A3E, 0x7A6A5AA8, 0xE40ECF0B, 0x9309FF9D, 0x0A00AE27, 0x7D079EB1,
    0xF00F9344, 0x8708A3D2, 0x1E01F268, 0x6906C2FE, 0xF762575D, 0x806567CB, 0x196C3671, 0x6E6B06E7,
    0xFED41B76, 0x89D32BE0, 0x10DA7A5A, 0x67DD4ACC, 0xF9B9DF6F, 0x8EBEEFF9, 0x17B7BE43, 0x60B08ED5,
    0xD6D6A3E8, 0xA1D1937E, 0x38D8C2C4, 0x4FDFF252, 0xD1BB67F1, 0xA6BC5767, 0x3FB506DD, 0x48B2364B,
    0xD80D2BDA, 0xAF0A1B4C, 0x36034AF6, 0x41047A60, 0xDF60EFC3, 0xA867DF55, 0x316E8EEF, 0x4669BE79,
    0xCB61B38C, 0xBC66831A, 0x256FD2A0, 0x5268E236, 0xCC0C7795, 0xBB0B4703, 0x220216B9, 0x5505262F,
    0xC5BA3BBE, 0xB2BD0B28, 0x2BB45A92, 0x5CB36A04, 0xC2D7FFA7, 0xB5D0CF31, 0x2CD99E8B, 0x5BDEAE1D,
    0x9B64C2B0, 0xEC63F226, 0x756AA39C, 0x026D930A, 0x9C0906A9, 0xEB0E363F, 0x72076785, 0x05005713,
    0x95BF4A82, 0xE2B87A14, 0x7BB12BAE, 0x0CB61B38, 0x92D28E9B, 0xE5D5BE0D, 0x7CDCEFB7, 0x0BDBDF21,
    0x86D3D2D4, 0xF1D4E242, 0x68DDB3F8, 0x1FDA836E, 0x81BE16CD, 0xF6B9265B, 0x6FB077E1, 0x18B74777,
    0x88085AE6, 0xFF0F6A70, 0x66063BCA, 0x11010B5C, 0x8F659EFF, 0xF862AE69, 0x616BFFD3, 0x166CCF45,
    0xA00AE278, 0xD70DD2EE, 0x4E048354, 0x3903B3C2, 0xA7672661, 0xD06016F7, 0x4969474D, 0x3E6E77DB,
    0xAED16A4A, 0xD9D65ADC, 0x40DF0B66, 0x37D83BF0, 0xA9BCAE53, 0xDEBB9EC5, 0x47B2CF7F, 0x30B5FFE9,
    0xBDBDF21C, 0xCABAC28A, 0x53B39330, 0x24B4A3A6, 0xBAD03605, 0xCDD70693, 0x54DE5729, 0x23D967BF,
    0xB3667A2E, 0xC4614AB8, 0x5D681B02, 0x2A6F2B94, 0xB40BBE37, 0xC30C8EA1, 0x5A05DF1B, 0x2D02EF8D,
];

#[cfg(test)]
mod tests {
    use super::*;
    use pst_reader::NameIdMap;

    fn provider_only_plan() -> NamedPropWritePlan {
        let mut p = NamedPropWritePlan::empty();
        p.insert(AllowlistedNamedProp::AttachmentProviderType);
        p
    }

    #[test]
    fn empty_plan_builds_no_props() {
        assert!(build_named_prop_map_pc(&NamedPropWritePlan::empty()).is_empty());
    }

    #[test]
    fn provider_plan_round_trips_nameid_map() {
        let plan = provider_only_plan();
        let props = build_named_prop_map_pc(&plan);
        assert_eq!(
            plan.npid(AllowlistedNamedProp::AttachmentProviderType),
            Some(0x8000)
        );

        let mut guid = None;
        let mut entry = None;
        let mut string = None;
        let mut bucket_count = None;
        let mut bucket_props = 0usize;
        for (id, val) in &props {
            match (*id, val) {
                (PID_NAMEID_BUCKET_COUNT, PcValue::I32(v)) => bucket_count = Some(*v),
                (PID_NAMEID_GUID_STREAM, PcValue::Binary(b)) => guid = Some(b.as_slice()),
                (PID_NAMEID_ENTRY_STREAM, PcValue::Binary(b)) => entry = Some(b.as_slice()),
                (PID_NAMEID_STRING_STREAM, PcValue::Binary(b)) => string = Some(b.as_slice()),
                (id, PcValue::Binary(b)) if (0x1000..=0x1000 + 250).contains(&id) => {
                    assert!(!b.is_empty(), "bucket {id:#x} must be non-empty");
                    bucket_props += 1;
                }
                _ => {}
            }
        }
        assert_eq!(bucket_count, Some(251));
        assert!(bucket_props >= 1, "at least one hash bucket required");

        let map = NameIdMap::from_streams(
            guid.expect("guid"),
            entry.expect("entry"),
            string.expect("string"),
        );
        assert!(!map.degraded);
        assert_eq!(map.attachment_provider_type_npid(), Some(0x8000));
        assert_eq!(
            map.resolve_name(&PSETID_ATTACHMENT, NAME_ATTACHMENT_PROVIDER_TYPE),
            Some(0x8000)
        );
    }

    #[test]
    fn sorted_npid_assignment_among_used() {
        let mut plan = NamedPropWritePlan::empty();
        // Insert out of order; NPID follows sorted name order.
        plan.insert(AllowlistedNamedProp::AttachmentUrl);
        plan.insert(AllowlistedNamedProp::AttachmentProviderType);
        plan.insert(AllowlistedNamedProp::AttachmentPermissionType);
        // Permission < Provider < Url alphabetically
        assert_eq!(
            plan.npid(AllowlistedNamedProp::AttachmentPermissionType),
            Some(0x8000)
        );
        assert_eq!(
            plan.npid(AllowlistedNamedProp::AttachmentProviderType),
            Some(0x8001)
        );
        assert_eq!(plan.npid(AllowlistedNamedProp::AttachmentUrl), Some(0x8002));
    }

    #[test]
    fn scan_messages_detects_cloud_provider_and_url() {
        let mut msg = WriteMessage {
            subject: "s".into(),
            ..WriteMessage::default()
        };
        msg.attachments.push(WriteAttachment {
            is_cloud_link: true,
            cloud_provider: Some("OneDrivePro".into()),
            cloud_url: Some("https://contoso.sharepoint.com/x".into()),
            cloud_permission_type: Some(1),
            ..Default::default()
        });
        let plan = NamedPropWritePlan::scan_messages(std::slice::from_ref(&msg));
        assert!(plan.contains(AllowlistedNamedProp::AttachmentProviderType));
        assert!(plan.contains(AllowlistedNamedProp::AttachmentUrl));
        assert!(plan.contains(AllowlistedNamedProp::AttachmentPermissionType));
    }

    #[test]
    fn scan_ignores_non_cloud_attaches() {
        let mut msg = WriteMessage::default();
        msg.attachments.push(WriteAttachment {
            is_cloud_link: false,
            cloud_provider: Some("OneDrivePro".into()),
            ..Default::default()
        });
        let plan = NamedPropWritePlan::scan_messages(std::slice::from_ref(&msg));
        assert!(plan.is_empty());
    }

    #[test]
    fn scan_respects_embedded_depth_cap() {
        // Nesting: top(depth0) -> emb1(depth1) -> emb2(depth2) with cloud on emb2.
        // max_depth=1: writer writes emb1 but not emb2's children; cloud on emb2
        // message body attaches are at depth 1 when scanning emb1's nested msg...
        // Build: top has embed attach containing mid, mid has cloud attach.
        // scan at max_depth=1: top depth0 scans cloud? no; recurses to mid at depth1;
        // at depth1 scans mid's cloud attach → included. Recurse to depth2 blocked.
        // scan at max_depth=1 with cloud only on a child of mid:
        let deep_cloud = WriteAttachment {
            is_cloud_link: true,
            cloud_provider: Some("OneDrivePro".into()),
            ..Default::default()
        };
        let mid = WriteMessage {
            attachments: vec![WriteAttachment {
                attach_method: Some(5),
                embedded_message: Some(Box::new(WriteMessage {
                    attachments: vec![deep_cloud],
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        };
        let top = WriteMessage {
            attachments: vec![WriteAttachment {
                attach_method: Some(5),
                embedded_message: Some(Box::new(mid)),
                ..Default::default()
            }],
            ..Default::default()
        };
        // depth path: top@0 -> mid@1 -> deep_msg@2 (cloud on deep_msg attaches).
        // max_depth=1: recurse only while depth < 1 → enter mid@1; at depth1
        // scan mid's embed attach (not cloud) and do not enter deep_msg → empty.
        let capped = NamedPropWritePlan::scan_messages_with_depth(std::slice::from_ref(&top), 1);
        assert!(
            capped.is_empty(),
            "cloud beyond depth cap must not enter plan: {capped:?}"
        );
        // max_depth=2: enter deep_msg@2 and pick up ProviderType.
        let allowed = NamedPropWritePlan::scan_messages_with_depth(std::slice::from_ref(&top), 2);
        assert!(
            allowed.contains(AllowlistedNamedProp::AttachmentProviderType),
            "cloud at depth boundary must be planned: {allowed:?}"
        );
    }

    #[test]
    fn deterministic_entry_stream_bytes() {
        let plan = provider_only_plan();
        let a = build_named_prop_map_pc(&plan);
        let b = build_named_prop_map_pc(&plan);
        let entry_a = a
            .iter()
            .find(|(id, _)| *id == PID_NAMEID_ENTRY_STREAM)
            .map(|(_, v)| v);
        let entry_b = b
            .iter()
            .find(|(id, _)| *id == PID_NAMEID_ENTRY_STREAM)
            .map(|(_, v)| v);
        match (entry_a, entry_b) {
            (Some(PcValue::Binary(x)), Some(PcValue::Binary(y))) => assert_eq!(x, y),
            _ => panic!("expected binary entry streams"),
        }
    }

    #[test]
    fn hash_bucket_crc_and_index_match_mspst() {
        let plan = provider_only_plan();
        let props = build_named_prop_map_pc(&plan);
        let name = NAME_ATTACHMENT_PROVIDER_TYPE;
        let utf16: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        let expected_crc = compute_crc_mspst(0, &utf16);
        let w_guid_n: u16 = (3u16 << 1) | 1;
        // Bucket index is derived from CRC (bucket-form NAMEID), not string offset.
        let expected_bucket = nameid_bucket_index(expected_crc, w_guid_n, NAMEID_BUCKET_COUNT);
        let bucket_pid = PID_NAMEID_BUCKET_BASE.saturating_add(expected_bucket);

        let bucket_bytes = props
            .iter()
            .find(|(id, _)| *id == bucket_pid)
            .and_then(|(_, v)| match v {
                PcValue::Binary(b) => Some(b.as_slice()),
                _ => None,
            })
            .expect("expected hash bucket for AttachmentProviderType");
        assert!(
            bucket_bytes.len() >= 8,
            "bucket must hold at least one NAMEID"
        );
        let dw = u32::from_le_bytes(bucket_bytes[0..4].try_into().expect("4 bytes"));
        assert_eq!(
            dw, expected_crc,
            "bucket dwPropertyID must be MS-PST ComputeCRC of UTF-16LE name"
        );
        let guid_n = u16::from_le_bytes(bucket_bytes[4..6].try_into().expect("2 bytes"));
        assert_eq!(guid_n, w_guid_n);
        let prop_idx = u16::from_le_bytes(bucket_bytes[6..8].try_into().expect("2 bytes"));
        assert_eq!(prop_idx, 0);
    }
}
