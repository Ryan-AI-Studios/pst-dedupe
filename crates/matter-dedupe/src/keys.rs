//! Compact fixed-size map keys for large-matter dedupe (DoD-2).
//!
//! - `logical_hash`: 64 lowercase hex → `[u8; 32]`
//! - Message-ID: normalize then SHA-256 → `[u8; 32]`
//!
//! Maps hold only fixed keys → canonical item id; never full `Item` bodies.

use matter_core::normalize_message_id;
use sha2::{Digest, Sha256};

/// Fixed-size map key (SHA-256 digest bytes).
pub type CompactKey = [u8; 32];

/// Decode a 64-char lowercase (or mixed-case) hex logical_hash into 32 bytes.
///
/// Returns `None` if the string is not valid 64-hex.
pub fn logical_hash_key(hex: &str) -> Option<CompactKey> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        if chunk.len() != 2 {
            return None;
        }
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

/// Normalize Message-ID then SHA-256 → fixed key.
///
/// Returns `None` if the normalized MID is empty.
pub fn message_id_key(raw_mid: &str) -> Option<CompactKey> {
    let norm = normalize_message_id(raw_mid);
    if norm.is_empty() {
        return None;
    }
    let digest = Sha256::digest(norm.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn compact_key_is_fixed_32_bytes() {
        assert_eq!(size_of::<CompactKey>(), 32);
    }

    #[test]
    fn logical_hash_decode_roundtrip_shape() {
        let hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let k = logical_hash_key(hex).expect("decode");
        assert_eq!(k[0], 0x01);
        assert_eq!(k[1], 0x23);
        assert_eq!(k[31], 0xef);
        assert!(logical_hash_key("short").is_none());
        assert!(logical_hash_key("gg").is_none());
    }

    #[test]
    fn message_id_key_normalizes_and_hashes() {
        let a = message_id_key("<ABC@Example.COM>").unwrap();
        let b = message_id_key("abc@example.com").unwrap();
        assert_eq!(a, b);
        assert!(message_id_key("   ").is_none());
        assert!(message_id_key("<>").is_none());
    }
}
