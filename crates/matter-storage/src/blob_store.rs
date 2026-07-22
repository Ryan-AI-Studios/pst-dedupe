//! [`BlobStore`] trait — streaming content-addressed blob storage.

use std::io::Read;

use crate::error::Result;

/// Content-addressed blob store with **streaming** multi-GB put/get.
///
/// # Identity
///
/// Digests are lowercase hex SHA-256 of the **plaintext** logical content (same
/// as matter-core `Cas`).
///
/// # Put modes
///
/// | Method | When to use |
/// |---|---|
/// | [`BlobStore::put_stream`] | **Plaintext** path: stream is hashed; identity is SHA-256(stream). Optional `expected_digest` must match. |
/// | [`BlobStore::put_at_digest`] | **Encryption** path: store ciphertext (or other non-matching bytes) under a **precomputed plaintext digest** key. Does **not** require SHA-256(stream) == digest. |
///
/// When matter encryption is enabled, callers hash **plaintext** for the CAS
/// digest, encrypt, then call [`BlobStore::put_at_digest`] with that plaintext
/// digest. Plaintext-only puts use [`BlobStore::put_stream`].
///
/// # Cloud integrity
///
/// Content-addressed puts (**`put_stream`**) **must** hash while streaming
/// ([`crate::HashingReader`]). On digest mismatch the incomplete object is
/// deleted and the put fails — never trust S3 multipart ETags as content hashes.
///
/// # Object safety
///
/// Methods take `&mut dyn Read` / return `Box<dyn Read + Send>` so the trait is
/// object-safe for `dyn BlobStore`.
pub trait BlobStore: Send + Sync {
    /// Stream bytes into the store under a content-addressed key.
    ///
    /// - If `expected_digest` is `Some`, the computed SHA-256 of the stream must
    ///   match or the put fails (and any partial cloud object is deleted).
    /// - If `None`, the digest is computed from the stream and returned.
    /// - Idempotent when the object already exists for that digest.
    ///
    /// Use for **plaintext** CAS. For ciphertext under a plaintext identity, use
    /// [`BlobStore::put_at_digest`].
    fn put_stream(&self, expected_digest: Option<&str>, reader: &mut dyn Read) -> Result<String>;

    /// Store the stream at the CAS key for `digest` **without** requiring
    /// SHA-256(stream) == digest.
    ///
    /// Intended for the **encryption path**: ciphertext bytes live under the
    /// plaintext SHA-256 identity (same as matter-core `Cas`). Still streams
    /// (O(1) RAM peak buffers); on failure any partial object is deleted.
    /// Idempotent when the object already exists for that digest.
    fn put_at_digest(&self, digest: &str, reader: &mut dyn Read) -> Result<()>;

    /// Open a streaming reader for `digest`. Fail closed if missing.
    fn get_stream(&self, digest: &str) -> Result<Box<dyn Read + Send>>;

    /// Whether a blob with this digest exists.
    fn exists(&self, digest: &str) -> Result<bool>;

    /// Delete a blob (best-effort GC / mismatch cleanup).
    fn delete(&self, digest: &str) -> Result<()>;

    /// Connectivity / readiness probe (local = layout ok; cloud = head/list).
    fn health_check(&self) -> Result<()>;
}
