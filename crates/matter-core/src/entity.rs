//! Entity / PII hit storage and item rollup flags (schema v25 / track 0046).
//!
//! **Privacy:** store only `masked_value` + `match_hash` — never cleartext PAN/SSN.
//! Offsets are UI hints only (may go stale after text edits).

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::matter::{new_id, now_rfc3339, Matter};

// ---------------------------------------------------------------------------
// Flag bits (document — stable for FilterSpec + desk chips)
// ---------------------------------------------------------------------------

/// Bit layout for `items.entity_flags`.
pub mod entity_flags {
    /// Email address hit(s).
    pub const EMAIL: i64 = 1;
    /// US phone hit(s).
    pub const PHONE: i64 = 2;
    /// US SSN-like hit(s).
    pub const SSN: i64 = 4;
    /// Credit-card hit(s) (Luhn-validated).
    pub const CARD: i64 = 8;
    /// USD currency amount hit(s).
    pub const CURRENCY: i64 = 16;

    /// Preset mask for "Has PII" (phone | SSN | card) — excludes email/currency.
    pub const PII_MASK: i64 = PHONE | SSN | CARD;
}

/// Map a pack / entity_type id to its rollup flag bit (or 0 if unknown).
pub fn flag_bit_for_entity_type(entity_type: &str) -> i64 {
    match entity_type.trim() {
        "email" => entity_flags::EMAIL,
        "phone_us" | "phone" => entity_flags::PHONE,
        "ssn_us" | "ssn" => entity_flags::SSN,
        "credit_card" | "card" => entity_flags::CARD,
        "currency_usd" | "currency" => entity_flags::CURRENCY,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One stored entity hit row (`item_entity_hits`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemEntityHit {
    pub id: String,
    pub matter_id: String,
    pub item_id: String,
    pub pack_id: String,
    pub pack_version: u32,
    pub entity_type: String,
    /// Byte offset start in scanned field (UI hint only).
    pub start_offset: i64,
    /// Byte offset end (exclusive) in scanned field (UI hint only).
    pub end_offset: i64,
    /// SHA-256 hex of normalized match form.
    pub match_hash: String,
    /// Display mask (email domain fully visible; PAN/SSN last4 style).
    pub masked_value: String,
    /// Scanned field: `text` | `subject` | `from` | …
    pub field: String,
    pub job_id: Option<String>,
    pub created_at: String,
}

/// Input row for [`Matter::replace_entity_hits_for_item`] (no id / timestamps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateEntityHitInput {
    pub pack_id: String,
    pub pack_version: u32,
    pub entity_type: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub match_hash: String,
    pub masked_value: String,
    pub field: String,
}

/// Transactional replace of all entity hits for one item + rollup columns.
#[derive(Debug, Clone)]
pub struct ReplaceEntityHitsInput<'a> {
    pub item_id: &'a str,
    pub hits: &'a [CreateEntityHitInput],
    pub flags: i64,
    pub hit_count: i64,
    pub scanned_text_sha256: Option<&'a str>,
    pub job_id: Option<&'a str>,
    pub scan_at: &'a str,
}

/// Thin candidate for entity scan pagination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityScanCandidate {
    pub id: String,
    pub text_sha256: Option<String>,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub entity_scanned_text_sha256: Option<String>,
}

const HIT_SELECT: &str = "id, matter_id, item_id, pack_id, pack_version, entity_type, \
    start_offset, end_offset, match_hash, masked_value, field, job_id, created_at";

