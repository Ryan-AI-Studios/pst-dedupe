//! Sentiment / tone item scores (schema v28 / track 0049).
//!
//! **NULL semantics:** `sentiment_polarity IS NULL` means **unscored** — not
//! the same as scored `neutral`. Threshold snapshot columns enable
//! threshold-only relabel without CAS re-read.

use rusqlite::params;

use crate::error::{Error, Result};
use crate::matter::{now_rfc3339, Matter};

// ---------------------------------------------------------------------------
// Polarity tokens (stable for FilterSpec + desk chips)
// ---------------------------------------------------------------------------

/// Polarity string tokens stored in `items.sentiment_polarity`.
pub mod sentiment_polarity {
    pub const POSITIVE: &str = "positive";
    pub const NEUTRAL: &str = "neutral";
    pub const NEGATIVE: &str = "negative";
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Thin candidate for sentiment job pagination (items with body text).
#[derive(Debug, Clone, PartialEq)]
pub struct SentimentCandidate {
    pub id: String,
    pub text_sha256: Option<String>,
    pub sentiment_scanned_text_sha256: Option<String>,
    pub sentiment_method: Option<String>,
    pub sentiment_compound: Option<f64>,
    pub sentiment_pos: Option<f64>,
    pub sentiment_neu: Option<f64>,
    pub sentiment_neg: Option<f64>,
    pub sentiment_compound_min: Option<f64>,
    pub sentiment_compound_max: Option<f64>,
    pub sentiment_polarity: Option<String>,
    pub sentiment_pos_threshold: Option<f64>,
    pub sentiment_neg_threshold: Option<f64>,
}

/// Full score write for one item after unit-extreme aggregation.
#[derive(Debug, Clone)]
pub struct WriteItemSentimentInput<'a> {
    pub item_id: &'a str,
    pub compound: f64,
    pub compound_min: f64,
    pub compound_max: f64,
    pub pos: f64,
    pub neu: f64,
    pub neg: f64,
    pub polarity: &'a str,
    pub method: &'a str,
    pub pos_threshold: f64,
    pub neg_threshold: f64,
    pub scanned_text_sha256: &'a str,
    pub job_id: Option<&'a str>,
    pub scanned_at: &'a str,
}

/// Threshold-only relabel: update polarity + threshold snapshot (no CAS re-read).
#[derive(Debug, Clone)]
pub struct RelabelItemSentimentInput<'a> {
    pub item_id: &'a str,
    pub polarity: &'a str,
    pub pos_threshold: f64,
    pub neg_threshold: f64,
    pub job_id: Option<&'a str>,
    pub scanned_at: &'a str,
}

