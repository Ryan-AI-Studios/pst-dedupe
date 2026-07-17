//! Best-effort package kind detection.

use std::fs::File;
use std::io::Read;

use camino::Utf8Path;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Stable package kind strings stored on `sources.kind` and audit params.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    SinglePst,
    SingleZip,
    PurviewPackage,
    RawDump,
    Unsupported,
}

impl PackageKind {
    /// Wire / DB representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SinglePst => "single_pst",
            Self::SingleZip => "single_zip",
            Self::PurviewPackage => "purview_package",
            Self::RawDump => "raw_dump",
            Self::Unsupported => "unsupported",
        }
    }
}

impl std::fmt::Display for PackageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Detector output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectResult {
    pub kind: PackageKind,
    /// Human-readable notes (heuristics fired).
    pub notes: Vec<String>,
}

/// Classify a user-selected path into a package kind.
///
/// Heuristics are best-effort: prefer `raw_dump` over false `purview_package`.
pub fn detect(path: &Utf8Path) -> Result<DetectResult> {
    if !path.as_std_path().exists() {
        return Ok(DetectResult {
            kind: PackageKind::Unsupported,
            notes: vec![format!("path does not exist: {path}")],
        });
    }

    // Evidence boundary: do not follow symlink/reparse roots.
    if let Err(e) = crate::path_safety::reject_symlink_or_reparse(path) {
        return Ok(DetectResult {
            kind: PackageKind::Unsupported,
            notes: vec![format!("rejected: {e}")],
        });
    }

    let meta = std::fs::symlink_metadata(path.as_std_path())?;
    if meta.is_file() {
        return detect_file(path);
    }
    if meta.is_dir() {
        return detect_directory(path);
    }

    Ok(DetectResult {
        kind: PackageKind::Unsupported,
        notes: vec!["path is neither file nor directory".into()],
    })
}

fn detect_file(path: &Utf8Path) -> Result<DetectResult> {
    let name = path.file_name().unwrap_or("").to_ascii_lowercase();
    if name.ends_with(".pst") {
        let mut notes = vec!["extension .pst".into()];
        if looks_like_pst_magic(path)? {
            notes.push("MS-PST magic present".into());
        }
        return Ok(DetectResult {
            kind: PackageKind::SinglePst,
            notes,
        });
    }
    if name.ends_with(".zip") || looks_like_zip_magic(path)? {
        return Ok(DetectResult {
            kind: PackageKind::SingleZip,
            notes: vec!["zip file".into()],
        });
    }
    if name.ends_with(".7z") {
        return Ok(DetectResult {
            kind: PackageKind::Unsupported,
            notes: vec!["7z container not expanded in 0016".into()],
        });
    }
    Ok(DetectResult {
        kind: PackageKind::Unsupported,
        notes: vec![format!("unsupported single file: {name}")],
    })
}

fn detect_directory(path: &Utf8Path) -> Result<DetectResult> {
    let mut notes = Vec::new();
    let mut has_pst = false;
    let mut has_zip = false;
    let mut has_report = false;
    let mut has_exchange_like = false;
    let mut file_count = 0u64;

    // Shallow + one-level walk for heuristics (not a full tree inventory).
    for entry in walkdir_shallow(path, 3)? {
        file_count += 1;
        let lower = entry.to_ascii_lowercase();
        if lower.ends_with(".pst") {
            has_pst = true;
        }
        if lower.ends_with(".zip") {
            has_zip = true;
        }
        if lower.ends_with(".csv") || lower.ends_with(".xml") {
            let base = Utf8Path::new(&entry)
                .file_name()
                .unwrap_or("")
                .to_ascii_lowercase();
            if base.contains("export")
                || base.contains("summary")
                || base.contains("manifest")
                || base.contains("report")
            {
                has_report = true;
            }
        }
        if lower.contains("exchange") || lower.contains("sharepoint") || lower.contains("custodian")
        {
            has_exchange_like = true;
        }
    }

    if file_count == 0 {
        return Ok(DetectResult {
            kind: PackageKind::Unsupported,
            notes: vec!["empty directory".into()],
        });
    }

    // Purview-ish: PST and/or nested zips plus export noise.
    let purview_score =
        (has_pst as u8) + (has_zip as u8) + (has_report as u8) + (has_exchange_like as u8);
    if purview_score >= 2 && (has_pst || has_zip) {
        if has_pst {
            notes.push("contains .pst".into());
        }
        if has_zip {
            notes.push("contains .zip".into());
        }
        if has_report {
            notes.push("export/report csv/xml present".into());
        }
        if has_exchange_like {
            notes.push("Exchange/SharePoint/custodian-like names".into());
        }
        return Ok(DetectResult {
            kind: PackageKind::PurviewPackage,
            notes,
        });
    }

    notes.push(format!(
        "directory with {file_count} files (no strong Purview signal)"
    ));
    Ok(DetectResult {
        kind: PackageKind::RawDump,
        notes,
    })
}

