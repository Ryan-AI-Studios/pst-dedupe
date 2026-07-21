//! Chunked AES-256-GCM for multi-GB blobs (no monolithic whole-object GCM).
//!
//! Each chunk uses a **fresh random 96-bit nonce** (stored with ciphertext).
//! Re-encrypts of the same logical object never reuse (key, nonce).

use std::io::{Read, Write};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::rngs::OsRng;
use rand::RngCore;

use super::kdf::Dek;
use crate::error::{Error, Result};

/// Default CAS / DB chunk size: 1 MiB.
pub const DEFAULT_CHUNK_BYTES: u32 = 1_048_576;

/// Magic for encrypted CAS objects.
pub const MAGIC_CAS: &[u8; 8] = b"DDCAS01\0";
/// Magic for encrypted SQLite container files.
pub const MAGIC_DB: &[u8; 8] = b"DDMATDB1";

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const HEADER_LEN: usize = 8 + 4 + 8; // magic + chunk_size + plain_len

/// True if buffer starts with a known encrypted-blob magic.
pub fn is_encrypted_blob(data: &[u8]) -> bool {
    data.len() >= 8 && (data.starts_with(MAGIC_CAS) || data.starts_with(MAGIC_DB))
}

/// True if the first 8 bytes match encrypted CAS magic.
pub fn starts_with_cas_magic(data: &[u8]) -> bool {
    data.len() >= 8 && data.starts_with(MAGIC_CAS)
}

/// Read plaintext length from an encrypted blob header without decrypting.
pub fn read_plain_len(data: &[u8]) -> Result<u64> {
    if data.len() < HEADER_LEN {
        return Err(Error::Crypto("encrypted blob header too short".into()));
    }
    let plain_len = u64::from_le_bytes(data[12..20].try_into().unwrap());
    Ok(plain_len)
}

fn cipher_for(dek: &Dek) -> Result<Aes256Gcm> {
    Aes256Gcm::new_from_slice(dek.as_bytes()).map_err(|e| Error::Crypto(format!("aes key: {e}")))
}

/// Fresh random 96-bit GCM nonce (must never be reused under the same key).
pub fn random_nonce() -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut n);
    n
}

/// Bind domain, extra, framing, and chunk index so header fields cannot be
/// silently truncated/forged without failing AEAD open.
fn aad_for(
    domain: &[u8],
    extra: &[u8],
    chunk_idx: u64,
    plain_len: u64,
    chunk_size: u32,
) -> Vec<u8> {
    let mut aad = Vec::with_capacity(domain.len() + extra.len() + 24);
    aad.extend_from_slice(domain);
    aad.push(0);
    aad.extend_from_slice(extra);
    aad.extend_from_slice(&chunk_idx.to_be_bytes());
    aad.extend_from_slice(&plain_len.to_be_bytes());
    aad.extend_from_slice(&chunk_size.to_be_bytes());
    aad
}

fn write_header(out: &mut Vec<u8>, magic: &[u8; 8], chunk_size: u32, plain_len: u64) {
    out.extend_from_slice(magic);
    out.extend_from_slice(&chunk_size.to_le_bytes());
    out.extend_from_slice(&plain_len.to_le_bytes());
}

/// Encrypt full plaintext into chunked AEAD blob (in memory for moderate sizes).
pub fn encrypt_chunked(
    dek: &Dek,
    magic: &[u8; 8],
    domain: &[u8],
    extra: &[u8],
    plain: &[u8],
    chunk_size: u32,
) -> Result<Vec<u8>> {
    let chunk_size = chunk_size.max(1) as usize;
    let cipher = cipher_for(dek)?;
    let mut out = Vec::with_capacity(
        HEADER_LEN + plain.len() + (plain.len() / chunk_size + 1) * (NONCE_LEN + TAG_LEN),
    );
    write_header(&mut out, magic, chunk_size as u32, plain.len() as u64);

    let mut idx = 0u64;
    let mut offset = 0usize;
    while offset < plain.len() {
        let end = (offset + chunk_size).min(plain.len());
        let chunk = &plain[offset..end];
        let nonce_bytes = random_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let aad = aad_for(domain, extra, idx, plain.len() as u64, chunk_size as u32);
        let ct = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: chunk,
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Crypto("chunk encrypt failed".into()))?;
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        offset = end;
        idx += 1;
    }
    Ok(out)
}

