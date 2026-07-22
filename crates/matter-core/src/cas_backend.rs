//! Route Matter CAS convenience APIs through local [`Cas`] or remote [`BlobStore`].
//!
//! When `matters.storage_backend_json` is cloud (S3/Azure), Matter open activates
//! a remote [`BlobStore`] (**fail closed** if open fails). Local kind keeps
//! [`Cas`] only. Encryption: hash plaintext for digest; store ciphertext via
//! [`BlobStore::put_at_digest`].

use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use matter_storage::{BlobStore, StorageError};
use sha2::{Digest, Sha256};

use crate::cas::{sha256_hex as cas_sha256_hex, CasReader};
use crate::crypto::{
    decrypt_chunked, decrypt_chunked_from_reader, encrypt_chunked, encrypt_chunked_from_reader,
    is_encrypted_blob, read_plain_len, Dek, MAGIC_CAS,
};
use crate::error::{Error, Result};

/// Blobs at or under this plaintext size decrypt / buffer into memory on open_read.
/// Larger blobs stream to an anonymous temp file (same threshold as local [`Cas::open_read`]).
const OPEN_READ_MEMORY_THRESHOLD: u64 = 4 * 1024 * 1024;

/// Map storage errors into matter-core errors.
fn map_storage(err: StorageError) -> Error {
    match err {
        StorageError::NotFound(d) => Error::BlobNotFound(d),
        StorageError::InvalidDigest(d) => Error::InvalidDigest(d),
        StorageError::Io(e) => Error::Io(e),
        other => Error::Other(format!("remote blob store: {other}")),
    }
}

fn normalize_digest(digest_hex: &str) -> Result<String> {
    let lower = digest_hex.trim().to_ascii_lowercase();
    if lower.len() != 64 || !lower.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::InvalidDigest(digest_hex.to_string()));
    }
    Ok(lower)
}

/// Put raw plaintext bytes to remote store (encrypt when DEK present).
pub fn put_bytes_remote(
    store: &dyn BlobStore,
    dek: Option<&Arc<Dek>>,
    chunk_bytes: u32,
    data: &[u8],
) -> Result<String> {
    let digest = cas_sha256_hex(data);
    if store.exists(&digest).map_err(map_storage)? {
        return Ok(digest);
    }
    if let Some(dek) = dek {
        let enc = encrypt_chunked(
            dek.as_ref(),
            MAGIC_CAS,
            b"cas",
            digest.as_bytes(),
            data,
            chunk_bytes.max(1),
        )?;
        store
            .put_at_digest(&digest, &mut Cursor::new(enc.as_slice()))
            .map_err(map_storage)?;
    } else {
        store
            .put_stream(Some(&digest), &mut Cursor::new(data))
            .map_err(map_storage)?;
    }
    Ok(digest)
}

/// Stream put to remote (O(1) RAM for plaintext; encryption stages via encrypt-from-reader
/// into a temp file then `put_at_digest`).
pub fn put_reader_remote<R: Read>(
    store: &dyn BlobStore,
    dek: Option<&Arc<Dek>>,
    chunk_bytes: u32,
    reader: &mut R,
) -> Result<String> {
    // Always stage plaintext to a temp file while hashing (same spirit as Cas::put_reader).
    let mut tmp = tempfile::tempfile().map_err(Error::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut plain_len = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        tmp.write_all(&buf[..n]).map_err(Error::Io)?;
        hasher.update(&buf[..n]);
        plain_len += n as u64;
    }
    let digest = hex_encode(hasher.finalize().as_ref());
    if store.exists(&digest).map_err(map_storage)? {
        return Ok(digest);
    }
    use std::io::{Seek, SeekFrom};
    tmp.seek(SeekFrom::Start(0)).map_err(Error::Io)?;

    if let Some(dek) = dek {
        let mut enc_tmp = tempfile::tempfile().map_err(Error::Io)?;
        encrypt_chunked_from_reader(
            dek.as_ref(),
            MAGIC_CAS,
            b"cas",
            digest.as_bytes(),
            &mut tmp,
            &mut enc_tmp,
            chunk_bytes.max(1),
            plain_len,
        )?;
        enc_tmp.seek(SeekFrom::Start(0)).map_err(Error::Io)?;
        store
            .put_at_digest(&digest, &mut enc_tmp)
            .map_err(map_storage)?;
    } else {
        store
            .put_stream(Some(&digest), &mut tmp)
            .map_err(map_storage)?;
    }
    Ok(digest)
}

