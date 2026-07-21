//! Conversation-centric review queries (track 0056).
//!
//! **Locks:**
//! - Group by `conversation_id` only (day-bucketed from 0055) — never channel-wide.
//! - Stream loads **all** messages in the bucket; filters badge hits, never hide neighbors.
//! - Search handoff uses a **centered** window around the anchor item.
//! - Prefer schema v34 (`idx_items_conversation`); no migration.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::matter::Matter;

// ---------------------------------------------------------------------------
// Caps (frozen for 0056)
// ---------------------------------------------------------------------------

/// Default page size for conversation list discovery.
pub const CONVERSATION_LIST_DEFAULT_LIMIT: u64 = 50;
/// Hard max page size for conversation list.
pub const CONVERSATION_LIST_MAX_LIMIT: u64 = 200;
/// Default page size for in-conversation message stream.
pub const CONVERSATION_STREAM_DEFAULT_LIMIT: u64 = 100;
/// Hard max page size for message stream (and around-window total clamp).
pub const CONVERSATION_STREAM_MAX_LIMIT: u64 = 500;
/// Default messages before anchor for search handoff.
pub const CONVERSATION_AROUND_BEFORE: u64 = 50;
/// Default messages after anchor for search handoff.
pub const CONVERSATION_AROUND_AFTER: u64 = 50;
/// Truncation length for “In reply to: …” parent snippets (chars).
pub const REPLY_SNIPPET_MAX_CHARS: usize = 100;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One day-bucket conversation for the left list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub conversation_id: String,
    pub chat_type: Option<String>,
    pub team_name: Option<String>,
    pub channel_name: Option<String>,
    /// Denorm `conversation_bucket_date` (`YYYY-MM-DD` or `unknown`).
    pub bucket_date: Option<String>,
    /// Count of **all** messages in the bucket (not only hits).
    pub message_count: i64,
    /// Count of messages intersecting the optional hit id set (0 when no hit filter).
    pub hit_count: i64,
    pub first_at: Option<String>,
    pub last_at: Option<String>,
}

/// Thin message row for the conversation stream (no body text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationMessageRow {
    pub id: String,
    pub conversation_id: String,
    pub sent_at: Option<String>,
    pub from_addr: Option<String>,
    pub subject: Option<String>,
    pub text_sha256: Option<String>,
    pub html_sha256: Option<String>,
    pub parent_item_id: Option<String>,
    pub chat_type: Option<String>,
    pub team_name: Option<String>,
    pub channel_name: Option<String>,
    pub conversation_bucket_date: Option<String>,
    pub file_category: Option<String>,
    pub role: Option<String>,
    pub path: Option<String>,
    /// Parent plain-text snippet when requested/loaded; `None` = not loaded;
    /// `Some(None)` is not used — missing parents use [`REPLY_SNIPPET_UNAVAILABLE`].
    pub reply_snippet: Option<String>,
}

/// Label shown when parent is missing or has no text.
pub const REPLY_SNIPPET_UNAVAILABLE: &str = "[unavailable]";

/// Clamp list limit to `[1, CONVERSATION_LIST_MAX_LIMIT]` (0 → default).
pub fn clamp_conversation_list_limit(limit: u64) -> u64 {
    if limit == 0 {
        CONVERSATION_LIST_DEFAULT_LIMIT
    } else {
        limit.min(CONVERSATION_LIST_MAX_LIMIT)
    }
}

/// Clamp stream limit to `[1, CONVERSATION_STREAM_MAX_LIMIT]` (0 → default).
pub fn clamp_conversation_stream_limit(limit: u64) -> u64 {
    if limit == 0 {
        CONVERSATION_STREAM_DEFAULT_LIMIT
    } else {
        limit.min(CONVERSATION_STREAM_MAX_LIMIT)
    }
}

/// Truncate UTF-8 text to at most `max_chars` Unicode scalar values.
pub fn truncate_snippet(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/// Collapse whitespace and truncate for reply chrome.
pub fn format_reply_snippet(raw: &str, max_chars: usize) -> String {
    let collapsed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return REPLY_SNIPPET_UNAVAILABLE.to_string();
    }
    truncate_snippet(&collapsed, max_chars)
}

// ---------------------------------------------------------------------------
// Matter APIs
// ---------------------------------------------------------------------------

