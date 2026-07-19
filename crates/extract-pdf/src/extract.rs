//! Top-level PDF text extract API.

use crate::detect;
use crate::error::{Error, Result};
use crate::limits::{
    methods, status, MAX_EXTRACTED_TEXT_BYTES, MAX_NATIVE_INPUT_BYTES, MAX_PAGES,
    MIN_TEXT_CHARS_PER_PAGE, MIN_TEXT_CHARS_TOTAL, TRUNCATION_MARKER,
};
use crate::text_buf::TextBuf;

/// Classification after embedded-text extract (spec §3.4.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextClass {
    /// Non-whitespace length 0.
    Empty,
    /// Some text but below total or per-page thresholds.
    LowText,
    /// Above both thresholds (or truncated at max text).
    Ok,
}

impl TextClass {
    pub fn as_status(self) -> &'static str {
        match self {
            Self::Empty => status::EMPTY,
            Self::LowText => status::LOW_TEXT,
            Self::Ok => status::OK,
        }
    }

    pub fn needs_ocr(self) -> bool {
        matches!(self, Self::Empty | Self::LowText)
    }
}

/// Successful extract payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedPdfText {
    pub text: String,
    pub method: String,
    /// True when output hit the text cap or page cap.
    pub partial: bool,
    pub page_count: u32,
    pub class: TextClass,
}

/// Count Unicode scalar chars that are not Unicode whitespace.
pub fn count_non_ws_chars(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count()
}

/// Classify extracted text given page count (spec thresholds).
pub fn classify_text(text: &str, page_count: u32) -> TextClass {
    let n = count_non_ws_chars(text);
    if n == 0 {
        return TextClass::Empty;
    }
    let pages = page_count.max(1) as usize;
    let per_page = n / pages;
    if n < MIN_TEXT_CHARS_TOTAL || per_page < MIN_TEXT_CHARS_PER_PAGE {
        return TextClass::LowText;
    }
    TextClass::Ok
}

/// Extract embedded plain text from PDF bytes.
pub fn extract_pdf(
    data: &[u8],
    path: Option<&str>,
    mime_type: Option<&str>,
) -> Result<ExtractedPdfText> {
    extract_pdf_with_limits(data, path, mime_type, MAX_EXTRACTED_TEXT_BYTES, MAX_PAGES)
}

