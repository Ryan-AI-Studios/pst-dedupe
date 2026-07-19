//! Job params for `ocr`.

use serde::{Deserialize, Serialize};

use crate::limits::{DEFAULT_DPI, MAX_PAGES};

/// JSON params for kind `"ocr"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrParams {
    /// Re-OCR even when already ok for the same native (default false).
    #[serde(default)]
    pub force: bool,
    /// Items between cancel checks / checkpoint writes (default 20).
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Tesseract language pack string (default `eng`).
    #[serde(default = "default_lang")]
    pub lang: String,
    /// Max pages per document (default 500).
    #[serde(default = "default_max_pages")]
    pub max_pages: usize,
    /// PDF render DPI (default 200).
    #[serde(default = "default_dpi")]
    pub dpi: u32,
    /// Desk enable gate — job fails closed when false.
    #[serde(default)]
    pub enabled: bool,
    /// Optional path to `tesseract` / `tesseract.exe`.
    #[serde(default)]
    pub tesseract_path: Option<String>,
    /// Optional tessdata directory (`TESSDATA_PREFIX`).
    #[serde(default)]
    pub tessdata_dir: Option<String>,
    /// Optional path to `pdftoppm` or `mutool`.
    #[serde(default)]
    pub pdf_renderer_path: Option<String>,
    /// Engine selector: production accepts only `"tesseract"`.
    /// `"mock"` is rejected on the production path; tests inject via
    /// `run_ocr_with_engine`.
    #[serde(default = "default_engine")]
    pub engine: String,
    /// Advanced PSM override (default 1 = OSD). Not for product default changes.
    #[serde(default = "default_psm")]
    pub psm: u32,
}

fn default_batch_size() -> usize {
    20
}

fn default_lang() -> String {
    "eng".into()
}

fn default_max_pages() -> usize {
    MAX_PAGES
}

fn default_dpi() -> u32 {
    DEFAULT_DPI
}

fn default_engine() -> String {
    "tesseract".into()
}

fn default_psm() -> u32 {
    1
}

impl Default for OcrParams {
    fn default() -> Self {
        Self {
            force: false,
            batch_size: default_batch_size(),
            lang: default_lang(),
            max_pages: default_max_pages(),
            dpi: default_dpi(),
            enabled: false,
            tesseract_path: None,
            tessdata_dir: None,
            pdf_renderer_path: None,
            engine: default_engine(),
            psm: default_psm(),
        }
    }
}

impl OcrParams {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.batch_size == 0 {
            return Err("batch_size must be >= 1".into());
        }
        if self.max_pages == 0 {
            return Err("max_pages must be >= 1".into());
        }
        if self.dpi == 0 || self.dpi > 600 {
            return Err("dpi must be 1..=600".into());
        }
        if self.lang.trim().is_empty() {
            return Err("lang must be non-empty".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_fail_closed_disabled() {
        let p = OcrParams::from_json("{}").unwrap();
        assert!(!p.enabled);
        assert!(!p.force);
        assert_eq!(p.batch_size, 20);
        assert_eq!(p.lang, "eng");
        assert_eq!(p.max_pages, 500);
        assert_eq!(p.dpi, 200);
        assert_eq!(p.engine, "tesseract");
        assert_eq!(p.psm, 1);
        p.validate().unwrap();
    }

    #[test]
    fn enabled_roundtrip() {
        let j = r#"{
            "force": false,
            "batch_size": 20,
            "lang": "eng",
            "max_pages": 500,
            "dpi": 200,
            "enabled": true,
            "tesseract_path": null,
            "tessdata_dir": null,
            "pdf_renderer_path": null,
            "engine": "mock"
        }"#;
        let p = OcrParams::from_json(j).unwrap();
        assert!(p.enabled);
        assert_eq!(p.engine, "mock");
    }
}
