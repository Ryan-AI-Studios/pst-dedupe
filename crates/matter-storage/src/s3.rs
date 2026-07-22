//! S3-compatible [`BlobStore`] via `object_store` (feature `cloud-s3`).
//!
//! Credentials: env / IAM only (`AmazonS3Builder::from_env`). Never matter.db.
//!
//! Multipart: part size ≤ 10 MiB (hard max 16 MiB), ≤ 2 concurrent parts.
//! HashingReader on content-addressed put; digest mismatch → abort + delete.
//! All non-digest multipart failures also abort + best-effort delete.

use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use futures::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, ObjectStoreExt, WriteMultipart};
use tokio::runtime::Runtime;

use crate::blob_store::BlobStore;
use crate::config::{StorageBackendConfig, StorageBackendKind};
use crate::digest::normalize_digest;
use crate::error::{Result, StorageError};
use crate::hashing_reader::{HashingReader, HASHING_READ_BUF};
use crate::key_layout::object_key;
use crate::multipart::MultipartLimits;

/// S3-compatible content-addressed blob store.
pub struct S3BlobStore {
    store: Arc<dyn ObjectStore>,
    config: StorageBackendConfig,
    limits: MultipartLimits,
    runtime: Runtime,
}

impl S3BlobStore {
    /// Build from non-secret config + env credentials.
    pub fn from_config(config: StorageBackendConfig) -> Result<Self> {
        if config.kind != StorageBackendKind::S3 {
            return Err(StorageError::Config("S3BlobStore requires kind=s3".into()));
        }
        config.validate()?;
        let limits = MultipartLimits::default();

        let bucket = config
            .bucket
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| StorageError::Config("s3 requires bucket".into()))?;

        let mut builder = AmazonS3Builder::from_env().with_bucket_name(bucket);
        if let Some(region) = config
            .region
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            builder = builder.with_region(region);
        }
        if let Some(endpoint) = config
            .endpoint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            builder = builder.with_endpoint(endpoint);
            if endpoint.starts_with("http://") {
                builder = builder.with_allow_http(true);
            }
        }

        let store = builder
            .build()
            .map_err(|e| StorageError::Cloud(format!("s3 builder: {e}")))?;

        let runtime = Runtime::new().map_err(|e| StorageError::Cloud(format!("tokio: {e}")))?;

        Ok(Self {
            store: Arc::new(store),
            config,
            limits,
            runtime,
        })
    }

    /// Wrap an existing `ObjectStore` (tests / memory mock).
    pub fn from_object_store(
        store: Arc<dyn ObjectStore>,
        config: StorageBackendConfig,
        limits: MultipartLimits,
    ) -> Result<Self> {
        limits_validate(&limits)?;
        let runtime = Runtime::new().map_err(|e| StorageError::Cloud(format!("tokio: {e}")))?;
        Ok(Self {
            store,
            config,
            limits,
            runtime,
        })
    }

    fn key_for(&self, digest: &str) -> Result<ObjectPath> {
        let key = object_key(
            self.config.prefix.as_deref(),
            self.config.tenant_id.as_deref(),
            self.config.matter_id.as_deref(),
            digest,
        )?;
        Ok(ObjectPath::from(key))
    }

    fn block_on<F, T>(&self, fut: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        self.runtime.block_on(fut)
    }
}

fn limits_validate(limits: &MultipartLimits) -> Result<()> {
    MultipartLimits::new(limits.part_size, limits.max_concurrent).map(|_| ())
}

impl BlobStore for S3BlobStore {
    fn put_stream(&self, expected_digest: Option<&str>, reader: &mut dyn Read) -> Result<String> {
        // P0: if expected_digest is Some, single-pass multipart to final key.
        // If None, stage to temp, hash, then upload by digest key.
        if let Some(expected) = expected_digest {
            let exp = normalize_digest(expected)?;
            let path = self.key_for(&exp)?;
            return self.block_on(async {
                put_multipart_hashed(self.store.as_ref(), &path, reader, Some(&exp), self.limits)
                    .await
            });
        }

        // Unknown digest: stage to temp, hash, then upload by digest key.
        let mut tmp = tempfile::tempfile().map_err(StorageError::Io)?;
        let mut hasher = HashingReader::new(reader);
        let mut buf = vec![0u8; HASHING_READ_BUF];
        loop {
            let n = hasher.read(&mut buf)?;
            if n == 0 {
                break;
            }
            tmp.write_all(&buf[..n])?;
        }
        let digest = hasher.finalize();
        tmp.seek(SeekFrom::Start(0))?;
        let path = self.key_for(&digest)?;
        self.block_on(async {
            put_multipart_hashed(
                self.store.as_ref(),
                &path,
                &mut tmp,
                Some(&digest),
                self.limits,
            )
            .await
        })
    }

