//! Process-local encrypted SQLite session (plain DB under matter workspace/temp).
//!
//! Pure-Rust equivalent of SQLCipher: ciphertext at rest in `matter.db`,
//! plaintext only while unlocked under `{matter}/workspace/temp/.enc-db/`.
//!
//! **Crash orphans:** plain session files under `.enc-db` are wiped when no
//! process-local session is held (confidentiality over recovering unsealed
//! in-progress writes).

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::sync::{Mutex, OnceLock};

use camino::{Utf8Path, Utf8PathBuf};

use super::aead_chunk::{decrypt_chunked, encrypt_chunked, MAGIC_DB};
use super::kdf::Dek;
use crate::error::{Error, Result};
use crate::matter::{DB_FILE, WORKSPACE_DIR, WORKSPACE_TEMP_DIR};

const ENC_DB_SUBDIR: &str = ".enc-db";

struct SessionEntry {
    refcount: usize,
    plain_path: Utf8PathBuf,
    dek: Dek,
    chunk_bytes: u32,
}

fn sessions() -> &'static Mutex<HashMap<String, SessionEntry>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, SessionEntry>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn root_key(root: &Utf8Path) -> String {
    root.as_str().replace('\\', "/").to_ascii_lowercase()
}

/// Path to the unlocked plain SQLite file for an encrypted matter.
pub fn plain_db_path(matter_root: &Utf8Path) -> Utf8PathBuf {
    matter_root
        .join(WORKSPACE_DIR)
        .join(WORKSPACE_TEMP_DIR)
        .join(ENC_DB_SUBDIR)
        .join(DB_FILE)
}

/// Directory holding the plain session DB.
pub fn enc_db_dir(matter_root: &Utf8Path) -> Utf8PathBuf {
    matter_root
        .join(WORKSPACE_DIR)
        .join(WORKSPACE_TEMP_DIR)
        .join(ENC_DB_SUBDIR)
}

/// True if this process currently holds a **live** unlocked session (`refcount > 0`).
///
/// On lock poison, returns `true` so callers do not wipe under a possibly-live handle.
pub fn has_active_session(matter_root: &Utf8Path) -> bool {
    let key = root_key(matter_root);
    sessions()
        .lock()
        .map(|m| m.get(&key).map(|e| e.refcount > 0).unwrap_or(false))
        .unwrap_or(true)
}

/// Wipe crash-orphan plain SQLite under `.enc-db` when **no** live process session is held.
///
/// Confidentiality fail-closed: unsealed in-progress writes are discarded.
/// Safe to call when passphrase is missing (still removes plaintext residue).
pub fn wipe_orphan_enc_db_session(matter_root: &Utf8Path) -> Result<()> {
    if has_active_session(matter_root) {
        return Ok(());
    }
    let dir = enc_db_dir(matter_root);
    if !dir.as_std_path().exists() {
        return Ok(());
    }
    // Remove entire directory tree (plain db + wal/shm/journal).
    // Best-effort: map I/O errors to Ok after attempt so PassphraseRequired
    // path still surfaces the passphrase error rather than a wipe I/O error.
    match fs::remove_dir_all(dir.as_std_path()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::Io(e)),
    }
}

/// Create a new empty plain session DB (encrypted matter create path).
pub fn create_db_session(
    matter_root: &Utf8Path,
    dek: &Dek,
    chunk_bytes: u32,
) -> Result<Utf8PathBuf> {
    let key = root_key(matter_root);
    let mut map = sessions()
        .lock()
        .map_err(|_| Error::Crypto("session lock poisoned".into()))?;

    if let Some(entry) = map.get(&key) {
        if entry.refcount > 0 {
            return Err(Error::Crypto(
                "encrypted session already open for this matter".into(),
            ));
        }
        // Stale zero-refcount after failed seal — drop and recreate.
        map.remove(&key);
    }

    let plain = plain_db_path(matter_root);
    wipe_plain_session(&plain);
    if let Some(parent) = plain.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }
    File::create(plain.as_std_path())?;

    map.insert(
        key,
        SessionEntry {
            refcount: 1,
            plain_path: plain.clone(),
            dek: dek.clone(),
            chunk_bytes,
        },
    );
    Ok(plain)
}

