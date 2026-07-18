//! Optional DeNIST / known-file filter (SHA-256 only).

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::error::{CullError, Result};

/// Loaded operator hash list for DeNIST matching.
#[derive(Debug, Clone)]
pub struct DenistList {
    /// Lowercase 64-hex SHA-256 digests.
    pub hashes: HashSet<String>,
}

/// Load a SHA-256 hash list from a text file.
///
/// - One digest per line; `#` comments and blank lines ignored.
/// - Digests normalized to lowercase.
/// - **Fail** if path missing/unreadable when caller enables DeNIST.
/// - **Fail** if zero valid 64-hex lines but file has 32-hex (MD5) and/or
///   40-hex (SHA-1) looking lines (`denist_hash_format`).
/// - **Fail** if empty after parse (no valid SHA-256).
pub fn load_sha256_list(path: &Path) -> Result<DenistList> {
    let text = fs::read_to_string(path).map_err(|e| {
        CullError::Denist(format!("cannot read hash list '{}': {e}", path.display()))
    })?;
    parse_sha256_list(&text)
}

/// Parse hash list text (same rules as [`load_sha256_list`]).
pub fn parse_sha256_list(text: &str) -> Result<DenistList> {
    let mut hashes = HashSet::new();
    let mut md5_like = 0u64;
    let mut sha1_like = 0u64;
    let mut other_tokens = 0u64;

    for line in text.lines() {
        let mut s = line.trim();
        if s.is_empty() {
            continue;
        }
        if let Some(pos) = s.find('#') {
            s = s[..pos].trim();
            if s.is_empty() {
                continue;
            }
        }
        // Take first whitespace-separated token (NSRL CSV-ish rows: first col).
        let token = s.split_whitespace().next().unwrap_or("").trim();
        // Strip optional quotes / commas from CSV.
        let token = token.trim_matches(|c| c == '"' || c == '\'' || c == ',');
        if token.is_empty() {
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if is_hex(&lower, 64) {
            hashes.insert(lower);
        } else if is_hex(&lower, 32) {
            md5_like += 1;
        } else if is_hex(&lower, 40) {
            sha1_like += 1;
        } else {
            other_tokens += 1;
        }
    }

    if hashes.is_empty() {
        if md5_like > 0 || sha1_like > 0 {
            return Err(CullError::Denist(format!(
                "denist_hash_format: expected SHA-256 (64 hex) digests; \
                 found {md5_like} MD5-like (32 hex) and {sha1_like} SHA-1-like (40 hex) lines. \
                 Legacy NSRL RDSv2 MD5/SHA-1 lists will not match native_sha256 — export SHA-256 from RDSv3"
            )));
        }
        let _ = other_tokens;
        return Err(CullError::Denist(
            "hash list contains no valid SHA-256 (64 hex) digests".into(),
        ));
    }

    Ok(DenistList { hashes })
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// True when `native_sha256` (any case) is in the list.
pub fn matches_denist(list: &DenistList, native_sha256: Option<&str>) -> bool {
    let Some(h) = native_sha256 else {
        return false;
    };
    let lower = h.trim().to_ascii_lowercase();
    list.hashes.contains(&lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sha256_with_comments() {
        let text = "\
# header
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
# mid
BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB
";
        let list = parse_sha256_list(text).unwrap();
        assert_eq!(list.hashes.len(), 2);
        assert!(matches_denist(
            &list,
            Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
        ));
    }

    #[test]
    fn fails_md5_only() {
        let text = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
";
        let err = parse_sha256_list(text).unwrap_err().to_string();
        assert!(err.contains("denist_hash_format"), "{err}");
    }

    #[test]
    fn fails_empty() {
        let err = parse_sha256_list("# only comments\n\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no valid SHA-256"), "{err}");
    }
}