/// Get plaintext bytes from remote.
pub fn get_bytes_remote(
    store: &dyn BlobStore,
    dek: Option<&Arc<Dek>>,
    digest_hex: &str,
) -> Result<Vec<u8>> {
    let digest = normalize_digest(digest_hex)?;
    let mut reader = store.get_stream(&digest).map_err(map_storage)?;
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(Error::Io)?;
    if let Some(dek) = dek {
        if !is_encrypted_blob(&buf) {
            return Err(Error::Crypto(
                "expected encrypted CAS blob but magic missing".into(),
            ));
        }
        let plain = decrypt_chunked(dek.as_ref(), MAGIC_CAS, b"cas", digest.as_bytes(), &buf)?;
        if cas_sha256_hex(&plain) != digest {
            return Err(Error::Crypto(
                "CAS plaintext digest mismatch after decrypt".into(),
            ));
        }
        return Ok(plain);
    }
    Ok(buf)
}

/// Plaintext length of a remote blob (header when encrypted; stream count otherwise).
pub fn blob_len_remote(
    store: &dyn BlobStore,
    dek: Option<&Arc<Dek>>,
    digest_hex: &str,
) -> Result<u64> {
    let digest = normalize_digest(digest_hex)?;
    let mut reader = store.get_stream(&digest).map_err(map_storage)?;
    if dek.is_some() {
        let mut hdr = [0u8; 20];
        reader.read_exact(&mut hdr).map_err(Error::Io)?;
        if !is_encrypted_blob(&hdr) {
            return Err(Error::Crypto(
                "expected encrypted CAS blob but magic missing".into(),
            ));
        }
        return read_plain_len(&hdr);
    }
    let mut total = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        total += n as u64;
    }
    Ok(total)
}

/// Get plaintext when length ≤ `max_bytes`.
pub fn get_bytes_capped_remote(
    store: &dyn BlobStore,
    dek: Option<&Arc<Dek>>,
    digest_hex: &str,
    max_bytes: u64,
) -> Result<Vec<u8>> {
    let len = blob_len_remote(store, dek, digest_hex)?;
    if len > max_bytes {
        return Err(Error::Other(format!(
            "CAS blob size {len} exceeds cap {max_bytes}"
        )));
    }
    get_bytes_remote(store, dek, digest_hex)
}

/// Writer that retains at most `cap` plaintext bytes while accepting the rest
/// (so full AEAD frame integrity still runs without holding multi-GB plaintext).
struct CapWriter {
    buf: Vec<u8>,
    cap: usize,
}

