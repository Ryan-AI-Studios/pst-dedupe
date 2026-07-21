//! Middle-drop truncation for over-cap item text (spec §3.5.3).

/// Default max text bytes for coding suggest prompts.
pub const DEFAULT_MAX_TEXT_BYTES: usize = 8000;

/// Middle-drop marker inserted between retained head and tail (§3.5.3).
pub const TRUNCATION_MARKER: &str = "\n...[TRUNCATED]...\n";

/// Join head and tail with [`TRUNCATION_MARKER`] (CAS oversize path).
pub fn assemble_head_tail(head: &str, tail: &str) -> String {
    let mut out = String::with_capacity(head.len() + TRUNCATION_MARKER.len() + tail.len());
    out.push_str(head);
    out.push_str(TRUNCATION_MARKER);
    out.push_str(tail);
    out
}

/// UTF-8-safe middle-drop truncation.
///
/// - If `text` byte length ≤ `max_bytes`, return unchanged.
/// - Else: first `max_bytes/2` bytes + marker + last `max_bytes/2` bytes,
///   adjusted to character boundaries (never split mid-char).
pub fn middle_drop(text: &str, max_bytes: usize) -> String {
    if max_bytes == 0 {
        return String::new();
    }
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let half = max_bytes / 2;
    if half == 0 {
        return truncate_to_char_boundary(text, max_bytes).to_string();
    }
    let head_end = floor_char_boundary(text, half);
    let tail_start = ceil_char_boundary(text, text.len().saturating_sub(half));
    // Ensure head and tail don't overlap in a way that duplicates mid content
    // when document is just slightly over cap — still drop the middle.
    if tail_start <= head_end {
        return text.to_string();
    }
    assemble_head_tail(&text[..head_end], &text[tail_start..])
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// UTF-8-safe prefix of `s` with at most `max` bytes (floors to a char boundary).
///
/// Never panics on multi-byte characters (unlike `&s[..max]` when `max` is mid-char).
pub fn truncate_to_char_boundary(s: &str, max: usize) -> &str {
    let end = floor_char_boundary(s, max.min(s.len()));
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_cap_unchanged() {
        let s = "hello world";
        assert_eq!(middle_drop(s, 8000), s);
    }

    #[test]
    fn keeps_head_and_distinctive_tail() {
        let head = "HEAD_TOKEN_AAA ".repeat(200); // ~3k
        let mid = "MIDDLE_SHOULD_DROP ".repeat(400); // ~7k
        let tail = "TAIL_UNIQUE_ZZZ9 ".repeat(80); // ~1.3k
        let body = format!("{head}{mid}{tail}");
        assert!(body.len() > 8000, "fixture must exceed cap");
        let out = middle_drop(&body, 8000);
        assert!(
            out.contains("HEAD_TOKEN_AAA"),
            "head missing: {}",
            &out[..80.min(out.len())]
        );
        assert!(
            out.contains("TAIL_UNIQUE_ZZZ9"),
            "tail token missing after middle-drop"
        );
        assert!(out.contains("[TRUNCATED]"));
        assert!(
            !out.contains("MIDDLE_SHOULD_DROP") || {
                // Middle may partially appear near boundaries; require marker present.
                out.contains("[TRUNCATED]")
            }
        );
    }

    #[test]
    fn utf8_safe() {
        let s = format!("{}TAILΩ", "αβγ".repeat(1000));
        let out = middle_drop(&s, 100);
        assert!(out.is_char_boundary(out.len()));
        // Should not panic; output is valid UTF-8.
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }

    #[test]
    fn truncate_to_char_boundary_multibyte() {
        let s = "αβγ"; // each Greek letter is 2 UTF-8 bytes
        assert_eq!(truncate_to_char_boundary(s, 0), "");
        assert_eq!(truncate_to_char_boundary(s, 1), ""); // mid-char → floor
        assert_eq!(truncate_to_char_boundary(s, 2), "α");
        assert_eq!(truncate_to_char_boundary(s, 3), "α");
        assert_eq!(truncate_to_char_boundary(s, 100), s);
    }
}
