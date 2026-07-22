//! Local filesystem blob store — parity with matter-core `Cas` unencrypted layout.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use camino::{Utf8Path, Utf8PathBuf};

use crate::blob_store::BlobStore;
use crate::digest::normalize_digest;
use crate::error::{Result, StorageError};
use crate::hashing_reader::{HashingReader, HASHING_READ_BUF};
use crate::key_layout::local_relative_path;

/// Directory name under matter root for CAS blobs (parity with matter-core).
pub const BLOBS_DIR: &str = "blobs";
/// Algorithm subdirectory under `blobs/`.
pub const SHA256_DIR: &str = "sha256";

static STREAM_TMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Local filesystem CAS with layout `blobs/sha256/<aa>/<hex>`.
///
/// Streaming put uses temp + rename (same spirit as matter-core `Cas::put_reader`).
/// Unencrypted only — encryption stays in matter-core `Cas` for P0.
#[derive(Debug, Clone)]
pub struct LocalFsBlobStore {
    /// Path to `blobs/` directory.
    blobs_root: Utf8PathBuf,
}

impl LocalFsBlobStore {
    /// Open store rooted at `matter_root/blobs` (does not create dirs).
    pub fn new(matter_root: impl AsRef<Utf8Path>) -> Self {
        Self {
            blobs_root: matter_root.as_ref().join(BLOBS_DIR),
        }
    }

    /// Open store with an explicit blobs root directory.
    pub fn with_blobs_root(blobs_root: impl AsRef<Utf8Path>) -> Self {
        Self {
            blobs_root: blobs_root.as_ref().to_path_buf(),
        }
    }

    /// Path to `blobs/`.
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
        let rel = local_relative_path(digest_hex)?;
        Ok(self.blobs_root.join(rel))
    }
}

