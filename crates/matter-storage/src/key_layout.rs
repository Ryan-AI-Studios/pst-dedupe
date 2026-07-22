//! Content-addressed object key layout with optional tenant/matter prefix.

use crate::digest::normalize_digest;
use crate::error::{Result, StorageError};

/// Segment names used under a matter/tenant root for CAS objects.
pub const CAS_SEGMENT: &str = "cas";
pub const SHA256_SEGMENT: &str = "sha256";

/// Reject path-escape and absolute-looking identity segments.
fn validate_segment(label: &str, value: &str) -> Result<()> {
    let v = value.trim();
    if v.is_empty() {
        return Err(StorageError::Config(format!(
            "{label} must not be empty when provided"
        )));
    }
    if v.contains("..") || v.contains('/') || v.contains('\\') || v.contains('\0') {
        return Err(StorageError::Config(format!(
            "{label} rejects path separators and '..': {value:?}"
        )));
    }
    if v.starts_with('/') || v.starts_with('\\') {
        return Err(StorageError::Config(format!(
            "{label} must not be absolute: {value:?}"
        )));
    }
    Ok(())
}

/// Build an object key: `{tenant?}/{matter?}/cas/sha256/{aa}/{hex}` or
/// `cas/sha256/{aa}/{hex}` when no prefixes; optional extra `prefix` root.
///
/// Layout matches matter-core CAS relative path `blobs/sha256/<aa>/<hex>` under
/// the `cas/sha256` segment (cloud) or local `blobs/sha256` (LocalFs).
pub fn object_key(
    prefix: Option<&str>,
    tenant_id: Option<&str>,
    matter_id: Option<&str>,
    digest_hex: &str,
) -> Result<String> {
    let digest = normalize_digest(digest_hex)?;
    let shard = &digest[..2];

    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = prefix.map(str::trim).filter(|s| !s.is_empty()) {
        // Prefix may contain multiple segments but never `..`.
        for seg in p.split(['/', '\\']) {
            if seg.is_empty() {
                continue;
            }
            validate_segment("prefix segment", seg)?;
            parts.push(seg.to_string());
        }
    }
    if let Some(t) = tenant_id.map(str::trim).filter(|s| !s.is_empty()) {
        validate_segment("tenant_id", t)?;
        parts.push(t.to_string());
    }
    if let Some(m) = matter_id.map(str::trim).filter(|s| !s.is_empty()) {
        validate_segment("matter_id", m)?;
        parts.push(m.to_string());
    }
    parts.push(CAS_SEGMENT.to_string());
    parts.push(SHA256_SEGMENT.to_string());
    parts.push(shard.to_string());
    parts.push(digest);
    Ok(parts.join("/"))
}

/// Local-FS relative path under a blobs root: `sha256/<aa>/<hex>` (no `cas/`).
///
/// Parity with matter-core `Cas` layout `blobs/sha256/<aa>/<hex>`.
pub fn local_relative_path(digest_hex: &str) -> Result<String> {
    let digest = normalize_digest(digest_hex)?;
    let shard = &digest[..2];
    Ok(format!("{SHA256_SEGMENT}/{shard}/{digest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_with_tenant_and_matter() {
        let d = "ab".to_string() + &"cd".repeat(31);
        let k = object_key(None, Some("ten1"), Some("mat1"), &d).expect("key");
        assert_eq!(k, format!("ten1/mat1/cas/sha256/ab/{d}"));
    }

    #[test]
    fn key_rejects_dotdot() {
        let d = "ab".to_string() + &"cd".repeat(31);
        assert!(object_key(None, Some(".."), Some("m"), &d).is_err());
        assert!(object_key(Some("a/../b"), None, None, &d).is_err());
    }

    #[test]
    fn key_rejects_slash_in_tenant() {
        let d = "ab".to_string() + &"cd".repeat(31);
        assert!(object_key(None, Some("a/b"), None, &d).is_err());
    }

    #[test]
    fn local_path_two_hex_shard() {
        let d = "ab".to_string() + &"cd".repeat(31);
        assert_eq!(
            local_relative_path(&d).expect("p"),
            format!("sha256/ab/{d}")
        );
    }
}