impl Matter {
    /// List day-bucket conversations (GROUP BY `conversation_id`).
    ///
    /// When `hit_item_ids` is `Some` and non-empty, only conversations that
    /// contain **≥1** of those ids are returned. `message_count` is always the
    /// full bucket count; `hit_count` is the intersection size.
    ///
    /// When `hit_item_ids` is `None` or empty, all conversations with at least
    /// one message are listed and `hit_count` is 0.
    ///
    /// Total order: `(last_at IS NULL) ASC, last_at DESC, conversation_id ASC`
    /// — non-null `last_at` first (newest first), null `last_at` last, then
    /// `conversation_id` as tie-break.
    ///
    /// Keyset: when `after_last_at` / `after_conversation_id` are set, returns
    /// rows strictly after that cursor in the total order. Limit is clamped to
    /// [`CONVERSATION_LIST_MAX_LIMIT`].
    pub fn list_conversations(
        &self,
        hit_item_ids: Option<&[String]>,
        after_last_at: Option<&str>,
        after_conversation_id: Option<&str>,
        limit: u64,
    ) -> Result<Vec<ConversationSummary>> {
        let limit_i = clamp_conversation_list_limit(limit) as i64;
        let use_keyset = after_last_at.is_some() || after_conversation_id.is_some();
        let after_cid = after_conversation_id.unwrap_or("");

        // Keyset: rows strictly after (cursor_last_at, cursor_cid) in
        // (last_at IS NULL) ASC, last_at DESC, conversation_id ASC.
        // Non-null last_at first (DESC), nulls last; then conversation_id ASC.
        let keyset_having = "\
            ( \
              CASE \
                WHEN ?2 IS NULL THEN (MAX(i.sent_at) IS NULL AND i.conversation_id > ?3) \
                ELSE ( \
                  (MAX(i.sent_at) IS NOT NULL AND ( \
                      MAX(i.sent_at) < ?2 OR (MAX(i.sent_at) = ?2 AND i.conversation_id > ?3) \
                  )) \
                  OR (MAX(i.sent_at) IS NULL) \
                ) \
              END \
            )";
        let keyset_having_plain = "\
            ( \
              CASE \
                WHEN ?2 IS NULL THEN (MAX(sent_at) IS NULL AND conversation_id > ?3) \
                ELSE ( \
                  (MAX(sent_at) IS NOT NULL AND ( \
                      MAX(sent_at) < ?2 OR (MAX(sent_at) = ?2 AND conversation_id > ?3) \
                  )) \
                  OR (MAX(sent_at) IS NULL) \
                ) \
              END \
            )";

        let hit_filter = hit_item_ids
            .filter(|ids| !ids.is_empty())
            .map(|ids| ids.to_vec());

        if let Some(ref hits) = hit_filter {
            // Temp table of hit ids for discovery + hit_count.
            self.connection().execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS tmp_conv_hits (id TEXT PRIMARY KEY); \
                 DELETE FROM tmp_conv_hits;",
            )?;
            {
                let mut insert = self
                    .connection()
                    .prepare("INSERT OR IGNORE INTO tmp_conv_hits (id) VALUES (?1)")?;
                for id in hits {
                    insert.execute(params![id])?;
                }
            }

            let sql = if use_keyset {
                format!(
                    "\
                SELECT \
                    i.conversation_id, \
                    MAX(i.chat_type), \
                    MAX(i.team_name), \
                    MAX(i.channel_name), \
                    MAX(i.conversation_bucket_date), \
                    COUNT(*) AS message_count, \
                    SUM(CASE WHEN h.id IS NOT NULL THEN 1 ELSE 0 END) AS hit_count, \
                    MIN(i.sent_at) AS first_at, \
                    MAX(i.sent_at) AS last_at \
                FROM items i \
                LEFT JOIN tmp_conv_hits h ON h.id = i.id \
                WHERE i.matter_id = ?1 \
                  AND i.conversation_id IS NOT NULL \
                  AND i.conversation_id IN ( \
                      SELECT DISTINCT i2.conversation_id \
                      FROM items i2 \
                      INNER JOIN tmp_conv_hits h2 ON h2.id = i2.id \
                      WHERE i2.matter_id = ?1 \
                        AND i2.conversation_id IS NOT NULL \
                  ) \
                GROUP BY i.conversation_id \
                HAVING {keyset_having} \
                ORDER BY (last_at IS NULL), last_at DESC, i.conversation_id ASC \
                LIMIT ?4"
                )
            } else {
                "\
                SELECT \
                    i.conversation_id, \
                    MAX(i.chat_type), \
                    MAX(i.team_name), \
                    MAX(i.channel_name), \
                    MAX(i.conversation_bucket_date), \
                    COUNT(*) AS message_count, \
                    SUM(CASE WHEN h.id IS NOT NULL THEN 1 ELSE 0 END) AS hit_count, \
                    MIN(i.sent_at) AS first_at, \
                    MAX(i.sent_at) AS last_at \
                FROM items i \
                LEFT JOIN tmp_conv_hits h ON h.id = i.id \
                WHERE i.matter_id = ?1 \
                  AND i.conversation_id IS NOT NULL \
                  AND i.conversation_id IN ( \
                      SELECT DISTINCT i2.conversation_id \
                      FROM items i2 \
                      INNER JOIN tmp_conv_hits h2 ON h2.id = i2.id \
                      WHERE i2.matter_id = ?1 \
                        AND i2.conversation_id IS NOT NULL \
                  ) \
                GROUP BY i.conversation_id \
                ORDER BY (last_at IS NULL), last_at DESC, i.conversation_id ASC \
                LIMIT ?2"
                    .to_string()
            };

            let mut stmt = self.connection().prepare(&sql)?;
            let out = if use_keyset {
                let rows = stmt.query_map(
                    params![self.id(), after_last_at, after_cid, limit_i],
                    map_summary,
                )?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                let rows = stmt.query_map(params![self.id(), limit_i], map_summary)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            let _ = self.connection().execute("DELETE FROM tmp_conv_hits", []);
            Ok(out)
        } else {
            let sql = if use_keyset {
                format!(
                    "\
                SELECT \
                    conversation_id, \
                    MAX(chat_type), \
                    MAX(team_name), \
                    MAX(channel_name), \
                    MAX(conversation_bucket_date), \
                    COUNT(*) AS message_count, \
                    0 AS hit_count, \
                    MIN(sent_at) AS first_at, \
                    MAX(sent_at) AS last_at \
                FROM items \
                WHERE matter_id = ?1 \
                  AND conversation_id IS NOT NULL \
                GROUP BY conversation_id \
                HAVING {keyset_having_plain} \
                ORDER BY (last_at IS NULL), last_at DESC, conversation_id ASC \
                LIMIT ?4"
                )
            } else {
                "\
                SELECT \
                    conversation_id, \
                    MAX(chat_type), \
                    MAX(team_name), \
                    MAX(channel_name), \
                    MAX(conversation_bucket_date), \
                    COUNT(*) AS message_count, \
                    0 AS hit_count, \
                    MIN(sent_at) AS first_at, \
                    MAX(sent_at) AS last_at \
                FROM items \
                WHERE matter_id = ?1 \
                  AND conversation_id IS NOT NULL \
                GROUP BY conversation_id \
                ORDER BY (last_at IS NULL), last_at DESC, conversation_id ASC \
                LIMIT ?2"
                    .to_string()
            };

            let mut stmt = self.connection().prepare(&sql)?;
            if use_keyset {
                let rows = stmt.query_map(
                    params![self.id(), after_last_at, after_cid, limit_i],
                    map_summary,
                )?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(Error::from)
            } else {
                let rows = stmt.query_map(params![self.id(), limit_i], map_summary)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(Error::from)
            }
        }
    }

