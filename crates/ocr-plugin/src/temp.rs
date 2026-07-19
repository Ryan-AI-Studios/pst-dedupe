//! Drop-guarded OCR temps + startup purge (spec §3.9.1).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use camino::{Utf8Path, Utf8PathBuf};
use tempfile::{Builder, NamedTempFile};

use crate::error::{Error, Result};
use crate::limits::OCR_TEMP_SUBDIR;

/// Matter-scoped OCR temp directory: `<matter_root>/workspace/temp/ocr/`.
pub fn ocr_temp_dir(matter_root: &Utf8Path) -> Utf8PathBuf {
    matter_root
        .join(matter_core::WORKSPACE_DIR)
        .join(matter_core::WORKSPACE_TEMP_DIR)
        .join(OCR_TEMP_SUBDIR)
}

/// Ensure OCR temp directory exists.
pub fn ensure_ocr_temp_dir(matter_root: &Utf8Path) -> Result<Utf8PathBuf> {
    let dir = ocr_temp_dir(matter_root);
    fs::create_dir_all(dir.as_std_path())?;
    Ok(dir)
}

/// Sweep and delete residual files under `workspace/temp/ocr/` (hard-crash leftovers).
///
/// Returns the number of entries removed. Keeps the directory itself.
pub fn purge_ocr_temp_dir(matter_root: &Utf8Path) -> Result<u64> {
    let dir = ocr_temp_dir(matter_root);
    if !dir.as_std_path().exists() {
        fs::create_dir_all(dir.as_std_path())?;
        return Ok(0);
    }
    let mut removed = 0u64;
    for entry in fs::read_dir(dir.as_std_path())? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
        removed += 1;
    }
    Ok(removed)
}

/// RAII page image temp: deleted on Drop (success, error, cancel, panic unwind).
///
/// Prefer one guard per page; drop **before** starting the next page.
pub struct OcrTempFile {
    inner: NamedTempFile,
    utf8_path: Utf8PathBuf,
}

impl OcrTempFile {
    /// Create a new temp file under the OCR temp directory with the given suffix
    /// (e.g. `.png`).
    pub fn new_in(matter_root: &Utf8Path, suffix: &str) -> Result<Self> {
        let dir = ensure_ocr_temp_dir(matter_root)?;
        let file = Builder::new()
            .prefix("ocr_page_")
            .suffix(suffix)
            .tempfile_in(dir.as_std_path())
            .map_err(Error::Io)?;
        let path = file.path().to_path_buf();
        let utf8_path = Utf8PathBuf::from_path_buf(path)
            .map_err(|p| Error::Other(format!("OCR temp path is not UTF-8: {}", p.display())))?;
        Ok(Self {
            inner: file,
            utf8_path,
        })
    }

    /// Write bytes and flush (e.g. CAS native materialization for image OCR).
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.inner.write_all(bytes)?;
        self.inner.flush()?;
        Ok(())
    }

    /// UTF-8 path for child process argv.
    pub fn path(&self) -> &Utf8Path {
        &self.utf8_path
    }

    /// Std path.
    pub fn std_path(&self) -> &Path {
        self.inner.path()
    }

    /// Persist path string for tests.
    pub fn path_buf(&self) -> PathBuf {
        self.inner.path().to_path_buf()
    }
}

impl Drop for OcrTempFile {
    fn drop(&mut self) {
        // NamedTempFile already deletes on drop; this is explicit documentation.
        // Keep the field so Drop of NamedTempFile runs.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn utf8_root() -> (tempfile::TempDir, Utf8PathBuf) {
        let dir = tempdir().unwrap();
        let p = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        (dir, p)
    }

    #[test]
    fn drop_deletes_file_on_error_path() {
        let (_tmp, root) = utf8_root();
        let path = {
            let mut t = OcrTempFile::new_in(&root, ".png").unwrap();
            t.write_all(b"fake-png").unwrap();
            let p = t.path_buf();
            assert!(p.exists());
            // Simulate error path: drop via scope end without explicit remove.
            p
        };
        assert!(
            !path.exists(),
            "Drop must delete temp on error-path scope exit"
        );
    }

    #[test]
    fn purge_removes_planted_orphan() {
        let (_tmp, root) = utf8_root();
        let dir = ensure_ocr_temp_dir(&root).unwrap();
        let orphan = dir.as_std_path().join("orphan_crash.png");
        fs::write(&orphan, b"leaked").unwrap();
        assert!(orphan.exists());
        let n = purge_ocr_temp_dir(&root).unwrap();
        assert!(n >= 1);
        assert!(!orphan.exists(), "purge must remove planted orphan");
    }
}
