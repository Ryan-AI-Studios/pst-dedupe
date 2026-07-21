//! Argon2id KDF + KEK-wrap-DEK helpers.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Error, Result};

/// Data-encryption key (32 bytes). Zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Dek([u8; 32]);

impl Dek {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// OWASP-ish interactive defaults (19 MiB, t=2, p=1).
pub const DEFAULT_ARGON2_M_KIB: u32 = 19_456;
pub const DEFAULT_ARGON2_T: u32 = 2;
pub const DEFAULT_ARGON2_P: u32 = 1;

const SALT_LEN: usize = 16;
const WRAP_NONCE_LEN: usize = 12;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub m: u32,
    /// Time cost (iterations).
    pub t: u32,
    /// Parallelism.
    pub p: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m: DEFAULT_ARGON2_M_KIB,
            t: DEFAULT_ARGON2_T,
            p: DEFAULT_ARGON2_P,
        }
    }
}

pub fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

pub fn generate_dek() -> Dek {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    Dek(key)
}

/// Derive 32-byte KEK from passphrase + salt via Argon2id.
pub fn derive_kek(passphrase: &[u8], salt: &[u8], params: &KdfParams) -> Result<[u8; 32]> {
    let argon_params = Params::new(params.m, params.t, params.p, Some(32))
        .map_err(|e| Error::Crypto(format!("invalid argon2 params: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(passphrase, salt, &mut out)
        .map_err(|e| Error::Crypto(format!("argon2id failed: {e}")))?;
    Ok(out)
}

/// Wrap DEK under KEK: `nonce(12) || ciphertext+tag`.
pub fn wrap_dek(kek: &[u8; 32], dek: &Dek) -> Result<Vec<u8>> {
    let cipher =
        Aes256Gcm::new_from_slice(kek).map_err(|e| Error::Crypto(format!("aes key: {e}")))?;
    let mut nonce_bytes = [0u8; WRAP_NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, dek.as_bytes().as_ref())
        .map_err(|_| Error::Crypto("dek wrap failed".into()))?;
    let mut out = Vec::with_capacity(WRAP_NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Unwrap DEK; AEAD failure → [`Error::WrongPassphrase`].
pub fn unwrap_dek(kek: &[u8; 32], wrapped: &[u8]) -> Result<Dek> {
    if wrapped.len() < WRAP_NONCE_LEN + 16 {
        return Err(Error::WrongPassphrase);
    }
    let (nonce_bytes, ct) = wrapped.split_at(WRAP_NONCE_LEN);
    let cipher =
        Aes256Gcm::new_from_slice(kek).map_err(|e| Error::Crypto(format!("aes key: {e}")))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let pt = cipher
        .decrypt(nonce, ct)
        .map_err(|_| Error::WrongPassphrase)?;
    if pt.len() != 32 {
        return Err(Error::WrongPassphrase);
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&pt);
    Ok(Dek(key))
}

/// Re-wrap existing DEK under a new passphrase (change passphrase).
///
/// KEKs are zeroized on both success and failure paths.
pub fn change_passphrase_wrap(
    old_passphrase: &str,
    new_passphrase: &str,
    salt: &[u8],
    kdf_params: &KdfParams,
    wrapped_dek: &[u8],
) -> Result<(Vec<u8>, Dek)> {
    let mut old_kek = derive_kek(old_passphrase.as_bytes(), salt, kdf_params)?;
    let dek = match unwrap_dek(&old_kek, wrapped_dek) {
        Ok(d) => d,
        Err(e) => {
            old_kek.zeroize();
            return Err(e);
        }
    };
    old_kek.zeroize();

    let mut new_kek = match derive_kek(new_passphrase.as_bytes(), salt, kdf_params) {
        Ok(k) => k,
        Err(e) => {
            // DEK remains in `dek` (ZeroizeOnDrop); no KEK material to clear yet.
            return Err(e);
        }
    };
    let wrapped = match wrap_dek(&new_kek, &dek) {
        Ok(w) => w,
        Err(e) => {
            new_kek.zeroize();
            return Err(e);
        }
    };
    new_kek.zeroize();
    Ok((wrapped, dek))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_unwrap_roundtrip() {
        let salt = random_salt();
        let params = KdfParams {
            m: 8_192,
            t: 1,
            p: 1,
        };
        let kek = derive_kek(b"test-pass", &salt, &params).expect("kdf");
        let dek = generate_dek();
        let wrapped = wrap_dek(&kek, &dek).expect("wrap");
        let out = unwrap_dek(&kek, &wrapped).expect("unwrap");
        assert_eq!(out.as_bytes(), dek.as_bytes());
    }

    #[test]
    fn wrong_kek_fails() {
        let salt = random_salt();
        let params = KdfParams {
            m: 8_192,
            t: 1,
            p: 1,
        };
        let kek = derive_kek(b"right", &salt, &params).expect("kdf");
        let wrong = derive_kek(b"wrong", &salt, &params).expect("kdf2");
        let dek = generate_dek();
        let wrapped = wrap_dek(&kek, &dek).expect("wrap");
        assert!(matches!(
            unwrap_dek(&wrong, &wrapped),
            Err(Error::WrongPassphrase)
        ));
    }
}
