//! SHA-256 hex digest helpers (CAS identity).

use sha2::{Digest, Sha256};

use crate::error::{Result, StorageError};

/// Length of a lowercase hex-encoded SHA-256 digest.
pub const DIGEST_HEX_LEN: usize = 64;

/// Normalize a digest to lowercase 64-char hex. Rejects any other form.
pub fn normalize_digest(digest_hex: &str) -> Result<String> {
    let lower = digest_hex.trim().to_ascii_lowercase();
    if lower.len() != DIGEST_HEX_LEN || !lower.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(StorageError::InvalidDigest(digest_hex.to_string()));
    }
    Ok(lower)
}

/// Compute lowercase hex SHA-256 of raw bytes.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    hex_encode(digest.as_ref())
}

/// Hex-encode bytes as lowercase.
pub fn hex_encode(bytes: &[u8]) -> String {
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

    #[test]
    fn empty_sha256() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn normalize_rejects_short() {
        assert!(normalize_digest("abcd").is_err());
    }

    #[test]
    fn normalize_lowercases() {
        let d = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";
        assert_eq!(normalize_digest(d).expect("ok"), d.to_ascii_lowercase());
    }
}