/// Ensure plain session DB exists (decrypt from encrypted `matter.db` if needed).
pub fn retain_db_session(
    matter_root: &Utf8Path,
    dek: &Dek,
    chunk_bytes: u32,
) -> Result<Utf8PathBuf> {
    let key = root_key(matter_root);
    let mut map = sessions()
        .lock()
        .map_err(|_| Error::Crypto("session lock poisoned".into()))?;

    if let Some(entry) = map.get(&key) {
        if entry.refcount > 0 {
            let entry = map.get_mut(&key).expect("just checked");
            entry.refcount += 1;
            return Ok(entry.plain_path.clone());
        }
        // Stale zero-refcount after failed seal: discard and re-decrypt below.
        map.remove(&key);
    }

    let plain = plain_db_path(matter_root);
    if let Some(parent) = plain.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }

    // Crash orphan: discard leftover plain and re-decrypt sealed ciphertext.
    if plain.as_std_path().exists() {
        wipe_plain_session(&plain);
        if let Some(parent) = plain.parent() {
            fs::create_dir_all(parent.as_std_path())?;
        }
    }

    decrypt_db_to_session(matter_root, dek, chunk_bytes, &plain)?;

    map.insert(
        key,
        SessionEntry {
            refcount: 1,
            plain_path: plain.clone(),
            dek: dek.clone(),
            chunk_bytes,
        },
    );
    Ok(plain)
}

/// Decrement refcount; on last handle, seal plain → root `matter.db` then wipe plain.
///
/// Seal is **transactional**: encrypt while the map entry is still present; only on
/// success remove the entry and wipe plaintext. On encrypt failure the entry stays
/// with `refcount == 1` so the session remains live for retry / next Drop.
pub fn release_db_session(matter_root: &Utf8Path) -> Result<()> {
    let key = root_key(matter_root);
    let mut map = sessions()
        .lock()
        .map_err(|_| Error::Crypto("session lock poisoned".into()))?;

    let Some(entry) = map.get_mut(&key) else {
        return Ok(());
    };
    if entry.refcount > 1 {
        entry.refcount -= 1;
        return Ok(());
    }

    // Last live handle (refcount == 1): encrypt FIRST while still in map.
    let plain_path = entry.plain_path.clone();
    let dek = entry.dek.clone();
    let chunk_bytes = entry.chunk_bytes;

    match encrypt_session_db_to_root(matter_root, &dek, chunk_bytes, &plain_path) {
        Ok(()) => {
            map.remove(&key);
            wipe_plain_session(&plain_path);
            Ok(())
        }
        Err(e) => {
            // Leave entry with refcount 1 — session still live; Drop may retry.
            Err(e)
        }
    }
}

pub fn decrypt_db_to_session(
    matter_root: &Utf8Path,
    dek: &Dek,
    chunk_bytes: u32,
    plain_path: &Utf8Path,
) -> Result<()> {
    let enc_path = matter_root.join(DB_FILE);
    if !enc_path.as_std_path().exists() {
        return Err(Error::DatabaseMissing(matter_root.to_string()));
    }
    let mut file = File::open(enc_path.as_std_path())?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    let plain = decrypt_chunked(dek, MAGIC_DB, b"matter-db", b"", &data)?;
    if let Some(parent) = plain_path.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }
    {
        let mut out = File::create(plain_path.as_std_path())?;
        out.write_all(&plain)?;
        out.sync_all()?;
    }
    let _ = chunk_bytes;
    Ok(())
}