    /// Page messages for one `conversation_id` (full day bucket — **no** FilterSpec WHERE).
    ///
    /// Total order: `(sent_at IS NULL) ASC, sent_at ASC, id ASC` — non-null
    /// timestamps first chronologically, then null `sent_at` by `id`.
    ///
    /// Keyset: when `after_sent_at` / `after_id` are set, returns rows strictly
    /// after that cursor in the total order. Limit clamped to
    /// [`CONVERSATION_STREAM_MAX_LIMIT`].
    ///
    /// When `include_reply_snippets` is true, batch-loads parent text previews for
    /// rows with `parent_item_id`.
    pub fn list_conversation_messages(
        &self,
        conversation_id: &str,
        after_sent_at: Option<&str>,
        after_id: Option<&str>,
        limit: u64,
        include_reply_snippets: bool,
    ) -> Result<Vec<ConversationMessageRow>> {
        let limit_i = clamp_conversation_stream_limit(limit) as i64;
        let cid = conversation_id.trim();
        if cid.is_empty() {
            return Err(Error::Other(
                "list_conversation_messages requires non-empty conversation_id".into(),
            ));
        }

        let mut rows = if after_sent_at.is_some() || after_id.is_some() {
            // Keyset after: strictly after cursor in
            // (sent_at IS NULL) ASC, sent_at ASC, id ASC.
            // Non-null first; nulls last. After a non-null cursor includes later
            // non-nulls and all nulls; after a null cursor is only later nulls.
            let sql = "\
                SELECT id, conversation_id, sent_at, from_addr, subject, text_sha256, html_sha256, \
                       parent_item_id, chat_type, team_name, channel_name, conversation_bucket_date, \
                       file_category, role, path \
                FROM items \
                WHERE matter_id = ?1 \
                  AND conversation_id = ?2 \
                  AND ( \
                    CASE \
                      WHEN ?3 IS NULL THEN (sent_at IS NULL AND id > ?4) \
                      ELSE ( \
                        (sent_at IS NOT NULL AND (sent_at > ?3 OR (sent_at = ?3 AND id > ?4))) \
                        OR (sent_at IS NULL) \
                      ) \
                    END \
                  ) \
                ORDER BY (sent_at IS NULL), sent_at ASC, id ASC \
                LIMIT ?5";
            let mut stmt = self.connection().prepare(sql)?;
            let mapped = stmt.query_map(
                params![
                    self.id(),
                    cid,
                    after_sent_at,
                    after_id.unwrap_or(""),
                    limit_i
                ],
                map_message_row,
            )?;
            mapped.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let sql = "\
                SELECT id, conversation_id, sent_at, from_addr, subject, text_sha256, html_sha256, \
                       parent_item_id, chat_type, team_name, channel_name, conversation_bucket_date, \
                       file_category, role, path \
                FROM items \
                WHERE matter_id = ?1 \
                  AND conversation_id = ?2 \
                ORDER BY (sent_at IS NULL), sent_at ASC, id ASC \
                LIMIT ?3";
            let mut stmt = self.connection().prepare(sql)?;
            let mapped = stmt.query_map(params![self.id(), cid, limit_i], map_message_row)?;
            mapped.collect::<std::result::Result<Vec<_>, _>>()?
        };

        if include_reply_snippets {
            self.attach_reply_snippets(&mut rows)?;
        }
        Ok(rows)
    }

