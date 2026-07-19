//! OCR engine trait.

use camino::Utf8Path;

use crate::error::Result;

/// Per-page OCR result.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrPageResult {
    pub text: String,
    pub confidence: Option<f64>,
}

/// Pluggable OCR backend (Tesseract CLI, mock, future engines).
pub trait OcrEngine: Send + Sync {
    /// Stable engine id (e.g. `tesseract_cli`, `mock`).
    fn id(&self) -> &str;

    /// Version string for audit (e.g. first line of `tesseract --version`).
    fn version(&self) -> Result<String>;

    /// OCR a single image file (PNG/JPEG/TIFF path).
    fn ocr_image(&self, path: &Utf8Path, lang: &str) -> Result<OcrPageResult>;
}
