//! Email pack: RFC-ish addresses; punctuation strip; domain-visible mask.

use regex::Regex;
use std::sync::OnceLock;

use crate::mask::{mask_email, match_hash, normalize_email};
use crate::scan::RawHit;

pub const PACK_ID: &str = "email";
pub const PACK_VERSION: u32 = 1;
pub const ENTITY_TYPE: &str = "email";

fn re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Compile-time-constant pattern (OnceLock init; pack tests cover). FA-safe: no backrefs.
        Regex::new(r"(?i)\b[A-Z0-9._%+\-]+@[A-Z0-9.\-]+\.[A-Z]{2,}\b")
            .unwrap_or_else(|e| panic!("static pack regex: {e}"))
    })
}

pub fn scan(text: &str, field: &str, out: &mut Vec<RawHit>) {
    for m in re().find_iter(text) {
        let raw = m.as_str();
        let Some(norm) = normalize_email(raw) else {
            continue;
        };
        out.push(RawHit {
            pack_id: PACK_ID.into(),
            pack_version: PACK_VERSION,
            entity_type: ENTITY_TYPE.into(),
            start_offset: m.start() as i64,
            end_offset: m.end() as i64,
            match_hash: match_hash(&norm),
            masked_value: mask_email(&norm),
            field: field.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_email_trailing_comma_normalized() {
        let mut hits = Vec::new();
        // Regex may not include trailing comma; also test bare address.
        scan("Contact bob@competitor.com, please", "text", &mut hits);
        assert!(!hits.is_empty());
        assert!(hits[0].masked_value.ends_with("@competitor.com"));
        let h1 = hits[0].match_hash.clone();
        hits.clear();
        scan("bob@competitor.com", "text", &mut hits);
        assert_eq!(hits[0].match_hash, h1);
    }
}