    /// Page messages **before** a keyset cursor for one `conversation_id`.
    ///
    /// Order returned: `(sent_at IS NULL) ASC, sent_at ASC, id ASC` (chronological
    /// for UI prepend). Keyset: when `before_sent_at` / `before_id` are set,
    /// returns rows strictly **before** that cursor in the same total order as
    /// [`list_conversation_messages`], fetched reverse then reversed. When both
    /// are `None`, returns the first (oldest) page.
    ///
    /// Limit clamped to [`CONVERSATION_STREAM_MAX_LIMIT`].
    pub fn list_conversation_messages_before(
        &self,
        conversation_id: &str,
        before_sent_at: Option<&str>,
        before_id: Option<&str>,
        limit: u64,
        include_reply_snippets: bool,
    ) -> Result<Vec<ConversationMessageRow>> {
        let limit_i = clamp_conversation_stream_limit(limit) as i64;
        let cid = conversation_id.trim();
        if cid.is_empty() {
            return Err(Error::Other(
                "list_conversation_messages_before requires non-empty conversation_id".into(),
            ));
        }

        // No cursor → oldest page (same as forward list without after).
        if before_sent_at.is_none() && before_id.is_none() {
            return self.list_conversation_messages(cid, None, None, limit, include_reply_snippets);
        }

        // Keyset before: inverse of after order. Before a non-null cursor is only
        // earlier non-nulls (nulls sort after all non-nulls). Before a null
        // cursor is all non-nulls plus earlier nulls.
        let sql = "\
            SELECT id, conversation_id, sent_at, from_addr, subject, text_sha256, html_sha256, \
                   parent_item_id, chat_type, team_name, channel_name, conversation_bucket_date, \
                   file_category, role, path \
            FROM items \
            WHERE matter_id = ?1 \
              AND conversation_id = ?2 \
              AND ( \
                CASE \
                  WHEN ?3 IS NULL THEN ( \
                    (sent_at IS NOT NULL) \
                    OR (sent_at IS NULL AND id < ?4) \
                  ) \
                  ELSE ( \
                    sent_at IS NOT NULL AND (sent_at < ?3 OR (sent_at = ?3 AND id < ?4)) \
                  ) \
                END \
              ) \
            ORDER BY (sent_at IS NULL) DESC, sent_at DESC, id DESC \
            LIMIT ?5";
        let mut stmt = self.connection().prepare(sql)?;
        let mapped = stmt.query_map(
            params![
                self.id(),
                cid,
                before_sent_at,
                before_id.unwrap_or(""),
                limit_i
            ],
            map_message_row,
        )?;
        let mut rows = mapped.collect::<std::result::Result<Vec<_>, _>>()?;
        rows.reverse(); // chronological ASC for UI

        if include_reply_snippets {
            self.attach_reply_snippets(&mut rows)?;
        }
        Ok(rows)
    }

