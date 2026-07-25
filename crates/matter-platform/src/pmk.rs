//! Platform Master Key (PMK) load + IdP secret AEAD.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Error, Result};

/// Env var holding PMK material (base64 32 bytes or hex 64 chars).
pub const ENV_PLATFORM_MASTER_KEY: &str = "PST_DEDUPE_PLATFORM_MASTER_KEY";

/// AEAD domain separator for IdP client secrets (binds ciphertext purpose).
pub const DOMAIN_IDP_SECRET: &[u8] = b"platform-idp-secret";

const NONCE_LEN: usize = 12;

/// Platform master key (32 bytes). Zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Pmk([u8; 32]);

impl Pmk {
    /// Borrow the raw 32-byte key material.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Wrap an existing 32-byte array (takes ownership; caller should not retain a copy).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl std::fmt::Debug for Pmk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pmk([REDACTED])")
    }
}

/// Load a 32-byte PMK from env or return None if unset.
///
/// Intermediate env `String` is zeroized after parse.
pub fn load_pmk_from_env() -> Result<Option<Pmk>> {
    match std::env::var(ENV_PLATFORM_MASTER_KEY) {
        Ok(mut raw) => {
            let parsed = {
                let t = raw.trim();
                if t.is_empty() {
                    Ok(None)
                } else {
                    parse_pmk(t).map(Some)
                }
            };
            zeroize_string(&mut raw);
            parsed
        }
        Err(_) => Ok(None),
    }
}

/// Parse PMK from base64 (32 decoded bytes) or hex (64 chars).
pub fn parse_pmk(raw: &str) -> Result<Pmk> {
    let t = raw.trim();
    if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut out = [0u8; 32];
        for i in 0..32 {
            let byte = u8::from_str_radix(&t[i * 2..i * 2 + 2], 16)
                .map_err(|e| Error::InvalidPmk(format!("hex: {e}")))?;
            out[i] = byte;
        }
        return Ok(Pmk(out));
    }
    let mut decoded = base64::engine::general_purpose::STANDARD
        .decode(t.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(t.as_bytes()))
        .map_err(|e| Error::InvalidPmk(format!("base64: {e}")))?;
    if decoded.len() != 32 {
        decoded.zeroize();
        return Err(Error::InvalidPmk(format!(
            "expected 32 bytes, got {}",
            decoded.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    decoded.zeroize();
    Ok(Pmk(out))
}

/// Generate a random 32-byte PMK (for tests / operators).
pub fn generate_pmk() -> Pmk {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    Pmk(key)
}

/// Encrypt IdP secret under PMK. Returns (nonce, ciphertext+tag).
pub fn encrypt_idp_secret(pmk: &Pmk, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(pmk.as_bytes())
        .map_err(|e| Error::Crypto(format!("aes key: {e}")))?;
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
pub fn decrypt_idp_secret(pmk: &Pmk, nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
    if nonce.len() != NONCE_LEN {
        return Err(Error::Crypto("invalid idp secret nonce length".into()));
    }
    let cipher = Aes256Gcm::new_from_slice(pmk.as_bytes())
        .map_err(|e| Error::Crypto(format!("aes key: {e}")))?;
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
        let mut plain = decrypt_idp_secret(&pmk, &nonce, &ct).expect("dec");
        assert_eq!(plain.as_slice(), secret);
        plain.zeroize();
    }

    #[test]
    fn parse_hex_pmk() {
        let pmk = generate_pmk();
        let hex: String = pmk.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let parsed = parse_pmk(&hex).expect("parse");
        assert_eq!(parsed.as_bytes(), pmk.as_bytes());
    }

    #[test]
    fn pmk_implements_zeroize_on_drop() {
        // Type-level: Pmk derives ZeroizeOnDrop (same pattern as matter-core Dek).
        fn assert_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
        assert_zeroize_on_drop::<Pmk>();
        // Compile-time proof that Debug does not leak key material.
        let pmk = generate_pmk();
        let dbg = format!("{pmk:?}");
        assert!(dbg.contains("REDACTED"));
        let full_hex: String = pmk.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        assert!(!dbg.contains(&full_hex));
    }
}
