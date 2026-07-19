//! Tesseract CLI sidecar engine (default `--psm 1` OSD).

use std::path::{Path, PathBuf};
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};

use super::trait_::{OcrEngine, OcrPageResult};
use crate::error::{Error, Result};
use crate::limits::engines;

/// Default page segmentation mode: Automatic page segmentation **with OSD**.
pub const DEFAULT_PSM: u32 = 1;

/// Tesseract executable sidecar.
#[derive(Debug, Clone)]
pub struct TesseractCliEngine {
    /// Resolved executable path.
    pub exe: Utf8PathBuf,
    /// Optional tessdata directory (`TESSDATA_PREFIX`).
    pub tessdata_dir: Option<Utf8PathBuf>,
    /// Page segmentation mode (default 1).
    pub psm: u32,
    /// When true, skip osd traineddata preflight (tests only).
    pub skip_osd_preflight: bool,
}

impl TesseractCliEngine {
    /// Discover Tesseract: settings path → PATH lookup.
    pub fn discover(
        settings_path: Option<&str>,
        tessdata_dir: Option<&str>,
        psm: u32,
    ) -> Result<Self> {
        let exe = resolve_tesseract(settings_path)?;
        let tessdata_dir = tessdata_dir
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(Utf8PathBuf::from);
        Ok(Self {
            exe,
            tessdata_dir,
            psm,
            skip_osd_preflight: false,
        })
    }

    /// Build argv for an image OCR invocation (unit-testable).
    ///
    /// Default includes `--psm 1` for OSD. Output goes to stdout (`stdout`).
    pub fn build_ocr_args(image: &Utf8Path, lang: &str, psm: u32) -> Vec<String> {
        vec![
            image.as_str().to_string(),
            "stdout".into(),
            "-l".into(),
            lang.to_string(),
            "--psm".into(),
            psm.to_string(),
        ]
    }

    /// Preflight: require `osd` traineddata when using PSM 1 (or any OSD mode).
    ///
    /// Does **not** silently fall back to PSM 3/6.
    pub fn preflight_osd(&self) -> Result<()> {
        if self.skip_osd_preflight || self.psm != 1 {
            return Ok(());
        }
        // Probe by listing tessdata or checking known paths. Prefer a cheap
        // existence check under tessdata_dir / TESSDATA_PREFIX / common install.
        if let Some(dir) = &self.tessdata_dir {
            let osd = dir.join("osd.traineddata");
            if osd.as_std_path().is_file() {
                return Ok(());
            }
            return Err(Error::OsdMissing(format!(
                "osd.traineddata not found under tessdata_dir={dir}; install tesseract-ocr-osd / full tessdata"
            )));
        }
        // Without an explicit tessdata dir, try env and common relative probes via
        // `tesseract --list-langs` which fails closed if osd is required later.
        // We still attempt to locate osd under TESSDATA_PREFIX.
        if let Ok(prefix) = std::env::var("TESSDATA_PREFIX") {
            let p = PathBuf::from(prefix);
            let candidates = [
                p.join("osd.traineddata"),
                p.join("tessdata").join("osd.traineddata"),
            ];
            if candidates.iter().any(|c| c.is_file()) {
                return Ok(());
            }
        }
        // Soft preflight when no tessdata path is known: allow start; page OCR
        // will surface Tesseract errors. Documented: operators should set
        // tessdata_dir when using custom installs. When tessdata_dir *is* set,
        // we fail closed above.
        Ok(())
    }

    fn apply_env(&self, cmd: &mut Command) {
        if let Some(dir) = &self.tessdata_dir {
            cmd.env("TESSDATA_PREFIX", dir.as_str());
        }
        // Best-effort: scrub proxy vars (no network OCR).
        cmd.env_remove("HTTP_PROXY");
        cmd.env_remove("HTTPS_PROXY");
        cmd.env_remove("http_proxy");
        cmd.env_remove("https_proxy");
        cmd.env_remove("ALL_PROXY");
    }
}

impl OcrEngine for TesseractCliEngine {
    fn id(&self) -> &str {
        engines::TESSERACT_CLI
    }

