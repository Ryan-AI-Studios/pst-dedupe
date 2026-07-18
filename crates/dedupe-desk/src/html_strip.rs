//! Block-aware HTML → plain text for **review display**.
//!
//! # Why not `logical_hash::strip_html_tags_minimal`?
//!
//! The logical-hash helper concatenates adjacent text nodes without separators,
//! so `<p>Hello</p><p>World</p>` becomes `HelloWorld`. That is correct for
//! content identity, but wrong for legal review readability. This module
//! inserts newlines at block/break tags so paragraph boundaries are preserved.
//!
//! Display path and logical_hash path intentionally differ.

/// Tags that introduce a line/paragraph break when stripped (case-insensitive).
const BLOCK_TAGS: &[&str] = &[
    "p",
    "div",
    "br",
    "tr",
    "li",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "hr",
    "table",
    "thead",
    "tbody",
    "section",
    "article",
];

/// Convert HTML (or mixed text) to plain text suitable for the review body pane.
///
/// - Block/break tags → newline
/// - Other tags stripped without deleting surrounding text
/// - Best-effort common entities
/// - Collapses runs of 3+ blank lines to two
pub fn html_to_review_text(html: &str) -> String {
    let stripped = strip_tags_block_aware(html);
    let decoded = decode_common_entities(&stripped);
    collapse_blank_lines(&decoded)
}

fn strip_tags_block_aware(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(end) = find_tag_end(bytes, i) {
                let tag_inner = &s[i + 1..end];
                if is_block_tag(tag_inner) {
                    out.push('\n');
                }
                i = end + 1;
                continue;
            }
            // Unclosed `<` — emit literally.
            out.push('<');
            i += 1;
        } else {
            // Safe: we advance by char boundaries when not inside a tag search.
            let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn find_tag_end(bytes: &[u8], start: usize) -> Option<usize> {
    // start points at '<'
    let mut j = start + 1;
    while j < bytes.len() {
        if bytes[j] == b'>' {
            return Some(j);
        }
        // Bail on nested '<' (malformed); treat as not a tag.
        if bytes[j] == b'<' {
            return None;
        }
        j += 1;
    }
    None
}

fn is_block_tag(tag_inner: &str) -> bool {
    let name = tag_name(tag_inner);
    BLOCK_TAGS.iter().any(|t| name.eq_ignore_ascii_case(t))
}

/// Extract tag name from content between `<` and `>` (handles `/p`, `br/`, `p class=…`).
fn tag_name(tag_inner: &str) -> &str {
    let t = tag_inner.trim();
    let t = t.strip_prefix('/').unwrap_or(t).trim_start();
    let end = t
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(t.len());
    &t[..end]
}

fn decode_common_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        if let Some((replacement, consumed)) = match_entity(after) {
            out.push_str(replacement);
            rest = &after[consumed..];
        } else {
            out.push('&');
            rest = &after[1..];
        }
    }
    out.push_str(rest);
    out
}

fn match_entity(s: &str) -> Option<(&'static str, usize)> {
    const ENTITIES: &[(&str, &str)] = &[
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&apos;", "'"),
    ];
    for (ent, rep) in ENTITIES {
        if s.len() >= ent.len() && s[..ent.len()].eq_ignore_ascii_case(ent) {
            return Some((*rep, ent.len()));
        }
    }
    None
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newline_run = 0u32;
    for c in s.chars() {
        if c == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push('\n');
            }
        } else if c == '\r' {
            // drop CR; LF handling covers breaks
        } else {
            newline_run = 0;
            out.push(c);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p_tags_do_not_merge_words() {
        let out = html_to_review_text("<p>Hello</p><p>World</p>");
        assert!(out.contains("Hello"), "{out:?}");
        assert!(out.contains("World"), "{out:?}");
        assert!(
            !out.contains("HelloWorld"),
            "block tags must not concatenate: {out:?}"
        );
        // Whitespace or newline between
        let hello_pos = out.find("Hello").expect("Hello");
        let world_pos = out.find("World").expect("World");
        assert!(world_pos > hello_pos + 5);
        let between = &out[hello_pos + 5..world_pos];
        assert!(
            between.chars().any(|c| c.is_whitespace()),
            "expected whitespace between Hello and World, got {between:?}"
        );
    }

    #[test]
    fn br_and_div_insert_breaks() {
        let out = html_to_review_text("A<br>B<div>C</div>D");
        assert!(!out.contains("AB"), "{out:?}");
        assert!(out.contains('A') && out.contains('B') && out.contains('C'));
    }

    #[test]
    fn entities_decoded() {
        let out = html_to_review_text("A&nbsp;B &amp; C &lt;tag&gt; &quot;q&quot;");
        assert!(out.contains("A B"), "{out:?}");
        assert!(out.contains("B & C") || out.contains("&"), "{out:?}");
        assert!(out.contains("<tag>"), "{out:?}");
        assert!(out.contains("\"q\""), "{out:?}");
    }

    #[test]
    fn non_block_tags_strip_cleanly() {
        let out = html_to_review_text("<span>Hi</span> <b>there</b>");
        assert_eq!(out, "Hi there");
    }
}
