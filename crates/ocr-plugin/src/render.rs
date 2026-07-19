//! Optional PDF page render sidecar (pdftoppm / mutool) — one page at a time.

use std::path::Path;
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{Error, Result};
use crate::temp::OcrTempFile;

/// Known PDF page renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfRendererKind {
    /// Poppler `pdftoppm`.
    Pdftoppm,
    /// MuPDF `mutool draw`.
    Mutool,
}

/// Resolved PDF page renderer.
#[derive(Debug, Clone)]
pub struct PdfRenderer {
    pub kind: PdfRendererKind,
    pub exe: Utf8PathBuf,
}

impl PdfRenderer {
    /// Discover renderer: settings path → PATH (`pdftoppm`, then `mutool`).
    pub fn discover(settings_path: Option<&str>) -> Result<Self> {
        if let Some(p) = settings_path.map(str::trim).filter(|s| !s.is_empty()) {
            let path = Path::new(p);
            if !path.is_file() {
                return Err(Error::PdfRendererMissing(format!(
                    "pdf_renderer_path not found: {p}"
                )));
            }
            let utf8 = Utf8PathBuf::from_path_buf(path.to_path_buf()).map_err(|_| {
                Error::PdfRendererMissing(format!("pdf_renderer_path is not UTF-8: {p}"))
            })?;
            let name = utf8.file_name().unwrap_or("").to_ascii_lowercase();
            let kind = if name.contains("mutool") {
                PdfRendererKind::Mutool
            } else {
                // Default treat as pdftoppm (poppler).
                PdfRendererKind::Pdftoppm
            };
            return Ok(Self { kind, exe: utf8 });
        }
        if let Some(found) = find_on_path("pdftoppm") {
            return Ok(Self {
                kind: PdfRendererKind::Pdftoppm,
                exe: found,
            });
        }
        if let Some(found) = find_on_path("mutool") {
            return Ok(Self {
                kind: PdfRendererKind::Mutool,
                exe: found,
            });
        }
        Err(Error::PdfRendererMissing(
            "no pdftoppm or mutool found (set Settings path or install Poppler/MuPDF)".into(),
        ))
    }

    /// Render a single 1-based page to a Drop-guarded PNG temp under matter OCR temp.
    ///
    /// Caller must drop the returned guard before rendering the next page.
    pub fn render_page(
        &self,
        matter_root: &Utf8Path,
        pdf_path: &Utf8Path,
        page: u32,
        dpi: u32,
    ) -> Result<OcrTempFile> {
        match self.kind {
            PdfRendererKind::Pdftoppm => self.render_pdftoppm(matter_root, pdf_path, page, dpi),
            PdfRendererKind::Mutool => self.render_mutool(matter_root, pdf_path, page, dpi),
        }
    }