/// Clear one item's sentiment scores (unscored after rescore; **not** neutral).
///
/// Score / polarity / method / threshold columns are set to NULL. When
/// [`ClearItemSentimentInput::scanned_text_sha256`] is `Some`, records a digest
/// fingerprint plus bookkeeping so re-runs can skip the same empty-attempt body
/// without re-reading CAS.
#[derive(Debug, Clone)]
pub struct ClearItemSentimentInput<'a> {
    pub item_id: &'a str,
    /// When set, fingerprints "processed this digest as unscored".
    pub scanned_text_sha256: Option<&'a str>,
    pub job_id: Option<&'a str>,
    pub scanned_at: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Keyset page of sentiment candidates.
    ///
    /// Includes items with body text **or** prior sentiment scores (so a later
    /// clear of `text_sha256` can mark the item unscored instead of leaving
    /// stale polarity).
    pub fn list_sentiment_candidates(
        &self,
        after_id: Option<&str>,
        limit: u64,
    ) -> Result<Vec<SentimentCandidate>> {
        let lim = limit.max(1) as i64;
        // text present OR any prior scored/fingerprint signal (compound, polarity,
        // or scanned digest). Scanned digest alone covers empty-attempt fingerprints.
        let where_clause = "matter_id = ?1 \
               AND (text_sha256 IS NOT NULL \
                    OR sentiment_compound IS NOT NULL \
                    OR sentiment_polarity IS NOT NULL \
                    OR sentiment_scanned_text_sha256 IS NOT NULL)";
        let sql = if after_id.is_some() {
            format!(
                "SELECT id, text_sha256, sentiment_scanned_text_sha256, sentiment_method, \
                    sentiment_compound, sentiment_pos, sentiment_neu, sentiment_neg, \
                    sentiment_compound_min, sentiment_compound_max, sentiment_polarity, \
                    sentiment_pos_threshold, sentiment_neg_threshold \
             FROM items \
             WHERE {where_clause} \
               AND id > ?2 \
             ORDER BY id ASC \
             LIMIT ?3"
            )
        } else {
            format!(
                "SELECT id, text_sha256, sentiment_scanned_text_sha256, sentiment_method, \
                    sentiment_compound, sentiment_pos, sentiment_neu, sentiment_neg, \
                    sentiment_compound_min, sentiment_compound_max, sentiment_polarity, \
                    sentiment_pos_threshold, sentiment_neg_threshold \
             FROM items \
             WHERE {where_clause} \
             ORDER BY id ASC \
             LIMIT ?2"
            )
        };
        let mut stmt = self.connection().prepare(&sql)?;
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<SentimentCandidate> {
            Ok(SentimentCandidate {
                id: row.get(0)?,
                text_sha256: row.get(1)?,
                sentiment_scanned_text_sha256: row.get(2)?,
                sentiment_method: row.get(3)?,
                sentiment_compound: row.get(4)?,
                sentiment_pos: row.get(5)?,
                sentiment_neu: row.get(6)?,
                sentiment_neg: row.get(7)?,
                sentiment_compound_min: row.get(8)?,
                sentiment_compound_max: row.get(9)?,
                sentiment_polarity: row.get(10)?,
                sentiment_pos_threshold: row.get(11)?,
                sentiment_neg_threshold: row.get(12)?,
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

    /// Write full sentiment scores for one item.
    pub fn write_item_sentiment(&self, input: WriteItemSentimentInput<'_>) -> Result<()> {
        self.ensure_item_in_matter(input.item_id)?;
        let n = self.connection().execute(
            "UPDATE items SET \
                sentiment_compound = ?1, \
                sentiment_compound_min = ?2, \
                sentiment_compound_max = ?3, \
                sentiment_pos = ?4, \
                sentiment_neu = ?5, \
                sentiment_neg = ?6, \
                sentiment_polarity = ?7, \
                sentiment_method = ?8, \
                sentiment_pos_threshold = ?9, \
                sentiment_neg_threshold = ?10, \
                sentiment_scanned_text_sha256 = ?11, \
                sentiment_scanned_at = ?12, \
                sentiment_job_id = ?13 \
             WHERE id = ?14 AND matter_id = ?15",
            params![
                input.compound,
                input.compound_min,
                input.compound_max,
                input.pos,
                input.neu,
                input.neg,
                input.polarity,
                input.method,
                input.pos_threshold,
                input.neg_threshold,
                input.scanned_text_sha256,
                input.scanned_at,
                input.job_id,
                input.item_id,
                self.id(),
            ],
        )?;
        if n == 0 {
            return Err(Error::ItemNotFound(input.item_id.to_string()));
        }
        Ok(())
    }

    /// Relabel polarity + thresholds only (stored compound unchanged; no CAS).
    pub fn relabel_item_sentiment(&self, input: RelabelItemSentimentInput<'_>) -> Result<()> {
        self.ensure_item_in_matter(input.item_id)?;
        let n = self.connection().execute(
            "UPDATE items SET \
                sentiment_polarity = ?1, \
                sentiment_pos_threshold = ?2, \
                sentiment_neg_threshold = ?3, \
                sentiment_scanned_at = ?4, \
                sentiment_job_id = ?5 \
             WHERE id = ?6 AND matter_id = ?7",
            params![
                input.polarity,
                input.pos_threshold,
                input.neg_threshold,
                input.scanned_at,
                input.job_id,
                input.item_id,
                self.id(),
            ],
        )?;
        if n == 0 {
            return Err(Error::ItemNotFound(input.item_id.to_string()));
        }
        Ok(())
    }

    /// Clear all sentiment columns for this matter (`reset: true`).
    pub fn clear_sentiment_for_matter(&self) -> Result<u64> {
        let n = self.connection().execute(
            "UPDATE items SET \
                sentiment_compound = NULL, \
                sentiment_compound_min = NULL, \
                sentiment_compound_max = NULL, \
                sentiment_pos = NULL, \
                sentiment_neu = NULL, \
                sentiment_neg = NULL, \
                sentiment_polarity = NULL, \
                sentiment_method = NULL, \
                sentiment_pos_threshold = NULL, \
                sentiment_neg_threshold = NULL, \
                sentiment_scanned_text_sha256 = NULL, \
                sentiment_scanned_at = NULL, \
                sentiment_job_id = NULL \
             WHERE matter_id = ?1",
            params![self.id()],
        )?;
        Ok(n as u64)
    }

    /// Clear sentiment columns for one item (unscored after rescore; not neutral).
    ///
    /// Same NULL score columns as [`Self::clear_sentiment_for_matter`], scoped to
    /// one item. Optional fingerprint fields mark "attempted and found empty" so
    /// the job can skip on re-run with the same text digest.
    pub fn clear_item_sentiment(&self, input: ClearItemSentimentInput<'_>) -> Result<()> {
        self.ensure_item_in_matter(input.item_id)?;
        let n = self.connection().execute(
            "UPDATE items SET \
                sentiment_compound = NULL, \
                sentiment_compound_min = NULL, \
                sentiment_compound_max = NULL, \
                sentiment_pos = NULL, \
                sentiment_neu = NULL, \
                sentiment_neg = NULL, \
                sentiment_polarity = NULL, \
                sentiment_method = NULL, \
                sentiment_pos_threshold = NULL, \
                sentiment_neg_threshold = NULL, \
                sentiment_scanned_text_sha256 = ?1, \
                sentiment_scanned_at = ?2, \
                sentiment_job_id = ?3 \
             WHERE id = ?4 AND matter_id = ?5",
            params![
                input.scanned_text_sha256,
                input.scanned_at,
                input.job_id,
                input.item_id,
                self.id(),
            ],
        )?;
        if n == 0 {
            return Err(Error::ItemNotFound(input.item_id.to_string()));
        }
        Ok(())
    }

    /// Convenience: current UTC timestamp for sentiment bookkeeping.
    pub fn sentiment_scan_now() -> String {
        now_rfc3339()
    }
}

#[cfg(test)]
mod tests {
    use super::sentiment_polarity;

    #[test]
    fn polarity_tokens_stable() {
        assert_eq!(sentiment_polarity::POSITIVE, "positive");
        assert_eq!(sentiment_polarity::NEUTRAL, "neutral");
        assert_eq!(sentiment_polarity::NEGATIVE, "negative");
    }
}
