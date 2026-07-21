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
//! - Input: raw physical **plaintext** bytes only (never normalized/logical content)
//! - Digest encoding: lowercase hex
//! - Collision policy: if the path exists and content differs → hard error
//! - Logical hashes are **not** stored in CAS (column on `items` only)
//!
//! # Encryption (opt-in)
//!
//! When a [`CasCrypto`] DEK is configured, objects are stored as chunked AES-GCM
//! blobs (`MAGIC_CAS`). Digests remain plaintext SHA-256. [`Cas::blob_len`] and
//! caps use the **plaintext** length from the AEAD header.
//!
//! # Streaming put
//!
//! [`Cas::put_reader`] hashes while writing to a temp file under `blobs/`, then
//! atomically renames (or encrypts) into the final shard path. Use this for multi-GB
//! attachments so callers never need a full `Vec<u8>` of the payload.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};

use crate::crypto::{
    decrypt_chunked, decrypt_chunked_from_reader, encrypt_chunked, encrypt_chunked_from_reader,
    is_encrypted_blob, is_encrypted_matter, read_plain_len, Dek, MAGIC_CAS,
};
use crate::error::{Error, Result};

/// Directory name under the matter root for CAS blobs.
pub const BLOBS_DIR: &str = "blobs";

/// Algorithm subdirectory under `blobs/`.
pub const SHA256_DIR: &str = "sha256";

/// Default read buffer for [`Cas::put_reader`] (64 KiB).
pub const PUT_READER_BUF_SIZE: usize = 64 * 1024;

/// Blobs at or under this plaintext size decrypt into memory on [`Cas::open_read`].
/// Larger encrypted blobs stream-decrypt to a temp file under `blobs/.tmp-read/`.
const OPEN_READ_MEMORY_THRESHOLD: u64 = 4 * 1024 * 1024;

static STREAM_TMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Crypto parameters for an encrypted CAS.
#[derive(Clone)]
pub struct CasCrypto {
    pub dek: Arc<Dek>,
    pub chunk_bytes: u32,
}

impl std::fmt::Debug for CasCrypto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CasCrypto")
            .field("chunk_bytes", &self.chunk_bytes)
            .field("dek", &"<redacted>")
            .finish()
    }
}

/// Read handle returned by [`Cas::open_read`].
///
/// Plain matters return a direct file. Encrypted matters return either an
/// in-memory cursor (small blobs) or a decrypted temp file (large blobs).
pub enum CasReader {
    Plain(File),
    Memory(std::io::Cursor<Vec<u8>>),
}

impl Read for CasReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            CasReader::Plain(f) => f.read(buf),
            CasReader::Memory(c) => c.read(buf),
        }
    }
}

impl std::io::Seek for CasReader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match self {
            CasReader::Plain(f) => f.seek(pos),
            CasReader::Memory(c) => c.seek(pos),
        }
    }
}

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
    /// Present when matter encryption is active.
    crypto: Option<CasCrypto>,
}

impl Cas {
    /// Open (or prepare) an **unencrypted** CAS under the given matter root.
    ///
    /// Does not create directories; call [`Cas::ensure_layout`] after matter
    /// directory creation.
    pub fn new(matter_root: impl AsRef<Utf8Path>) -> Self {
        let blobs_root = matter_root.as_ref().join(BLOBS_DIR);
        Self {
            blobs_root,
            crypto: None,
        }
    }

    /// Open a CAS that encrypts/decrypts objects under the DEK.
    pub fn with_crypto(matter_root: impl AsRef<Utf8Path>, dek: Arc<Dek>, chunk_bytes: u32) -> Self {
        let blobs_root = matter_root.as_ref().join(BLOBS_DIR);
        Self {
            blobs_root,
            crypto: Some(CasCrypto {
                dek,
                chunk_bytes: chunk_bytes.max(1),
            }),
        }
    }