    fn version(&self) -> Result<String> {
        let mut cmd = Command::new(self.exe.as_str());
        cmd.arg("--version");
        self.apply_env(&mut cmd);
        let out = cmd
            .output()
            .map_err(|e| Error::EngineNotFound(format!("failed to run {}: {e}", self.exe)))?;
        if !out.status.success() {
            return Err(Error::Engine(format!(
                "tesseract --version failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let first = text.lines().next().unwrap_or("tesseract").trim();
        Ok(first.to_string())
    }

    fn ocr_image(&self, path: &Utf8Path, lang: &str) -> Result<OcrPageResult> {
        let args = Self::build_ocr_args(path, lang, self.psm);
        let mut cmd = Command::new(self.exe.as_str());
        cmd.args(&args);
        self.apply_env(&mut cmd);
        let out = cmd
            .output()
            .map_err(|e| Error::Engine(format!("spawn tesseract: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // Detect missing OSD and map to honest code (no silent PSM fallback).
            if stderr.to_ascii_lowercase().contains("osd")
                && (stderr.contains("Error")
                    || stderr.contains("failed")
                    || stderr.contains("Error opening"))
            {
                return Err(Error::OsdMissing(stderr.trim().to_string()));
            }
            return Err(Error::Engine(format!(
                "tesseract failed ({}): {}",
                out.status,
                stderr.trim()
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        Ok(OcrPageResult {
            text,
            confidence: None,
        })
    }
}

fn resolve_tesseract(settings_path: Option<&str>) -> Result<Utf8PathBuf> {
    if let Some(p) = settings_path.map(str::trim).filter(|s| !s.is_empty()) {
        let path = Path::new(p);
        if path.is_file() {
            return Utf8PathBuf::from_path_buf(path.to_path_buf())
                .map_err(|_| Error::EngineNotFound(format!("tesseract path is not UTF-8: {p}")));
        }
        return Err(Error::EngineNotFound(format!(
            "tesseract_path not found: {p}"
        )));
    }
    // PATH lookup
    if let Some(found) = find_on_path("tesseract") {
        return Utf8PathBuf::from_path_buf(found)
            .map_err(|_| Error::EngineNotFound("tesseract on PATH is not UTF-8".into()));
    }
    #[cfg(windows)]
    {
        if let Some(found) = find_on_path("tesseract.exe") {
            return Utf8PathBuf::from_path_buf(found)
                .map_err(|_| Error::EngineNotFound("tesseract.exe on PATH is not UTF-8".into()));
        }
    }
    Err(Error::EngineNotFound(
        "tesseract not found (set Settings path or install and add to PATH)".into(),
    ))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            // PATHEXT may already be covered by is_file for .exe; also try bare.
            let with_exe = dir.join(format!("{name}.exe"));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
    }
    None
}

/// Public helper for tests: default argv includes `--psm 1`.
pub fn default_ocr_argv(image: &str, lang: &str) -> Vec<String> {
    TesseractCliEngine::build_ocr_args(Utf8Path::new(image), lang, DEFAULT_PSM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::codes;

    #[test]
    fn discover_missing_settings_path_is_engine_not_found() {
        let missing = if cfg!(windows) {
            r"C:\nonexistent\ocr-plugin-tesseract-missing\tesseract.exe"
        } else {
            "/nonexistent/ocr-plugin-tesseract-missing/tesseract"
        };
        let err = TesseractCliEngine::discover(Some(missing), None, DEFAULT_PSM)
            .expect_err("missing path must fail");
        assert_eq!(err.code(), codes::OCR_ENGINE_NOT_FOUND);
        let msg = err.to_string().to_ascii_lowercase();
        assert!(
            msg.contains("not found") || msg.contains("tesseract"),
            "honest message: {err}"
        );
    }

    #[test]
    fn default_argv_includes_psm_1() {
        let args = default_ocr_argv("page.png", "eng");
        assert!(args.contains(&"--psm".into()), "args={args:?}");
        let psm_idx = args.iter().position(|a| a == "--psm").unwrap();
        assert_eq!(args.get(psm_idx + 1).map(String::as_str), Some("1"));
        assert!(args.contains(&"-l".into()));
        assert!(args.contains(&"eng".into()));
        assert!(args.contains(&"stdout".into()));
        // Must not default to bare PSM 3/6 without OSD.
        assert!(!args
            .windows(2)
            .any(|w| w[0] == "--psm" && (w[1] == "3" || w[1] == "6")));
    }

    #[test]
    fn custom_psm_respected() {
        let args = TesseractCliEngine::build_ocr_args(Utf8Path::new("x.png"), "eng+spa", 6);
        let psm_idx = args.iter().position(|a| a == "--psm").unwrap();
        assert_eq!(args.get(psm_idx + 1).map(String::as_str), Some("6"));
        assert!(args.contains(&"eng+spa".into()));
    }

    #[test]
    fn osd_preflight_fails_when_tessdata_set_without_osd() {
        let dir = tempfile::tempdir().unwrap();
        let eng = TesseractCliEngine {
            exe: Utf8PathBuf::from("tesseract"),
            tessdata_dir: Some(Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8")),
            psm: 1,
            skip_osd_preflight: false,
        };
        let err = eng.preflight_osd().unwrap_err();
        assert_eq!(err.code(), codes::OCR_OSD_MISSING);
    }

    #[test]
    fn osd_preflight_ok_when_osd_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("osd.traineddata"), b"fake").unwrap();
        let eng = TesseractCliEngine {
            exe: Utf8PathBuf::from("tesseract"),
            tessdata_dir: Some(Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8")),
            psm: 1,
            skip_osd_preflight: false,
        };
        eng.preflight_osd().unwrap();
    }
}