    fn put_at_digest(&self, digest: &str, reader: &mut dyn Read) -> Result<()> {
        let d = normalize_digest(digest)?;
        let path = self.key_for(&d)?;
        self.block_on(async {
            put_multipart_raw(self.store.as_ref(), &path, reader, self.limits).await
        })
    }

    fn get_stream(&self, digest: &str) -> Result<Box<dyn Read + Send>> {
        let d = normalize_digest(digest)?;
        let path = self.key_for(&d)?;
        let store = Arc::clone(&self.store);

        // Stream chunks into an OS temp file (wipe-on-drop) — never `bytes()` whole object.
        let mut tmp = tempfile::tempfile().map_err(StorageError::Io)?;
        self.block_on(async {
            let result = store.get(&path).await.map_err(|e| match e {
                object_store::Error::NotFound { .. } => StorageError::NotFound(d.clone()),
                other => StorageError::Cloud(format!("get: {other}")),
            })?;
            let mut stream = result.into_stream();
            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(|e| StorageError::Cloud(format!("get stream: {e}")))?;
                tmp.write_all(&bytes).map_err(StorageError::Io)?;
            }
            tmp.sync_all().map_err(StorageError::Io)?;
            Ok::<(), StorageError>(())
        })?;
        tmp.seek(SeekFrom::Start(0))?;
        Ok(Box::new(tmp))
    }

    fn exists(&self, digest: &str) -> Result<bool> {
        let d = normalize_digest(digest)?;
        let path = self.key_for(&d)?;
        let store = Arc::clone(&self.store);
        self.block_on(async {
            match store.head(&path).await {
                Ok(_) => Ok(true),
                Err(object_store::Error::NotFound { .. }) => Ok(false),
                Err(e) => Err(StorageError::Cloud(format!("head: {e}"))),
            }
        })
    }

    fn delete(&self, digest: &str) -> Result<()> {
        let d = normalize_digest(digest)?;
        let path = self.key_for(&d)?;
        let store = Arc::clone(&self.store);
        self.block_on(async {
            match store.delete(&path).await {
                Ok(()) => Ok(()),
                Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(e) => Err(StorageError::Cloud(format!("delete: {e}"))),
            }
        })
    }

    fn health_check(&self) -> Result<()> {
        let store = Arc::clone(&self.store);
        self.block_on(async {
            // List with limit 1 as connectivity probe.
            let mut stream = store.list(None);
            match stream.next().await {
                None => Ok(()),
                Some(Ok(_)) => Ok(()),
                Some(Err(e)) => Err(StorageError::Cloud(format!("health list: {e}"))),
            }
        })
    }
}

/// Content-addressed multipart put with HashingReader.
///
/// On **any** failure after `put_multipart` starts (read, capacity, digest mismatch,
/// finish), aborts the multipart upload and best-effort deletes the object key.
async fn put_multipart_hashed(
    store: &dyn ObjectStore,
    path: &ObjectPath,
    reader: &mut dyn Read,
    expected: Option<&str>,
    limits: MultipartLimits,
) -> Result<String> {
    // Idempotent content-addressed put: object key embeds the SHA-256 digest.
    if store.head(path).await.is_ok() {
        if let Some(exp) = expected {
            let exp = normalize_digest(exp)?;
            return Ok(exp);
        }
        let mut hasher = HashingReader::new(reader);
        let mut buf = vec![0u8; HASHING_READ_BUF];
        loop {
            let n = hasher.read(&mut buf).map_err(StorageError::Io)?;
            if n == 0 {
                break;
            }
        }
        return Ok(hasher.finalize());
    }

    let upload = store
        .put_multipart(path)
        .await
        .map_err(|e| StorageError::Cloud(format!("put_multipart: {e}")))?;

    let mut write = Some(WriteMultipart::new_with_chunk_size(
        upload,
        limits.part_size,
    ));
    let mut hasher = HashingReader::new(reader);
    let mut buf = vec![0u8; HASHING_READ_BUF];

    let body = async {
        loop {
            write
                .as_mut()
                .ok_or_else(|| StorageError::Other("multipart write missing".into()))?
                .wait_for_capacity(limits.max_concurrent)
                .await
                .map_err(|e| StorageError::Cloud(format!("wait_for_capacity: {e}")))?;
            let n = hasher.read(&mut buf).map_err(StorageError::Io)?;
            if n == 0 {
                break;
            }
            write
                .as_mut()
                .ok_or_else(|| StorageError::Other("multipart write missing".into()))?
                .write(&buf[..n]);
        }

        let digest = hasher.finalize();
        if let Some(exp) = expected {
            let exp = normalize_digest(exp)?;
            if exp != digest {
                return Err(StorageError::DigestMismatch {
                    expected: exp,
                    computed: digest,
                });
            }
        }

        let w = write
            .take()
            .ok_or_else(|| StorageError::Other("multipart write missing".into()))?;
        w.finish()
            .await
            .map_err(|e| StorageError::Cloud(format!("multipart finish: {e}")))?;
        Ok(digest)
    }
    .await;

    match body {
        Ok(d) => Ok(d),
        Err(e) => {
            if let Some(w) = write.take() {
                let _ = w.abort().await;
            }
            let _ = store.delete(path).await;
            Err(e)
        }
    }
}