    /// Whether this CAS encrypts objects at rest.
    pub fn is_encrypted(&self) -> bool {
        self.crypto.is_some()
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

    /// Matter-local staging for plaintext CAS intermediates (`workspace/temp/.cas-stage/`).
    fn cas_stage_dir(&self) -> Option<Utf8PathBuf> {
        let matter_root = self.blobs_root.parent()?;
        Some(
            matter_root
                .join(crate::matter::WORKSPACE_DIR)
                .join(crate::matter::WORKSPACE_TEMP_DIR)
                .join(".cas-stage"),
        )
    }

    /// Remove leftover plaintext CAS staging under `workspace/temp/.cas-stage/` and legacy
    /// `blobs/.stream-*` / `blobs/.tmp-read/`.
    pub fn cleanup_crypto_temps(&self) -> Result<()> {
        if let Some(stage) = self.cas_stage_dir() {
            if stage.as_std_path().exists() {
                let _ = fs::remove_dir_all(stage.as_std_path());
            }
        }
        if self.blobs_root.as_std_path().is_dir() {
            for entry in fs::read_dir(self.blobs_root.as_std_path())? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".stream-") && name.ends_with(".tmp") {
                    let path = entry.path();
                    if path.is_file() {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
        let tmp_read = self.blobs_root.join(".tmp-read");
        if tmp_read.as_std_path().exists() {
            let _ = fs::remove_dir_all(tmp_read.as_std_path());
        }
        Ok(())
    }

    /// Object path: `blobs/sha256/<aa>/<fullhex>`.
    pub fn object_path(&self, digest_hex: &str) -> Result<Utf8PathBuf> {
        let digest = normalize_digest(digest_hex)?;
        let shard = &digest[..2];
        Ok(self.blobs_root.join(SHA256_DIR).join(shard).join(&digest))
    }

    /// Return true if a blob with this digest already exists (ciphertext path when encrypted).
    pub fn exists(&self, digest_hex: &str) -> Result<bool> {
        let path = self.object_path(digest_hex)?;
        Ok(path.as_std_path().exists())
    }

    /// Store raw bytes. Returns the SHA-256 hex digest of the **plaintext**.
    ///
    /// If the object already exists with identical content, this is a no-op
    /// success. If it exists with different content, returns
    /// [`Error::CasCollision`].
    pub fn put_bytes(&self, data: &[u8]) -> Result<String> {
        let digest = sha256_hex(data);
        let path = self.object_path(&digest)?;

        if path.as_std_path().exists() {
            return self.verify_existing(&path, &digest, data);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }

        let tmp_name = format!(".{digest}.tmp");
        let tmp_path = path
            .parent()
            .map(|p| p.join(&tmp_name))
            .unwrap_or_else(|| path.with_file_name(&tmp_name));

        {
            let mut file = File::create(tmp_path.as_std_path())?;
            if let Some(crypto) = &self.crypto {
                let enc = encrypt_chunked(
                    crypto.dek.as_ref(),
                    MAGIC_CAS,
                    b"cas",
                    digest.as_bytes(),
                    data,
                    crypto.chunk_bytes,
                )?;
                file.write_all(&enc)?;
            } else {
                file.write_all(data)?;
            }
            file.sync_all()?;
        }

        // On Windows, rename fails if destination exists; re-check race.
        if path.as_std_path().exists() {
            let _ = fs::remove_file(tmp_path.as_std_path());
            return self.verify_existing(&path, &digest, data);
        }

        fs::rename(tmp_path.as_std_path(), path.as_std_path())?;
        Ok(digest)
    }

    /// Stream raw bytes from `reader` into CAS. Returns the SHA-256 hex digest of plaintext.
    ///
    /// Hashes while writing to a temp file under `blobs/` named `.stream-*.tmp`, then
    /// renames (or encrypts) into the final two-hex-shard path. Uses a bounded buffer
    /// ([`PUT_READER_BUF_SIZE`]). The plain staging temp is **always** removed on all
    /// error paths (and after successful encrypt).
    ///
    /// Collision policy matches [`Cas::put_bytes`]: if the digest path already
    /// exists, this is success (same digest path ⇒ same content for SHA-256).
    /// On a rename race where the destination appears mid-flight, the temp is
    /// discarded and the existing object is kept.
    pub fn put_reader<R: Read>(&self, reader: &mut R) -> Result<String> {
        fs::create_dir_all(self.blobs_root.as_std_path())?;

        let seq = STREAM_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_name = format!(".stream-{seq}-{}.tmp", std::process::id());
        // Encrypted: stage plaintext under workspace/temp/.cas-stage (encryption boundary).
        // Unencrypted: legacy blobs/ staging is fine.
        let tmp_path = if self.crypto.is_some() {
            let stage = self.cas_stage_dir().ok_or_else(|| {
                Error::Crypto("cannot resolve matter workspace for CAS staging".into())
            })?;
            fs::create_dir_all(stage.as_std_path())?;
            stage.join(&tmp_name)
        } else {
            self.blobs_root.join(&tmp_name)
        };

        // Ensure plaintext staging temp is removed on every exit path.
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

        let (digest, plain_len) = {
            let mut file = File::create(tmp_path.as_std_path())?;
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; PUT_READER_BUF_SIZE];
            let mut total = 0u64;
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])?;
                hasher.update(&buf[..n]);
                total += n as u64;
            }
            file.sync_all()?;
            (hex_encode(hasher.finalize().as_ref()), total)
        };