    /// Load a window **centered** on `anchor_item_id` within `conversation_id`.
    ///
    /// Defaults: [`CONVERSATION_AROUND_BEFORE`] / [`CONVERSATION_AROUND_AFTER`].
    /// Total window is clamped so `before + 1 + after <= CONVERSATION_STREAM_MAX_LIMIT`.
    /// Anchor **must** belong to the conversation; otherwise `ItemNotFound` /
    /// mismatch error.
    pub fn list_conversation_messages_around(
        &self,
        conversation_id: &str,
        anchor_item_id: &str,
        before: Option<u64>,
        after: Option<u64>,
        include_reply_snippets: bool,
    ) -> Result<Vec<ConversationMessageRow>> {
        let cid = conversation_id.trim();
        let aid = anchor_item_id.trim();
        if cid.is_empty() || aid.is_empty() {
            return Err(Error::Other(
                "list_conversation_messages_around requires conversation_id and anchor_item_id"
                    .into(),
            ));
        }

        // Clamp each side first so oversized inputs never overflow u64 addition.
        let max_sides = CONVERSATION_STREAM_MAX_LIMIT.saturating_sub(1);
        let mut before_n = before.unwrap_or(CONVERSATION_AROUND_BEFORE).min(max_sides);
        let mut after_n = after.unwrap_or(CONVERSATION_AROUND_AFTER).min(max_sides);
        // Clamp total window including the anchor itself (`before + 1 + after`).
        let sides = before_n.saturating_add(after_n);
        if sides > max_sides {
            // Split remaining budget fairly when both sides are still oversized.
            // Prefer keeping both sides; shrink the larger side first when possible.
            let half = max_sides / 2;
            let other = max_sides - half;
            if before_n >= half && after_n >= other {
                before_n = half;
                after_n = other;
            } else if before_n > after_n {
                let excess = sides - max_sides;
                before_n = before_n.saturating_sub(excess);
                // Re-check after shrink.
                let sides2 = before_n.saturating_add(after_n);
                if sides2 > max_sides {
                    after_n = max_sides.saturating_sub(before_n);
                }
            } else {
                let excess = sides - max_sides;
                after_n = after_n.saturating_sub(excess);
                let sides2 = before_n.saturating_add(after_n);
                if sides2 > max_sides {
                    before_n = max_sides.saturating_sub(after_n);
                }
            }
        }

        // Resolve anchor.
        let anchor: (Option<String>, String) = self
            .connection()
            .query_row(
                "SELECT sent_at, conversation_id FROM items \
                 WHERE matter_id = ?1 AND id = ?2",
                params![self.id(), aid],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::ItemNotFound(aid.to_string()))?;

        let (anchor_sent_at, anchor_cid) = anchor;
        if anchor_cid != cid {
            return Err(Error::Other(format!(
                "anchor item {aid} is not in conversation {cid}"
            )));
        }

        // Messages strictly before anchor — same total order as stream keyset.
        let before_sql = "\
            SELECT id, conversation_id, sent_at, from_addr, subject, text_sha256, html_sha256, \
                   parent_item_id, chat_type, team_name, channel_name, conversation_bucket_date, \
                   file_category, role, path \
            FROM items \
            WHERE matter_id = ?1 \
              AND conversation_id = ?2 \
              AND ( \
                CASE \
                  WHEN ?3 IS NULL THEN ( \
                    (sent_at IS NOT NULL) \
                    OR (sent_at IS NULL AND id < ?4) \
                  ) \
                  ELSE ( \
                    sent_at IS NOT NULL AND (sent_at < ?3 OR (sent_at = ?3 AND id < ?4)) \
                  ) \
                END \
              ) \
            ORDER BY (sent_at IS NULL) DESC, sent_at DESC, id DESC \
            LIMIT ?5";

        let mut before_rows: Vec<ConversationMessageRow> = {
            let mut stmt = self.connection().prepare(before_sql)?;
            let mapped = stmt.query_map(
                params![
                    self.id(),
                    cid,
                    anchor_sent_at.as_deref(),
                    aid,
                    before_n as i64
                ],
                map_message_row,
            )?;
            let mut v = mapped.collect::<std::result::Result<Vec<_>, _>>()?;
            v.reverse(); // chronological
            v
        };

        // Anchor + after (inclusive of anchor) in the same total order.
        let after_limit = (after_n + 1) as i64;
        let after_sql = "\
            SELECT id, conversation_id, sent_at, from_addr, subject, text_sha256, html_sha256, \
                   parent_item_id, chat_type, team_name, channel_name, conversation_bucket_date, \
                   file_category, role, path \
            FROM items \
            WHERE matter_id = ?1 \
              AND conversation_id = ?2 \
              AND ( \
                id = ?4 \
                OR ( \
                  CASE \
                    WHEN ?3 IS NULL THEN (sent_at IS NULL AND id > ?4) \
                    ELSE ( \
                      (sent_at IS NOT NULL AND (sent_at > ?3 OR (sent_at = ?3 AND id > ?4))) \
                      OR (sent_at IS NULL) \
                    ) \
                  END \
                ) \
              ) \
            ORDER BY (sent_at IS NULL), sent_at ASC, id ASC \
            LIMIT ?5";

        let after_rows: Vec<ConversationMessageRow> = {
            let mut stmt = self.connection().prepare(after_sql)?;
            let mapped = stmt.query_map(
                params![self.id(), cid, anchor_sent_at.as_deref(), aid, after_limit],
                map_message_row,
            )?;
            mapped.collect::<std::result::Result<Vec<_>, _>>()?
        };

        // Ensure anchor is present (should always be in after_rows).
        if !after_rows.iter().any(|r| r.id == aid) && !before_rows.iter().any(|r| r.id == aid) {
            // Fallback: fetch anchor alone.
            let sql = "\
                SELECT id, conversation_id, sent_at, from_addr, subject, text_sha256, html_sha256, \
                       parent_item_id, chat_type, team_name, channel_name, conversation_bucket_date, \
                       file_category, role, path \
                FROM items \
                WHERE matter_id = ?1 AND id = ?2";
            let mut stmt = self.connection().prepare(sql)?;
            let mapped = stmt.query_map(params![self.id(), aid], map_message_row)?;
            let mut solo = mapped.collect::<std::result::Result<Vec<_>, _>>()?;
            before_rows.append(&mut solo);
        }

        before_rows.extend(after_rows);

        // Dedup by id while preserving order (edge overlap).
        let mut seen = HashSet::new();
        before_rows.retain(|r| seen.insert(r.id.clone()));

        if !before_rows.iter().any(|r| r.id == aid) {
            return Err(Error::Other(format!(
                "centered window failed to include anchor {aid}"
            )));
        }

        if include_reply_snippets {
            self.attach_reply_snippets(&mut before_rows)?;
        }
        Ok(before_rows)
    }

