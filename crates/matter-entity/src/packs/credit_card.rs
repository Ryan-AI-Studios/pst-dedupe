//! Credit-card pack: digit runs with separators; Luhn + length + rough IIN.

use regex::Regex;
use std::sync::OnceLock;

use crate::luhn::luhn_valid;
use crate::mask::{digits_only, mask_card, match_hash};
use crate::scan::RawHit;

pub const PACK_ID: &str = "credit_card";
pub const PACK_VERSION: u32 = 1;
pub const ENTITY_TYPE: &str = "credit_card";

fn re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Compile-time-constant pattern (OnceLock init; pack tests cover).
        // 13–19 digit groups with optional spaces/dashes (FA-safe).
        Regex::new(r"\b(?:\d[ \-]?){12,18}\d\b")
            .unwrap_or_else(|e| panic!("static pack regex: {e}"))
    })
}

/// Length 13–19, Luhn, rough IIN class (Visa/MC/Amex/Discover/generic).
pub fn card_valid(digits: &str) -> bool {
    let len = digits.len();
    if !(13..=19).contains(&len) {
        return false;
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if !luhn_valid(digits) {
        return false;
    }
    // Rough IIN: accept common major schemes + generic for test PANs.
    let first = digits.as_bytes()[0];
    matches!(first, b'2' | b'3' | b'4' | b'5' | b'6')
}

pub fn scan(text: &str, field: &str, out: &mut Vec<RawHit>) {
    for m in re().find_iter(text) {
        let raw = m.as_str();
        let d = digits_only(raw);
        if !card_valid(&d) {
            continue;
        }
        out.push(RawHit {
            pack_id: PACK_ID.into(),
            pack_version: PACK_VERSION,
            entity_type: ENTITY_TYPE.into(),
            start_offset: m.start() as i64,
            end_offset: m.end() as i64,
            match_hash: match_hash(&d),
            masked_value: mask_card(&d),
            field: field.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visa_accepted() {
        assert!(card_valid("4111111111111111"));
        let mut hits = Vec::new();
        scan("Card 4111-1111-1111-1111 on file", "text", &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].masked_value, "****-****-****-1111");
        // Never store cleartext in hit.
        assert!(!hits[0].masked_value.contains("4111"));
    }

    #[test]
    fn invalid_luhn_rejected() {
        assert!(!card_valid("4111111111111112"));
        let mut hits = Vec::new();
        scan("Card 4111-1111-1111-1112 bad", "text", &mut hits);
        assert!(hits.is_empty());
    }
}
