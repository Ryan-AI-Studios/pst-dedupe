//! Disk LRU cache wrapper for cloud blob gets ([`CachedBlobStore`]).

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use camino::{Utf8Path, Utf8PathBuf};

use crate::blob_store::BlobStore;
use crate::digest::normalize_digest;
use crate::error::{Result, StorageError};
use crate::hashing_reader::HASHING_READ_BUF;

/// Default cache directory name under matter root.
pub const CACHE_DIR: &str = ".cache";
/// Subdir for CAS blob cache entries.
pub const CACHE_BLOBS_DIR: &str = "blobs";

/// Disk LRU cache over an inner [`BlobStore`] (typically cloud).
///
/// - **Location:** matter-local `.cache/blobs/` (not OS temp for case content).
/// - **Flow:** `get_stream` hit → local file; miss → stream from inner into cache, then serve.
/// - Local backends may skip this wrapper (already on disk).
pub struct CachedBlobStore<S: BlobStore> {
    inner: S,
    cache_root: Utf8PathBuf,
    max_bytes: u64,
    /// digest → (size, last_access_unix_ms)
    index: Mutex<HashMap<String, (u64, u64)>>,
    /// Successful fills from inner (misses that fetched).
    cloud_fetches: AtomicU64,
}

impl<S: BlobStore> CachedBlobStore<S> {
    /// Wrap `inner` with a disk cache at `cache_root` (e.g. `matter/.cache`).
    pub fn new(inner: S, cache_root: impl AsRef<Utf8Path>, max_bytes: u64) -> Result<Self> {
        let cache_root = cache_root.as_ref().to_path_buf();
        let blobs = cache_root.join(CACHE_BLOBS_DIR);
        fs::create_dir_all(blobs.as_std_path())?;
        Ok(Self {
            inner,
            cache_root,
            max_bytes: max_bytes.max(1),
            index: Mutex::new(HashMap::new()),
            cloud_fetches: AtomicU64::new(0),
        })
    }

    /// Under matter root: `matter_root/.cache`.
    pub fn under_matter(
        inner: S,
        matter_root: impl AsRef<Utf8Path>,
        max_bytes: u64,
    ) -> Result<Self> {
        Self::new(inner, matter_root.as_ref().join(CACHE_DIR), max_bytes)
    }

    /// Number of times a get missed cache and fetched from inner.
    pub fn cloud_fetch_count(&self) -> u64 {
        self.cloud_fetches.load(Ordering::SeqCst)
    }

    fn cache_path(&self, digest: &str) -> Result<Utf8PathBuf> {
        let d = normalize_digest(digest)?;
        let shard = &d[..2];
        Ok(self.cache_root.join(CACHE_BLOBS_DIR).join(shard).join(&d))
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn total_cached(index: &HashMap<String, (u64, u64)>) -> u64 {
        index.values().map(|(sz, _)| *sz).sum()
    }

    fn evict_if_needed(&self, index: &mut HashMap<String, (u64, u64)>, need: u64) -> Result<()> {
        while Self::total_cached(index) + need > self.max_bytes && !index.is_empty() {
            // Evict least-recently accessed.
            let victim = index
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(k, _)| k.clone());
            if let Some(d) = victim {
                if let Ok(path) = self.cache_path(&d) {
                    let _ = fs::remove_file(path.as_std_path());
                }
                index.remove(&d);
            } else {
                break;
            }
        }
        Ok(())
    }

    fn fill_from_inner(&self, digest: &str) -> Result<Utf8PathBuf> {
        let d = normalize_digest(digest)?;
        let path = self.cache_path(&d)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }

        let mut src = self.inner.get_stream(&d)?;
        self.cloud_fetches.fetch_add(1, Ordering::SeqCst);

        let tmp = path.with_extension("tmp");
        let mut size = 0u64;
        {
            let mut out = File::create(tmp.as_std_path())?;
            let mut buf = vec![0u8; HASHING_READ_BUF];
            loop {
                let n = src.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                size += n as u64;
            }
            out.sync_all()?;
        }