/// Stream-encrypt from reader without holding full plaintext (CAS put_reader path).
#[allow(clippy::too_many_arguments)]
pub fn encrypt_chunked_from_reader<R: Read, W: Write>(
    dek: &Dek,
    magic: &[u8; 8],
    domain: &[u8],
    extra: &[u8],
    reader: &mut R,
    writer: &mut W,
    chunk_size: u32,
    plain_len: u64,
) -> Result<()> {
    let chunk_size = chunk_size.max(1) as usize;
    let cipher = cipher_for(dek)?;
    let mut header = Vec::with_capacity(HEADER_LEN);
    write_header(&mut header, magic, chunk_size as u32, plain_len);
    writer.write_all(&header)?;

    let mut buf = vec![0u8; chunk_size];
    let mut idx = 0u64;
    let mut remaining = plain_len;
    while remaining > 0 {
        let want = (remaining as usize).min(chunk_size);
        reader.read_exact(&mut buf[..want])?;
        let nonce_bytes = random_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let aad = aad_for(domain, extra, idx, plain_len, chunk_size as u32);
        let ct = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: &buf[..want],
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Crypto("chunk encrypt failed".into()))?;
        writer.write_all(&nonce_bytes)?;
        writer.write_all(&ct)?;
        remaining -= want as u64;
        idx += 1;
    }
    Ok(())
}

/// Decrypt full chunked AEAD blob to plaintext bytes.
pub fn decrypt_chunked(
    dek: &Dek,
    expected_magic: &[u8; 8],
    domain: &[u8],
    extra: &[u8],
    data: &[u8],
) -> Result<Vec<u8>> {
    if data.len() < HEADER_LEN {
        return Err(Error::Crypto("encrypted blob too short".into()));
    }
    if &data[..8] != expected_magic.as_slice() {
        return Err(Error::Crypto("encrypted blob magic mismatch".into()));
    }
    let chunk_size_u32 = u32::from_le_bytes(data[8..12].try_into().unwrap());
    let plain_len_u64 = u64::from_le_bytes(data[12..20].try_into().unwrap());
    let plain_len = plain_len_u64 as usize;
    if chunk_size_u32 == 0 || chunk_size_u32 > 4 * 1024 * 1024 {
        return Err(Error::Crypto(format!(
            "encrypted blob chunk_size {chunk_size_u32} out of 1..=4MiB"
        )));
    }
    let chunk_size = chunk_size_u32 as usize;
    // Do not allocate full plain_len up-front from untrusted header; grow per chunk.
    let cipher = cipher_for(dek)?;
    let mut plain = Vec::new();
    let mut offset = HEADER_LEN;
    let mut idx = 0u64;
    while plain.len() < plain_len {
        let need = (plain_len - plain.len()).min(chunk_size);
        let frame = NONCE_LEN + need + TAG_LEN;
        if offset + frame > data.len() {
            return Err(Error::Crypto("encrypted blob truncated".into()));
        }
        let nonce_bytes = &data[offset..offset + NONCE_LEN];
        let ct = &data[offset + NONCE_LEN..offset + frame];
        let nonce = Nonce::from_slice(nonce_bytes);
        let aad = aad_for(domain, extra, idx, plain_len_u64, chunk_size as u32);
        let pt = cipher
            .decrypt(nonce, Payload { msg: ct, aad: &aad })
            .map_err(|_| Error::Crypto("chunk decrypt failed".into()))?;
        if pt.len() != need {
            return Err(Error::Crypto("chunk length mismatch".into()));
        }
        plain.extend_from_slice(&pt);
        offset += frame;
        idx += 1;
    }
    if offset != data.len() {
        return Err(Error::Crypto("encrypted blob has trailing garbage".into()));
    }
    Ok(plain)
}

/// Decrypt streaming path from in-memory ciphertext (delegates to [`decrypt_chunked`]).
pub fn decrypt_chunked_streaming<W: Write>(
    dek: &Dek,
    expected_magic: &[u8; 8],
    domain: &[u8],
    extra: &[u8],
    data: &[u8],
    writer: &mut W,
) -> Result<u64> {
    let plain = decrypt_chunked(dek, expected_magic, domain, extra, data)?;
    writer.write_all(&plain)?;
    Ok(plain.len() as u64)
}

