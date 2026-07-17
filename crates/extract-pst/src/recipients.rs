//! Best-effort Display* recipient list parsing.
//!
//! P0 strategy: split semicolon-separated DisplayTo / DisplayCc / DisplayBcc
//! strings. Never invent BCC — missing property → empty `Vec`.

/// Parse a MAPI Display* recipient string into address-ish tokens.
///
/// Outlook typically formats as `Name <email@x>; Name2 <email2@y>` or bare
/// addresses separated by `;`. We split on `;`, trim, and drop empties.
/// We do **not** invent addresses from names alone when no `@` is present —
/// bare display names are kept as-is for best-effort inventory (logical_hash
/// will lowercase them).
pub fn parse_display_list(raw: Option<&str>) -> Vec<String> {
    let Some(s) = raw else {
        return Vec::new();
    };
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    s.split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(extract_angle_addr_or_keep)
        .filter(|p| !p.is_empty())
        .collect()
}

/// Prefer `local@domain` inside `<…>`; otherwise keep the whole token.
fn extract_angle_addr_or_keep(token: &str) -> String {
    if let Some(start) = token.find('<') {
        if let Some(end) = token[start + 1..].find('>') {
            let inner = token[start + 1..start + 1 + end].trim();
            if !inner.is_empty() {
                return inner.to_string();
            }
        }
    }
    token.trim().to_string()
}

/// BCC input for logical hash: never fabricate — empty when unknown.
pub fn bcc_for_logical(display_bcc: Option<&str>) -> Vec<String> {
    parse_display_list(display_bcc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_and_none() {
        assert!(parse_display_list(None).is_empty());
        assert!(parse_display_list(Some("")).is_empty());
        assert!(parse_display_list(Some("   ")).is_empty());
    }

    #[test]
    fn parse_semicolon_list_with_angles() {
        let got = parse_display_list(Some("Alice <alice@example.com>; Bob <bob@example.com>"));
        assert_eq!(
            got,
            vec![
                "alice@example.com".to_string(),
                "bob@example.com".to_string()
            ]
        );
    }

    #[test]
    fn parse_bare_addresses() {
        let got = parse_display_list(Some("a@x.com; b@y.com"));
        assert_eq!(got, vec!["a@x.com".to_string(), "b@y.com".to_string()]);
    }

    #[test]
    fn bcc_never_invented_from_none() {
        assert!(bcc_for_logical(None).is_empty());
    }

    #[test]
    fn bcc_present_parses() {
        let got = bcc_for_logical(Some("secret <bcc@hidden.example>"));
        assert_eq!(got, vec!["bcc@hidden.example".to_string()]);
    }
}
