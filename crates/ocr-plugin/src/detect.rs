//! Image vs PDF candidate sniffing for OCR.

/// Image extensions eligible for OCR (lowercase, no dot).
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "webp"];

/// True when path/mime/file_category look like an OCR-able image.
pub fn is_image_meta(path: Option<&str>, mime: Option<&str>, file_category: Option<&str>) -> bool {
    if file_category
        .map(|c| c.eq_ignore_ascii_case("image"))
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(m) = mime {
        let m = m.to_ascii_lowercase();
        if m.starts_with("image/png")
            || m.starts_with("image/jpeg")
            || m.starts_with("image/jpg")
            || m.starts_with("image/tiff")
            || m.starts_with("image/webp")
        {
            return true;
        }
    }
    if let Some(p) = path {
        let lower = p.to_ascii_lowercase();
        for ext in IMAGE_EXTS {
            if lower.ends_with(&format!(".{ext}")) {
                return true;
            }
        }
    }
    false
}

/// True when path/mime/file_category look like a PDF.
pub fn is_pdf_meta(path: Option<&str>, mime: Option<&str>, file_category: Option<&str>) -> bool {
    if file_category
        .map(|c| c.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
    {
        return true;
    }
    if mime
        .map(|m| m.to_ascii_lowercase().starts_with("application/pdf"))
        .unwrap_or(false)
    {
        return true;
    }
    path.map(|p| p.to_ascii_lowercase().ends_with(".pdf"))
        .unwrap_or(false)
}

/// PDF magic (`%PDF`).
pub fn looks_like_pdf(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[0..4] == b"%PDF"
}

/// PNG magic.
pub fn looks_like_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[0..8] == [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']
}

/// JPEG magic.
pub fn looks_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_meta_path() {
        assert!(is_image_meta(Some("a.PNG"), None, None));
        assert!(is_image_meta(Some("x.jpeg"), None, None));
        assert!(!is_image_meta(Some("a.pdf"), None, None));
    }

    #[test]
    fn pdf_meta() {
        assert!(is_pdf_meta(Some("a.pdf"), None, None));
        assert!(is_pdf_meta(None, Some("application/pdf"), None));
        assert!(!is_pdf_meta(Some("a.png"), None, None));
    }
}
