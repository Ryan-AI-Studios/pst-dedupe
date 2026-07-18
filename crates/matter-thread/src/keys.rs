//! Compact fixed-size map keys for large-matter threading.
//!
//! Message-IDs and subject keys are SHA-256'd to `[u8; 32]` for map storage.
//! Reverse maps keep normalized strings only for MIDs present on matter items.

use matter_core::normalize_message_id;
use sha2::{Digest, Sha256};

/// Fixed-size map key (SHA-256 digest bytes).
pub type CompactKey = [u8; 32];

/// Normalize Message-ID then SHA-256 → fixed key.
///
/// Returns `None` if the normalized MID is empty.
pub fn message_id_key(raw_mid: &str) -> Option<CompactKey> {
    let norm = normalize_message_id(raw_mid);
    if norm.is_empty() {
        return None;
    }
    Some(hash_bytes(norm.as_bytes()))
}

/// Hash an already-normalized non-empty MID string.
pub fn normalized_mid_key(norm: &str) -> CompactKey {
    hash_bytes(norm.as_bytes())
}

/// Hash a subject thread key (already normalized/lowercased).
pub fn subject_key_hash(subject_key: &str) -> CompactKey {
    hash_bytes(subject_key.as_bytes())
}

/// Hash a ConversationIndex prefix (lowercase hex, typically 44 chars).
pub fn conversation_index_key(prefix_hex: &str) -> CompactKey {
    hash_bytes(prefix_hex.as_bytes())
}

fn hash_bytes(bytes: &[u8]) -> CompactKey {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Full SHA-256 hex (64 lowercase chars) of a preimage string.
pub fn sha256_hex(preimage: &str) -> String {
    let digest = Sha256::digest(preimage.as_bytes());
    let mut out = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in digest.as_slice() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
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
    fn message_id_key_normalizes() {
        let a = message_id_key("<ABC@Example.COM>").unwrap();
        let b = message_id_key("abc@example.com").unwrap();
        assert_eq!(a, b);
        assert!(message_id_key("   ").is_none());
    }

    #[test]
    fn sha256_hex_is_64_lower() {
        let h = sha256_hex("thread:v1\ntest");
        assert_eq!(h.len(), 64);
        assert!(h
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
