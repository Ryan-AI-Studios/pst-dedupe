//! Mock OCR engine for CI (no system Tesseract).

use camino::Utf8Path;

use super::trait_::{OcrEngine, OcrPageResult};
use crate::error::Result;
use crate::limits::engines;

/// Fixed-text mock engine used by default tests.
///
/// Returns deterministic text for any image path so integration tests can assert
/// CAS digests without installing Tesseract.
#[derive(Debug)]
pub struct MockOcrEngine {
    /// Fixed text returned for every image.
    pub text: String,
    /// Optional confidence.
    pub confidence: Option<f64>,
    /// Optional page texts for multi-page simulation (index by call order).
    pub page_texts: Vec<String>,
    call_count: std::sync::atomic::AtomicUsize,
}

impl Default for MockOcrEngine {
    fn default() -> Self {
        Self {
            text: "MOCK_OCR_TEXT hello world".into(),
            confidence: Some(0.95),
            page_texts: Vec::new(),
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl MockOcrEngine {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            confidence: Some(0.95),
            page_texts: Vec::new(),
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn with_pages(pages: Vec<String>) -> Self {
        Self {
            text: String::new(),
            confidence: Some(0.9),
            page_texts: pages,
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl OcrEngine for MockOcrEngine {
    fn id(&self) -> &str {
        engines::MOCK
    }

    fn version(&self) -> Result<String> {
        Ok("mock-1.0".into())
    }

    fn ocr_image(&self, path: &Utf8Path, _lang: &str) -> Result<OcrPageResult> {
        // Ensure the path exists so Drop-guard lifecycle tests exercise real files.
        if !path.as_std_path().exists() {
            return Err(crate::error::Error::Engine(format!(
                "mock: image not found: {path}"
            )));
        }
        let n = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let text = if !self.page_texts.is_empty() {
            self.page_texts
                .get(n)
                .cloned()
                .unwrap_or_else(|| self.page_texts.last().cloned().unwrap_or_default())
        } else {
            self.text.clone()
        };
        Ok(OcrPageResult {
            text,
            confidence: self.confidence,
        })
    }
}