    fn render_pdftoppm(
        &self,
        matter_root: &Utf8Path,
        pdf_path: &Utf8Path,
        page: u32,
        dpi: u32,
    ) -> Result<OcrTempFile> {
        // pdftoppm writes `<prefix>.png` (-singlefile) or `<prefix>-<page>.png`.
        // Own *all* candidate output paths with RAII from before spawn so panic /
        // error cannot orphan ESI bitmaps (spec §3.9.1).
        let dir = crate::temp::ensure_ocr_temp_dir(matter_root)?;
        let mut guard = PdftoppmOutGuard::new(&dir, page)?;
        let mut cmd = Command::new(self.exe.as_str());
        cmd.args([
            "-png",
            "-r",
            &dpi.to_string(),
            "-f",
            &page.to_string(),
            "-l",
            &page.to_string(),
            "-singlefile",
            pdf_path.as_str(),
            guard.prefix.as_str(),
        ]);
        scrub_proxy(&mut cmd);
        let out = cmd
            .output()
            .map_err(|e| Error::Engine(format!("spawn pdftoppm: {e}")))?;
        if !out.status.success() {
            return Err(Error::Engine(format!(
                "pdftoppm failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let produced = guard
            .find_produced()
            .ok_or_else(|| Error::Engine("pdftoppm produced no PNG".into()))?;
        // Load into NamedTempFile-backed OcrTempFile; guard Drop still cleans
        // any intermediate if load fails or panics mid-copy.
        let result = load_into_temp(matter_root, &produced);
        // Successful re-home: disarm only the path we already deleted in load.
        if result.is_ok() {
            guard.forget_path(&produced);
        }
        result
    }

    fn render_mutool(
        &self,
        matter_root: &Utf8Path,
        pdf_path: &Utf8Path,
        page: u32,
        dpi: u32,
    ) -> Result<OcrTempFile> {
        let mut temp = OcrTempFile::new_in(matter_root, ".png")?;
        // mutool draw -o out.png -r DPI -F png in.pdf N
        let mut cmd = Command::new(self.exe.as_str());
        cmd.args([
            "draw",
            "-o",
            temp.path().as_str(),
            "-r",
            &dpi.to_string(),
            "-F",
            "png",
            pdf_path.as_str(),
            &page.to_string(),
        ]);
        scrub_proxy(&mut cmd);
        let out = cmd
            .output()
            .map_err(|e| Error::Engine(format!("spawn mutool: {e}")))?;
        if !out.status.success() {
            return Err(Error::Engine(format!(
                "mutool draw failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        // Touch path for existence check.
        if !temp.std_path().is_file() {
            return Err(Error::Engine("mutool produced no PNG".into()));
        }
        // re-open not needed; file already at temp path. Silence unused mut.
        let _ = &mut temp;
        Ok(temp)
    }
}

fn load_into_temp(matter_root: &Utf8Path, produced: &Path) -> Result<OcrTempFile> {
    let bytes = std::fs::read(produced)?;
    // Delete source after successful read (caller may also Drop-guard).
    let _ = std::fs::remove_file(produced);
    let mut temp = OcrTempFile::new_in(matter_root, ".png")?;
    temp.write_all(&bytes)?;
    Ok(temp)
}

/// RAII owner for pdftoppm output paths under the matter OCR temp directory.
///
/// Tracks both `-singlefile` (`prefix.png`) and multi-page (`prefix-N.png`)
/// naming so a crash between spawn and re-home cannot leave bare ESI PNGs.
struct PdftoppmOutGuard {
    prefix: Utf8PathBuf,
    tracked: Vec<std::path::PathBuf>,
}

impl PdftoppmOutGuard {
    fn new(dir: &Utf8Path, page: u32) -> Result<Self> {
        let unique = format!(
            "pdftoppm_{page}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let prefix = dir.join(&unique);
        let single = Path::new(prefix.as_str()).with_extension("png");
        let multi = Path::new(&format!("{}-{page}.png", prefix.as_str())).to_path_buf();
        Ok(Self {
            prefix,
            tracked: vec![single, multi],
        })
    }

    fn find_produced(&self) -> Option<std::path::PathBuf> {
        self.tracked.iter().find(|p| p.is_file()).cloned()
    }

    fn forget_path(&mut self, path: &Path) {
        self.tracked.retain(|p| p != path);
    }
}

impl Drop for PdftoppmOutGuard {
    fn drop(&mut self) {
        for p in &self.tracked {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn find_on_path(name: &str) -> Option<Utf8PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Utf8PathBuf::from_path_buf(candidate).ok();
        }
        #[cfg(windows)]
        {
            let with_exe = dir.join(format!("{name}.exe"));
            if with_exe.is_file() {
                return Utf8PathBuf::from_path_buf(with_exe).ok();
            }
        }
    }
    None
}

fn scrub_proxy(cmd: &mut Command) {
    cmd.env_remove("HTTP_PROXY");
    cmd.env_remove("HTTPS_PROXY");
    cmd.env_remove("http_proxy");
    cmd.env_remove("https_proxy");
    cmd.env_remove("ALL_PROXY");
}