        {
            let mut index = self
                .index
                .lock()
                .map_err(|e| StorageError::Other(format!("lock: {e}")))?;
            self.evict_if_needed(&mut index, size)?;
            if tmp.as_std_path().exists() {
                if path.as_std_path().exists() {
                    let _ = fs::remove_file(path.as_std_path());
                }
                fs::rename(tmp.as_std_path(), path.as_std_path())?;
            }
            index.insert(d, (size, Self::now_ms()));
        }
        Ok(path)
    }
}

impl<S: BlobStore> BlobStore for CachedBlobStore<S> {
    fn put_stream(&self, expected_digest: Option<&str>, reader: &mut dyn Read) -> Result<String> {
        // Puts go straight to inner (cache is for gets).
        self.inner.put_stream(expected_digest, reader)
    }

    fn put_at_digest(&self, digest: &str, reader: &mut dyn Read) -> Result<()> {
        self.inner.put_at_digest(digest, reader)
    }

    fn get_stream(&self, digest: &str) -> Result<Box<dyn Read + Send>> {
        let d = normalize_digest(digest)?;
        let path = self.cache_path(&d)?;

        if path.as_std_path().exists() {
            if let Ok(mut index) = self.index.lock() {
                if let Some(entry) = index.get_mut(&d) {
                    entry.1 = Self::now_ms();
                } else if let Ok(meta) = fs::metadata(path.as_std_path()) {
                    index.insert(d.clone(), (meta.len(), Self::now_ms()));
                }
            }
            let f = File::open(path.as_std_path())?;
            return Ok(Box::new(f));
        }

        let path = self.fill_from_inner(&d)?;
        let f = File::open(path.as_std_path())?;
        Ok(Box::new(f))
    }

    fn exists(&self, digest: &str) -> Result<bool> {
        let d = normalize_digest(digest)?;
        let path = self.cache_path(&d)?;
        if path.as_std_path().exists() {
            return Ok(true);
        }
        self.inner.exists(&d)
    }

    fn delete(&self, digest: &str) -> Result<()> {
        let d = normalize_digest(digest)?;
        let path = self.cache_path(&d)?;
        if path.as_std_path().exists() {
            let _ = fs::remove_file(path.as_std_path());
        }
        if let Ok(mut index) = self.index.lock() {
            index.remove(&d);
        }
        self.inner.delete(&d)
    }

    fn health_check(&self) -> Result<()> {
        fs::create_dir_all(self.cache_root.join(CACHE_BLOBS_DIR).as_std_path())?;
        self.inner.health_check()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::sha256_hex;
    use crate::memory_store::{CountingBlobStore, InMemoryBlobStore};
    use std::io::Cursor;
    use tempfile::tempdir;

    #[test]
    fn second_get_is_cache_hit() {
        let dir = tempdir().expect("tmp");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let mem = InMemoryBlobStore::new();
        let counting = CountingBlobStore::new(mem);
        let data = b"cached cloud blob";
        let digest = counting
            .put_stream(None, &mut Cursor::new(data.as_slice()))
            .expect("put");
        assert_eq!(digest, sha256_hex(data));

        let cached =
            CachedBlobStore::under_matter(counting, &root, 10 * 1024 * 1024).expect("cache");

        let mut r1 = cached.get_stream(&digest).expect("get1");
        let mut out1 = Vec::new();
        r1.read_to_end(&mut out1).expect("read1");
        assert_eq!(out1, data);
        assert_eq!(cached.cloud_fetch_count(), 1);
        assert_eq!(cached.inner.fetches(), 1);

        let mut r2 = cached.get_stream(&digest).expect("get2");
        let mut out2 = Vec::new();
        r2.read_to_end(&mut out2).expect("read2");
        assert_eq!(out2, data);
        // No second cloud fetch.
        assert_eq!(cached.cloud_fetch_count(), 1);
        assert_eq!(cached.inner.fetches(), 1);
    }
}
