//! Shared matter-root open helpers for chrome commands.

use std::fs;
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::{is_encrypted_matter, Matter};

use crate::error::CommandError;
use crate::matter_cmd::map_root_metadata_err;

pub(crate) fn ensure_root_accessible(root: &str) -> Result<(), CommandError> {
    match fs::metadata(Path::new(root)) {
        Ok(_) => Ok(()),
        Err(e) => Err(map_root_metadata_err(root, e)),
    }
}

pub(crate) fn utf8_root(root: &str) -> Result<Utf8PathBuf, CommandError> {
    Utf8PathBuf::from_path_buf(Path::new(root).to_path_buf())
        .map_err(|_| CommandError::failed(format!("Matter root is not valid UTF-8: {root}")))
}

/// Fail closed before any `open_*` so encrypted roots never hit PassphraseRequired.
pub(crate) fn reject_encrypted(root: &Utf8Path) -> Result<(), CommandError> {
    if is_encrypted_matter(root) {
        return Err(CommandError::encrypted(
            "Encrypted matters are not opened in this chrome; use Dedupe Desk.",
        ));
    }
    Ok(())
}

pub(crate) fn open_matter_read(root: &str) -> Result<Matter, CommandError> {
    ensure_root_accessible(root)?;
    let utf8 = utf8_root(root)?;
    reject_encrypted(&utf8)?;
    Matter::open_for_read(&utf8).map_err(|e| CommandError::failed(e.to_string()))
}

pub(crate) fn open_matter_write(root: &str) -> Result<Matter, CommandError> {
    ensure_root_accessible(root)?;
    let utf8 = utf8_root(root)?;
    reject_encrypted(&utf8)?;
    Matter::open(&utf8).map_err(|e| CommandError::failed(e.to_string()))
}
