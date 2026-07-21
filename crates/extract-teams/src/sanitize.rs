//! HTML → plain text via **ammonia** (strict allowlist → strip tags).
//!
//! Forbidden as sole defense: ad-hoc regex strip of scripts. Ammonia parses
//! and removes scripts/event handlers; empty tag allowlist forces plain text.

use std::collections::HashSet;

use crate::limits::{MAX_EXTRACTED_TEXT_BYTES, TRUNCATION_MARKER};

/// Convert untrusted HTML (or HTML fragments) to plain text suitable for CAS.
///
/// Uses ammonia with **no tags allowed** so scripts, event handlers, and markup
/// are discarded; text nodes remain (entities decoded).
pub fn html_to_plain_text(html: &str) -> String {
    let cleaned = ammonia::Builder::default()
        .tags(HashSet::new())
        .clean(html)
        .to_string();
    normalize_plain(cleaned)
}

/// Collapse runs of whitespace while preserving intentional newlines.
fn normalize_plain(s: String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_was_space = false;
    let mut prev_was_newline = false;
    for ch in s.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            if !prev_was_newline {
                // trim trailing spaces on the line
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push('\n');
                prev_was_newline = true;
                prev_was_space = false;
            }
            continue;
        }
        if ch.is_whitespace() {
            if !prev_was_space && !prev_was_newline && !out.is_empty() {
                out.push(' ');
                prev_was_space = true;
            }
            continue;
        }
        out.push(ch);
        prev_was_space = false;
        prev_was_newline = false;
    }
    while out.ends_with(' ') || out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Cap plain text length; append truncation marker when cut.
///
/// Finds a valid UTF-8 char boundary **before** calling [`String::truncate`]
/// (which panics if the index is mid-codepoint).
pub fn cap_text(text: String) -> (String, bool) {
    if text.len() <= MAX_EXTRACTED_TEXT_BYTES {
        return (text, false);
    }
    let mut end = MAX_EXTRACTED_TEXT_BYTES.saturating_sub(TRUNCATION_MARKER.len());
    // Walk back to a char boundary (and leave room for the marker).
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut cut = text;
    cut.truncate(end);
    cut.push_str(TRUNCATION_MARKER);
    (cut, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_and_onclick() {
        let html = r#"Hello <script>alert(1)</script> world <a onclick="evil()">x</a>"#;
        let plain = html_to_plain_text(html);
        assert!(!plain.to_lowercase().contains("<script"));
        assert!(!plain.contains("alert(1)"));
        assert!(!plain.to_lowercase().contains("onclick"));
        assert!(plain.contains("Hello"));
        assert!(plain.contains("world"));
        assert!(plain.contains('x'));
    }

    #[test]
    fn no_script_tags_in_output() {
        let html =
            r#"<div class="body">Safe <script type="text/javascript">alert(1)</script> text</div>"#;
        let plain = html_to_plain_text(html);
        assert!(!plain.contains('<'));
        assert!(!plain.contains("script"));
    }

    #[test]
    fn cap_text_multibyte_boundary_no_panic() {
        // Build a string that exceeds the cap with a multibyte char straddling
        // the naive byte cutoff (emoji is 4 bytes).
        let unit = "字😊"; // multi-byte chars
        let mut s = String::new();
        while s.len() < MAX_EXTRACTED_TEXT_BYTES + 64 {
            s.push_str(unit);
        }
        let (capped, truncated) = cap_text(s);
        assert!(truncated);
        assert!(capped.ends_with(TRUNCATION_MARKER));
        assert!(capped.len() <= MAX_EXTRACTED_TEXT_BYTES);
        // Must be valid UTF-8 (already guaranteed by String) and not panic.
        assert!(std::str::from_utf8(capped.as_bytes()).is_ok());
    }
}
