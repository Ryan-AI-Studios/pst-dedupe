//! Teams / chat extract bookkeeping (schema v34 / track 0055).
//!
//! Apply path puts plain-text review body into CAS (`text_sha256`), records
//! conversation/chat fields + `teams_*` job columns, invalidates redacted
//! artifact on body change, and clears FTS bookkeeping so re-index picks up
//! chat text. **Never** rewrites native CAS.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::matter::{now_rfc3339, Matter};

/// `teams_extract_status` values.
pub mod teams_extract_status {
    pub const OK: &str = "ok";
    pub const SKIPPED: &str = "skipped";
    pub const ERROR: &str = "error";
}

/// Input for [`Matter::apply_teams_extract`].
#[derive(Debug, Clone, Default)]
pub struct ApplyTeamsExtractInput {
    pub item_id: String,
    /// When true, re-extract even if already at a successful terminal.
    pub force: bool,
    /// Plain-text review body. `None` for error/skip/parent bookkeeping only.
    pub text: Option<String>,
    pub method: Option<String>,
    /// `ok` | `skipped` | `error`
    pub status: Option<String>,
    pub error: Option<String>,
    // --- chat metadata ---
    pub conversation_id: Option<String>,
    pub chat_type: Option<String>,
    pub team_name: Option<String>,
    pub channel_name: Option<String>,
    pub chat_export_format: Option<String>,
    pub conversation_bucket_date: Option<String>,
    pub file_category: Option<String>,
    pub refine_file_category: bool,
    pub role: Option<String>,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub sent_at: Option<String>,
    pub message_id: Option<String>,
    pub message_class: Option<String>,
    pub extra_json: Option<String>,
}

/// Result of applying teams extract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamsExtractApplyResult {
    /// Idempotent skip — successful terminal and not force.
    Skipped,
    /// Text CAS written and/or chat columns updated.
    Applied {
        text_sha256: Option<String>,
        text_changed: bool,
    },
    /// Error bookkeeping only.
    Error { error: String },
}

/// Thin candidate row for `teams_extract` job listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamsCandidate {
    pub id: String,
    pub path: Option<String>,
    pub mime_type: Option<String>,
    pub native_sha256: Option<String>,
    pub text_sha256: Option<String>,
    pub teams_extract_status: Option<String>,
    pub message_class: Option<String>,
    pub file_category: Option<String>,
    pub parent_item_id: Option<String>,
    pub role: Option<String>,
    pub source_id: Option<String>,
    pub from_addr: Option<String>,
    pub sent_at: Option<String>,
    pub subject: Option<String>,
    pub conversation_id: Option<String>,
}

fn is_successful_terminal(status: Option<&str>) -> bool {
    matches!(
        status,
        Some(teams_extract_status::OK) | Some(teams_extract_status::SKIPPED)
    )
}

