//! In-memory [`BlobStore`] for unit tests (no cloud deps).

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::blob_store::BlobStore;
use crate::digest::normalize_digest;
use crate::error::{Result, StorageError};
use crate::hashing_reader::{HashingReader, HASHING_READ_BUF};

/// Thread-safe in-memory blob store with optional fetch counter (cache tests).
#[derive(Debug, Default)]
pub struct InMemoryBlobStore {
    inner: Mutex<HashMap<String, Vec<u8>>>,
    /// Incremented on each successful `get_stream` (for cache hit tests).
    get_count: AtomicU64,
    /// Peak single part buffer observed during instrumented multipart puts.
    peak_part_bytes: AtomicU64,
}

impl InMemoryBlobStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of successful `get_stream` calls.
    pub fn get_count(&self) -> u64 {
        self.get_count.load(Ordering::SeqCst)
    }

    /// Peak part buffer size recorded by [`Self::put_stream_multipart_instrumented`].
    pub fn peak_part_bytes(&self) -> u64 {
        self.peak_part_bytes.load(Ordering::SeqCst)
    }

    /// Multipart-style put that never holds more than `part_size` of the stream
    /// at once, tracking peak buffer size. Used for O(1) RAM proof tests.
    ///
    /// Logical stream size may be multi-GB; only `part_size × concurrent` is
    /// buffered. Does **not** retain full object bytes when
    /// `store_bytes` is false (use for multi-GB proof).
    pub fn put_stream_multipart_instrumented(
        &self,
        expected_digest: Option<&str>,
        reader: &mut dyn Read,
        part_size: usize,
        max_concurrent: usize,
        store_bytes: bool,
    ) -> Result<String> {
        if part_size == 0 || part_size > crate::multipart::MAX_PART_SIZE {
            return Err(StorageError::Config(format!(
                "part_size must be 1..={} bytes",
                crate::multipart::MAX_PART_SIZE
            )));
        }
        if max_concurrent == 0 || max_concurrent > crate::multipart::MAX_CONCURRENT_PARTS {
            return Err(StorageError::Config(format!(
                "max_concurrent must be 1..={}",
                crate::multipart::MAX_CONCURRENT_PARTS
            )));
        }

        self.peak_part_bytes.store(0, Ordering::SeqCst);
        // Simulated live part buffers: at most max_concurrent parts.
        let mut live: Vec<Vec<u8>> = Vec::new();
        let mut assembled = if store_bytes { Some(Vec::new()) } else { None };
        let mut hasher = HashingReader::new(reader);
        let mut part_buf = Vec::with_capacity(part_size);

        loop {
            let mut chunk = vec![0u8; HASHING_READ_BUF.min(part_size)];
            let n = hasher.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            part_buf.extend_from_slice(&chunk[..n]);
            while part_buf.len() >= part_size {
                let part: Vec<u8> = part_buf.drain(..part_size).collect();
                if live.len() >= max_concurrent {
                    // "Upload" oldest part: free buffer (optionally assemble).
                    let done = live.remove(0);
                    if let Some(ref mut a) = assembled {
                        a.extend_from_slice(&done);
                    }
                    drop(done);
                }
                live.push(part);
                let peak = live.len() * part_size;
                self.peak_part_bytes
                    .fetch_max(peak as u64, Ordering::SeqCst);
            }
        }
        if !part_buf.is_empty() {
            if live.len() >= max_concurrent {
                let done = live.remove(0);
                if let Some(ref mut a) = assembled {
                    a.extend_from_slice(&done);
                }
            }
            live.push(part_buf);
            let peak = live.iter().map(Vec::len).sum::<usize>();
            self.peak_part_bytes
                .fetch_max(peak as u64, Ordering::SeqCst);
        }
        for p in live {
            if let Some(ref mut a) = assembled {
                a.extend_from_slice(&p);
            }
            drop(p);
        }

        let digest = hasher.finalize();
        if let Some(expected) = expected_digest {
            let exp = normalize_digest(expected)?;
            if exp != digest {
                return Err(StorageError::DigestMismatch {
                    expected: exp,
                    computed: digest,
                });
            }
        }
        if let Some(data) = assembled {
            self.inner
                .lock()
                .map_err(|e| StorageError::Other(format!("lock: {e}")))?
                .insert(digest.clone(), data);
        } else {
            // Placeholder entry so exists() can succeed without multi-GB RAM.
            self.inner
                .lock()
                .map_err(|e| StorageError::Other(format!("lock: {e}")))?
                .insert(digest.clone(), Vec::new());
        }
        Ok(digest)
    }
}

