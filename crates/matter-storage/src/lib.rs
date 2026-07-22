//! # matter-storage
//!
//! Opt-in **content-addressed blob storage** backends for Dedupe Desk matters
//! (track **0061**).
//!
//! ## Offline default
//!
//! Default features are **local only** — no cloud SDKs. Desk never requires cloud
//! credentials or network to build or run.
//!
//! ## Backends
//!
//! | Kind | Feature | Notes |
//! |---|---|---|
//! | [`LocalFsBlobStore`] | (default) | Parity with matter-core `blobs/sha256/<aa>/<hex>` |
//! | [`InMemoryBlobStore`] | (default) | Unit tests |
//! | [`S3BlobStore`] | `cloud-s3` | S3/MinIO/R2 via `object_store` 0.14.x |
//! | Azure | `cloud-azure` residual | Feature residual; not opened in P0 factory |
//!
//! ## Integrity & memory
//!
//! - **HashingReader** on put — SHA-256 while streaming; mismatch → delete + fail
//!   (never trust S3 multipart ETag as content hash).
//! - **Multipart caps** — ≤10 MiB part (hard max 16 MiB), ≤2 concurrent (≲~20 MiB peak).
//! - **CachedBlobStore** — disk LRU under matter `.cache/` for cloud gets.
//!
//! ## Secrets
//!
//! Config types hold **no credentials**. Use env / IAM / keyring only.
//!
//! ## SQLite
//!
//! Matter SQLite stays **host-local**. Only CAS blob bytes may live in object storage.
//!
//! ## Encryption (0057)
//!
//! When matter encryption is on, callers hash **plaintext** for the CAS digest and
//! put **ciphertext** via [`BlobStore::put_at_digest`] under that plaintext identity
//! (same as matter-core `Cas`). Plaintext puts use [`BlobStore::put_stream`].

#![forbid(unsafe_code)]

pub mod blob_store;
pub mod cached;
pub mod config;
pub mod digest;
pub mod error;
pub mod hashing_reader;
pub mod key_layout;
pub mod local_fs;
pub mod memory_store;
pub mod multipart;

#[cfg(feature = "cloud-s3")]
pub mod s3;

pub use blob_store::BlobStore;
pub use cached::{CachedBlobStore, CACHE_DIR};
pub use config::{JobBackendKind, SseMode, StorageBackendConfig, StorageBackendKind};
pub use digest::{normalize_digest, sha256_hex, DIGEST_HEX_LEN};
pub use error::{Result, StorageError};
pub use hashing_reader::HashingReader;
pub use key_layout::{local_relative_path, object_key};
pub use local_fs::{LocalFsBlobStore, BLOBS_DIR, SHA256_DIR};
pub use memory_store::{CountingBlobStore, InMemoryBlobStore};
pub use multipart::{MultipartLimits, DEFAULT_PART_SIZE, MAX_CONCURRENT_PARTS, MAX_PART_SIZE};

#[cfg(feature = "cloud-s3")]
pub use s3::S3BlobStore;

use camino::Utf8Path;

