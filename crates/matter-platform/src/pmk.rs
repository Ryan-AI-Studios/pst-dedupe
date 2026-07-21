//! Platform Master Key (PMK) load + IdP secret AEAD.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::Zeroize;

use crate::error::{Error, Result};

/// Env var holding PMK material (base64 32 bytes or hex 64 chars).
pub const ENV_PLATFORM_MASTER_KEY: &str = "PST_DEDUPE_PLATFORM_MASTER_KEY";

/// AEAD domain separator for IdP client secrets (binds ciphertext purpose).
pub const DOMAIN_IDP_SECRET: &[u8] = b"platform-idp-secret";

const NONCE_LEN: usize = 12;

/// Load a 32-byte PMK from env or return None if unset.
pub fn load_pmk_from_env() -> Result<Option<[u8; 32]>> {
    match std::env::var(ENV_PLATFORM_MASTER_KEY) {
        Ok(raw) => {
            let t = raw.trim();
            if t.is_empty() {
                return Ok(None);
            }
            Ok(Some(parse_pmk(t)?))
        }
        Err(_) => Ok(None),
    }
}

/// Parse PMK from base64 (32 decoded bytes) or hex (64 chars).
pub fn parse_pmk(raw: &str) -> Result<[u8; 32]> {
    let t = raw.trim();
    if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut out = [0u8; 32];
        for i in 0..32 {
            let byte = u8::from_str_radix(&t[i * 2..i * 2 + 2], 16)
                .map_err(|e| Error::InvalidPmk(format!("hex: {e}")))?;
            out[i] = byte;
        }
        return Ok(out);
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(t.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(t.as_bytes()))
        .map_err(|e| Error::InvalidPmk(format!("base64: {e}")))?;
    if decoded.len() != 32 {
        return Err(Error::InvalidPmk(format!(
            "expected 32 bytes, got {}",
            decoded.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    Ok(out)
}

/// Generate a random 32-byte PMK (for tests / operators).
pub fn generate_pmk() -> [u8; 32] {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

/// Encrypt IdP secret under PMK. Returns (nonce, ciphertext+tag).
pub fn encrypt_idp_secret(pmk: &[u8; 32], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher =
        Aes256Gcm::new_from_slice(pmk).map_err(|e| Error::Crypto(format!("aes key: {e}")))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: plaintext,
                aad: DOMAIN_IDP_SECRET,
            },
        )
        .map_err(|_| Error::Crypto("idp secret encrypt failed".into()))?;
    Ok((nonce_bytes.to_vec(), ct))
}

/// Decrypt IdP secret under PMK.
pub fn decrypt_idp_secret(pmk: &[u8; 32], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if nonce.len() != NONCE_LEN {
        return Err(Error::Crypto("invalid idp secret nonce length".into()));
    }
    let cipher =
        Aes256Gcm::new_from_slice(pmk).map_err(|e| Error::Crypto(format!("aes key: {e}")))?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: ciphertext,
                aad: DOMAIN_IDP_SECRET,
            },
        )
        .map_err(|_| Error::Crypto("idp secret decrypt failed".into()))
}

/// Zeroize helper for temporary secret strings.
pub fn zeroize_string(s: &mut String) {
    s.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmk_roundtrip_secret() {
        let pmk = generate_pmk();
        let secret = b"super-secret-client-value";
        let (nonce, ct) = encrypt_idp_secret(&pmk, secret).expect("enc");
        let plain = decrypt_idp_secret(&pmk, &nonce, &ct).expect("dec");
        assert_eq!(plain, secret);
    }

    #[test]
    fn parse_hex_pmk() {
        let pmk = generate_pmk();
        let hex: String = pmk.iter().map(|b| format!("{b:02x}")).collect();
        let parsed = parse_pmk(&hex).expect("parse");
        assert_eq!(parsed, pmk);
    }
}
