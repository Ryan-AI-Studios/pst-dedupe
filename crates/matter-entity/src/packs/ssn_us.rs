//! US SSN-like pack with light invalid-area/group/serial rules.

use regex::Regex;
use std::sync::OnceLock;

use crate::mask::{digits_only, mask_ssn, match_hash};
use crate::scan::RawHit;

pub const PACK_ID: &str = "ssn_us";
pub const PACK_VERSION: u32 = 1;
pub const ENTITY_TYPE: &str = "ssn_us";

fn re_hyphen() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Compile-time-constant pattern (OnceLock init; pack tests cover).
    RE.get_or_init(|| {
        Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap_or_else(|e| panic!("static pack regex: {e}"))
    })
}

fn re_contig() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Contiguous 9 digits — may overlap cards; SSN validate + card Luhn separate.
    // Compile-time-constant pattern (OnceLock init; pack tests cover).
    RE.get_or_init(|| Regex::new(r"\b\d{9}\b").unwrap_or_else(|e| panic!("static pack regex: {e}")))
}

/// Reject area 000/666/9xx, group 00, serial 0000.
pub fn ssn_valid(digits: &str) -> bool {
    if digits.len() != 9 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let area: u32 = digits[0..3].parse().unwrap_or(0);
    let group: u32 = digits[3..5].parse().unwrap_or(0);
    let serial: u32 = digits[5..9].parse().unwrap_or(0);
    if area == 0 || area == 666 || (900..=999).contains(&area) {
        return false;
    }
    if group == 0 {
        return false;
    }
    if serial == 0 {
        return false;
    }
    true
}

pub fn scan(text: &str, field: &str, out: &mut Vec<RawHit>) {
    for m in re_hyphen().find_iter(text) {
        push_if_valid(m.as_str(), m.start(), m.end(), field, out);
    }
    for m in re_contig().find_iter(text) {
        // Skip if this span already covered by a hyphenated hit.
        let start = m.start() as i64;
        let end = m.end() as i64;
        if out.iter().any(|h| {
            h.field == field
                && h.entity_type == ENTITY_TYPE
                && h.start_offset <= start
                && h.end_offset >= end
        }) {
            continue;
        }
        push_if_valid(m.as_str(), m.start(), m.end(), field, out);
    }
}

fn push_if_valid(raw: &str, start: usize, end: usize, field: &str, out: &mut Vec<RawHit>) {
    let d = digits_only(raw);
    if !ssn_valid(&d) {
        return;
    }
    out.push(RawHit {
        pack_id: PACK_ID.into(),
        pack_version: PACK_VERSION,
        entity_type: ENTITY_TYPE.into(),
        start_offset: start as i64,
        end_offset: end as i64,
        match_hash: match_hash(&d),
        masked_value: mask_ssn(&d),
        field: field.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_areas() {
        assert!(!ssn_valid("000121234"));
        assert!(!ssn_valid("666121234"));
        assert!(!ssn_valid("900121234"));
        assert!(!ssn_valid("219001234")); // group 00
        assert!(!ssn_valid("219120000")); // serial 0000
    }

    #[test]
    fn accepts_plausible_format() {
        // Synthetic test number — not a real assigned SSN.
        assert!(ssn_valid("219099999"));
        let mut hits = Vec::new();
        scan("SSN 219-09-9999 on file", "text", &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].masked_value, "***-**-9999");
    }
}