impl BlobStore for LocalFsBlobStore {
    fn put_stream(&self, expected_digest: Option<&str>, reader: &mut dyn Read) -> Result<String> {
        fs::create_dir_all(self.blobs_root.as_std_path())?;

        let seq = STREAM_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_name = format!(".stream-{seq}-{}.tmp", std::process::id());
        let tmp_path = self.blobs_root.join(&tmp_name);

        struct WipeTmp(Option<Utf8PathBuf>);
        impl Drop for WipeTmp {
            fn drop(&mut self) {
                if let Some(p) = self.0.take() {
                    let _ = fs::remove_file(p.as_std_path());
                }
            }
        }
        impl WipeTmp {
            fn disarm(&mut self) {
                self.0 = None;
            }
        }
        let mut wipe = WipeTmp(Some(tmp_path.clone()));

        let digest = {
            let mut file = File::create(tmp_path.as_std_path())?;
            let mut hasher = HashingReader::new(reader);
            let mut buf = vec![0u8; HASHING_READ_BUF];
            loop {
                let n = hasher.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])?;
            }
            file.sync_all()?;
            hasher.finalize()
        };

        if let Some(expected) = expected_digest {
            let exp = normalize_digest(expected)?;
            if exp != digest {
                // Temp wiped by Drop; no final object written.
                return Err(StorageError::DigestMismatch {
                    expected: exp,
                    computed: digest,
                });
            }
        }

        let path = self.object_path(&digest)?;
        if path.as_std_path().exists() {
            // Idempotent: same digest path ⇒ same content for SHA-256.
            wipe.disarm();
            let _ = fs::remove_file(tmp_path.as_std_path());
            return Ok(digest);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }

        if path.as_std_path().exists() {
            wipe.disarm();
            let _ = fs::remove_file(tmp_path.as_std_path());
            return Ok(digest);
        }

        match fs::rename(tmp_path.as_std_path(), path.as_std_path()) {
            Ok(()) => {
                wipe.disarm();
                Ok(digest)
            }
            Err(e) => {
                if path.as_std_path().exists() {
                    Ok(digest)
                } else {
                    Err(StorageError::Io(e))
                }
            }
        }
    }

    fn put_at_digest(&self, digest: &str, reader: &mut dyn Read) -> Result<()> {
        let digest = normalize_digest(digest)?;
        fs::create_dir_all(self.blobs_root.as_std_path())?;

        let path = self.object_path(&digest)?;
        if path.as_std_path().exists() {
            // Idempotent: key already present under plaintext identity.
            return Ok(());
        }

        let seq = STREAM_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_name = format!(".at-digest-{seq}-{}.tmp", std::process::id());
        let tmp_path = self.blobs_root.join(&tmp_name);

        struct WipeTmp(Option<Utf8PathBuf>);
        impl Drop for WipeTmp {
            fn drop(&mut self) {
                if let Some(p) = self.0.take() {
                    let _ = fs::remove_file(p.as_std_path());
                }
            }
        }
        impl WipeTmp {
            fn disarm(&mut self) {
                self.0 = None;
            }
        }
        let mut wipe = WipeTmp(Some(tmp_path.clone()));

        {
            let mut file = File::create(tmp_path.as_std_path())?;
            let mut buf = vec![0u8; HASHING_READ_BUF];
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])?;
            }
            file.sync_all()?;
        }

        if path.as_std_path().exists() {
            wipe.disarm();
            let _ = fs::remove_file(tmp_path.as_std_path());
            return Ok(());
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }

        if path.as_std_path().exists() {
            wipe.disarm();
            let _ = fs::remove_file(tmp_path.as_std_path());
            return Ok(());
        }

        match fs::rename(tmp_path.as_std_path(), path.as_std_path()) {
            Ok(()) => {
                wipe.disarm();
                Ok(())
            }
            Err(e) => {
                if path.as_std_path().exists() {
                    Ok(())
                } else {
                    Err(StorageError::Io(e))
                }
            }
        }
    }

    fn get_stream(&self, digest: &str) -> Result<Box<dyn Read + Send>> {
        let d = normalize_digest(digest)?;
        let path = self.object_path(&d)?;
        if !path.as_std_path().exists() {
            return Err(StorageError::NotFound(d));
        }
        let f = File::open(path.as_std_path())?;
        Ok(Box::new(f))
    }

    fn exists(&self, digest: &str) -> Result<bool> {
        let path = self.object_path(digest)?;
        Ok(path.as_std_path().exists())
    }

    fn delete(&self, digest: &str) -> Result<()> {
        let path = self.object_path(digest)?;
        if path.as_std_path().exists() {
            fs::remove_file(path.as_std_path())?;
        }
        Ok(())
    }

    fn health_check(&self) -> Result<()> {
        self.ensure_layout()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::sha256_hex;
    use std::io::Cursor;
    use tempfile::tempdir;

    #[test]
    fn put_get_exists_parity() {
        let dir = tempdir().expect("tmp");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let store = LocalFsBlobStore::new(&root);
        store.ensure_layout().expect("layout");

        let data = b"local fs blob parity";
        let digest = store
            .put_stream(None, &mut Cursor::new(data.as_slice()))
            .expect("put");
        assert_eq!(digest, sha256_hex(data));
        assert!(store.exists(&digest).expect("exists"));

        let mut r = store.get_stream(&digest).expect("get");
        let mut out = Vec::new();
        r.read_to_end(&mut out).expect("read");
        assert_eq!(out, data);

        let path = store.object_path(&digest).expect("path");
        assert!(path
            .as_str()
            .replace('\\', "/")
            .ends_with(&format!("blobs/sha256/{}/{digest}", &digest[..2])));
    }

    #[test]
    fn multi_mb_streaming_put() {
        let dir = tempdir().expect("tmp");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let store = LocalFsBlobStore::new(&root);

        // ~3 MiB of patterned data.
        let mut data = Vec::with_capacity(3 * 1024 * 1024);
        for i in 0..(3 * 1024 * 1024) {
            data.push((i % 251) as u8);
        }
        let expected = sha256_hex(&data);
        let got = store
            .put_stream(Some(&expected), &mut Cursor::new(data.as_slice()))
            .expect("put");
        assert_eq!(got, expected);
        let mut r = store.get_stream(&got).expect("get");
        let mut out = Vec::new();
        r.read_to_end(&mut out).expect("read");
        assert_eq!(out.len(), data.len());
        assert_eq!(out, data);
    }

    #[test]
    fn digest_mismatch_leaves_no_object() {
        let dir = tempdir().expect("tmp");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let store = LocalFsBlobStore::new(&root);
        let wrong = "ab".to_string() + &"cd".repeat(31);
        let err = store
            .put_stream(Some(&wrong), &mut Cursor::new(b"hello".as_slice()))
            .expect_err("mismatch");
        match err {
            StorageError::DigestMismatch { .. } => {}
            e => panic!("unexpected: {e}"),
        }
        assert!(!store.exists(&wrong).expect("exists"));
    }

    #[test]
    fn put_at_digest_allows_non_matching_stream() {
        let dir = tempdir().expect("tmp");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let store = LocalFsBlobStore::new(&root);
        // Pretend plaintext digest; store different "ciphertext" bytes under it.
        let plain_digest = sha256_hex(b"plaintext identity");
        let cipher = b"ciphertext-bytes-not-matching-digest";
        store
            .put_at_digest(&plain_digest, &mut Cursor::new(cipher.as_slice()))
            .expect("put_at_digest");
        assert!(store.exists(&plain_digest).expect("exists"));
        let mut r = store.get_stream(&plain_digest).expect("get");
        let mut out = Vec::new();
        r.read_to_end(&mut out).expect("read");
        assert_eq!(out, cipher);
        assert_ne!(sha256_hex(&out), plain_digest);
    }
}
