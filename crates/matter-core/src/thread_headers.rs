//! Threading header helpers (track 0022).
//!
//! Pure string/bytes utilities used by extract-pst and matter-thread:
//! - RFC 2822-safe References parsing
//! - ConversationIndex canonical lowercase hex (MAPI bytes or Base64 Thread-Index)

use crate::logical_hash::normalize_message_id;

/// Unfold RFC 2822 folding whitespace: CRLF/LF + WSP → single space.
pub fn unfold_header_value(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'\n'
            && i + 2 < bytes.len()
            && (bytes[i + 2] == b' ' || bytes[i + 2] == b'\t')
        {
            out.push(' ');
            i += 3;
            // Consume additional leading WSP on the continuation line.
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'\n'
            && i + 1 < bytes.len()
            && (bytes[i + 1] == b' ' || bytes[i + 1] == b'\t')
        {
            out.push(' ');
            i += 2;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Parse a References (or multi-valued In-Reply-To) header into normalized MIDs.
///
/// Steps (spec §3.3.1):
/// 1. Unfold CRLF/LF + WSP folds
/// 2. Extract angle-bracketed tokens via `<…>` scan (not space-split alone)
/// 3. `normalize_message_id` each capture
/// 4. Preserve first-seen order; drop empties; de-dupe while preserving order
pub fn parse_references_header(raw: &str) -> Vec<String> {
    let unfolded = unfold_header_value(raw);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let bytes = unfolded.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'>' {
                j += 1;
            }
            if j < bytes.len() {
                // SAFETY: indices from byte scan of UTF-8; angle content is ASCII MID.
                let inner = &unfolded[start..j];
                let norm = normalize_message_id(inner);
                if !norm.is_empty() && seen.insert(norm.clone()) {
                    out.push(norm);
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Serialize normalized References as a JSON array string.
pub fn references_to_json(refs: &[String]) -> String {
    serde_json::to_string(refs).unwrap_or_else(|_| "[]".into())
}

/// Parse `references_json` back to a list (empty on null/invalid).
pub fn parse_references_json(json: Option<&str>) -> Vec<String> {
    let Some(s) = json.filter(|s| !s.trim().is_empty()) else {
        return Vec::new();
    };
    match serde_json::from_str::<Vec<String>>(s) {
        Ok(v) => v
            .into_iter()
            .map(|m| normalize_message_id(&m))
            .filter(|m| !m.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// First normalized Message-ID from an In-Reply-To header (may be multi-valued).
pub fn parse_in_reply_to(raw: &str) -> Option<String> {
    parse_references_header(raw).into_iter().next()
}

/// Input form for ConversationIndex normalization (spec §3.3.2).
#[derive(Debug, Clone, Copy)]
pub enum ConversationIndexInput<'a> {
    /// Raw MAPI binary bytes.
    Bytes(&'a [u8]),
    /// Transport / EML `Thread-Index` Base64 string.
    Base64(&'a str),
}

/// Canonical lowercase hex for ConversationIndex.
///
/// - Bytes → hex as-is (empty → `None`)
/// - Base64 → decode (standard alphabet; ignore embedded whitespace) → hex;
///   invalid Base64 → `None` (never store raw Base64 as hex)
pub fn normalize_conversation_index_to_hex(input: ConversationIndexInput<'_>) -> Option<String> {
    match input {
        ConversationIndexInput::Bytes(bytes) => {
            if bytes.is_empty() {
                return None;
            }
            Some(bytes_to_hex_lower(bytes))
        }
        ConversationIndexInput::Base64(s) => {
            let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
            if cleaned.is_empty() {
                return None;
            }
            let bytes = decode_base64_std(&cleaned)?;
            if bytes.is_empty() {
                return None;
            }
            Some(bytes_to_hex_lower(&bytes))
        }
    }
}

fn bytes_to_hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

/// Minimal standard Base64 decoder (no URL-safe alphabet). Returns `None` on error.
fn decode_base64_std(input: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes = input.as_bytes();
    // Strip padding for length calc but require valid pad placement.
    let mut clean = Vec::with_capacity(bytes.len());
    let mut pad = 0usize;
    for &b in bytes {
        if b == b'=' {
            pad += 1;
            if pad > 2 {
                return None;
            }
            continue;
        }
        if pad > 0 {
            // Non-pad after pad
            return None;
        }
        clean.push(b);
    }
    if clean.is_empty() && pad == 0 {
        return Some(Vec::new());
    }
    // Valid groups: data length mod 4 with padding to multiple of 4.
    let total_with_pad = clean.len() + pad;
    if !total_with_pad.is_multiple_of(4) {
        // Allow unpadded input that is multiple of 4 already handled;
        // also allow remainder 2 or 3 (implicit pad).
        if clean.len() % 4 == 1 {
            return None;
        }
    }

    let mut out = Vec::with_capacity(clean.len() * 3 / 4 + 2);
    let mut i = 0;
    while i < clean.len() {
        let remaining = clean.len() - i;
        let b0 = val(clean[i])?;
        let b1 = if remaining > 1 {
            val(clean[i + 1])?
        } else {
            return None;
        };
        out.push((b0 << 2) | (b1 >> 4));
        if remaining == 2 {
            // one output byte; remaining bits of b1 should be zero ideally
            break;
        }
        let b2 = val(clean[i + 2])?;
        out.push(((b1 & 0x0f) << 4) | (b2 >> 2));
        if remaining == 3 {
            break;
        }
        let b3 = val(clean[i + 3])?;
        out.push(((b2 & 0x03) << 6) | b3);
        i += 4;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfold_crlf_wsp() {
        let folded = "a\r\n b\r\n\tc";
        assert_eq!(unfold_header_value(folded), "a b c");
    }

    #[test]
    fn references_folded_multi_mid() {
        let folded = "<one@ex.com>\r\n\t<two@ex.com> <three@ex.com>";
        let unfolded = "<one@ex.com> <two@ex.com> <three@ex.com>";
        let a = parse_references_header(folded);
        let b = parse_references_header(unfolded);
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
        assert_eq!(a[0], "one@ex.com");
        assert_eq!(a[1], "two@ex.com");
        assert_eq!(a[2], "three@ex.com");
    }

    #[test]
    fn references_dedupe_preserve_order() {
        let raw = "<a@ex.com> <b@ex.com> <A@ex.com>";
        let v = parse_references_header(raw);
        assert_eq!(v, vec!["a@ex.com".to_string(), "b@ex.com".to_string()]);
    }

    #[test]
    fn references_json_roundtrip_shape() {
        let refs = parse_references_header("<x@y> <z@w>");
        let j = references_to_json(&refs);
        assert!(j.contains("x@y"));
        let back = parse_references_json(Some(&j));
        assert_eq!(back, refs);
    }

    #[test]
    fn in_reply_to_first_only() {
        let v = parse_in_reply_to("<first@ex.com> <second@ex.com>");
        assert_eq!(v.as_deref(), Some("first@ex.com"));
    }

    #[test]
    fn conversation_index_bytes_vs_base64_same_hex() {
        let bytes: Vec<u8> = (0u8..22).collect();
        let hex_from_bytes =
            normalize_conversation_index_to_hex(ConversationIndexInput::Bytes(&bytes))
                .expect("bytes");
        // Standard base64 of those 22 bytes
        let b64 = encode_base64_std_test(&bytes);
        let hex_from_b64 =
            normalize_conversation_index_to_hex(ConversationIndexInput::Base64(&b64)).expect("b64");
        assert_eq!(hex_from_bytes, hex_from_b64);
        assert_eq!(hex_from_bytes.len(), 44);
        assert!(hex_from_bytes
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn conversation_index_base64_with_whitespace() {
        let bytes = b"\x01\x02\x03\x04";
        let b64 = encode_base64_std_test(bytes);
        let spaced = format!("{}\n {}", &b64[..2], &b64[2..]);
        let a = normalize_conversation_index_to_hex(ConversationIndexInput::Bytes(bytes)).unwrap();
        let b =
            normalize_conversation_index_to_hex(ConversationIndexInput::Base64(&spaced)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn conversation_index_invalid_base64_is_none() {
        assert!(
            normalize_conversation_index_to_hex(ConversationIndexInput::Base64("not!!valid"))
                .is_none()
        );
        assert!(normalize_conversation_index_to_hex(ConversationIndexInput::Bytes(&[])).is_none());
        assert!(
            normalize_conversation_index_to_hex(ConversationIndexInput::Base64("   ")).is_none()
        );
    }

    /// Local encoder for tests only (standard alphabet).
    fn encode_base64_std_test(input: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        let mut i = 0;
        while i + 3 <= input.len() {
            let n =
                ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
            out.push(T[((n >> 6) & 63) as usize] as char);
            out.push(T[(n & 63) as usize] as char);
            i += 3;
        }
        let rem = input.len() - i;
        if rem == 1 {
            let n = (input[i] as u32) << 16;
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
            out.push('=');
            out.push('=');
        } else if rem == 2 {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
            out.push(T[((n >> 6) & 63) as usize] as char);
            out.push('=');
        }
        out
    }
}