/// Extract with injectable caps (tests).
pub fn extract_pdf_with_limits(
    data: &[u8],
    path: Option<&str>,
    mime_type: Option<&str>,
    max_text_bytes: usize,
    max_pages: usize,
) -> Result<ExtractedPdfText> {
    if data.len() as u64 > MAX_NATIVE_INPUT_BYTES {
        return Err(Error::limit(format!(
            "native size {} exceeds max {MAX_NATIVE_INPUT_BYTES}",
            data.len()
        )));
    }

    // Path/mime said PDF but sniff fails → still try parse if magic present;
    // pure non-PDF without magic → not_pdf.
    let meta_says_pdf = detect::is_pdf_eligible_meta(path, mime_type);
    let magic = detect::looks_like_pdf(data);
    if !meta_says_pdf && !magic {
        return Err(Error::NotPdf("bytes do not look like PDF".into()));
    }
    if meta_says_pdf && !magic && !data.is_empty() {
        // Extension said PDF but no magic — still attempt load; if it fails, not_pdf/parse.
    }
    if !magic {
        // Strict: require %PDF- for parse path to avoid feeding garbage to parser.
        return Err(Error::NotPdf("missing %PDF- magic".into()));
    }

    // Load structure for encryption + page count (lopdf via pdf-extract re-export).
    let doc = match pdf_extract::Document::load_mem(data) {
        Ok(d) => d,
        Err(e) => {
            return Err(Error::parse(format!("document load failed: {e}")));
        }
    };

    if doc.is_encrypted() {
        return Err(Error::Encrypted(
            "password-encrypted PDF (fail closed)".into(),
        ));
    }

    let pages_map = doc.get_pages();
    let total_pages = pages_map.len();
    let page_count_u32 = total_pages.min(u32::MAX as usize) as u32;

    // Prefer page-ordered extract; fall back to whole-doc extract on failure.
    let page_texts = match pdf_extract::extract_text_from_mem_by_pages(data) {
        Ok(v) => v,
        Err(e) => {
            // Fallback single blob
            match pdf_extract::extract_text_from_mem(data) {
                Ok(t) => vec![t],
                Err(e2) => {
                    return Err(Error::parse(format!(
                        "text extract failed: {e}; fallback: {e2}"
                    )));
                }
            }
        }
    };

    let mut buf = TextBuf::with_limit(max_text_bytes);
    let mut pages_used = 0usize;
    let mut page_capped = false;

    for (i, page_text) in page_texts.iter().enumerate() {
        if pages_used >= max_pages {
            page_capped = true;
            break;
        }
        if buf.is_full() {
            break;
        }
        if pages_used > 0 && !buf.push_str("\n\n") {
            break;
        }
        // Optional page marker for multi-page readability (skip when empty page).
        if page_texts.len() > 1 {
            let marker = format!("--- Page {} ---\n", i + 1);
            if !buf.push_str(&marker) {
                break;
            }
        }
        if !buf.push_str(page_text) {
            break;
        }
        pages_used += 1;
    }

    if pages_used == 0 && page_texts.is_empty() {
        // No pages extracted — still record page_count from structure.
    }

    let (text, text_partial) = buf.into_string();
    let partial = text_partial || page_capped;
    // Prefer structural page count; fall back to pages we saw in text extract.
    let page_count = if page_count_u32 > 0 {
        page_count_u32
    } else {
        pages_used.min(u32::MAX as usize) as u32
    };

    // Classification uses full accumulated text (including truncation marker
    // bytes as content — marker is non-ws so truncated docs rarely classify low).
    let class = classify_text(&text, page_count.max(1));

    // Empty: return empty text with Empty class (caller leaves text_sha256 NULL).
    if class == TextClass::Empty {
        return Ok(ExtractedPdfText {
            text: String::new(),
            method: methods::PDF_EXTRACT_V1.into(),
            partial,
            page_count,
            class,
        });
    }

    // Ensure truncation marker constant is linked for callers/tests.
    let _ = TRUNCATION_MARKER;

    Ok(ExtractedPdfText {
        text,
        method: methods::PDF_EXTRACT_V1.into(),
        partial,
        page_count,
        class,
    })
}

/// Panic-isolating wrapper for job use. Converts panics to parse errors.
pub fn extract_pdf_catch_unwind(
    data: &[u8],
    path: Option<&str>,
    mime_type: Option<&str>,
) -> Result<ExtractedPdfText> {
    let data_owned = data.to_vec();
    let path_owned = path.map(|s| s.to_string());
    let mime_owned = mime_type.map(|s| s.to_string());
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_pdf(&data_owned, path_owned.as_deref(), mime_owned.as_deref())
    })) {
        Ok(r) => r,
        Err(payload) => {
            let msg = panic_message(payload);
            Err(Error::parse(format!("parser panic isolated: {msg}")))
        }
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_empty_whitespace() {
        assert_eq!(classify_text("   \n\t  ", 1), TextClass::Empty);
        assert_eq!(classify_text("", 3), TextClass::Empty);
    }

    #[test]
    fn classify_low_text_total() {
        // 15 non-ws chars < 50
        assert_eq!(classify_text("BATES-ONLY-15xx", 1), TextClass::LowText);
    }

    #[test]
    fn classify_low_text_per_page() {
        // 40 non-ws chars, 3 pages → 13/page < 20
        let t = "a".repeat(40);
        assert_eq!(classify_text(&t, 3), TextClass::LowText);
    }

    #[test]
    fn classify_ok() {
        let t = "word ".repeat(20); // plenty of non-ws
        assert_eq!(classify_text(&t, 1), TextClass::Ok);
    }
}
