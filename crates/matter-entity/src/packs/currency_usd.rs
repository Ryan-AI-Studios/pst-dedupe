//! USD currency amount pack (low sensitivity).

use regex::Regex;
use std::sync::OnceLock;

use crate::mask::{mask_currency, match_hash};
use crate::scan::RawHit;

pub const PACK_ID: &str = "currency_usd";
pub const PACK_VERSION: u32 = 1;
pub const ENTITY_TYPE: &str = "currency_usd";

fn re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Compile-time-constant pattern (OnceLock init; pack tests cover).
        Regex::new(
            r"(?x)
            (?:
                \$\s?\d{1,3}(?:,\d{3})*(?:\.\d{2})?
              | USD\s?\d{1,3}(?:,\d{3})*(?:\.\d{2})?
            )
            \b?",
        )
        .unwrap_or_else(|e| panic!("static pack regex: {e}"))
    })
}

/// Normalize amount for hash: strip `$`/USD/commas/spaces; keep digits + decimal.
pub fn normalize_amount(raw: &str) -> Option<String> {
    let s = raw.trim();
    let s = s
        .trim_start_matches('$')
        .trim_start_matches("USD")
        .trim_start_matches("usd")
        .trim();
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    // Must parse as f64-ish number.
    let n: f64 = cleaned.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    Some(cleaned)
}

pub fn scan(text: &str, field: &str, out: &mut Vec<RawHit>) {
    for m in re().find_iter(text) {
        let raw = m.as_str();
        let Some(norm) = normalize_amount(raw) else {
            continue;
        };
        out.push(RawHit {
            pack_id: PACK_ID.into(),
            pack_version: PACK_VERSION,
            entity_type: ENTITY_TYPE.into(),
            start_offset: m.start() as i64,
            end_offset: m.end() as i64,
            match_hash: match_hash(&norm),
            masked_value: mask_currency(raw),
            field: field.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dollar_amount() {
        let mut hits = Vec::new();
        scan("Invoice total $1,234.56 due", "text", &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].masked_value, "$***.**");
    }
}
