//! On-disk `matter.crypto.json` header (no raw secrets).

use std::fs;
use std::io::Write;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

use super::kdf::KdfParams;
use crate::error::{Error, Result};

/// Header filename adjacent to the matter root.
pub const CRYPTO_HEADER_FILE: &str = "matter.crypto.json";

/// Env var for headless unlock (CLI / automation).
pub const ENV_MATTER_PASSPHRASE: &str = "PST_DEDUPE_MATTER_PASSPHRASE";

pub const CRYPTO_HEADER_VERSION: u32 = 1;

/// Cipher stack identifier stored in the header.
pub const DEFAULT_CIPHER_ID: &str = "aead_db_file_v1+aead_cas_chunk_v1+fts_dir_v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoHeader {
    pub version: u32,
    pub kdf: String,
    pub salt_b64: String,
    pub kdf_params: KdfParams,
    pub wrapped_dek_b64: String,
    pub cipher: String,
    pub cas_chunk_bytes: u32,
    pub created_at: String,

    /// Decoded salt (not serialized).
    #[serde(skip)]
    pub salt: Vec<u8>,
    /// Decoded wrapped DEK (not serialized).
    #[serde(skip)]
    pub wrapped_dek: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CryptoHeaderSerde {
    version: u32,
    kdf: String,
    salt_b64: String,
    kdf_params: KdfParams,
    wrapped_dek_b64: String,
    cipher: String,
    cas_chunk_bytes: u32,
    created_at: String,
}

impl CryptoHeader {
    fn to_serde(&self) -> CryptoHeaderSerde {
        CryptoHeaderSerde {
            version: self.version,
            kdf: self.kdf.clone(),
            salt_b64: self.salt_b64.clone(),
            kdf_params: self.kdf_params.clone(),
            wrapped_dek_b64: self.wrapped_dek_b64.clone(),
            cipher: self.cipher.clone(),
            cas_chunk_bytes: self.cas_chunk_bytes,
            created_at: self.created_at.clone(),
        }
    }

    fn from_serde(s: CryptoHeaderSerde) -> Result<Self> {
        if s.version != CRYPTO_HEADER_VERSION {
            return Err(Error::Crypto(format!(
                "unsupported crypto header version {}",
                s.version
            )));
        }
        if s.kdf != "argon2id" {
            return Err(Error::Crypto(format!("unsupported kdf {}", s.kdf)));
        }
        if s.cas_chunk_bytes < 64 * 1024 || s.cas_chunk_bytes > 4 * 1024 * 1024 {
            return Err(Error::Crypto(format!(
                "cas_chunk_bytes {} out of 64KiB..=4MiB range",
                s.cas_chunk_bytes
            )));
        }
        let salt = b64_decode(&s.salt_b64)?;
        let wrapped_dek = b64_decode(&s.wrapped_dek_b64)?;
        Ok(Self {
            version: s.version,
            kdf: s.kdf,
            salt_b64: s.salt_b64,
            kdf_params: s.kdf_params,
            wrapped_dek_b64: s.wrapped_dek_b64,
            cipher: s.cipher,
            cas_chunk_bytes: s.cas_chunk_bytes,
            created_at: s.created_at,
            salt,
            wrapped_dek,
        })
    }
}

pub fn header_path(matter_root: &Utf8Path) -> Utf8PathBuf {
    matter_root.join(CRYPTO_HEADER_FILE)
}

pub fn is_encrypted_matter(matter_root: &Utf8Path) -> bool {
    header_path(matter_root).as_std_path().exists()
}

pub fn load_header(matter_root: &Utf8Path) -> Result<CryptoHeader> {
    let path = header_path(matter_root);
    if !path.as_std_path().exists() {
        return Err(Error::CryptoHeaderMissing(matter_root.to_string()));
    }
    let text = fs::read_to_string(path.as_std_path())?;
    let s: CryptoHeaderSerde = serde_json::from_str(&text)?;
    CryptoHeader::from_serde(s)
}

pub fn save_header(matter_root: &Utf8Path, header: &CryptoHeader) -> Result<()> {
    let path = header_path(matter_root);
    let json = serde_json::to_string_pretty(&header.to_serde())?;
    // Unique temp beside the header (Windows cannot rename-over existing dest).
    let tmp = matter_root.join(format!(
        ".{}.tmp.{}",
        CRYPTO_HEADER_FILE,
        std::process::id()
    ));
    {
        let mut f = fs::File::create(tmp.as_std_path())?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
    }
    replace_file(&tmp, &path)?;
    Ok(())
}

/// If change-passphrase crash left a header temp and no destination, promote it.
pub fn recover_header_temp(matter_root: &Utf8Path) -> Result<()> {
    let path = header_path(matter_root);
    if path.as_std_path().exists() {
        return Ok(());
    }
    // Look for `.matter.crypto.json.tmp.<pid>` leftovers.
    let Ok(entries) = fs::read_dir(matter_root.as_std_path()) else {
        return Ok(());
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&format!(".{CRYPTO_HEADER_FILE}.tmp.")) {
            let _ = fs::rename(e.path(), path.as_std_path());
            break;
        }
    }
    Ok(())
}

/// Replace `dest` with `src` (src removed). Windows-safe (remove dest first).
pub(crate) fn replace_file(src: &Utf8Path, dest: &Utf8Path) -> Result<()> {
    if dest.as_std_path().exists() {
        fs::remove_file(dest.as_std_path())?;
    }
    fs::rename(src.as_std_path(), dest.as_std_path())?;
    Ok(())
}

pub fn b64_encode(data: &[u8]) -> String {
    B64.encode(data)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    B64.decode(s.trim())
        .map_err(|e| Error::Crypto(format!("base64 decode: {e}")))
}