impl Write for CapWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let room = self.cap.saturating_sub(self.buf.len());
        if room > 0 {
            let n = room.min(data.len());
            self.buf.extend_from_slice(&data[..n]);
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Read a plaintext prefix (sniff) from remote.
///
/// Encrypted path stream-decrypts frame-by-frame but only retains `max_bytes`
/// of plaintext in memory (rest discarded after decrypt for integrity).
pub fn read_prefix_remote(
    store: &dyn BlobStore,
    dek: Option<&Arc<Dek>>,
    digest_hex: &str,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let digest = normalize_digest(digest_hex)?;
    let mut reader = store.get_stream(&digest).map_err(map_storage)?;
    if let Some(dek) = dek {
        let mut cap = CapWriter {
            buf: Vec::with_capacity(max_bytes.min(64 * 1024)),
            cap: max_bytes,
        };
        decrypt_chunked_from_reader(
            dek.as_ref(),
            MAGIC_CAS,
            b"cas",
            digest.as_bytes(),
            &mut reader,
            &mut cap,
        )?;
        Ok(cap.buf)
    } else {
        let mut buf = vec![0u8; max_bytes];
        let n = reader.read(&mut buf).map_err(Error::Io)?;
        buf.truncate(n);
        Ok(buf)
    }
}

/// Whether remote blob exists.
pub fn exists_remote(store: &dyn BlobStore, digest_hex: &str) -> Result<bool> {
    let digest = normalize_digest(digest_hex)?;
    store.exists(&digest).map_err(map_storage)
}

/// Open a [`CasReader`] (Read+Seek) over remote blob **plaintext**.
///
/// Encrypted: stream-decrypt; small → memory, large → anonymous temp file.
/// Plain: buffer small into memory; spill large to anonymous temp (Seek support).
pub fn open_read_remote(
    store: &dyn BlobStore,
    dek: Option<&Arc<Dek>>,
    digest_hex: &str,
) -> Result<CasReader> {
    use std::io::{Seek, SeekFrom};

    let digest = normalize_digest(digest_hex)?;
    let mut reader = store.get_stream(&digest).map_err(map_storage)?;

    if let Some(dek) = dek {
        // Peek encrypted header for plaintext length (stream may not be Seek).
        let mut hdr = [0u8; 20];
        reader.read_exact(&mut hdr).map_err(Error::Io)?;
        if !is_encrypted_blob(&hdr) {
            return Err(Error::Crypto(
                "expected encrypted CAS blob but magic missing".into(),
            ));
        }
        let plain_len = read_plain_len(&hdr)?;
        let mut chained = Cursor::new(hdr).chain(reader);

        if plain_len <= OPEN_READ_MEMORY_THRESHOLD {
            let mut plain = Vec::new();
            decrypt_chunked_from_reader(
                dek.as_ref(),
                MAGIC_CAS,
                b"cas",
                digest.as_bytes(),
                &mut chained,
                &mut plain,
            )?;
            if cas_sha256_hex(&plain) != digest {
                return Err(Error::Crypto(
                    "CAS plaintext digest mismatch after decrypt".into(),
                ));
            }
            return Ok(CasReader::Memory(Cursor::new(plain)));
        }

        // Large encrypted: stream-decrypt to anonymous temp while hashing.
        let mut tmp = tempfile::tempfile().map_err(Error::Io)?;
        let mut hasher = Sha256::new();
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
                inner: &mut tmp,
                hasher: &mut hasher,
            };
            decrypt_chunked_from_reader(
                dek.as_ref(),
                MAGIC_CAS,
                b"cas",
                digest.as_bytes(),
                &mut chained,
                &mut tee,
            )?;
        }
        tmp.flush().map_err(Error::Io)?;
        let got = hex_encode(hasher.finalize().as_ref());
        if got != digest {
            return Err(Error::Crypto(
                "CAS plaintext digest mismatch after decrypt".into(),
            ));
        }
        tmp.seek(SeekFrom::Start(0)).map_err(Error::Io)?;
        return Ok(CasReader::Plain(tmp));
    }

    // Unencrypted remote: small → memory; large → temp for Seek.
    let cap = OPEN_READ_MEMORY_THRESHOLD as usize;
    let mut first = vec![0u8; cap + 1];
    let mut filled = 0usize;
    while filled < first.len() {
        let n = reader.read(&mut first[filled..]).map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    if filled <= cap {
        first.truncate(filled);
        return Ok(CasReader::Memory(Cursor::new(first)));
    }
    let mut tmp = tempfile::tempfile().map_err(Error::Io)?;
    tmp.write_all(&first[..filled]).map_err(Error::Io)?;
    std::io::copy(&mut reader, &mut tmp).map_err(Error::Io)?;
    tmp.flush().map_err(Error::Io)?;
    tmp.seek(SeekFrom::Start(0)).map_err(Error::Io)?;
    Ok(CasReader::Plain(tmp))
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