        let path = self.object_path(&digest)?;

        if path.as_std_path().exists() {
            // Must verify encrypted framing when crypto is on (not bare path existence).
            let mut plain_file = File::open(tmp_path.as_std_path())?;
            let mut data = Vec::new();
            plain_file.read_to_end(&mut data)?;
            return self.verify_existing(&path, &digest, &data);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }

        if path.as_std_path().exists() {
            let mut plain_file = File::open(tmp_path.as_std_path())?;
            let mut data = Vec::new();
            plain_file.read_to_end(&mut data)?;
            return self.verify_existing(&path, &digest, &data);
        }

        if let Some(crypto) = &self.crypto {
            // Encrypt plain temp → final path (via enc temp), then delete plain temp.
            let enc_tmp_name = format!(".{digest}.enc.tmp");
            let enc_tmp = path
                .parent()
                .map(|p| p.join(&enc_tmp_name))
                .unwrap_or_else(|| path.with_file_name(&enc_tmp_name));
            let enc_result = (|| -> Result<String> {
                {
                    let mut plain_in = File::open(tmp_path.as_std_path())?;
                    let mut out = File::create(enc_tmp.as_std_path())?;
                    encrypt_chunked_from_reader(
                        crypto.dek.as_ref(),
                        MAGIC_CAS,
                        b"cas",
                        digest.as_bytes(),
                        &mut plain_in,
                        &mut out,
                        crypto.chunk_bytes,
                        plain_len,
                    )?;
                    out.sync_all()?;
                }
                // Plain staging no longer needed once enc temp is written.
                let _ = fs::remove_file(tmp_path.as_std_path());
                wipe.disarm();
                if path.as_std_path().exists() {
                    let _ = fs::remove_file(enc_tmp.as_std_path());
                    return Ok(digest.clone());
                }
                match fs::rename(enc_tmp.as_std_path(), path.as_std_path()) {
                    Ok(()) => Ok(digest.clone()),
                    Err(e) => {
                        if path.as_std_path().exists() {
                            let _ = fs::remove_file(enc_tmp.as_std_path());
                            return Ok(digest.clone());
                        }
                        let _ = fs::remove_file(enc_tmp.as_std_path());
                        Err(Error::Io(e))
                    }
                }
            })();
            if enc_result.is_err() {
                let _ = fs::remove_file(enc_tmp.as_std_path());
            }
            enc_result
        } else {
            match fs::rename(tmp_path.as_std_path(), path.as_std_path()) {
                Ok(()) => {
                    wipe.disarm();
                    Ok(digest)
                }
                Err(e) => {
                    // Windows: destination exists → treat as success if final path present.
                    if path.as_std_path().exists() {
                        return Ok(digest);
                    }
                    Err(Error::Io(e))
                }
            }
        }
    }

    /// Open a read handle for a digest (streaming get of **plaintext**).
    ///
    /// Encrypted large blobs stream-decrypt to a temp under `blobs/.tmp-read/`
    /// then return a [`CasReader::Plain`] file. Small blobs use memory.
    pub fn open_read(&self, digest_hex: &str) -> Result<CasReader> {
        let digest = normalize_digest(digest_hex)?;
        let path = self.object_path(&digest)?;
        if !path.as_std_path().exists() {
            return Err(Error::BlobNotFound(digest));
        }

        if let Some(crypto) = &self.crypto {
            let mut file = File::open(path.as_std_path())?;
            let mut hdr = [0u8; 20];
            file.read_exact(&mut hdr)?;
            if !is_encrypted_blob(&hdr) {
                return Err(Error::Crypto(
                    "expected encrypted CAS blob but magic missing".into(),
                ));
            }
            let plain_len = read_plain_len(&hdr)?;

            // Rewind for full frame stream decrypt.
            use std::io::Seek;
            file.seek(std::io::SeekFrom::Start(0))?;

            if plain_len <= OPEN_READ_MEMORY_THRESHOLD {
                let mut plain = Vec::new();
                decrypt_chunked_from_reader(
                    crypto.dek.as_ref(),
                    MAGIC_CAS,
                    b"cas",
                    digest.as_bytes(),
                    &mut file,
                    &mut plain,
                )?;
                // Integrity: digest path must match plaintext SHA-256.
                if sha256_hex(&plain) != digest {
                    return Err(Error::Crypto(
                        "CAS plaintext digest mismatch after decrypt".into(),
                    ));
                }
                return Ok(CasReader::Memory(std::io::Cursor::new(plain)));
            }

            // Large: frame-stream decrypt to workspace temp (no full ciphertext buffer).
            let stage = self.cas_stage_dir().ok_or_else(|| {
                Error::Crypto("cannot resolve matter workspace for CAS staging".into())
            })?;
            fs::create_dir_all(stage.as_std_path())?;
            let seq = STREAM_TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let tmp_path = stage.join(format!(".read-{seq}-{digest}.tmp"));
            {
                let mut out = File::create(tmp_path.as_std_path())?;
                let mut hasher = Sha256::new();
                // Tee writer: hash while decrypting to temp.
                struct HashWrite<'a, W: Write> {
                    inner: &'a mut W,
                    hasher: &'a mut Sha256,
                }
                impl<W: Write> Write for HashWrite<'_, W> {
                    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                        self.hasher.update(buf);
                        self.inner.write(buf)
                    }
                    fn flush(&mut self) -> std::io::Result<()> {
                        self.inner.flush()
                    }
                }
                {
                    let mut tee = HashWrite {
                        inner: &mut out,
                        hasher: &mut hasher,
                    };
                    decrypt_chunked_from_reader(
                        crypto.dek.as_ref(),
                        MAGIC_CAS,
                        b"cas",
                        digest.as_bytes(),
                        &mut file,
                        &mut tee,
                    )?;
                }
                out.sync_all()?;
                let got = hex_encode(hasher.finalize().as_ref());
                if got != digest {
                    let _ = fs::remove_file(tmp_path.as_std_path());
                    return Err(Error::Crypto(
                        "CAS plaintext digest mismatch after decrypt".into(),
                    ));
                }
            }
            let f = File::open(tmp_path.as_std_path())?;
            let _ = fs::remove_file(tmp_path.as_std_path());
            Ok(CasReader::Plain(f))
        } else {
            // Encrypted matter without DEK: never serve ciphertext as plaintext.
            if let Some(root) = self.blobs_root.parent() {
                if is_encrypted_matter(root) {
                    return Err(Error::Crypto(
                        "encrypted CAS blob requires unlocked matter".into(),
                    ));
                }
            }
            // Unencrypted matter: raw bytes (magic-looking content is legal).
            Ok(CasReader::Plain(File::open(path.as_std_path())?))
        }
    }

    /// Return the **plaintext** byte length of a blob without full decrypt.
    pub fn blob_len(&self, digest_hex: &str) -> Result<u64> {
        let path = self.object_path(digest_hex)?;
        if !path.as_std_path().exists() {
            return Err(Error::BlobNotFound(normalize_digest(digest_hex)?));
        }
        if self.crypto.is_some() {
            let mut hdr = [0u8; 20];
            let mut file = File::open(path.as_std_path())?;
            file.read_exact(&mut hdr)?;
            if is_encrypted_blob(&hdr) {
                return read_plain_len(&hdr);
            }
            return Err(Error::Crypto(
                "expected encrypted CAS blob but magic missing".into(),
            ));
        }
        let meta = fs::metadata(path.as_std_path())?;
        Ok(meta.len())
    }

    /// Read raw **plaintext** bytes for a digest.
    pub fn get_bytes(&self, digest_hex: &str) -> Result<Vec<u8>> {
        let digest = normalize_digest(digest_hex)?;
        let path = self.object_path(&digest)?;
        if !path.as_std_path().exists() {
            return Err(Error::BlobNotFound(digest));
        }
        let mut file = File::open(path.as_std_path())?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        if let Some(crypto) = &self.crypto {
            if !is_encrypted_blob(&buf) {
                return Err(Error::Crypto(
                    "expected encrypted CAS blob but magic missing".into(),
                ));
            }
            let plain = decrypt_chunked(
                crypto.dek.as_ref(),
                MAGIC_CAS,
                b"cas",
                digest.as_bytes(),
                &buf,
            )?;
            if sha256_hex(&plain) != digest {
                return Err(Error::Crypto(
                    "CAS plaintext digest mismatch after decrypt".into(),
                ));
            }
            return Ok(plain);
        }
        if let Some(root) = self.blobs_root.parent() {
            if is_encrypted_matter(root) {
                return Err(Error::Crypto(
                    "encrypted CAS blob requires unlocked matter".into(),
                ));
            }
        }
        // Unencrypted path: raw bytes (magic-looking content is legal for plain matters).
        Ok(buf)
    }

    /// Read raw plaintext bytes only when the plaintext length is `<= max_bytes`.
    ///
    /// Stats the CAS header/file first so oversized blobs are rejected without a full
    /// allocation (callers that need a hard memory bound should prefer this over
    /// [`Cas::get_bytes`]).
    pub fn get_bytes_capped(&self, digest_hex: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let len = self.blob_len(digest_hex)?;
        if len > max_bytes {
            return Err(Error::Other(format!(
                "CAS blob size {len} exceeds cap {max_bytes}"
            )));
        }
        self.get_bytes(digest_hex)
    }

    fn verify_existing(&self, path: &Utf8Path, digest: &str, data: &[u8]) -> Result<String> {
        let existing = fs::read(path.as_std_path())?;
        if let Some(crypto) = &self.crypto {
            if !is_encrypted_blob(&existing) {
                return Err(Error::Crypto(format!(
                    "encrypted CAS requires AEAD object at digest {digest}; found plaintext leftover"
                )));
            }
            match decrypt_chunked(
                crypto.dek.as_ref(),
                MAGIC_CAS,
                b"cas",
                digest.as_bytes(),
                &existing,
            ) {
                Ok(plain) if plain.as_slice() == data => Ok(digest.to_string()),
                Ok(_) => Err(Error::CasCollision {
                    digest: digest.to_string(),
                }),
                Err(_) => Err(Error::CasCollision {
                    digest: digest.to_string(),
                }),
            }
        } else if existing.as_slice() != data {
            Err(Error::CasCollision {
                digest: digest.to_string(),
            })
        } else {
            Ok(digest.to_string())
        }
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
    use std::io::Cursor;
    use tempfile::tempdir;

    use crate::crypto::{generate_dek, DEFAULT_CHUNK_BYTES};

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

    #[test]
    fn put_reader_multi_chunk_matches_put_bytes() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let cas = Cas::new(&root);
        cas.ensure_layout().expect("layout");

        // Content larger than PUT_READER_BUF_SIZE to force multi-chunk reads.
        let mut data = Vec::with_capacity(PUT_READER_BUF_SIZE * 3 + 17);
        for i in 0..(PUT_READER_BUF_SIZE * 3 + 17) {
            data.push((i % 251) as u8);
        }

        let expected = cas.put_bytes(&data).expect("put_bytes");

        // Second object via multi-chunk reader (same bytes → same digest path).
        let mut cursor = Cursor::new(data.clone());
        // Use a separate CAS root for a clean stream write path comparison of digest.
        let dir2 = tempdir().expect("tempdir2");
        let root2 = Utf8PathBuf::from_path_buf(dir2.path().to_path_buf()).expect("utf8");
        let cas2 = Cas::new(&root2);
        cas2.ensure_layout().expect("layout2");
        let streamed = cas2.put_reader(&mut cursor).expect("put_reader");
        assert_eq!(streamed, expected);

        let got = cas2.get_bytes(&streamed).expect("get");
        assert_eq!(got.as_slice(), data.as_slice());
    }

    #[test]
    fn put_reader_idempotent_when_exists() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let cas = Cas::new(&root);
        cas.ensure_layout().expect("layout");

        let data = b"stream-idempotent-payload";
        let d1 = cas
            .put_reader(&mut Cursor::new(data.as_slice()))
            .expect("p1");
        let d2 = cas
            .put_reader(&mut Cursor::new(data.as_slice()))
            .expect("p2");
        assert_eq!(d1, d2);
    }

    #[test]
    fn blob_len_and_get_bytes_capped() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let cas = Cas::new(&root);
        cas.ensure_layout().expect("layout");

        let data = b"capped-payload-bytes";
        let digest = cas.put_bytes(data).expect("put");
        assert_eq!(cas.blob_len(&digest).expect("len"), data.len() as u64);
        let got = cas
            .get_bytes_capped(&digest, data.len() as u64)
            .expect("capped ok");
        assert_eq!(got.as_slice(), data.as_slice());
        let err = cas
            .get_bytes_capped(&digest, (data.len() as u64).saturating_sub(1))
            .expect_err("over cap");
        assert!(err.to_string().contains("exceeds cap"));
    }

    #[test]
    fn encrypted_put_get_roundtrip_multi_chunk() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let dek = Arc::new(generate_dek());
        let cas = Cas::with_crypto(&root, dek, 64);
        cas.ensure_layout().expect("layout");

        let plain: Vec<u8> = (0..200u8).collect();
        let digest = cas.put_bytes(&plain).expect("put");
        assert_eq!(cas.blob_len(&digest).expect("len"), plain.len() as u64);
        let got = cas.get_bytes(&digest).expect("get");
        assert_eq!(got, plain);

        // On-disk must not be raw plaintext.
        let path = cas.object_path(&digest).expect("path");
        let raw = fs::read(path.as_std_path()).expect("raw");
        assert!(is_encrypted_blob(&raw));
        assert_ne!(raw, plain);

        let mut reader = cas.open_read(&digest).expect("open");
        let mut from_reader = Vec::new();
        reader.read_to_end(&mut from_reader).expect("read");
        assert_eq!(from_reader, plain);
    }

    #[test]
    fn plain_cas_refuses_encrypted_blob_when_header_present() {
        // Fail closed when matter.crypto.json exists (encrypted matter) without DEK.
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let dek = Arc::new(generate_dek());
        let enc_cas = Cas::with_crypto(&root, Arc::clone(&dek), DEFAULT_CHUNK_BYTES);
        enc_cas.ensure_layout().expect("layout");
        let digest = enc_cas.put_bytes(b"secret-plain").expect("put enc");
        fs::write(root.join("matter.crypto.json").as_std_path(), b"{}").expect("header");

        let plain_cas = Cas::new(&root);
        let err = match plain_cas.get_bytes(&digest) {
            Ok(_) => panic!("plain Cas must refuse encrypted blob"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("unlocked matter")
                || err.to_string().to_lowercase().contains("encrypted"),
            "unexpected: {err}"
        );
        let err2 = match plain_cas.open_read(&digest) {
            Ok(_) => panic!("plain Cas open_read must refuse encrypted blob"),
            Err(e) => e,
        };
        assert!(
            err2.to_string().contains("unlocked matter")
                || err2.to_string().to_lowercase().contains("encrypted"),
            "unexpected: {err2}"
        );
    }

    #[test]
    fn plain_cas_allows_magic_looking_plaintext_without_header() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let cas = Cas::new(&root);
        cas.ensure_layout().expect("layout");
        let mut data = MAGIC_CAS.to_vec();
        data.extend_from_slice(b"not-really-encrypted-payload");
        let digest = cas.put_bytes(&data).expect("put");
        assert_eq!(cas.get_bytes(&digest).expect("get"), data);
    }

    #[test]
    fn encrypted_put_reader_roundtrip() {
        let dir = tempdir().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        let dek = Arc::new(generate_dek());
        let cas = Cas::with_crypto(&root, dek, DEFAULT_CHUNK_BYTES);
        cas.ensure_layout().expect("layout");

        let data = b"encrypted-stream-payload-xyz";
        let digest = cas
            .put_reader(&mut Cursor::new(data.as_slice()))
            .expect("put_reader");
        let got = cas.get_bytes(&digest).expect("get");
        assert_eq!(got.as_slice(), data.as_slice());
    }
}
