//! Scan text fields with enabled packs.

use matter_core::{flag_bit_for_entity_type, CreateEntityHitInput};

use crate::packs;

/// Intermediate hit before persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHit {
    pub pack_id: String,
    pub pack_version: u32,
    pub entity_type: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub match_hash: String,
    pub masked_value: String,
    pub field: String,
}

impl RawHit {
    pub fn into_create(self) -> CreateEntityHitInput {
        CreateEntityHitInput {
            pack_id: self.pack_id,
            pack_version: self.pack_version,
            entity_type: self.entity_type,
            start_offset: self.start_offset,
            end_offset: self.end_offset,
            match_hash: self.match_hash,
            masked_value: self.masked_value,
            field: self.field,
        }
    }
}

/// Scan `text` with each enabled pack for `field`.
pub fn scan_text(text: &str, field: &str, packs: &[String]) -> Vec<RawHit> {
    let mut out = Vec::new();
    for p in packs {
        packs::scan_pack(p, text, field, &mut out);
    }
    out
}

/// Aggregate entity_flags bits from hits.
pub fn flags_from_hits(hits: &[RawHit]) -> i64 {
    let mut flags = 0i64;
    for h in hits {
        flags |= flag_bit_for_entity_type(&h.entity_type);
    }
    flags
}

/// Safe UTF-8 byte slice for UI highlight — returns `None` on OOB / invalid range.
///
/// Offsets are **hints only**; never panic.
pub fn safe_byte_slice(text: &str, start: i64, end: i64) -> Option<&str> {
    if start < 0 || end < 0 || end < start {
        return None;
    }
    let start = start as usize;
    let end = end as usize;
    if end > text.len() {
        return None;
    }
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return None;
    }
    Some(&text[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_slice_oob_no_panic() {
        assert!(safe_byte_slice("hi", 0, 100).is_none());
        assert!(safe_byte_slice("hi", -1, 1).is_none());
        assert_eq!(safe_byte_slice("hello", 0, 5), Some("hello"));
    }

    #[test]
    fn multi_pack_scan() {
        let packs = packs::default_pack_ids();
        let text = "Email bob@example.com card 4111111111111111 phone (415) 555-2671 \
                    SSN 219-09-9999 total $99.00";
        let hits = scan_text(text, "text", &packs);
        assert!(hits.iter().any(|h| h.entity_type == "email"));
        assert!(hits.iter().any(|h| h.entity_type == "credit_card"));
        assert!(hits.iter().any(|h| h.entity_type == "phone_us"));
        assert!(hits.iter().any(|h| h.entity_type == "ssn_us"));
        assert!(hits.iter().any(|h| h.entity_type == "currency_usd"));
        let flags = flags_from_hits(&hits);
        assert!(flags & matter_core::entity_flags::EMAIL != 0);
        assert!(flags & matter_core::entity_flags::CARD != 0);
    }
}