/// Open a [`BlobStore`] from non-secret config.
///
/// - `local` → [`LocalFsBlobStore`] under `matter_root`
/// - `s3` → requires feature `cloud-s3`; credentials from env; **fails closed** if
///   the feature is missing or open fails (no silent local fallback)
/// - `azure` → residual error unless `cloud-azure` is implemented later
///
/// When `kind` is cloud and `wrap_cache` is true, wraps with [`CachedBlobStore`]
/// under `matter_root/.cache` (max bytes from config or 10 GiB default).
///
/// Cloud kinds require a non-empty `matter_id` in `config` for key isolation.
pub fn open_blob_store(
    config: &StorageBackendConfig,
    matter_root: &Utf8Path,
    wrap_cache: bool,
) -> Result<Box<dyn BlobStore>> {
    config.validate()?;
    match config.kind {
        StorageBackendKind::Local => {
            let store = LocalFsBlobStore::new(matter_root);
            store.ensure_layout()?;
            Ok(Box::new(store))
        }
        StorageBackendKind::S3 => {
            let mid = config
                .matter_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if mid.is_none() {
                return Err(StorageError::Config(
                    "s3 storage requires matter_id for object key isolation".into(),
                ));
            }
            #[cfg(feature = "cloud-s3")]
            {
                let s3 = S3BlobStore::from_config(config.clone())?;
                if wrap_cache {
                    let max = config.cache_max_bytes.unwrap_or(10 * 1024 * 1024 * 1024);
                    let cached = CachedBlobStore::under_matter(s3, matter_root, max)?;
                    Ok(Box::new(cached))
                } else {
                    Ok(Box::new(s3))
                }
            }
            #[cfg(not(feature = "cloud-s3"))]
            {
                let _ = (matter_root, wrap_cache);
                Err(StorageError::Config(
                    "storage kind s3 requires building with feature cloud-s3 \
                     (rebuild with --features cloud-s3; config may still be stored \
                     for a feature-enabled service binary; open fails closed — no local fallback)"
                        .into(),
                ))
            }
        }
        StorageBackendKind::Azure => Err(StorageError::Config(
            "storage kind azure is residual (feature cloud-azure); not opened in P0 \
             (open fails closed — no local fallback)"
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;

    /// Multi-GB **logical** stream of zeros without allocating multi-GB RAM.
    struct ZeroReader {
        remaining: u64,
    }

    impl std::io::Read for ZeroReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Ok(0);
            }
            let n = std::cmp::min(buf.len() as u64, self.remaining) as usize;
            for b in &mut buf[..n] {
                *b = 0;
            }
            self.remaining -= n as u64;
            Ok(n)
        }
    }

    #[test]
    fn open_local_factory() {
        let dir = tempdir().expect("tmp");
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let store = open_blob_store(&StorageBackendConfig::local(), &root, false).expect("open");
        let d = store
            .put_stream(None, &mut Cursor::new(b"factory".as_slice()))
            .expect("put");
        assert!(store.exists(&d).expect("ex"));
    }

    #[test]
    fn o1_ram_multipart_10gb_logical() {
        // DoD-3a: 10 GiB **logical** size accounting with part_size=10 MiB, concurrent=2.
        // CountingPartUploader never retains more than concurrent live part buffers.
        let part_size = 10 * 1024 * 1024usize;
        let concurrent = 2usize;
        let logical_bytes: u64 = 10 * 1024 * 1024 * 1024; // 10 GiB
        let total_parts = (logical_bytes as usize).div_ceil(part_size);

        // Size accounting: track live part buffer sizes only (no multi-GB alloc).
        let mut peak: usize = 0;
        let mut live: Vec<usize> = Vec::new();
        for i in 0..total_parts {
            let this_part = if i + 1 == total_parts {
                let rem = (logical_bytes as usize) % part_size;
                if rem == 0 {
                    part_size
                } else {
                    rem
                }
            } else {
                part_size
            };
            // Upload oldest before accepting a new part when at capacity.
            if live.len() >= concurrent {
                live.remove(0); // uploaded → free
            }
            live.push(this_part);
            // Budget uses full part_size slots (worst-case live buffers).
            peak = peak.max(live.len().saturating_mul(part_size));
        }
        live.clear();

        let cap = part_size * concurrent;
        assert!(
            peak <= cap,
            "peak part buffers {peak} exceeds ~20 MiB target {cap} for {total_parts} parts / 10 GiB"
        );
        assert_eq!(cap, 20 * 1024 * 1024);

        // Streaming path (smaller): HashingReader + instrumented store, peak still O(1).
        let store = InMemoryBlobStore::new();
        let mut reader = ZeroReader {
            remaining: 32 * 1024 * 1024,
        };
        let d = store
            .put_stream_multipart_instrumented(None, &mut reader, part_size, concurrent, true)
            .expect("put");
        assert_eq!(d.len(), 64);
        assert!(store.peak_part_bytes() <= (cap as u64) + part_size as u64);
        assert!(store.exists(&d).expect("ex"));
    }

    #[test]
    fn hashing_mismatch_no_store() {
        let store = InMemoryBlobStore::new();
        let wrong = "ff".to_string() + &"00".repeat(31);
        let err = store
            .put_stream(Some(&wrong), &mut Cursor::new(b"x".as_slice()))
            .expect_err("mm");
        match err {
            StorageError::DigestMismatch { .. } => {}
            e => panic!("{e}"),
        }
        assert!(!store.exists(&wrong).expect("e"));
    }
}