fn map_hit_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ItemEntityHit> {
    Ok(ItemEntityHit {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        item_id: row.get(2)?,
        pack_id: row.get(3)?,
        pack_version: row.get::<_, i64>(4)? as u32,
        entity_type: row.get(5)?,
        start_offset: row.get(6)?,
        end_offset: row.get(7)?,
        match_hash: row.get(8)?,
        masked_value: row.get(9)?,
        field: row.get(10)?,
        job_id: row.get(11)?,
        created_at: row.get(12)?,
    })
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// List entity hits for an item (created_at order).
    pub fn list_entity_hits(&self, item_id: &str) -> Result<Vec<ItemEntityHit>> {
        self.ensure_item_in_matter(item_id)?;
        let mut stmt = self.connection().prepare(&format!(
            "SELECT {HIT_SELECT} FROM item_entity_hits \
             WHERE item_id = ?1 AND matter_id = ?2 \
             ORDER BY created_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map(params![item_id, self.id()], map_hit_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Delete all entity hits for one item (does not clear item rollup columns).
    pub fn delete_entity_hits_for_item(&self, item_id: &str) -> Result<u64> {
        self.ensure_item_in_matter(item_id)?;
        let n = self.connection().execute(
            "DELETE FROM item_entity_hits WHERE item_id = ?1 AND matter_id = ?2",
            params![item_id, self.id()],
        )?;
        Ok(n as u64)
    }

    /// Replace all entity hits for an item and update rollup columns in one transaction.
    ///
    /// Deletes prior hits for the item, inserts `hits`, then sets
    /// `entity_flags`, `entity_hit_count`, `entity_scanned_text_sha256`,
    /// `entity_scan_at`, `entity_scan_job_id`.
    pub fn replace_entity_hits_for_item(&self, input: ReplaceEntityHitsInput<'_>) -> Result<()> {
        self.ensure_item_in_matter(input.item_id)?;
        let matter_id = self.id().to_string();
        self.with_transaction(|conn| {
            conn.execute(
                "DELETE FROM item_entity_hits WHERE item_id = ?1 AND matter_id = ?2",
                params![input.item_id, matter_id],
            )?;
            for h in input.hits {
                let id = new_id("ent");
                conn.execute(
                    "INSERT INTO item_entity_hits \
                     (id, matter_id, item_id, pack_id, pack_version, entity_type, \
                      start_offset, end_offset, match_hash, masked_value, field, job_id, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        id,
                        matter_id,
                        input.item_id,
                        h.pack_id,
                        h.pack_version as i64,
                        h.entity_type,
                        h.start_offset,
                        h.end_offset,
                        h.match_hash,
                        h.masked_value,
                        h.field,
                        input.job_id,
                        input.scan_at,
                    ],
                )?;
            }
            conn.execute(
                "UPDATE items SET \
                    entity_flags = ?1, \
                    entity_hit_count = ?2, \
                    entity_scanned_text_sha256 = ?3, \
                    entity_scan_at = ?4, \
                    entity_scan_job_id = ?5 \
                 WHERE id = ?6 AND matter_id = ?7",
                params![
                    input.flags,
                    input.hit_count,
                    input.scanned_text_sha256,
                    input.scan_at,
                    input.job_id,
                    input.item_id,
                    matter_id,
                ],
            )?;
            Ok(())
        })
    }

    /// Clear all entity hits for this matter and reset item entity columns (`reset: true`).
    pub fn clear_entity_hits_for_matter(&self) -> Result<u64> {
        let matter_id = self.id().to_string();
        self.with_transaction(|conn| {
            let n = conn.execute(
                "DELETE FROM item_entity_hits WHERE matter_id = ?1",
                params![matter_id],
            )?;
            conn.execute(
                "UPDATE items SET \
                    entity_flags = 0, \
                    entity_hit_count = 0, \
                    entity_scanned_text_sha256 = NULL, \
                    entity_scan_at = NULL, \
                    entity_scan_job_id = NULL \
                 WHERE matter_id = ?1",
                params![matter_id],
            )?;
            Ok(n as u64)
        })
    }

    /// Keyset page of entity-scan candidates (has body text and/or non-empty subject).
    pub fn list_entity_scan_candidates(
        &self,
        after_id: Option<&str>,
        limit: u64,
    ) -> Result<Vec<EntityScanCandidate>> {
        let lim = limit.max(1) as i64;
        let sql = if after_id.is_some() {
            "SELECT id, text_sha256, subject, from_addr, entity_scanned_text_sha256 \
             FROM items \
             WHERE matter_id = ?1 \
               AND (text_sha256 IS NOT NULL \
                    OR (subject IS NOT NULL AND TRIM(subject) != '')) \
               AND id > ?2 \
             ORDER BY id ASC \
             LIMIT ?3"
        } else {
            "SELECT id, text_sha256, subject, from_addr, entity_scanned_text_sha256 \
             FROM items \
             WHERE matter_id = ?1 \
               AND (text_sha256 IS NOT NULL \
                    OR (subject IS NOT NULL AND TRIM(subject) != '')) \
             ORDER BY id ASC \
             LIMIT ?2"
        };
        let mut stmt = self.connection().prepare(sql)?;
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<EntityScanCandidate> {
            Ok(EntityScanCandidate {
                id: row.get(0)?,
                text_sha256: row.get(1)?,
                subject: row.get(2)?,
                from_addr: row.get(3)?,
                entity_scanned_text_sha256: row.get(4)?,
            })
        };
        let rows = if let Some(aid) = after_id {
            stmt.query_map(params![self.id(), aid, lim], map)?
        } else {
            stmt.query_map(params![self.id(), lim], map)?
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Convenience: current UTC timestamp for entity scan bookkeeping.
    pub fn entity_scan_now() -> String {
        now_rfc3339()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_bits_documented() {
        assert_eq!(entity_flags::EMAIL, 1);
        assert_eq!(entity_flags::PHONE, 2);
        assert_eq!(entity_flags::SSN, 4);
        assert_eq!(entity_flags::CARD, 8);
        assert_eq!(entity_flags::CURRENCY, 16);
        assert_eq!(entity_flags::PII_MASK, 2 | 4 | 8);
        assert_eq!(flag_bit_for_entity_type("email"), 1);
        assert_eq!(flag_bit_for_entity_type("ssn_us"), 4);
    }
}