fn walkdir_shallow(root: &Utf8Path, max_depth: usize) -> Result<Vec<String>> {
    let mut out = Vec::new();
    fn rec(dir: &Utf8Path, depth: usize, max_depth: usize, out: &mut Vec<String>) -> Result<()> {
        if depth > max_depth {
            return Ok(());
        }
        let rd = std::fs::read_dir(dir.as_std_path()).map_err(Error::Io)?;
        for entry in rd {
            let entry = entry?;
            let path = entry.path();
            let is_dir = entry.file_type()?.is_dir();
            if let Some(s) = path.to_str() {
                out.push(s.to_string());
                if is_dir {
                    rec(Utf8Path::new(s), depth + 1, max_depth, out)?;
                }
            }
        }
        Ok(())
    }
    rec(root, 0, max_depth, &mut out)?;
    Ok(out)
}

fn looks_like_zip_magic(path: &Utf8Path) -> Result<bool> {
    let mut f = File::open(path.as_std_path())?;
    let mut buf = [0u8; 4];
    let n = f.read(&mut buf)?;
    if n < 2 {
        return Ok(false);
    }
    // Local file header / EOCD / spanning
    Ok(&buf[..2] == b"PK")
}

fn looks_like_pst_magic(path: &Utf8Path) -> Result<bool> {
    let mut f = File::open(path.as_std_path())?;
    let mut buf = [0u8; 4];
    let n = f.read(&mut buf)?;
    if n < 4 {
        return Ok(false);
    }
    // !BDN little-endian magic used by Unicode PST
    Ok(&buf == b"!BDN")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn utf8_tmp() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let d = tempdir().unwrap();
        let p = camino::Utf8PathBuf::from_path_buf(d.path().to_path_buf()).unwrap();
        (d, p)
    }

    #[test]
    fn detect_single_pst_by_extension() {
        let (_t, base) = utf8_tmp();
        let pst = base.join("mail.pst");
        fs::write(pst.as_std_path(), b"!BDNdummy").unwrap();
        let r = detect(&pst).unwrap();
        assert_eq!(r.kind, PackageKind::SinglePst);
    }

    #[test]
    fn detect_single_zip() {
        let (_t, base) = utf8_tmp();
        let z = base.join("a.zip");
        fs::write(z.as_std_path(), b"PK\x03\x04rest").unwrap();
        let r = detect(&z).unwrap();
        assert_eq!(r.kind, PackageKind::SingleZip);
    }

    #[test]
    fn detect_purview_package() {
        let (_t, base) = utf8_tmp();
        let pkg = base.join("export");
        fs::create_dir_all(pkg.as_std_path()).unwrap();
        fs::write(pkg.join("mail.pst").as_std_path(), b"!BDNx").unwrap();
        fs::write(pkg.join("files.zip").as_std_path(), b"PK\x03\x04").unwrap();
        fs::write(pkg.join("ExportSummary.csv").as_std_path(), b"a,b\n").unwrap();
        let r = detect(&pkg).unwrap();
        assert_eq!(r.kind, PackageKind::PurviewPackage);
    }

    #[test]
    fn detect_raw_dump() {
        let (_t, base) = utf8_tmp();
        let pkg = base.join("misc");
        fs::create_dir_all(pkg.as_std_path()).unwrap();
        fs::write(pkg.join("notes.txt").as_std_path(), b"hi").unwrap();
        let r = detect(&pkg).unwrap();
        assert_eq!(r.kind, PackageKind::RawDump);
    }

    #[test]
    fn detect_missing_unsupported() {
        let (_t, base) = utf8_tmp();
        let r = detect(&base.join("nope")).unwrap();
        assert_eq!(r.kind, PackageKind::Unsupported);
    }
}