    /// Which of `candidate_ids` belong to `conversation_id` **and** are in the
    /// optional hit set? When `hit_ids` is empty/None, returns all candidates in
    /// the conversation (useful for “all are hits” unfiltered mode).
    ///
    /// Desk typically passes the active filter/FTS hit set for **badging only**.
    pub fn conversation_hit_id_set(
        &self,
        conversation_id: &str,
        candidate_ids: &[String],
        hit_ids: Option<&HashSet<String>>,
    ) -> Result<HashSet<String>> {
        if candidate_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let cid = conversation_id.trim();
        if cid.is_empty() {
            return Err(Error::Other(
                "conversation_hit_id_set requires non-empty conversation_id".into(),
            ));
        }

        // Pure in-memory path when hit set is provided: desk already has hits;
        // only need to know which candidates are in the conversation.
        let in_conv = self.filter_ids_in_conversation(cid, candidate_ids)?;
        match hit_ids {
            None => Ok(in_conv),
            Some(hits) if hits.is_empty() => Ok(HashSet::new()),
            Some(hits) => Ok(in_conv.into_iter().filter(|id| hits.contains(id)).collect()),
        }
    }

    /// Subset of `candidate_ids` that have `conversation_id = ?`.
    pub fn filter_ids_in_conversation(
        &self,
        conversation_id: &str,
        candidate_ids: &[String],
    ) -> Result<HashSet<String>> {
        if candidate_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let cid = conversation_id.trim();
        if cid.is_empty() {
            return Err(Error::Other(
                "filter_ids_in_conversation requires non-empty conversation_id".into(),
            ));
        }

        self.connection().execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS tmp_conv_cand (id TEXT PRIMARY KEY); \
             DELETE FROM tmp_conv_cand;",
        )?;
        {
            let mut insert = self
                .connection()
                .prepare("INSERT OR IGNORE INTO tmp_conv_cand (id) VALUES (?1)")?;
            for id in candidate_ids {
                insert.execute(params![id])?;
            }
        }

