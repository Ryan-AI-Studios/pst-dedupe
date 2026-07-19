//! ICS eligibility and sniff helpers.

/// True when path extension is `.ics` or `.ical` (case-insensitive).
pub fn from_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let leaf = lower.rsplit(['/', '\\', '!']).next().unwrap_or(&lower);
    leaf.ends_with(".ics") || leaf.ends_with(".ical")
}

/// True when mime looks like text/calendar.
pub fn from_mime(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m == "text/calendar" || m.starts_with("text/calendar;") || m.contains("text/calendar")
}

/// Sniff `BEGIN:VCALENDAR` after optional leading whitespace / UTF-8 BOM.
pub fn looks_like_ics(bytes: &[u8]) -> bool {
    let mut i = 0usize;
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        i = 3;
    }
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    let rest = &bytes[i..];
    rest.len() >= 15 && rest[..15].eq_ignore_ascii_case(b"BEGIN:VCALENDAR")
}

/// True when path/mime suggests an ICS-eligible item (without reading bytes).
pub fn is_ics_eligible_meta(path: Option<&str>, mime_type: Option<&str>) -> bool {
    if let Some(p) = path {
        if from_extension(p) {
            return true;
        }
    }
    if let Some(m) = mime_type {
        if from_mime(m) {
            return true;
        }
    }
    false
}

/// Detect ICS from path, mime, and/or bytes.
pub fn detect_ics(path: Option<&str>, mime_type: Option<&str>, bytes: Option<&[u8]>) -> bool {
    if is_ics_eligible_meta(path, mime_type) {
        return true;
    }
    if let Some(b) = bytes {
        return looks_like_ics(b);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_and_mime() {
        assert!(from_extension("a.ICS"));
        assert!(from_extension("a.ical"));
        assert!(from_extension(r"C:\x\cal.ics"));
        assert!(!from_extension("a.pdf"));
        assert!(from_mime("text/calendar"));
        assert!(from_mime("text/calendar; charset=utf-8"));
        assert!(!from_mime("text/plain"));
    }

    #[test]
    fn sniff_magic() {
        assert!(looks_like_ics(b"BEGIN:VCALENDAR\r\n"));
        assert!(looks_like_ics(b"  \nBEGIN:VCALENDAR"));
        assert!(looks_like_ics(b"\xEF\xBB\xBFbegin:vcalendar\n"));
        assert!(!looks_like_ics(b"%PDF-1.4"));
        assert!(!looks_like_ics(b""));
    }
}
