//! Bounded text accumulator with truncation marker.
//!
//! Content is capped so that content + truncation marker fits within `max_bytes`
//! whenever `max_bytes >= TRUNCATION_MARKER.len()`.

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

    /// True when no more content can be accepted (truncated or content cap hit).
    pub fn is_full(&self) -> bool {
        self.partial || self.buf.len() >= self.content_cap()
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

    /// Max bytes reserved for content (marker room reserved when possible).
    fn content_cap(&self) -> usize {
        let marker_len = TRUNCATION_MARKER.len();
        if self.max_bytes > marker_len {
            self.max_bytes - marker_len
        } else {
            // Cap smaller than marker: fill up to max_bytes; marker may be truncated.
            self.max_bytes
        }
    }

    /// Append `s`, truncating mid-string if needed. Returns `false` if full after.
    pub fn push_str(&mut self, s: &str) -> bool {
        if self.partial {
            return false;
        }
        let content_cap = self.content_cap();
        let remaining = content_cap.saturating_sub(self.buf.len());
        if remaining == 0 {
            self.mark_truncated();
            return false;
        }
        if s.len() <= remaining {
            self.buf.push_str(s);
            if self.buf.len() >= content_cap {
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

    /// Append truncation marker so total length stays `<= max_bytes` when
    /// `max_bytes >= TRUNCATION_MARKER.len()`.
    fn mark_truncated(&mut self) {
        if self.partial {
            return;
        }
        self.partial = true;
        let marker = TRUNCATION_MARKER;
        let content_cap = if self.max_bytes > marker.len() {
            self.max_bytes - marker.len()
        } else {
            0
        };
        if self.buf.len() > content_cap {
            let mut end = content_cap;
            while end > 0 && !self.buf.is_char_boundary(end) {
                end -= 1;
            }
            self.buf.truncate(end);
        }
        let room = self.max_bytes.saturating_sub(self.buf.len());
        if room == 0 {
            return;
        }
        if marker.len() <= room {
            self.buf.push_str(marker);
        } else {
            let mut end = room;
            while end > 0 && !marker.is_char_boundary(end) {
                end -= 1;
            }
            self.buf.push_str(&marker[..end]);
        }
        debug_assert!(self.buf.len() <= self.max_bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_with_marker() {
        let max = TRUNCATION_MARKER.len() + 10;
        let mut t = TextBuf::with_limit(max);
        assert!(t.push_str("hello "));
        assert!(!t.push_str("world and more text that is long"));
        assert!(t.is_partial());
        assert!(t.as_str().contains(TRUNCATION_MARKER));
        assert!(
            t.as_str().len() <= max,
            "hard cap: len={} text={:?}",
            t.as_str().len(),
            t.as_str()
        );
    }

    #[test]
    fn hard_cap_when_max_ge_marker() {
        let marker_len = TRUNCATION_MARKER.len();
        let max = marker_len + 10;
        let mut t = TextBuf::with_limit(max);
        assert!(!t.push_str(&"x".repeat(100)));
        assert!(t.is_partial());
        assert!(t.as_str().contains(TRUNCATION_MARKER));
        assert!(
            t.as_str().len() <= max,
            "len={} > max={max}",
            t.as_str().len()
        );
    }
}