        let sql = "\
            SELECT i.id FROM items i \
            INNER JOIN tmp_conv_cand c ON c.id = i.id \
            WHERE i.matter_id = ?1 AND i.conversation_id = ?2";
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params![self.id(), cid], |row| row.get::<_, String>(0))?;
        let mut out = HashSet::new();
        for r in rows {
            out.insert(r?);
        }
        let _ = self.connection().execute("DELETE FROM tmp_conv_cand", []);
        Ok(out)
    }

    /// All item ids in a day-bucket conversation, ordered by `sent_at ASC, id ASC`.
    ///
    /// Used for **Code entire day bucket** (pass to [`Matter::apply_codes`]).
    /// No hard cap for P0 day-bounded buckets (audit correctness).
    pub fn list_conversation_item_ids(&self, conversation_id: &str) -> Result<Vec<String>> {
        let cid = conversation_id.trim();
        if cid.is_empty() {
            return Err(Error::Other(
                "list_conversation_item_ids requires non-empty conversation_id".into(),
            ));
        }
        let mut stmt = self.connection().prepare(
            "SELECT id FROM items \
             WHERE matter_id = ?1 AND conversation_id = ?2 \
             ORDER BY (sent_at IS NULL), sent_at ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![self.id(), cid], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Parent reply snippets for the given parent item ids (CAS text preferred,
    /// then subject). Missing / empty → [`REPLY_SNIPPET_UNAVAILABLE`].
    pub fn parent_reply_snippets(&self, parent_ids: &[String]) -> Result<HashMap<String, String>> {
        let mut out = HashMap::new();
        if parent_ids.is_empty() {
            return Ok(out);
        }

        // Dedupe while preserving query cost.
        let mut unique: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for id in parent_ids {
            if seen.insert(id.clone()) {
                unique.push(id.clone());
            }
        }

        self.connection().execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS tmp_conv_parents (id TEXT PRIMARY KEY); \
             DELETE FROM tmp_conv_parents;",
        )?;
        {
            let mut insert = self
                .connection()
                .prepare("INSERT OR IGNORE INTO tmp_conv_parents (id) VALUES (?1)")?;
            for id in &unique {
                insert.execute(params![id])?;
            }
        }

        let sql = "\
            SELECT i.id, i.text_sha256, i.subject \
            FROM items i \
            INNER JOIN tmp_conv_parents p ON p.id = i.id \
            WHERE i.matter_id = ?1";
        let mut stmt = self.connection().prepare(sql)?;
        let rows = stmt.query_map(params![self.id()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;

        let mut found = HashSet::new();
        for row in rows {
            let (id, text_sha, subject) = row?;
            found.insert(id.clone());
            let snippet = self.snippet_from_item_fields(text_sha.as_deref(), subject.as_deref());
            out.insert(id, snippet);
        }

        for id in &unique {
            if !found.contains(id) {
                out.insert(id.clone(), REPLY_SNIPPET_UNAVAILABLE.to_string());
            }
        }

        let _ = self
            .connection()
            .execute("DELETE FROM tmp_conv_parents", []);
        Ok(out)
    }

    /// Convenience: snippet for a single parent.
    pub fn reply_snippet_for_parent(&self, parent_item_id: &str) -> Result<String> {
        let map = self.parent_reply_snippets(&[parent_item_id.to_string()])?;
        Ok(map
            .get(parent_item_id)
            .cloned()
            .unwrap_or_else(|| REPLY_SNIPPET_UNAVAILABLE.to_string()))
    }

    fn attach_reply_snippets(&self, rows: &mut [ConversationMessageRow]) -> Result<()> {
        let parent_ids: Vec<String> = rows
            .iter()
            .filter_map(|r| r.parent_item_id.clone())
            .collect();
        if parent_ids.is_empty() {
            return Ok(());
        }
        let map = self.parent_reply_snippets(&parent_ids)?;
        for row in rows.iter_mut() {
            if let Some(ref pid) = row.parent_item_id {
                row.reply_snippet = Some(
                    map.get(pid)
                        .cloned()
                        .unwrap_or_else(|| REPLY_SNIPPET_UNAVAILABLE.to_string()),
                );
            }
        }
        Ok(())
    }

    fn snippet_from_item_fields(&self, text_sha256: Option<&str>, subject: Option<&str>) -> String {
        if let Some(digest) = text_sha256 {
            // Read a small prefix only (snippet is 100 chars).
            match self.read_cas_prefix(digest, 512) {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let snip = format_reply_snippet(&text, REPLY_SNIPPET_MAX_CHARS);
                    if snip != REPLY_SNIPPET_UNAVAILABLE {
                        return snip;
                    }
                }
                Err(_) => { /* fall through */ }
            }
        }
        if let Some(subj) = subject {
            let snip = format_reply_snippet(subj, REPLY_SNIPPET_MAX_CHARS);
            if snip != REPLY_SNIPPET_UNAVAILABLE {
                return snip;
            }
        }
        REPLY_SNIPPET_UNAVAILABLE.to_string()
    }
}

fn map_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationSummary> {
    Ok(ConversationSummary {
        conversation_id: row.get(0)?,
        chat_type: row.get(1)?,
        team_name: row.get(2)?,
        channel_name: row.get(3)?,
        bucket_date: row.get(4)?,
        message_count: row.get(5)?,
        hit_count: row.get(6)?,
        first_at: row.get(7)?,
        last_at: row.get(8)?,
    })
}

fn map_message_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationMessageRow> {
    Ok(ConversationMessageRow {
        id: row.get(0)?,
        conversation_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        sent_at: row.get(2)?,
        from_addr: row.get(3)?,
        subject: row.get(4)?,
        text_sha256: row.get(5)?,
        html_sha256: row.get(6)?,
        parent_item_id: row.get(7)?,
        chat_type: row.get(8)?,
        team_name: row.get(9)?,
        channel_name: row.get(10)?,
        conversation_bucket_date: row.get(11)?,
        file_category: row.get(12)?,
        role: row.get(13)?,
        path: row.get(14)?,
        reply_snippet: None,
    })
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn clamp_list_limit_caps() {
        assert_eq!(
            clamp_conversation_list_limit(0),
            CONVERSATION_LIST_DEFAULT_LIMIT
        );
        assert_eq!(clamp_conversation_list_limit(10), 10);
        assert_eq!(
            clamp_conversation_list_limit(10_000),
            CONVERSATION_LIST_MAX_LIMIT
        );
    }

    #[test]
    fn clamp_stream_limit_caps() {
        assert_eq!(
            clamp_conversation_stream_limit(0),
            CONVERSATION_STREAM_DEFAULT_LIMIT
        );
        assert_eq!(clamp_conversation_stream_limit(250), 250);
        assert_eq!(
            clamp_conversation_stream_limit(9999),
            CONVERSATION_STREAM_MAX_LIMIT
        );
    }

    #[test]
    fn truncate_snippet_respects_chars() {
        let s = format_reply_snippet("hello     world  again", 8);
        assert!(s.starts_with("hello w"));
        assert!(s.ends_with('…') || s.len() <= 9);
        assert_eq!(format_reply_snippet("   ", 10), REPLY_SNIPPET_UNAVAILABLE);
    }
}