pub fn encrypt_session_db_to_root(
    matter_root: &Utf8Path,
    dek: &Dek,
    chunk_bytes: u32,
    plain_path: &Utf8Path,
) -> Result<()> {
    if !plain_path.as_std_path().exists() {
        return Err(Error::Crypto(
            "plain session db missing at seal time".into(),
        ));
    }
    let mut plain = Vec::new();
    {
        let mut f = File::open(plain_path.as_std_path())?;
        f.read_to_end(&mut plain)?;
    }
    let enc = encrypt_chunked(dek, MAGIC_DB, b"matter-db", b"", &plain, chunk_bytes)?;
    let enc_path = matter_root.join(DB_FILE);
    let tmp = matter_root.join(format!(".{DB_FILE}.enc.tmp"));
    {
        let mut out = File::create(tmp.as_std_path())?;
        out.write_all(&enc)?;
        out.sync_all()?;
    }
    // Keep prior sealed DB until the new ciphertext is fully written; then replace.
    // On Windows, rename cannot overwrite — remove dest then rename. If rename fails
    // after remove, `recover_seal_temp` on open can restore from `.matter.db.enc.tmp`.
    if enc_path.as_std_path().exists() {
        fs::remove_file(enc_path.as_std_path())?;
    }
    match fs::rename(tmp.as_std_path(), enc_path.as_std_path()) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Leave tmp for recovery if dest is missing.
            Err(Error::Io(e))
        }
    }
}

/// If a prior seal crash left `.matter.db.enc.tmp` and no `matter.db`, promote the temp.
pub fn recover_seal_temp(matter_root: &Utf8Path) -> Result<()> {
    let enc_path = matter_root.join(DB_FILE);
    let tmp = matter_root.join(format!(".{DB_FILE}.enc.tmp"));
    if !enc_path.as_std_path().exists() && tmp.as_std_path().exists() {
        fs::rename(tmp.as_std_path(), enc_path.as_std_path())?;
    }
    Ok(())
}

fn wipe_plain_session(plain_path: &Utf8Path) {
    let _ = fs::remove_file(plain_path.as_std_path());
    let s = plain_path.as_str();
    let _ = fs::remove_file(format!("{s}-wal"));
    let _ = fs::remove_file(format!("{s}-shm"));
    let _ = fs::remove_file(format!("{s}-journal"));
    if let Some(parent) = plain_path.parent() {
        let _ = fs::remove_dir_all(parent.as_std_path());
    }
}

/// RAII guard that releases the session on drop.
pub struct EncryptedDbSession {
    root: Utf8PathBuf,
    plain_path: Utf8PathBuf,
    active: bool,
}

impl EncryptedDbSession {
    /// Open existing encrypted matter (decrypt at-rest `matter.db`).
    pub fn acquire(matter_root: &Utf8Path, dek: &Dek, chunk_bytes: u32) -> Result<Self> {
        let plain_path = retain_db_session(matter_root, dek, chunk_bytes)?;
        Ok(Self {
            root: matter_root.to_path_buf(),
            plain_path,
            active: true,
        })
    }

    /// Create new encrypted matter session (empty plain DB).
    pub fn create(matter_root: &Utf8Path, dek: &Dek, chunk_bytes: u32) -> Result<Self> {
        let plain_path = create_db_session(matter_root, dek, chunk_bytes)?;
        Ok(Self {
            root: matter_root.to_path_buf(),
            plain_path,
            active: true,
        })
    }

    pub fn plain_path(&self) -> &Utf8Path {
        &self.plain_path
    }

    /// Seal now (for tests); subsequent Drop is a no-op.
    pub fn seal_now(mut self) -> Result<()> {
        if self.active {
            release_db_session(&self.root)?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for EncryptedDbSession {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        // Fallible seal cannot surface through Drop; retry once, then leave
        // session entry alive on failure (refcount stays) for next open wipe.
        match release_db_session(&self.root) {
            Ok(()) => self.active = false,
            Err(_) => {
                // Keep `active` true only conceptually — Drop ends. Session map
                // still holds the entry when seal failed (see release_db_session).
                // Best-effort second attempt.
                if release_db_session(&self.root).is_ok() {
                    self.active = false;
                }
            }
        }
    }
}
