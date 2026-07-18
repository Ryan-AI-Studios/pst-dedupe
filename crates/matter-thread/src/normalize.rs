//! Subject normalizer for threading (does **not** change logical_hash rules).
//!
//! `normalize_subject_strict` keeps RE:/FW: for content identity.
//! Threading uses [`normalize_subject_thread`] which strips those prefixes.

/// Collapse whitespace, repeatedly strip leading `RE:`/`FW:`/`FWD:`, lowercase.
///
/// P0: RE/FW/FWD only (no `[tag]` stripping).
pub fn normalize_subject_thread(subject: &str) -> String {
    let collapsed = collapse_whitespace(subject.trim());
    if collapsed.is_empty() {
        return String::new();
    }
    let mut s = collapsed;
    loop {
        let lower = s.to_ascii_lowercase();
        let stripped = if let Some(rest) = strip_prefix_ci(&lower, "re:") {
            rest
        } else if let Some(rest) = strip_prefix_ci(&lower, "fw:") {
            rest
        } else if let Some(rest) = strip_prefix_ci(&lower, "fwd:") {
            rest
        } else {
            break;
        };
        // Apply the same cut length on the original-case string, then re-collapse.
        let cut = s.len() - stripped.len();
        s = collapse_whitespace(s[cut..].trim_start());
        if s.is_empty() {
            break;
        }
    }
    s.to_ascii_lowercase()
}

fn strip_prefix_ci<'a>(lower: &'a str, prefix: &str) -> Option<&'a str> {
    lower.strip_prefix(prefix).map(|rest| rest.trim_start())
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
    fn empty_after_strip() {
        assert_eq!(normalize_subject_thread("Re:"), "");
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
