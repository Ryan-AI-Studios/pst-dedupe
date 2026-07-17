//! Content-addressable blob store for **raw physical bytes only**.
//!
//! # Path layout
//!
//! ```text
//! blobs/sha256/<aa>/<fullhex>
//! ```
//!
//! where `<aa>` is the first two lowercase hex characters of the SHA-256 digest
//! and `<fullhex>` is the full 64-character lowercase hex digest.
//!
//! # Hash contract
//!
//! - Algorithm: SHA-256
//! - Input: raw physical bytes only (never normalized/logical content)
//! - Digest encoding: lowercase hex
//! - Collision policy: if the path exists and content differs → hard error
//! - Logical hashes are **not** stored in CAS (column on `items` only)

use std::fs::{self, File};
use std::io::{Read, Write};

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Directory name under the matter root for CAS blobs.
pub const BLOBS_DIR: &str = "blobs";

/// Algorithm subdirectory under `blobs/`.
pub const SHA256_DIR: &str = "sha256";

/// Compute lowercase hex SHA-256 of raw bytes.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    hex_encode(digest.as_ref())
}

/// Content-addressable store rooted at `matter_root/blobs`.
#[derive(Debug, Clone)]
pub struct Cas {
    /// Absolute or relative path to `blobs/` under the matter root.
    blobs_root: Utf8PathBuf,
}

impl Cas {
    /// Open (or prepare) a CAS under the given matter root.
    ///
    /// Does not create directories; call [`Cas::ensure_layout`] after matter
    /// directory creation.
    pub fn new(matter_root: impl AsRef<Utf8Path>) -> Self {
        let blobs_root = matter_root.as_ref().join(BLOBS_DIR);
        Self { blobs_root }
    }

    /// Path to the `blobs/` directory.
    pub fn root(&self) -> &Utf8Path {
        &self.blobs_root
    }

    /// Ensure `blobs/sha256/` exists.
    pub fn ensure_layout(&self) -> Result<()> {
        let sha_root = self.blobs_root.join(SHA256_DIR);
        fs::create_dir_all(sha_root.as_std_path())?;
        Ok(())
    }

    /// Object path: `blobs/sha256/<aa>/<fullhex>`.
    pub fn object_path(&self, digest_hex: &str) -> Result<Utf8PathBuf> {
        let digest = normalize_digest(digest_hex)?;
        let shard = &digest[..2];
        Ok(self.blobs_root.join(SHA256_DIR).join(shard).join(&digest))
    }

    /// Return true if a blob with this digest already exists.
    pub fn exists(&self, digest_hex: &str) -> Result<bool> {
        let path = self.object_path(digest_hex)?;
        Ok(path.as_std_path().exists())
    }

    /// Store raw bytes. Returns the SHA-256 hex digest.
    ///
    /// If the object already exists with identical content, this is a no-op
    /// success. If it exists with different content, returns
    /// [`Error::CasCollision`].
    pub fn put_bytes(&self, data: &[u8]) -> Result<String> {
        let digest = sha256_hex(data);
        let path = self.object_path(&digest)?;

        if path.as_std_path().exists() {
            let existing = fs::read(path.as_std_path())?;
            if existing.as_slice() != data {
                return Err(Error::CasCollision { digest });
            }
            return Ok(digest);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }

        // Write via temp file in the same directory then rename for atomicity.
        let tmp_name = format!(".{digest}.tmp");
        let tmp_path = path
            .parent()
            .map(|p| p.join(&tmp_name))
            .unwrap_or_else(|| path.with_file_name(&tmp_name));

        {
            let mut file = File::create(tmp_path.as_std_path())?;
            file.write_all(data)?;
            file.sync_all()?;
        }

        // On Windows, rename fails if destination exists; re-check race.
        if path.as_std_path().exists() {
            let existing = fs::read(path.as_std_path())?;
            let _ = fs::remove_file(tmp_path.as_std_path());
            if existing.as_slice() != data {
                return Err(Error::CasCollision { digest });
            }
            return Ok(digest);
        }

        fs::rename(tmp_path.as_std_path(), path.as_std_path())?;
        Ok(digest)
    }

    /// Read raw bytes for a digest.
    pub fn get_bytes(&self, digest_hex: &str) -> Result<Vec<u8>> {
        let path = self.object_path(digest_hex)?;
        if !path.as_std_path().exists() {
            return Err(Error::BlobNotFound(normalize_digest(digest_hex)?));
        }
        let mut file = File::open(path.as_std_path())?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(buf)
    }
}

fn normalize_digest(digest_hex: &str) -> Result<String> {
    let lower = digest_hex.trim().to_ascii_lowercase();
    if lower.len() != 64 || !lower.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::InvalidDigest(digest_hex.to_string()));
    }
    Ok(lower)
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
    use tempfile::tempdir;

    #[test]
    fn sha256_hex_known_vector() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn object_path_two_hex_shard() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let cas = Cas::new(&root);
        let digest = "ab".to_string() + &"cd".repeat(31);
        let path = cas.object_path(&digest).expect("path");
        assert!(path
            .as_str()
            .replace('\\', "/")
            .ends_with(&format!("blobs/sha256/ab/{digest}")));
    }
}
