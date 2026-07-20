//! US / NANP-ish phone pack.

use regex::Regex;
use std::sync::OnceLock;

use crate::mask::{digits_only, mask_phone, match_hash};
use crate::scan::RawHit;

pub const PACK_ID: &str = "phone_us";
pub const PACK_VERSION: u32 = 1;
pub const ENTITY_TYPE: &str = "phone_us";

fn re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Compile-time-constant pattern (OnceLock init; pack tests cover).
        // Optional +1 / 1, area, exchange, line with common separators.
        Regex::new(
            r"(?x)
            (?: \+?1 [\s\-.]? )?
            (?: \( \d{3} \) | \d{3} )
            [\s\-.]?
            \d{3}
            [\s\-.]?
            \d{4}
            \b",
        )
        .unwrap_or_else(|e| panic!("static pack regex: {e}"))
    })
}

/// Normalize to 10-digit NANP form (strip leading country `1` when present).
pub fn normalize_phone(raw: &str) -> Option<String> {
    let mut d = digits_only(raw);
    if d.len() == 11 && d.starts_with('1') {
        d = d[1..].to_string();
    }
    if d.len() == 10 {
        Some(d)
    } else {
        None
    }
}

pub fn scan(text: &str, field: &str, out: &mut Vec<RawHit>) {
    for m in re().find_iter(text) {
        let raw = m.as_str();
        let Some(norm) = normalize_phone(raw) else {
            continue;
        };
        out.push(RawHit {
            pack_id: PACK_ID.into(),
            pack_version: PACK_VERSION,
            entity_type: ENTITY_TYPE.into(),
            start_offset: m.start() as i64,
            end_offset: m.end() as i64,
            match_hash: match_hash(&norm),
            masked_value: mask_phone(&norm),
            field: field.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_us_phone() {
        let mut hits = Vec::new();
        scan("Call (415) 555-2671 today", "text", &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].masked_value, "***-***-2671");
        assert_eq!(
            normalize_phone("+1 415-555-2671").as_deref(),
            Some("4155552671")
        );
    }
}
