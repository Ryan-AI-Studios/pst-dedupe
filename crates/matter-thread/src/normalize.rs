//! Subject normalizer for threading (does **not** change logical_hash rules).
//!
//! `normalize_subject_strict` keeps RE:/FW: for content identity.
//! Threading uses [`normalize_subject_thread`] which strips those prefixes.

/// Collapse whitespace, repeatedly strip leading `RE:`/`FW:`/`FWD:`, lowercase.
///
/// Spec: `(?i)(re|fw|fwd)\s*:\s*` — optional whitespace around the colon
/// (e.g. `Re : Topic`, `FW  : x`). P0: RE/FW/FWD only (no `[tag]` stripping).
/// Does **not** change `normalize_subject_strict` / logical_hash rules.
pub fn normalize_subject_thread(subject: &str) -> String {
    let collapsed = collapse_whitespace(subject.trim());
    if collapsed.is_empty() {
        return String::new();
    }
    let mut s = collapsed;
    while let Some(rest) = strip_leading_reply_prefix(&s) {
        s = collapse_whitespace(rest.trim_start());
        if s.is_empty() {
            break;
        }
    }
    s.to_ascii_lowercase()
}

/// Strip one leading `(?i)(re|fw|fwd)\s*:\s*` prefix. Longer tokens first (`fwd` before `fw`).
fn strip_leading_reply_prefix(s: &str) -> Option<&str> {
    let lower = s.to_ascii_lowercase();
    // Check longer prefixes first so "fwd" is not partially matched as "fw".
    for token in ["fwd", "fw", "re"] {
        if !lower.starts_with(token) {
            continue;
        }
        // SAFETY: `token` is ASCII; byte length equals char length.
        let after_token = &s[token.len()..];
        let after_ws = after_token.trim_start();
        let Some(after_colon) = after_ws.strip_prefix(':') else {
            continue;
        };
        return Some(after_colon.trim_start());
    }
    None
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_re_fw_fwd_repeatedly() {
        assert_eq!(normalize_subject_thread("Re: Meeting"), "meeting");
        assert_eq!(normalize_subject_thread("FW: Re: FWD: Test"), "test");
        assert_eq!(normalize_subject_thread("  Hello World  "), "hello world");
        assert_eq!(normalize_subject_thread("RE:RE: invoice"), "invoice");
    }

    #[test]
    fn strips_optional_whitespace_around_colon() {
        // Spec `(?i)(re|fw|fwd)\s*:\s*`
        assert_eq!(normalize_subject_thread("Re : Topic"), "topic");
        assert_eq!(normalize_subject_thread("RE  :  Topic"), "topic");
        assert_eq!(normalize_subject_thread("FW  : x"), "x");
        assert_eq!(normalize_subject_thread("fwd : Hello"), "hello");
        assert_eq!(normalize_subject_thread("Fw : Re : Nested"), "nested");
    }

    #[test]
    fn empty_after_strip() {
        assert_eq!(normalize_subject_thread("Re:"), "");
        assert_eq!(normalize_subject_thread("Re :"), "");
        assert_eq!(normalize_subject_thread("   "), "");
    }

    #[test]
    fn only_strips_leading_re_fw_fwd() {
        // Does not strip mid-string RE: or arbitrary tags — leading only.
        assert_eq!(
            normalize_subject_thread("[ext] Re: Invoice"),
            "[ext] re: invoice"
        );
        assert_eq!(
            normalize_subject_thread("Re: [ext] Invoice"),
            "[ext] invoice"
        );
    }
}
