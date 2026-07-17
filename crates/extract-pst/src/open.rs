//! Open PST evidence from filesystem or matter CAS via `workspace/temp/`.

use std::fs::{self, File};
use std::io::{copy, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use camino::{Utf8Path, Utf8PathBuf};
use matter_core::Matter;
use pst_reader::{PstError, PstFile};

use crate::error::{Error, Result};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// How to locate a PST for extract.
#[derive(Debug, Clone)]
pub struct PstOpenSpec {
    /// Inventory logical path (e.g. `mail.pst` or `files.zip!/mail.pst`).
    pub inventory_path: String,
    /// Whole-file SHA-256 when the PST lives in CAS.
    pub native_sha256: Option<String>,
    /// Absolute filesystem path to try first (package root + relative).
    pub filesystem_path: Option<Utf8PathBuf>,
}

/// Opened PST plus optional temp file guard (deleted on drop).
pub struct OpenedPst {
    pub pst: PstFile,
    /// Path actually opened (FS or matter temp).
    pub opened_path: Utf8PathBuf,
    /// True when materialised under `workspace/temp/`.
    pub from_cas_temp: bool,
    _guard: Option<TempFileGuard>,
}

/// RAII delete of a matter-local temp file.
struct TempFileGuard {
    path: PathBuf,
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Open a PST preferring filesystem path, else CAS → `workspace/temp/`.
///
/// **Never** uses `std::env::temp_dir()` for evidence materialization.
pub fn open_pst(matter: &Matter, job_id: &str, spec: &PstOpenSpec) -> Result<OpenedPst> {
    // 1) Filesystem path when present and exists.
    if let Some(ref fs_path) = spec.filesystem_path {
        if fs_path.as_std_path().is_file() {
            return open_from_path(fs_path, false, None);
        }
    }

    // 2) CAS materialize under matter workspace/temp.
    let digest = spec.native_sha256.as_deref().ok_or_else(|| {
        Error::PstOpenFailed(format!(
            "no filesystem path and no native_sha256 for {}",
            spec.inventory_path
        ))
    })?;

    if !matter.blob_exists(digest)? {
        return Err(Error::PstOpenFailed(format!(
            "CAS blob missing for PST digest {digest}"
        )));
    }

    let temp_dir = matter.workspace_temp_dir();
    fs::create_dir_all(temp_dir.as_std_path())?;

    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let digest_prefix: String = digest.chars().take(12).collect();
    let file_name = format!("{job_id}_{digest_prefix}_{seq}_{}.pst", std::process::id());
    let temp_path = temp_dir.join(&file_name);

    // Stream CAS → temp (bounded buffer via copy).
    // Create the RAII guard immediately after File::create so every subsequent
    // error path (copy/flush/sync/open) deletes the partial temp file.
    let mut src = matter.cas().open_read(digest)?;
    let mut dst = File::create(temp_path.as_std_path())?;
    let guard = TempFileGuard {
        path: temp_path.as_std_path().to_path_buf(),
    };
    copy(&mut src, &mut dst)?;
    dst.flush()?;
    dst.sync_all()?;
    // Close the writer before re-opening for parse (Windows file locks).
    drop(dst);
    drop(src);

    // Assert we did not land in OS temp.
    let temp_str = temp_path.as_str().replace('\\', "/").to_ascii_lowercase();
    if let Ok(os_temp) = std::env::temp_dir().canonicalize() {
        let os_s = os_temp
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        if temp_str.starts_with(&os_s) {
            // Guard Drop deletes the file; explicit remove is redundant.
            return Err(Error::PstOpenFailed(
                "refusing to materialize PST under OS temp_dir".into(),
            ));
        }
    }

    open_from_path(&temp_path, true, Some(guard))
}

fn open_from_path(
    path: &Utf8Path,
    from_cas_temp: bool,
    guard: Option<TempFileGuard>,
) -> Result<OpenedPst> {
    match PstFile::open(path.as_std_path()) {
        Ok(pst) => Ok(OpenedPst {
            pst,
            opened_path: path.to_path_buf(),
            from_cas_temp,
            _guard: guard,
        }),
        Err(PstError::AnsiPstNotSupported(v)) => Err(Error::PstAnsiRejected(format!(
            "ANSI PST wVer={v} at {path}"
        ))),
        Err(e) => Err(Error::PstOpenFailed(format!("{path}: {e}"))),
    }
}

/// Build a candidate FS path from source package root + inventory relative path.
///
/// Inventory paths may be nested (`files.zip!/mail.pst`). For nested forms we
/// only attempt the leaf name under the package root (0016 may still have the
/// original PST beside the zip). Full nested materialization is CAS's job.
pub fn candidate_fs_path(source_path: &str, inventory_path: &str) -> Option<Utf8PathBuf> {
    let source = Utf8PathBuf::from(source_path);
    // Direct only for **absolute** inventory paths. Relative names like `mail.pst`
    // must not be resolved against the process CWD (shadowing package root).
    let direct = Utf8PathBuf::from(inventory_path);
    if direct.is_absolute() && direct.as_std_path().is_file() {
        return Some(direct);
    }
    // source is a file (.pst) itself.
    if source.as_std_path().is_file()
        && source
            .extension()
            .map(|e| e.eq_ignore_ascii_case("pst"))
            .unwrap_or(false)
    {
        return Some(source);
    }
    // source is a directory / package root.
    let root = if source.as_std_path().is_dir() {
        source
    } else {
        source.parent()?.to_path_buf()
    };
    // Strip zip!/ nesting.
    let rel = inventory_path
        .split("!/")
        .last()
        .unwrap_or(inventory_path)
        .trim_start_matches(['/', '\\']);
    let candidate = root.join(rel);
    if candidate.as_std_path().is_file() {
        return Some(candidate);
    }
    // Try leaf filename only under root.
    if let Some(name) = Utf8Path::new(rel).file_name() {
        let c2 = root.join(name);
        if c2.as_std_path().is_file() {
            return Some(c2);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn relative_inventory_does_not_use_cwd() {
        let dir = tempdir().unwrap();
        let pkg = dir.path().join("pkg");
        fs::create_dir_all(&pkg).unwrap();
        let real = pkg.join("mail.pst");
        fs::write(&real, b"!BDNfake").unwrap();
        // CWD decoy
        let decoy = dir.path().join("mail.pst");
        fs::write(&decoy, b"decoy").unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let found = candidate_fs_path(pkg.to_str().unwrap(), "mail.pst");
        std::env::set_current_dir(prev).unwrap();
        let found = found.expect("package relative");
        assert!(
            found.as_str().replace('\\', "/").ends_with("pkg/mail.pst")
                || found.as_str().ends_with("pkg\\mail.pst")
        );
        assert_ne!(found.as_str(), decoy.to_str().unwrap());
    }

    #[test]
    fn absolute_inventory_path_accepted() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("abs.pst");
        fs::write(&f, b"x").unwrap();
        let abs = f.canonicalize().unwrap();
        let found = candidate_fs_path("C:\\nope", abs.to_str().unwrap()).expect("abs");
        assert!(found.as_std_path().is_file());
    }
}
