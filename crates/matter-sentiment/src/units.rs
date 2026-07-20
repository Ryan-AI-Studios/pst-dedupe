//! Split prepared text into scoring units (sentences preferred; lines fallback).

/// Split `text` into units for unit-extreme aggregation.
///
/// Preference order:
/// 1. Sentence boundaries: `.` `!` `?` followed by whitespace or end
/// 2. Non-empty lines if sentence split yields a single blob that is multi-line
///
/// Caps at `max_units` (first N units kept).
pub fn split_units(text: &str, max_units: u32) -> Vec<String> {
    let cap = max_units.max(1) as usize;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut sentences = split_sentences(trimmed);
    if sentences.is_empty() {
        sentences = split_lines(trimmed);
    } else if sentences.len() == 1 && trimmed.contains('\n') {
        // Single "sentence" spanning many lines → prefer line units for dual-tone docs.
        let lines = split_lines(trimmed);
        if lines.len() > 1 {
            sentences = lines;
        }
    }

    sentences.truncate(cap);
    sentences
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if matches!(c, '.' | '!' | '?') {
            // Look ahead for whitespace or end after optional quotes/parens.
            let mut j = i + 1;
            while j < bytes.len() {
                let n = bytes[j] as char;
                if n == '"' || n == '\'' || n == ')' || n == ']' {
                    j += 1;
                    continue;
                }
                break;
            }
            let boundary = j >= bytes.len()
                || bytes[j].is_ascii_whitespace()
                || bytes[j] == b'\n'
                || bytes[j] == b'\r';
            if boundary {
                let unit = text[start..=i].trim();
                if !unit.is_empty() {
                    out.push(unit.to_string());
                }
                // Skip whitespace after boundary.
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_sentence_boundaries() {
        let units = split_units("Great news. Bad news? Meh!", 10);
        assert_eq!(units.len(), 3);
        assert!(units[0].starts_with("Great"));
        assert!(units[1].starts_with("Bad"));
        assert!(units[2].starts_with("Meh"));
    }

    #[test]
    fn respects_max_units() {
        let units = split_units("A. B. C. D.", 2);
        assert_eq!(units.len(), 2);
    }
}
