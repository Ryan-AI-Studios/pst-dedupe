//! OCR engines (Tesseract CLI + mock).

// `trait` is a keyword; module lives in trait.rs via path attribute.
#[path = "trait.rs"]
mod trait_;

pub mod mock;
pub mod tesseract;

pub use mock::MockOcrEngine;
pub use tesseract::{default_ocr_argv, TesseractCliEngine, DEFAULT_PSM};
pub use trait_::{OcrEngine, OcrPageResult};
