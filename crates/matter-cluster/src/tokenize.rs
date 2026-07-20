//! Lowercase Unicode word tokenize + stopwords + structural drops.

use crate::prep::is_structural_token;
use crate::stopwords::is_stopword;

/// Tokenize: lowercase, Unicode word tokens, drop stopwords / structural / optional digits.
pub fn tokenize(text: &str, drop_digits: bool) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                cur.push(lc);
            }
        } else if !cur.is_empty() {
            push_token(&mut tokens, &cur, drop_digits);
            cur.clear();
        }
    }
    if !cur.is_empty() {
        push_token(&mut tokens, &cur, drop_digits);
    }
    tokens
}

fn push_token(out: &mut Vec<String>, tok: &str, drop_digits: bool) {
    if tok.is_empty() {
        return;
    }
    if drop_digits && tok.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    if is_stopword(tok) || is_structural_token(tok) {
        return;
    }
    // Skip ultra-short tokens (noise).
    if tok.chars().count() < 2 {
        return;
    }
    out.push(tok.to_string());
}

/// Build term → count map from tokens.
pub fn term_counts(tokens: &[String]) -> std::collections::BTreeMap<String, u32> {
    let mut m = std::collections::BTreeMap::new();
    for t in tokens {
        *m.entry(t.clone()).or_insert(0) += 1;
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_stops_and_from() {
        let toks = tokenize("From the invoice payment to vendor", true);
        assert!(!toks.iter().any(|t| t == "the" || t == "to" || t == "from"));
        assert!(toks.iter().any(|t| t == "invoice"));
        assert!(toks.iter().any(|t| t == "payment"));
    }
}
