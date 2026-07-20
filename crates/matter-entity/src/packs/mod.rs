//! Built-in offline entity packs (Rust `regex` FA engine only).

mod credit_card;
mod currency_usd;
mod email;
mod phone_us;
mod ssn_us;

pub use credit_card::{PACK_ID as CREDIT_CARD_PACK, PACK_VERSION as CREDIT_CARD_VERSION};
pub use currency_usd::{PACK_ID as CURRENCY_PACK, PACK_VERSION as CURRENCY_VERSION};
pub use email::{PACK_ID as EMAIL_PACK, PACK_VERSION as EMAIL_VERSION};
pub use phone_us::{PACK_ID as PHONE_PACK, PACK_VERSION as PHONE_VERSION};
pub use ssn_us::{PACK_ID as SSN_PACK, PACK_VERSION as SSN_VERSION};

use crate::scan::RawHit;

/// Stable pack id list (default enable set).
pub fn default_pack_ids() -> Vec<String> {
    vec![
        EMAIL_PACK.into(),
        PHONE_PACK.into(),
        SSN_PACK.into(),
        CREDIT_CARD_PACK.into(),
        CURRENCY_PACK.into(),
    ]
}

/// True when `id` is a built-in pack.
pub fn is_known_pack(id: &str) -> bool {
    matches!(
        id,
        EMAIL_PACK | PHONE_PACK | SSN_PACK | CREDIT_CARD_PACK | CURRENCY_PACK
    )
}

/// Pack version for audit / hit rows.
pub fn pack_version(id: &str) -> u32 {
    match id {
        EMAIL_PACK => EMAIL_VERSION,
        PHONE_PACK => PHONE_VERSION,
        SSN_PACK => SSN_VERSION,
        CREDIT_CARD_PACK => CREDIT_CARD_VERSION,
        CURRENCY_PACK => CURRENCY_VERSION,
        _ => 0,
    }
}

/// Run one pack over `text`, appending validated hits.
pub fn scan_pack(pack_id: &str, text: &str, field: &str, out: &mut Vec<RawHit>) {
    match pack_id {
        EMAIL_PACK => email::scan(text, field, out),
        PHONE_PACK => phone_us::scan(text, field, out),
        SSN_PACK => ssn_us::scan(text, field, out),
        CREDIT_CARD_PACK => credit_card::scan(text, field, out),
        CURRENCY_PACK => currency_usd::scan(text, field, out),
        _ => {}
    }
}

/// Audit-friendly pack descriptors for complete events.
pub fn pack_audit_entries(packs: &[String]) -> Vec<serde_json::Value> {
    packs
        .iter()
        .map(|id| {
            serde_json::json!({
                "pack_id": id,
                "pack_version": pack_version(id),
            })
        })
        .collect()
}
