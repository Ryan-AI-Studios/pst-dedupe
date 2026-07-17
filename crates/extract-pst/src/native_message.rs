//! Deterministic **pst-native-message-v1** framing for parent `native_sha256`.
//!
//! This is **not** synthetic EML. Format is frozen for chain-of-custody; bump
//! the version constant and leave v1 digests historical when changing fields.

use sha2::{Digest, Sha256};

/// Wire format tag stored in `extra_json` and embedded in the blob.
pub const NATIVE_FORMAT_V1: &str = "pst-native-message-v1";

/// Magic bytes: ASCII `PNM1`
pub const NATIVE_MAGIC: &[u8; 4] = b"PNM1";

/// Format version u32 little-endian after magic.
pub const NATIVE_VERSION: u32 = 1;

/// One attachment record in the native blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeAttachment {
    pub filename: String,
    pub size: u64,
    pub native_sha256: String,
}

/// Fixed-order field set for pst-native-message-v1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeMessageV1 {
    pub message_nid: u64,
    pub message_id: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    /// RFC3339 or empty.
    pub sent: String,
    /// RFC3339 or empty.
    pub received: String,
    /// Raw body bytes as stored for native identity (UTF-8 of plain body).
    pub body: Vec<u8>,
    pub attachments: Vec<NativeAttachment>,
}

/// Serialize to the exact v1 byte layout.
///
/// ```text
/// magic[4] = "PNM1"
/// version_u32_le = 1
/// message_nid_u64_le
/// then length-prefixed UTF-8 fields in fixed order:
///   message_id, subject, from, to, cc, bcc, sent, received
/// body: u64_le length + bytes
/// attach_count_u32_le
/// for each attach (extract order):
///   filename (u32_le len + utf8)
///   size_u64_le
///   native_sha256 (u32_le len + utf8 hex)
/// ```
pub fn encode_native_message_v1(msg: &NativeMessageV1) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(NATIVE_MAGIC);
    out.extend_from_slice(&NATIVE_VERSION.to_le_bytes());
    out.extend_from_slice(&msg.message_nid.to_le_bytes());
    write_str(&mut out, &msg.message_id);
    write_str(&mut out, &msg.subject);
    write_str(&mut out, &msg.from);
    write_str(&mut out, &msg.to);
    write_str(&mut out, &msg.cc);
    write_str(&mut out, &msg.bcc);
    write_str(&mut out, &msg.sent);
    write_str(&mut out, &msg.received);
    out.extend_from_slice(&(msg.body.len() as u64).to_le_bytes());
    out.extend_from_slice(&msg.body);
    out.extend_from_slice(&(msg.attachments.len() as u32).to_le_bytes());
    for a in &msg.attachments {
        write_str(&mut out, &a.filename);
        out.extend_from_slice(&a.size.to_le_bytes());
        write_str(&mut out, &a.native_sha256);
    }
    out
}

/// SHA-256 lowercase hex of the encoded native blob.
pub fn native_message_v1_digest(msg: &NativeMessageV1) -> String {
    let bytes = encode_native_message_v1(msg);
    let dig = Sha256::digest(&bytes);
    hex_encode(dig.as_ref())
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonical synthetic field set for the golden digest pin.
    pub(crate) fn sample() -> NativeMessageV1 {
        NativeMessageV1 {
            message_nid: 0x2004,
            message_id: "<msg-1@example.com>".into(),
            subject: "Hello".into(),
            from: "a@example.com".into(),
            to: "b@example.com".into(),
            cc: "".into(),
            bcc: "".into(),
            sent: "2020-01-02T03:04:05Z".into(),
            received: "2020-01-02T03:05:00Z".into(),
            body: b"body text".to_vec(),
            attachments: vec![NativeAttachment {
                filename: "a.txt".into(),
                size: 3,
                native_sha256: "aabbccdd".into(),
            }],
        }
    }

    #[test]
    fn encode_has_magic_and_version() {
        let bytes = encode_native_message_v1(&sample());
        assert_eq!(&bytes[0..4], b"PNM1");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 1);
        assert_eq!(u64::from_le_bytes(bytes[8..16].try_into().unwrap()), 0x2004);
    }

    #[test]
    fn golden_stable_digest() {
        // Frozen golden for `sample()` — regenerate only with intentional
        // native format version bump (and leave old digests historical).
        const GOLDEN: &str = "09b8a17797e679fd028aae7b48e05a9e1a1796fb37f6f8c13a5ca548d6ab8160";
        let d1 = native_message_v1_digest(&sample());
        let d2 = native_message_v1_digest(&sample());
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 64);
        assert_eq!(
            d1, GOLDEN,
            "native v1 digest churn — bump NATIVE_VERSION if intentional"
        );
        let bytes = encode_native_message_v1(&sample());
        assert!(bytes.starts_with(b"PNM1"));
        assert_eq!(hex_encode(Sha256::digest(&bytes).as_ref()), GOLDEN);
    }

    #[test]
    fn field_change_changes_digest() {
        let mut m = sample();
        let d0 = native_message_v1_digest(&m);
        m.subject = "Hello!".into();
        assert_ne!(d0, native_message_v1_digest(&m));
    }
}
