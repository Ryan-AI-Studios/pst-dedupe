//! Top-level PDF text extract API.
//!
//! Residual: lopdf still loads the full object graph (native size already capped
//! at 100 MiB). Page-by-page text extract avoids materializing all page strings
//! up front and allows early break on text/page caps.

use pdf_extract::{output_doc_page, Document, PlainTextOutput};

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
///
/// Callers must pass **raw** page text only — never display page markers.
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

    // Load once (structure + text). Never re-parse via extract_text_from_mem*.
    let doc = match Document::load_mem(data) {
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
    let page_nums: Vec<u32> = pages_map.keys().copied().collect();
    let multi_page = page_nums.len() > 1;

    // Display (optional markers) vs raw (classification). Both hard-capped.
    // Per-page PlainTextOutput writes into CappedString so a single hostile
    // page cannot grow an unbounded String before TextBuf sees it.
    let mut display_buf = TextBuf::with_limit(max_text_bytes);
    let mut raw_buf = TextBuf::with_limit(max_text_bytes);
    let mut pages_used = 0usize;
    let mut page_capped = false;
    let mut any_page_ok = false;
    let mut first_page_err: Option<String> = None;

    for &page_num in &page_nums {
        if pages_used >= max_pages {
            page_capped = true;
            break;
        }
        if display_buf.is_full() || raw_buf.is_full() {
            break;
        }

        // Remaining budget for this page (+1 so empty pages still "run").
        let page_cap = max_text_bytes.saturating_sub(raw_buf.len()).max(1);
        let mut page_sink = CappedString::new(page_cap);
        if let Err(e) = {
            let mut output = PlainTextOutput::new(&mut page_sink);
            output_doc_page(&doc, &mut output, page_num)
        } {
            if !any_page_ok && first_page_err.is_none() {
                first_page_err = Some(e.to_string());
            }
            // No whole-document fallback (unbounded). Continue with empty page.
            page_sink.clear();
        } else {
            any_page_ok = true;
        }

        let page_s = page_sink.into_inner();

        // Push into both raw (classification) and display (CAS) buffers.
        // Do not short-circuit after raw fills — display must still receive
        // the same page text (or truncation) for low_text/ok CAS writes.
        if pages_used > 0 {
            let _ = raw_buf.push_str("\n\n");
            let _ = display_buf.push_str("\n\n");
        }
        if multi_page {
            let marker = format!("--- Page {} ---\n", pages_used + 1);
            let _ = display_buf.push_str(&marker);
        }
        let _ = raw_buf.push_str(&page_s);
        let _ = display_buf.push_str(&page_s);
        pages_used += 1;
        if raw_buf.is_full() || display_buf.is_full() {
            break;
        }
    }

    // Single-page hard failure with no successful page → parse error.
    if !any_page_ok && page_nums.len() == 1 {
        if let Some(e) = first_page_err {
            return Err(Error::parse(format!("text extract failed: {e}")));
        }
    }

    let (display_text, text_partial) = display_buf.into_string();
    let (raw_text, raw_partial) = raw_buf.into_string();
    let partial = text_partial || raw_partial || page_capped;
    let page_count = if page_count_u32 > 0 {
        page_count_u32
    } else {
        pages_used.min(u32::MAX as usize) as u32
    };

    let class = classify_text(&raw_text, page_count.max(1));

    if class == TextClass::Empty {
        return Ok(ExtractedPdfText {
            text: String::new(),
            method: methods::PDF_EXTRACT_V1.into(),
            partial,
            page_count,
            class,
        });
    }

    let _ = TRUNCATION_MARKER;

    Ok(ExtractedPdfText {
        text: display_text,
        method: methods::PDF_EXTRACT_V1.into(),
        partial,
        page_count,
        class,
    })
}

// --- Capped sink for per-page PlainTextOutput ---

/// `fmt::Write` sink that never grows past `max` bytes (char-boundary safe).
struct CappedString {
    s: String,
    max: usize,
    truncated: bool,
}

impl CappedString {
    fn new(max: usize) -> Self {
        Self {
            s: String::with_capacity(max.min(64 * 1024)),
            max,
            truncated: false,
        }
    }

    fn clear(&mut self) {
        self.s.clear();
        self.truncated = false;
    }

    fn into_inner(self) -> String {
        self.s
    }
}

impl std::fmt::Write for CappedString {
    fn write_str(&mut self, t: &str) -> std::fmt::Result {
        if self.truncated || self.s.len() >= self.max {
            self.truncated = true;
            return Ok(());
        }
        let remaining = self.max - self.s.len();
        if t.len() <= remaining {
            self.s.push_str(t);
            if self.s.len() >= self.max {
                self.truncated = true;
            }
            return Ok(());
        }
        let mut end = remaining;
        while end > 0 && !t.is_char_boundary(end) {
            end -= 1;
        }
        self.s.push_str(&t[..end]);
        self.truncated = true;
        Ok(())
    }
}

impl pdf_extract::ConvertToFmt for &mut CappedString {
    type Writer = Self;
    fn convert(self) -> Self::Writer {
        self
    }
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

    #[test]
    fn classify_ignores_display_markers_when_raw_empty() {
        // Markers alone must not be classified as low_text if raw is empty.
        assert_eq!(classify_text("", 3), TextClass::Empty);
        // Sparse real text stays low even if display would include markers.
        assert_eq!(classify_text("ab", 2), TextClass::LowText);
    }
}