impl Matter {
    /// Apply teams/chat extract result: put plain text CAS when non-empty, set
    /// conversation/chat + teams_* columns, invalidate redacted artifact + clear
    /// FTS bookkeeping on text change.
    ///
    /// **Never** rewrites native CAS.
    pub fn apply_teams_extract(
        &self,
        input: ApplyTeamsExtractInput,
    ) -> Result<TeamsExtractApplyResult> {
        let item = self.get_item(&input.item_id)?;

        let prior_success = is_successful_terminal(item.teams_extract_status.as_deref());
        if !input.force
            && prior_success
            && (input.text.is_some()
                || input
                    .status
                    .as_deref()
                    .is_some_and(|s| s == teams_extract_status::SKIPPED)
                || input.status.is_none())
        {
            let now = now_rfc3339();
            self.connection().execute(
                "UPDATE items SET teams_extract_status = ?1, teams_extracted_at = ?2 \
                 WHERE id = ?3 AND matter_id = ?4",
                params![teams_extract_status::SKIPPED, now, input.item_id, self.id()],
            )?;
            return Ok(TeamsExtractApplyResult::Skipped);
        }

        if input.text.is_none() {
            let status = input
                .status
                .clone()
                .unwrap_or_else(|| teams_extract_status::ERROR.into());
            let err = input.error.clone().unwrap_or_else(|| status.clone());
            let now = now_rfc3339();

            if status == teams_extract_status::SKIPPED {
                self.connection().execute(
                    "UPDATE items SET teams_extract_status = ?1, teams_extracted_at = ?2, \
                            teams_extract_error = COALESCE(?3, teams_extract_error), \
                            teams_extract_method = COALESCE(?4, teams_extract_method) \
                     WHERE id = ?5 AND matter_id = ?6",
                    params![
                        status,
                        now,
                        input.error,
                        input.method,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(TeamsExtractApplyResult::Skipped);
            }

            if status == teams_extract_status::OK {
                let file_cat = if input.refine_file_category {
                    input.file_category.clone()
                } else {
                    None
                };
                self.connection().execute(
                    "UPDATE items SET \
                            teams_extract_status = ?1, teams_extract_error = NULL, \
                            teams_extracted_at = ?2, \
                            teams_extract_method = COALESCE(?3, teams_extract_method), \
                            conversation_id = COALESCE(?4, conversation_id), \
                            chat_type = COALESCE(?5, chat_type), \
                            team_name = COALESCE(?6, team_name), \
                            channel_name = COALESCE(?7, channel_name), \
                            chat_export_format = COALESCE(?8, chat_export_format), \
                            conversation_bucket_date = COALESCE(?9, conversation_bucket_date), \
                            file_category = COALESCE(?10, file_category), \
                            role = COALESCE(?11, role), \
                            subject = COALESCE(?12, subject), \
                            from_addr = COALESCE(?13, from_addr), \
                            sent_at = COALESCE(?14, sent_at), \
                            message_id = COALESCE(?15, message_id), \
                            message_class = COALESCE(?16, message_class), \
                            extra_json = COALESCE(?17, extra_json) \
                     WHERE id = ?18 AND matter_id = ?19",
                    params![
                        teams_extract_status::OK,
                        now,
                        input.method,
                        input.conversation_id,
                        input.chat_type,
                        input.team_name,
                        input.channel_name,
                        input.chat_export_format,
                        input.conversation_bucket_date,
                        file_cat,
                        input.role,
                        input.subject,
                        input.from_addr,
                        input.sent_at,
                        input.message_id,
                        input.message_class,
                        input.extra_json,
                        input.item_id,
                        self.id()
                    ],
                )?;
                return Ok(TeamsExtractApplyResult::Applied {
                    text_sha256: None,
                    text_changed: false,
                });
            }

            // Error bookkeeping — retryable (does not lock as successful).
            self.connection().execute(
                "UPDATE items SET teams_extract_status = ?1, teams_extract_error = ?2, \
                        teams_extracted_at = ?3, \
                        teams_extract_method = COALESCE(?4, teams_extract_method) \
                 WHERE id = ?5 AND matter_id = ?6",
                params![status, err, now, input.method, input.item_id, self.id()],
            )?;
            return Ok(TeamsExtractApplyResult::Error { error: err });
        }

        let Some(text) = input.text else {
            return Ok(TeamsExtractApplyResult::Error {
                error: "internal: missing text payload".into(),
            });
        };

        let text_sha = if text.is_empty() {
            None
        } else {
            Some(self.put_bytes(text.as_bytes())?)
        };
        let text_changed = item.text_sha256.as_deref() != text_sha.as_deref();
        let now = now_rfc3339();
        let status = input
            .status
            .unwrap_or_else(|| teams_extract_status::OK.into());
        let method = input.method;
        let err = input.error;
        let file_cat = if input.refine_file_category {
            input.file_category
        } else {
            None
        };

        self.connection().execute(
            "UPDATE items SET \
                text_sha256 = COALESCE(?1, text_sha256), \
                teams_extract_status = ?2, \
                teams_extract_method = ?3, \
                teams_extracted_at = ?4, \
                teams_extract_error = ?5, \
                conversation_id = COALESCE(?6, conversation_id), \
                chat_type = COALESCE(?7, chat_type), \
                team_name = COALESCE(?8, team_name), \
                channel_name = COALESCE(?9, channel_name), \
                chat_export_format = COALESCE(?10, chat_export_format), \
                conversation_bucket_date = COALESCE(?11, conversation_bucket_date), \
                file_category = COALESCE(?12, file_category), \
                role = COALESCE(?13, role), \
                subject = COALESCE(?14, subject), \
                from_addr = COALESCE(?15, from_addr), \
                sent_at = COALESCE(?16, sent_at), \
                message_id = COALESCE(?17, message_id), \
                message_class = COALESCE(?18, message_class), \
                extra_json = COALESCE(?19, extra_json), \
                redacted_text_sha256 = CASE WHEN ?20 THEN NULL ELSE redacted_text_sha256 END, \
                redacted_text_at = CASE WHEN ?20 THEN NULL ELSE redacted_text_at END, \
                redacted_source_digest = CASE WHEN ?20 THEN NULL ELSE redacted_source_digest END, \
                fts_text_sha256 = NULL, \
                fts_indexed_at = NULL, \
                fts_error = NULL \
             WHERE id = ?21 AND matter_id = ?22",
            params![
                text_sha,
                status,
                method,
                now,
                err,
                input.conversation_id,
                input.chat_type,
                input.team_name,
                input.channel_name,
                input.chat_export_format,
                input.conversation_bucket_date,
                file_cat,
                input.role,
                input.subject,
                input.from_addr,
                input.sent_at,
                input.message_id,
                input.message_class,
                input.extra_json,
                text_changed,
                input.item_id,
                self.id(),
            ],
        )?;

        Ok(TeamsExtractApplyResult::Applied {
            text_sha256: text_sha,
            text_changed,
        })
    }

    /// List Teams/chat-eligible candidates for the extract job.
    ///
    /// Ordered set of items with either:
    /// - HTML/JSON export path/mime,
    /// - PST-shaped Teams signals (`message_class` SkypeTeams / path heuristics).
    ///
    /// Excludes pure `chat_message` children (products of prior HTML/JSON expand)
    /// unless they also look like PST Teams messages. Does **not** filter on
    /// existing extract status — callers skip in-process.
    pub fn list_teams_candidates(
        &self,
        offset: u64,
        limit: u64,
        source_id: Option<&str>,
    ) -> Result<Vec<TeamsCandidate>> {
        let limit_i = if limit == u64::MAX {
            i64::MAX
        } else {
            limit as i64
        };

        let source_clause = if source_id.is_some() {
            " AND source_id = ?4 "
        } else {
            " "
        };

        let sql = format!(
            "SELECT id, path, mime_type, native_sha256, text_sha256, \
                    teams_extract_status, message_class, file_category, \
                    parent_item_id, role, source_id, from_addr, sent_at, subject, \
                    conversation_id \
             FROM items \
             WHERE matter_id = ?1 \
               AND ( \
                 lower(IFNULL(path, '')) LIKE '%.html' \
                 OR lower(IFNULL(path, '')) LIKE '%.htm' \
                 OR lower(IFNULL(path, '')) LIKE '%.json' \
                 OR IFNULL(mime_type, '') LIKE '%text/html%' \
                 OR IFNULL(mime_type, '') LIKE '%application/json%' \
                 OR IFNULL(message_class, '') LIKE '%SkypeTeams%' \
                 OR IFNULL(message_class, '') LIKE '%IPM.SkypeTeams%' \
                 OR lower(IFNULL(path, '')) LIKE '%team chat%' \
                 OR lower(IFNULL(path, '')) LIKE '%conversation history%' \
               ) \
               AND NOT ( \
                 IFNULL(role, '') = 'chat_message' \
                 AND IFNULL(message_class, '') NOT LIKE '%SkypeTeams%' \
                 AND IFNULL(message_class, '') NOT LIKE '%IPM.SkypeTeams%' \
               ) \
               {source_clause}\
             ORDER BY imported_at ASC, path ASC, id ASC \
             LIMIT ?2 OFFSET ?3"
        );

        let mut stmt = self.connection().prepare(&sql)?;
        let map_row = |row: &rusqlite::Row<'_>| {
            Ok(TeamsCandidate {
                id: row.get(0)?,
                path: row.get(1)?,
                mime_type: row.get(2)?,
                native_sha256: row.get(3)?,
                text_sha256: row.get(4)?,
                teams_extract_status: row.get(5)?,
                message_class: row.get(6)?,
                file_category: row.get(7)?,
                parent_item_id: row.get(8)?,
                role: row.get(9)?,
                source_id: row.get(10)?,
                from_addr: row.get(11)?,
                sent_at: row.get(12)?,
                subject: row.get(13)?,
                conversation_id: row.get(14)?,
            })
        };

        let rows = if let Some(sid) = source_id {
            stmt.query_map(params![self.id(), limit_i, offset as i64, sid], map_row)?
        } else {
            stmt.query_map(params![self.id(), limit_i, offset as i64], map_row)?
        };

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matter::{item_status, ItemInput};
    use tempfile::tempdir;

    #[test]
    fn apply_teams_extract_sets_chat_fields_and_text() {
        let dir = tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let matter = Matter::create(root.join("m"), "Teams").unwrap();
        let item = matter
            .insert_item(ItemInput {
                path: Some("export/chat.html".into()),
                status: item_status::EXTRACTED.into(),
                ..Default::default()
            })
            .unwrap();

        let r = matter
            .apply_teams_extract(ApplyTeamsExtractInput {
                item_id: item.id.clone(),
                force: false,
                text: Some("Hello\n[Reaction: bob 👍]".into()),
                method: Some("html_fixture_v1".into()),
                status: Some(teams_extract_status::OK.into()),
                conversation_id: Some("abc".into()),
                chat_type: Some("channel".into()),
                team_name: Some("Team Alpha".into()),
                channel_name: Some("General".into()),
                chat_export_format: Some("html".into()),
                conversation_bucket_date: Some("2024-06-01".into()),
                file_category: Some("chat".into()),
                refine_file_category: true,
                ..Default::default()
            })
            .unwrap();
        assert!(matches!(r, TeamsExtractApplyResult::Applied { .. }));

        let after = matter.get_item(&item.id).unwrap();
        assert_eq!(after.conversation_id.as_deref(), Some("abc"));
        assert_eq!(after.chat_type.as_deref(), Some("channel"));
        assert_eq!(after.team_name.as_deref(), Some("Team Alpha"));
        assert_eq!(after.channel_name.as_deref(), Some("General"));
        assert_eq!(after.chat_export_format.as_deref(), Some("html"));
        assert_eq!(
            after.conversation_bucket_date.as_deref(),
            Some("2024-06-01")
        );
        assert_eq!(after.teams_extract_status.as_deref(), Some("ok"));
        assert_eq!(after.file_category.as_deref(), Some("chat"));
        let text = String::from_utf8(
            matter
                .get_bytes(after.text_sha256.as_deref().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(text.contains("[Reaction:"));
    }
}