impl BlobStore for InMemoryBlobStore {
    fn put_stream(&self, expected_digest: Option<&str>, reader: &mut dyn Read) -> Result<String> {
        let mut hasher = HashingReader::new(reader);
        let mut data = Vec::new();
        let mut buf = vec![0u8; HASHING_READ_BUF];
        loop {
            let n = hasher.read(&mut buf)?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        let digest = hasher.finalize();
        if let Some(expected) = expected_digest {
            let exp = normalize_digest(expected)?;
            if exp != digest {
                return Err(StorageError::DigestMismatch {
                    expected: exp,
                    computed: digest,
                });
            }
        }
        self.inner
            .lock()
            .map_err(|e| StorageError::Other(format!("lock: {e}")))?
            .insert(digest.clone(), data);
        Ok(digest)
    }

    fn put_at_digest(&self, digest: &str, reader: &mut dyn Read) -> Result<()> {
        let d = normalize_digest(digest)?;
        let mut data = Vec::new();
        let mut buf = vec![0u8; HASHING_READ_BUF];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buf[..n]);
        }
        self.inner
            .lock()
            .map_err(|e| StorageError::Other(format!("lock: {e}")))?
            .insert(d, data);
        Ok(())
    }

    fn get_stream(&self, digest: &str) -> Result<Box<dyn Read + Send>> {
        let d = normalize_digest(digest)?;
        let map = self
            .inner
            .lock()
            .map_err(|e| StorageError::Other(format!("lock: {e}")))?;
        let data = map
            .get(&d)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(d.clone()))?;
        self.get_count.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Cursor::new(data)))
    }

    fn exists(&self, digest: &str) -> Result<bool> {
        let d = normalize_digest(digest)?;
        let map = self
            .inner
            .lock()
            .map_err(|e| StorageError::Other(format!("lock: {e}")))?;
        Ok(map.contains_key(&d))
    }

    fn delete(&self, digest: &str) -> Result<()> {
        let d = normalize_digest(digest)?;
        let mut map = self
            .inner
            .lock()
            .map_err(|e| StorageError::Other(format!("lock: {e}")))?;
        map.remove(&d);
        Ok(())
    }

    fn health_check(&self) -> Result<()> {
        Ok(())
    }
}

/// Counting wrapper: increments `fetch_count` on each get (for CachedBlobStore tests).
#[derive(Debug)]
pub struct CountingBlobStore<S: BlobStore> {
    inner: S,
    pub fetch_count: AtomicU64,
}

impl<S: BlobStore> CountingBlobStore<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            fetch_count: AtomicU64::new(0),
        }
    }

    pub fn fetches(&self) -> u64 {
        self.fetch_count.load(Ordering::SeqCst)
    }
}

impl<S: BlobStore> BlobStore for CountingBlobStore<S> {
    fn put_stream(&self, expected_digest: Option<&str>, reader: &mut dyn Read) -> Result<String> {
        self.inner.put_stream(expected_digest, reader)
    }

    fn put_at_digest(&self, digest: &str, reader: &mut dyn Read) -> Result<()> {
        self.inner.put_at_digest(digest, reader)
    }

    fn get_stream(&self, digest: &str) -> Result<Box<dyn Read + Send>> {
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        self.inner.get_stream(digest)
    }

    fn exists(&self, digest: &str) -> Result<bool> {
        self.inner.exists(digest)
    }

    fn delete(&self, digest: &str) -> Result<()> {
        self.inner.delete(digest)
    }

    fn health_check(&self) -> Result<()> {
        self.inner.health_check()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::sha256_hex;
    use std::io::Cursor;

    #[test]
    fn put_at_digest_non_matching_content() {
        let store = InMemoryBlobStore::new();
        let plain = sha256_hex(b"plain");
        let cipher = b"not-the-plain-bytes";
        store
            .put_at_digest(&plain, &mut Cursor::new(cipher.as_slice()))
            .expect("put");
        assert!(store.exists(&plain).expect("ex"));
        let mut r = store.get_stream(&plain).expect("get");
        let mut out = Vec::new();
        r.read_to_end(&mut out).expect("read");
        assert_eq!(out, cipher);
        assert_ne!(sha256_hex(&out), plain);
    }
}
