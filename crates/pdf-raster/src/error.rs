//! Typed errors for raster and burn.

use thiserror::Error;

/// Result alias for pdf-raster operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors returned by raster, burn, and search helpers.
#[derive(Debug, Error)]
pub enum Error {
    #[error("native exceeds 100 MiB cap")]
    TooLarge { bytes: u64 },
    #[error("page count {count} exceeds 500-page cap")]
    TooManyPages { count: usize },
    #[error("pdf_encrypted")]
    Encrypted,
    #[error("not a page image (native-only; no print-to-TIFF)")]
    UnsupportedKind,
    #[error("g4 encode failed")]
    G4EncodeFailed,
    #[error("tiff ifd write failed: {0}")]
    TiffIfd(String),
    #[error("page index {index} out of range (page_count={count})")]
    PageOutOfRange { index: u32, count: u32 },
    #[error("jpeg encode failed; refuse codec swap")]
    JpegEncodeFailed,
    #[error("png encode failed")]
    PngEncodeFailed,
    #[error("image decode failed: {0}")]
    ImageDecode(String),
    #[error("pdf raster failed: {0}")]
    Raster(String),
    #[error("pdf burn failed: {0}")]
    Burn(String),
    #[error("corrupt or unreadable PDF: {0}")]
    Corrupt(String),
    #[error("pdf engine panicked")]
    Panicked,
}

impl Error {
    /// Stable kind string for chrome extras / empty states.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::TooLarge { .. } => "too_large",
            Error::TooManyPages { .. } => "too_many_pages",
            Error::Encrypted => "pdf_encrypted",
            Error::UnsupportedKind => "unsupported_kind",
            Error::G4EncodeFailed => "g4_encode_failed",
            Error::TiffIfd(_) => "tiff_ifd",
            Error::PageOutOfRange { .. } => "page_out_of_range",
            Error::JpegEncodeFailed => "jpeg_encode_failed",
            Error::PngEncodeFailed => "png_encode_failed",
            Error::ImageDecode(_) => "image_decode",
            Error::Raster(_) => "pdf_raster_failed",
            Error::Burn(_) => "pdf_burn_failed",
            Error::Corrupt(_) => "corrupt",
            Error::Panicked => "panicked",
        }
    }
}
