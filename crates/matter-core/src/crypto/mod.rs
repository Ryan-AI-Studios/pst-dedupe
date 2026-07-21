//! Optional matter encryption at rest (track 0057).
//!
//! Pure-Rust stack (SQLCipher not used — OpenSSL/perl link cost on Windows):
//! - Passphrase → Argon2id → **KEK** wraps random **DEK**
//! - SQLite: whole-file chunked AEAD container (equivalent page-file protection)
//! - CAS / FTS: chunked AES-256-GCM under the DEK
//!
//! Header file [`CRYPTO_HEADER_FILE`] is the source of truth for “is encrypted”.

mod aead_chunk;
mod header;
mod kdf;
mod session;

pub use aead_chunk::{
    decrypt_bytes_domain, decrypt_chunked, decrypt_chunked_from_reader, decrypt_chunked_streaming,
    encrypt_bytes_domain, encrypt_chunked, encrypt_chunked_from_reader, is_encrypted_blob,
    read_plain_len, starts_with_cas_magic, DEFAULT_CHUNK_BYTES, MAGIC_CAS, MAGIC_DB,
};
pub use header::{
    b64_encode, header_path, is_encrypted_matter, load_header, recover_header_temp, save_header,
    CryptoHeader, CRYPTO_HEADER_FILE, CRYPTO_HEADER_VERSION, DEFAULT_CIPHER_ID,
    ENV_MATTER_PASSPHRASE,
};
pub use kdf::{
    change_passphrase_wrap, derive_kek, generate_dek, unwrap_dek, wrap_dek, Dek, KdfParams,
    DEFAULT_ARGON2_M_KIB, DEFAULT_ARGON2_P, DEFAULT_ARGON2_T,
};
pub use session::{
    create_db_session, decrypt_db_to_session, encrypt_session_db_to_root, has_active_session,
    plain_db_path, recover_seal_temp, release_db_session, retain_db_session,
    wipe_orphan_enc_db_session, EncryptedDbSession,
};

use zeroize::Zeroize;

use crate::error::{Error, Result};

/// Resolve passphrase from env [`ENV_MATTER_PASSPHRASE`] (trimmed non-empty).
pub fn passphrase_from_env() -> Option<String> {
    std::env::var(ENV_MATTER_PASSPHRASE)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Derive DEK from passphrase + header; fail closed on wrong password.
pub fn unlock_dek(passphrase: &str, header: &CryptoHeader) -> Result<Dek> {
    let mut kek = derive_kek(passphrase.as_bytes(), &header.salt, &header.kdf_params)?;
    let dek = unwrap_dek(&kek, &header.wrapped_dek).map_err(|e| {
        kek.zeroize();
        // Map AEAD failure to wrong passphrase (fail closed, honest).
        match e {
            Error::Crypto(_) | Error::WrongPassphrase => Error::WrongPassphrase,
            other => other,
        }
    })?;
    kek.zeroize();
    Ok(dek)
}

/// Build a new header wrapping a fresh DEK under the passphrase.
pub fn create_header(passphrase: &str, created_at: &str) -> Result<(CryptoHeader, Dek)> {
    let salt = kdf::random_salt();
    let kdf_params = KdfParams::default();
    let mut kek = derive_kek(passphrase.as_bytes(), &salt, &kdf_params)?;
    let dek = generate_dek();
    let wrapped = wrap_dek(&kek, &dek)?;
    kek.zeroize();
    let header = CryptoHeader {
        version: CRYPTO_HEADER_VERSION,
        kdf: "argon2id".into(),
        salt_b64: header::b64_encode(&salt),
        kdf_params,
        wrapped_dek_b64: header::b64_encode(&wrapped),
        cipher: DEFAULT_CIPHER_ID.into(),
        cas_chunk_bytes: DEFAULT_CHUNK_BYTES,
        created_at: created_at.to_string(),
        // filled for convenience after decode helpers
        salt: salt.to_vec(),
        wrapped_dek: wrapped,
    };
    Ok((header, dek))
}