/// Multipart put without content-hash identity (ciphertext under plaintext digest).
///
/// Aborts + best-effort delete on all failure paths after multipart starts.
async fn put_multipart_raw(
    store: &dyn ObjectStore,
    path: &ObjectPath,
    reader: &mut dyn Read,
    limits: MultipartLimits,
) -> Result<()> {
    if store.head(path).await.is_ok() {
        return Ok(());
    }

    let upload = store
        .put_multipart(path)
        .await
        .map_err(|e| StorageError::Cloud(format!("put_multipart: {e}")))?;

    let mut write = Some(WriteMultipart::new_with_chunk_size(
        upload,
        limits.part_size,
    ));
    let mut buf = vec![0u8; HASHING_READ_BUF];

    let body = async {
        loop {
            write
                .as_mut()
                .ok_or_else(|| StorageError::Other("multipart write missing".into()))?
                .wait_for_capacity(limits.max_concurrent)
                .await
                .map_err(|e| StorageError::Cloud(format!("wait_for_capacity: {e}")))?;
            let n = reader.read(&mut buf).map_err(StorageError::Io)?;
            if n == 0 {
                break;
            }
            write
                .as_mut()
                .ok_or_else(|| StorageError::Other("multipart write missing".into()))?
                .write(&buf[..n]);
        }
        let w = write
            .take()
            .ok_or_else(|| StorageError::Other("multipart write missing".into()))?;
        w.finish()
            .await
            .map_err(|e| StorageError::Cloud(format!("multipart finish: {e}")))?;
        Ok(())
    }
    .await;

    match body {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Some(w) = write.take() {
                let _ = w.abort().await;
            }
            let _ = store.delete(path).await;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::sha256_hex;
    use object_store::memory::InMemory;
    use std::io::Cursor;

    fn test_config() -> StorageBackendConfig {
        StorageBackendConfig {
            kind: StorageBackendKind::S3,
            bucket: Some("test".into()),
            prefix: Some("pfx".into()),
            tenant_id: Some("ten".into()),
            matter_id: Some("mat".into()),
            ..Default::default()
        }
    }

    #[test]
    fn memory_backend_put_get_round_trip() {
        let mem = Arc::new(InMemory::new());
        let store = S3BlobStore::from_object_store(mem, test_config(), MultipartLimits::default())
            .expect("store");

        let data = b"s3 memory round trip";
        let digest = store
            .put_stream(None, &mut Cursor::new(data.as_slice()))
            .expect("put");
        assert_eq!(digest, sha256_hex(data));
        assert!(store.exists(&digest).expect("exists"));
        let mut r = store.get_stream(&digest).expect("get");
        let mut out = Vec::new();
        r.read_to_end(&mut out).expect("read");
        assert_eq!(out, data);
    }

    #[test]
    fn digest_mismatch_deletes_object() {
        let mem = Arc::new(InMemory::new());
        let store = S3BlobStore::from_object_store(
            mem.clone(),
            test_config(),
            MultipartLimits::new(1024 * 1024, 1).expect("limits"),
        )
        .expect("store");

        let wrong = "ab".to_string() + &"cd".repeat(31);
        let err = store
            .put_stream(Some(&wrong), &mut Cursor::new(b"payload".as_slice()))
            .expect_err("mismatch");
        match err {
            StorageError::DigestMismatch { .. } => {}
            e => panic!("unexpected {e}"),
        }
        assert!(!store.exists(&wrong).expect("exists"));
    }

    #[test]
    fn put_at_digest_non_matching_stream() {
        let mem = Arc::new(InMemory::new());
        let store = S3BlobStore::from_object_store(mem, test_config(), MultipartLimits::default())
            .expect("store");
        let plain = sha256_hex(b"plaintext-id");
        let cipher = b"ciphertext-under-plain-key";
        store
            .put_at_digest(&plain, &mut Cursor::new(cipher.as_slice()))
            .expect("put_at_digest");
        assert!(store.exists(&plain).expect("exists"));
        let mut r = store.get_stream(&plain).expect("get");
        let mut out = Vec::new();
        r.read_to_end(&mut out).expect("read");
        assert_eq!(out, cipher);
        assert_ne!(sha256_hex(&out), plain);
    }
}
