//! PDF eligibility and sniff helpers.

/// True when path extension is `.pdf` (case-insensitive).
pub fn from_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let leaf = lower.rsplit(['/', '\\', '!']).next().unwrap_or(&lower);
    leaf.ends_with(".pdf")
}

/// True when mime looks like PDF.
pub fn from_mime(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m == "application/pdf" || m.starts_with("application/pdf;")
}

/// Sniff `%PDF-` after optional leading whitespace / UTF-8 BOM.
pub fn looks_like_pdf(bytes: &[u8]) -> bool {
    let mut i = 0usize;
    // UTF-8 BOM
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        i = 3;
    }
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    let rest = &bytes[i..];
    rest.len() >= 5 && rest.starts_with(b"%PDF-")
}

/// True when path/mime suggests a PDF-eligible item (without reading bytes).
pub fn is_pdf_eligible_meta(path: Option<&str>, mime_type: Option<&str>) -> bool {
    if let Some(p) = path {
        if from_extension(p) {
            return true;
        }
    }
    if let Some(m) = mime_type {
        if from_mime(m) {
            return true;
        }
    }
    false
}

/// Detect PDF from path, mime, and/or bytes. Returns true when eligible.
pub fn detect_pdf(path: Option<&str>, mime_type: Option<&str>, bytes: Option<&[u8]>) -> bool {
    if is_pdf_eligible_meta(path, mime_type) {
        return true;
    }
    if let Some(b) = bytes {
        return looks_like_pdf(b);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_and_mime() {
        assert!(from_extension("a.PDF"));
        assert!(from_extension(r"C:\x\doc.pdf"));
        assert!(!from_extension("a.docx"));
        assert!(from_mime("application/pdf"));
        assert!(from_mime("application/pdf; charset=binary"));
        assert!(!from_mime("text/plain"));
    }

    #[test]
    fn sniff_magic() {
        assert!(looks_like_pdf(b"%PDF-1.4\n"));
        assert!(looks_like_pdf(b"  \n%PDF-1.7"));
        assert!(looks_like_pdf(b"\xEF\xBB\xBF%PDF-1.4"));
        assert!(!looks_like_pdf(b"PK\x03\x04"));
        assert!(!looks_like_pdf(b""));
    }
}