/// Frame-by-frame decrypt from a sequential reader (no full-ciphertext buffer).
pub fn decrypt_chunked_from_reader<R: Read, W: Write>(
    dek: &Dek,
    expected_magic: &[u8; 8],
    domain: &[u8],
    extra: &[u8],
    reader: &mut R,
    writer: &mut W,
) -> Result<u64> {
    let mut header = [0u8; HEADER_LEN];
    reader.read_exact(&mut header)?;
    if &header[..8] != expected_magic.as_slice() {
        return Err(Error::Crypto("encrypted blob magic mismatch".into()));
    }
    let chunk_size_u32 = u32::from_le_bytes(header[8..12].try_into().unwrap());
    let plain_len = u64::from_le_bytes(header[12..20].try_into().unwrap());
    // Bound untrusted chunk_size before allocating frame buffer (1..=4 MiB).
    if chunk_size_u32 == 0 || chunk_size_u32 > 4 * 1024 * 1024 {
        return Err(Error::Crypto(format!(
            "encrypted blob chunk_size {chunk_size_u32} out of 1..=4MiB"
        )));
    }
    let chunk_size = chunk_size_u32 as usize;
    let cipher = cipher_for(dek)?;
    let mut idx = 0u64;
    let mut written = 0u64;
    let mut frame_buf = vec![0u8; NONCE_LEN + chunk_size + TAG_LEN];
    while written < plain_len {
        let need = ((plain_len - written) as usize).min(chunk_size);
        let frame = NONCE_LEN + need + TAG_LEN;
        reader.read_exact(&mut frame_buf[..frame])?;
        let nonce = Nonce::from_slice(&frame_buf[..NONCE_LEN]);
        let ct = &frame_buf[NONCE_LEN..frame];
        let aad = aad_for(domain, extra, idx, plain_len, chunk_size as u32);
        let pt = cipher
            .decrypt(nonce, Payload { msg: ct, aad: &aad })
            .map_err(|_| Error::Crypto("chunk decrypt failed".into()))?;
        if pt.len() != need {
            return Err(Error::Crypto("chunk length mismatch".into()));
        }
        writer.write_all(&pt)?;
        written += pt.len() as u64;
        idx += 1;
    }
    let mut extra_byte = [0u8; 1];
    match reader.read(&mut extra_byte)? {
        0 => Ok(written),
        _ => Err(Error::Crypto("encrypted blob has trailing garbage".into())),
    }
}

/// Encrypt arbitrary domain bytes (FTS file names use domain + path).
pub fn encrypt_bytes_domain(
    dek: &Dek,
    domain: &[u8],
    extra: &[u8],
    plain: &[u8],
    chunk_bytes: u32,
) -> Result<Vec<u8>> {
    encrypt_chunked(dek, MAGIC_CAS, domain, extra, plain, chunk_bytes)
}

pub fn decrypt_bytes_domain(
    dek: &Dek,
    domain: &[u8],
    extra: &[u8],
    data: &[u8],
) -> Result<Vec<u8>> {
    decrypt_chunked(dek, MAGIC_CAS, domain, extra, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::kdf::generate_dek;

    #[test]
    fn multi_chunk_roundtrip() {
        let dek = generate_dek();
        let chunk = 64u32;
        let plain: Vec<u8> = (0..200u8).collect();
        let enc = encrypt_chunked(&dek, MAGIC_CAS, b"cas", b"digest", &plain, chunk).expect("enc");
        assert!(is_encrypted_blob(&enc));
        let dec = decrypt_chunked(&dek, MAGIC_CAS, b"cas", b"digest", &enc).expect("dec");
        assert_eq!(dec, plain);
    }

    #[test]
    fn reencrypt_uses_distinct_nonces() {
        let dek = generate_dek();
        let plain = b"same-plaintext-for-both-seals";
        let a = encrypt_chunked(&dek, MAGIC_DB, b"matter-db", b"", plain, 64).expect("a");
        let b = encrypt_chunked(&dek, MAGIC_DB, b"matter-db", b"", plain, 64).expect("b");
        // Ciphertexts must differ (random nonces); both decrypt.
        assert_ne!(a, b);
        assert_eq!(
            decrypt_chunked(&dek, MAGIC_DB, b"matter-db", b"", &a).expect("da"),
            plain
        );
        assert_eq!(
            decrypt_chunked(&dek, MAGIC_DB, b"matter-db", b"", &b).expect("db"),
            plain
        );
    }

    #[test]
    fn boundary_exact_chunk() {
        let dek = generate_dek();
        let plain = vec![7u8; 128];
        let enc = encrypt_chunked(&dek, MAGIC_CAS, b"cas", b"x", &plain, 128).expect("enc");
        let dec = decrypt_chunked(&dek, MAGIC_CAS, b"cas", b"x", &enc).expect("dec");
        assert_eq!(dec, plain);
    }

    #[test]
    fn empty_plain() {
        let dek = generate_dek();
        let enc = encrypt_chunked(&dek, MAGIC_DB, b"db", b"", &[], 1024).expect("enc");
        let dec = decrypt_chunked(&dek, MAGIC_DB, b"db", b"", &enc).expect("dec");
        assert!(dec.is_empty());
    }

    #[test]
    fn wrong_aad_domain_fails() {
        let dek = generate_dek();
        let plain = b"secret";
        let enc = encrypt_chunked(&dek, MAGIC_CAS, b"cas", b"d", plain, 64).expect("enc");
        assert!(decrypt_chunked(&dek, MAGIC_CAS, b"other", b"d", &enc).is_err());
    }
}
