//! Normalization + masking + match_hash helpers.

use sha2::{Digest, Sha256};

/// SHA-256 hex of UTF-8 bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(out.len() * 2);
    for &b in out.iter() {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

/// Strip leading/trailing punctuation often stuck to email captures.
pub fn strip_edge_punctuation(s: &str) -> &str {
    const EDGE: &[char] = &[
        ',', '.', ';', ':', ')', '(', '<', '>', '"', '\'', '[', ']', '{', '}', '!', '?',
    ];
    s.trim_matches(EDGE)
}

/// Case-fold email for stable hash (local + domain).
pub fn normalize_email(raw: &str) -> Option<String> {
    let s = strip_edge_punctuation(raw.trim());
    if s.is_empty() {
        return None;
    }
    let (local, domain) = s.split_once('@')?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return None;
    }
    Some(format!(
        "{}@{}",
        local.to_ascii_lowercase(),
        domain.to_ascii_lowercase()
    ))
}

/// Email mask: first char of local + `***` + `@` + **full domain**.
///
/// Example: `bob@competitor.com` → `b***@competitor.com`.
pub fn mask_email(normalized: &str) -> String {
    let Some((local, domain)) = normalized.split_once('@') else {
        return "***".into();
    };
    let first = local.chars().next().unwrap_or('*');
    format!("{first}***@{domain}")
}

/// Digits only.
pub fn digits_only(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Phone mask: show last 4 digits.
pub fn mask_phone(digits: &str) -> String {
    let last4 = last_n(digits, 4);
    format!("***-***-{last4}")
}

/// SSN mask: `***-**-6789`.
pub fn mask_ssn(digits: &str) -> String {
    let last4 = last_n(digits, 4);
    format!("***-**-{last4}")
}

/// Card mask: `****-****-****-1111` (last 4).
pub fn mask_card(digits: &str) -> String {
    let last4 = last_n(digits, 4);
    format!("****-****-****-{last4}")
}

/// Currency: low-sensitivity partial mask.
pub fn mask_currency(_raw: &str) -> String {
    "$***.**".into()
}

fn last_n(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    s[s.len() - n..].to_string()
}

/// Match hash for a normalized form string.
pub fn match_hash(normalized: &str) -> String {
    sha256_hex(normalized.as_bytes())
}

/// Subject-only scan marker: `subject:` + sha256(subject UTF-8).
pub fn subject_scan_marker(subject: &str) -> String {
    format!("subject:{}", sha256_hex(subject.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_punctuation_same_hash() {
        let a = normalize_email("bob@example.com,").unwrap();
        let b = normalize_email("bob@example.com").unwrap();
        assert_eq!(a, b);
        assert_eq!(match_hash(&a), match_hash(&b));
        let m = mask_email(&a);
        assert!(m.contains("@example.com"), "domain visible: {m}");
        assert!(m.starts_with('b'), "local first char: {m}");
        assert!(!m.contains("bob@"), "local not cleartext: {m}");
    }

    #[test]
    fn email_case_fold() {
        let a = normalize_email("Bob@Example.COM").unwrap();
        let b = normalize_email("bob@example.com").unwrap();
        assert_eq!(a, b);
    }
}
