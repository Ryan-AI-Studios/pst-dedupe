//! Bounded text accumulator with truncation marker.

use crate::limits::{MAX_EXTRACTED_TEXT_BYTES, TRUNCATION_MARKER};

/// Running UTF-8 text buffer that stops at [`MAX_EXTRACTED_TEXT_BYTES`].
#[derive(Debug, Clone)]
pub struct TextBuf {
    buf: String,
    partial: bool,
    max_bytes: usize,
}

impl Default for TextBuf {
    fn default() -> Self {
        Self::with_limit(MAX_EXTRACTED_TEXT_BYTES)
    }
}

impl TextBuf {
    pub fn with_limit(max_bytes: usize) -> Self {
        Self {
            buf: String::new(),
            partial: false,
            max_bytes,
        }
    }

    pub fn is_partial(&self) -> bool {
        self.partial
    }

    pub fn is_full(&self) -> bool {
        self.partial || self.buf.len() >= self.max_bytes
    }

    pub fn as_str(&self) -> &str {
        &self.buf
    }

    pub fn into_string(self) -> (String, bool) {
        (self.buf, self.partial)
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Append `s`, truncating mid-string if needed. Returns `false` if full after.
    pub fn push_str(&mut self, s: &str) -> bool {
        if self.partial {
            return false;
        }
        let remaining = self.max_bytes.saturating_sub(self.buf.len());
        if remaining == 0 {
            self.mark_truncated();
            return false;
        }
        if s.len() <= remaining {
            self.buf.push_str(s);
            if self.buf.len() >= self.max_bytes {
                self.mark_truncated();
                return false;
            }
            return true;
        }
        // Truncate on a char boundary.
        let mut end = remaining;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        self.buf.push_str(&s[..end]);
        self.mark_truncated();
        false
    }

    pub fn push_char(&mut self, c: char) -> bool {
        let mut tmp = [0u8; 4];
        self.push_str(c.encode_utf8(&mut tmp))
    }

    fn mark_truncated(&mut self) {
        if self.partial {
            return;
        }
        self.partial = true;
        // Ensure room for marker by trimming if needed.
        let marker = TRUNCATION_MARKER;
        if self.buf.len() + marker.len() > self.max_bytes + marker.len() {
            // already over theoretical max — still append marker
        }
        // Prefer keeping content under max and append marker beyond soft max.
        if self.buf.len() > self.max_bytes {
            let mut end = self.max_bytes;
            while end > 0 && !self.buf.is_char_boundary(end) {
                end -= 1;
            }
            self.buf.truncate(end);
        }
        self.buf.push_str(marker);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_with_marker() {
        let mut t = TextBuf::with_limit(20);
        assert!(t.push_str("hello "));
        assert!(!t.push_str("world and more text that is long"));
        assert!(t.is_partial());
        assert!(t.as_str().contains(TRUNCATION_MARKER));
    }
}
