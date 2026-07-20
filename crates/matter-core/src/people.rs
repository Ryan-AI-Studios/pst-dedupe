//! People–comms graph storage and list APIs (schema v26 / track 0047).
//!
//! **Identity:** `person_id` is the full **64-char** lowercase hex of
//! `sha256(identity_kind || "\0" || normalized_key)`.
//!
//! **BCC:** stored separately; default pair strength is `visible_count = to + cc`.
//! **Self-mail:** no A→A edges; tracked on `people.self_mail_count`.

use chrono::Datelike;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::cas::sha256_hex;
use crate::error::{Error, Result};
use crate::matter::{new_id, now_rfc3339, Matter};

// ---------------------------------------------------------------------------
// Identity helpers
// ---------------------------------------------------------------------------

/// Identity kinds stored in `people.identity_kind`.
pub mod identity_kind {
    pub const SMTP: &str = "smtp";
    pub const DISPLAY: &str = "display";
    pub const X500: &str = "x500";
    pub const OTHER: &str = "other";
}

/// Participant roles stored in `item_participants.role`.
pub mod participant_role {
    pub const FROM: &str = "from";
    pub const TO: &str = "to";
    pub const CC: &str = "cc";
    pub const BCC: &str = "bcc";
}

/// Build-pass markers on `matters.people_graph_pass`.
pub mod people_graph_pass {
    pub const PASS1: &str = "pass1";
    pub const PASS2: &str = "pass2";
    pub const COMPLETE: &str = "complete";
}

/// Deterministic person primary key: full 64-char SHA-256 hex of
/// `identity_kind || "\0" || normalized_key`.
pub fn person_id_for(identity_kind: &str, normalized_key: &str) -> String {
    let mut buf = Vec::with_capacity(identity_kind.len() + 1 + normalized_key.len());
    buf.extend_from_slice(identity_kind.as_bytes());
    buf.push(0);
    buf.extend_from_slice(normalized_key.as_bytes());
    sha256_hex(&buf)
}

/// Deterministic edge id from matter + from/to person ids.
pub fn people_edge_id(matter_id: &str, from_person_id: &str, to_person_id: &str) -> String {
    let mut buf =
        Vec::with_capacity(matter_id.len() + from_person_id.len() + to_person_id.len() + 8);
    buf.extend_from_slice(b"edge\0");
    buf.extend_from_slice(matter_id.as_bytes());
    buf.push(0);
    buf.extend_from_slice(from_person_id.as_bytes());
    buf.push(0);
    buf.extend_from_slice(to_person_id.as_bytes());
    format!("pe_{}", sha256_hex(&buf))
}

/// Deterministic timeline row id.
pub fn people_timeline_id(
    matter_id: &str,
    grain: &str,
    bucket_start: &str,
    person_id: Option<&str>,
) -> String {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(b"tl\0");
    buf.extend_from_slice(matter_id.as_bytes());
    buf.push(0);
    buf.extend_from_slice(grain.as_bytes());
    buf.push(0);
    buf.extend_from_slice(bucket_start.as_bytes());
    buf.push(0);
    if let Some(p) = person_id {
        buf.extend_from_slice(p.as_bytes());
    } else {
        buf.extend_from_slice(b"_all");
    }
    format!("pt_{}", sha256_hex(&buf))
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One row from `people`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub id: String,
    pub matter_id: String,
    pub identity_kind: String,
    pub normalized_key: String,
    pub email_domain: Option<String>,
    pub display_label: Option<String>,
    pub message_count: i64,
    pub as_from_count: i64,
    pub as_to_count: i64,
    pub as_cc_count: i64,
    pub as_bcc_count: i64,
    pub self_mail_count: i64,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
}

/// One row from `item_participants`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemParticipant {
    pub id: String,
    pub matter_id: String,
    pub item_id: String,
    pub person_id: String,
    pub role: String,
    pub source: String,
    pub raw_value: Option<String>,
    pub item_at: Option<String>,
}

/// Directed aggregate edge (`people_edges`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeopleEdge {
    pub id: String,
    pub matter_id: String,
    pub from_person_id: String,
    pub to_person_id: String,
    pub to_count: i64,
    pub cc_count: i64,
    pub bcc_count: i64,
    /// `to_count + cc_count` (BCC excluded) — default Top Pairs sort key.
    pub visible_count: i64,
    pub first_at: Option<String>,
    pub last_at: Option<String>,
    /// Optional join labels for desk display.
    pub from_label: Option<String>,
    pub to_label: Option<String>,
    pub from_key: Option<String>,
    pub to_key: Option<String>,
}

/// One timeline bucket (`people_timeline`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeopleTimelineBucket {
    pub id: String,
    pub matter_id: String,
    pub bucket_start: String,
    pub grain: String,
    pub person_id: Option<String>,
    pub message_count: i64,
}

/// Domain rollup row (smtp nodes only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainRollupRow {
    pub email_domain: String,
    pub person_count: i64,
    pub message_count: i64,
}

/// Build status for Desk incomplete warnings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PeopleGraphStatus {
    pub built_at: Option<String>,
    pub fingerprint: Option<String>,
    pub job_id: Option<String>,
    /// `pass1` | `pass2` | `complete` | None never run.
    pub pass: Option<String>,
    pub people_count: i64,
    pub edge_count: i64,
    pub participant_count: i64,
    pub is_complete: bool,
}

/// Pass-1 candidate (item with any address field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeoplePass1Candidate {
    pub id: String,
    pub from_addr: Option<String>,
    pub to_addrs_json: Option<String>,
    pub cc_addrs_json: Option<String>,
    pub bcc_addrs_json: Option<String>,
    pub sent_at: Option<String>,
    pub received_at: Option<String>,
    pub created_at: Option<String>,
}

/// Stub person upsert input (Pass 1).
#[derive(Debug, Clone)]
pub struct UpsertPersonStubInput<'a> {
    pub identity_kind: &'a str,
    pub normalized_key: &'a str,
    pub email_domain: Option<&'a str>,
    pub display_label: Option<&'a str>,
}

/// Item participant upsert input (Pass 1).
#[derive(Debug, Clone)]
pub struct UpsertItemParticipantInput<'a> {
    pub item_id: &'a str,
    pub person_id: &'a str,
    pub role: &'a str,
    pub source: &'a str,
    pub raw_value: Option<&'a str>,
    pub item_at: Option<&'a str>,
}

const PERSON_SELECT: &str = "id, matter_id, identity_kind, normalized_key, email_domain, \
    display_label, message_count, as_from_count, as_to_count, as_cc_count, as_bcc_count, \
    self_mail_count, first_seen_at, last_seen_at";

fn map_person_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Person> {
    Ok(Person {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        identity_kind: row.get(2)?,
        normalized_key: row.get(3)?,
        email_domain: row.get(4)?,
        display_label: row.get(5)?,
        message_count: row.get(6)?,
        as_from_count: row.get(7)?,
        as_to_count: row.get(8)?,
        as_cc_count: row.get(9)?,
        as_bcc_count: row.get(10)?,
        self_mail_count: row.get(11)?,
        first_seen_at: row.get(12)?,
        last_seen_at: row.get(13)?,
    })
}

// ---------------------------------------------------------------------------
// Matter API
// ---------------------------------------------------------------------------

impl Matter {
    /// Graph build status + table counts.
    pub fn people_graph_status(&self) -> Result<PeopleGraphStatus> {
        let matter_id = self.id();
        let (built_at, fingerprint, job_id, pass): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = self.connection().query_row(
            "SELECT people_graph_built_at, people_graph_fingerprint, \
                    people_graph_job_id, people_graph_pass \
             FROM matters WHERE id = ?1",
            params![matter_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let people_count: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM people WHERE matter_id = ?1",
            params![matter_id],
            |row| row.get(0),
        )?;
        let edge_count: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM people_edges WHERE matter_id = ?1",
            params![matter_id],
            |row| row.get(0),
        )?;
        let participant_count: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM item_participants WHERE matter_id = ?1",
            params![matter_id],
            |row| row.get(0),
        )?;
        let is_complete =
            pass.as_deref() == Some(people_graph_pass::COMPLETE) && built_at.is_some();
        Ok(PeopleGraphStatus {
            built_at,
            fingerprint,
            job_id,
            pass,
            people_count,
            edge_count,
            participant_count,
            is_complete,
        })
    }

    /// Set `people_graph_pass` only (e.g. `pass1` / `pass2`).
    pub fn set_people_graph_pass(&self, pass: Option<&str>, job_id: Option<&str>) -> Result<()> {
        self.connection().execute(
            "UPDATE matters SET people_graph_pass = ?1, people_graph_job_id = COALESCE(?2, people_graph_job_id) \
             WHERE id = ?3",
            params![pass, job_id, self.id()],
        )?;
        Ok(())
    }

    /// Mark graph complete: `built_at`, fingerprint, pass=`complete`, job id.
    pub fn set_people_graph_complete(
        &self,
        fingerprint: &str,
        job_id: Option<&str>,
    ) -> Result<String> {
        let now = now_rfc3339();
        self.connection().execute(
            "UPDATE matters SET \
                people_graph_built_at = ?1, \
                people_graph_fingerprint = ?2, \
                people_graph_job_id = ?3, \
                people_graph_pass = ?4 \
             WHERE id = ?5",
            params![
                now,
                fingerprint,
                job_id,
                people_graph_pass::COMPLETE,
                self.id()
            ],
        )?;
        Ok(now)
    }

    /// Clear all people-graph tables and matter build columns for this matter.
    pub fn clear_people_graph_tables(&self) -> Result<()> {
        let matter_id = self.id().to_string();
        self.with_transaction(|conn| {
            conn.execute(
                "DELETE FROM people_timeline WHERE matter_id = ?1",
                params![matter_id],
            )?;
            conn.execute(
                "DELETE FROM people_edges WHERE matter_id = ?1",
                params![matter_id],
            )?;
            conn.execute(
                "DELETE FROM item_participants WHERE matter_id = ?1",
                params![matter_id],
            )?;
            conn.execute(
                "DELETE FROM people WHERE matter_id = ?1",
                params![matter_id],
            )?;
            conn.execute(
                "UPDATE matters SET \
                    people_graph_built_at = NULL, \
                    people_graph_fingerprint = NULL, \
                    people_graph_job_id = NULL, \
                    people_graph_pass = NULL \
                 WHERE id = ?1",
                params![matter_id],
            )?;
            Ok(())
        })
    }

    /// Top people by `message_count` DESC.
    pub fn list_people(&self, limit: u64) -> Result<Vec<Person>> {
        let lim = limit.max(1) as i64;
        let mut stmt = self.connection().prepare(&format!(
            "SELECT {PERSON_SELECT} FROM people \
             WHERE matter_id = ?1 \
             ORDER BY message_count DESC, normalized_key ASC \
             LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![self.id(), lim], map_person_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Directed edges ordered by **visible_count** DESC (to+cc; BCC excluded).
    pub fn list_people_edges(&self, limit: u64) -> Result<Vec<PeopleEdge>> {
        let lim = limit.max(1) as i64;
        let mut stmt = self.connection().prepare(
            "SELECT e.id, e.matter_id, e.from_person_id, e.to_person_id, \
                    e.to_count, e.cc_count, e.bcc_count, e.visible_count, \
                    e.first_at, e.last_at, \
                    pf.display_label, pt.display_label, \
                    pf.normalized_key, pt.normalized_key \
             FROM people_edges e \
             LEFT JOIN people pf ON pf.id = e.from_person_id \
             LEFT JOIN people pt ON pt.id = e.to_person_id \
             WHERE e.matter_id = ?1 \
             ORDER BY e.visible_count DESC, e.to_count DESC, e.id ASC \
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![self.id(), lim], |row| {
            Ok(PeopleEdge {
                id: row.get(0)?,
                matter_id: row.get(1)?,
                from_person_id: row.get(2)?,
                to_person_id: row.get(3)?,
                to_count: row.get(4)?,
                cc_count: row.get(5)?,
                bcc_count: row.get(6)?,
                visible_count: row.get(7)?,
                first_at: row.get(8)?,
                last_at: row.get(9)?,
                from_label: row.get(10)?,
                to_label: row.get(11)?,
                from_key: row.get(12)?,
                to_key: row.get(13)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Timeline buckets (`person_id` NULL = matter-wide).
    pub fn list_people_timeline(
        &self,
        grain: &str,
        person_id: Option<&str>,
        limit: u64,
    ) -> Result<Vec<PeopleTimelineBucket>> {
        let lim = limit.max(1) as i64;
        let matter_id = self.id();
        if let Some(pid) = person_id {
            let mut stmt = self.connection().prepare(
                "SELECT id, matter_id, bucket_start, grain, person_id, message_count \
                 FROM people_timeline \
                 WHERE matter_id = ?1 AND grain = ?2 AND person_id = ?3 \
                 ORDER BY bucket_start ASC \
                 LIMIT ?4",
            )?;
            let rows = stmt.query_map(params![matter_id, grain, pid, lim], map_timeline_row)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::from)
        } else {
            let mut stmt = self.connection().prepare(
                "SELECT id, matter_id, bucket_start, grain, person_id, message_count \
                 FROM people_timeline \
                 WHERE matter_id = ?1 AND grain = ?2 AND person_id IS NULL \
                 ORDER BY bucket_start ASC \
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![matter_id, grain, lim], map_timeline_row)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::from)
        }
    }

    /// SMTP domain rollup.
    pub fn list_domain_rollup(&self, limit: u64) -> Result<Vec<DomainRollupRow>> {
        let lim = limit.max(1) as i64;
        let mut stmt = self.connection().prepare(
            "SELECT email_domain, COUNT(*) AS person_count, COALESCE(SUM(message_count), 0) \
             FROM people \
             WHERE matter_id = ?1 \
               AND identity_kind = 'smtp' \
               AND email_domain IS NOT NULL \
               AND TRIM(email_domain) != '' \
             GROUP BY email_domain \
             ORDER BY person_count DESC, email_domain ASC \
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![self.id(), lim], |row| {
            Ok(DomainRollupRow {
                email_domain: row.get(0)?,
                person_count: row.get(1)?,
                message_count: row.get(2)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Keyset page of Pass-1 candidates (items with any address field).
    pub fn list_people_pass1_candidates(
        &self,
        after_id: Option<&str>,
        limit: u64,
    ) -> Result<Vec<PeoplePass1Candidate>> {
        let lim = limit.max(1) as i64;
        let addr_pred = "(\
            (from_addr IS NOT NULL AND TRIM(from_addr) != '') \
            OR (to_addrs_json IS NOT NULL AND TRIM(to_addrs_json) != '' AND TRIM(to_addrs_json) != '[]') \
            OR (cc_addrs_json IS NOT NULL AND TRIM(cc_addrs_json) != '' AND TRIM(cc_addrs_json) != '[]') \
            OR (bcc_addrs_json IS NOT NULL AND TRIM(bcc_addrs_json) != '' AND TRIM(bcc_addrs_json) != '[]') \
        )";
        let sql = if after_id.is_some() {
            format!(
                "SELECT id, from_addr, to_addrs_json, cc_addrs_json, bcc_addrs_json, \
                        sent_at, received_at, created_at \
                 FROM items \
                 WHERE matter_id = ?1 AND {addr_pred} AND id > ?2 \
                 ORDER BY id ASC LIMIT ?3"
            )
        } else {
            format!(
                "SELECT id, from_addr, to_addrs_json, cc_addrs_json, bcc_addrs_json, \
                        sent_at, received_at, created_at \
                 FROM items \
                 WHERE matter_id = ?1 AND {addr_pred} \
                 ORDER BY id ASC LIMIT ?2"
            )
        };
        let mut stmt = self.connection().prepare(&sql)?;
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<PeoplePass1Candidate> {
            Ok(PeoplePass1Candidate {
                id: row.get(0)?,
                from_addr: row.get(1)?,
                to_addrs_json: row.get(2)?,
                cc_addrs_json: row.get(3)?,
                bcc_addrs_json: row.get(4)?,
                sent_at: row.get(5)?,
                received_at: row.get(6)?,
                created_at: row.get(7)?,
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

    /// Upsert a person stub (Pass 1). Returns person id.
    pub fn upsert_person_stub(&self, input: UpsertPersonStubInput<'_>) -> Result<String> {
        let id = person_id_for(input.identity_kind, input.normalized_key);
        let matter_id = self.id();
        self.connection().execute(
            "INSERT INTO people (id, matter_id, identity_kind, normalized_key, email_domain, display_label) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(matter_id, identity_kind, normalized_key) DO UPDATE SET \
               email_domain = COALESCE(people.email_domain, excluded.email_domain), \
               display_label = CASE \
                 WHEN excluded.display_label IS NOT NULL \
                      AND length(excluded.display_label) > length(COALESCE(people.display_label, '')) \
                 THEN excluded.display_label \
                 ELSE people.display_label \
               END",
            params![
                id,
                matter_id,
                input.identity_kind,
                input.normalized_key,
                input.email_domain,
                input.display_label,
            ],
        )?;
        Ok(id)
    }

    /// Idempotent upsert of one `item_participants` row (Pass 1).
    pub fn upsert_item_participant(&self, input: UpsertItemParticipantInput<'_>) -> Result<()> {
        let id = new_id("ip");
        self.connection().execute(
            "INSERT INTO item_participants \
             (id, matter_id, item_id, person_id, role, source, raw_value, item_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT(item_id, person_id, role, source) DO UPDATE SET \
               raw_value = COALESCE(excluded.raw_value, item_participants.raw_value), \
               item_at = COALESCE(item_participants.item_at, excluded.item_at)",
            params![
                id,
                self.id(),
                input.item_id,
                input.person_id,
                input.role,
                input.source,
                input.raw_value,
                input.item_at,
            ],
        )?;
        Ok(())
    }

    /// Pass 2: delete edges + timeline, rebuild people aggregates, edges, timeline.
    ///
    /// Intended to run in one transaction (caller may wrap). Idempotent from
    /// fully populated `item_participants`. Does **not** set `built_at`.
    pub fn rebuild_people_graph_aggregates(&self, grain: &str) -> Result<()> {
        let matter_id = self.id().to_string();
        let g = if grain == "week" { "week" } else { "day" };
        self.with_transaction(|conn| {
            rebuild_aggregates_conn(conn, &matter_id, g)?;
            Ok(())
        })
    }

    /// Lookup person by id (matter-scoped).
    pub fn get_person(&self, person_id: &str) -> Result<Option<Person>> {
        let mut stmt = self.connection().prepare(&format!(
            "SELECT {PERSON_SELECT} FROM people WHERE id = ?1 AND matter_id = ?2"
        ))?;
        let mut rows = stmt.query(params![person_id, self.id()])?;
        match rows.next()? {
            Some(row) => Ok(Some(map_person_row(row)?)),
            None => Ok(None),
        }
    }
}

fn map_timeline_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PeopleTimelineBucket> {
    Ok(PeopleTimelineBucket {
        id: row.get(0)?,
        matter_id: row.get(1)?,
        bucket_start: row.get(2)?,
        grain: row.get(3)?,
        person_id: row.get(4)?,
        message_count: row.get(5)?,
    })
}

/// Bulk Pass-2 rebuild on an open connection (already inside a transaction).
fn rebuild_aggregates_conn(
    conn: &rusqlite::Connection,
    matter_id: &str,
    grain: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM people_edges WHERE matter_id = ?1",
        params![matter_id],
    )?;
    conn.execute(
        "DELETE FROM people_timeline WHERE matter_id = ?1",
        params![matter_id],
    )?;

    // Reset people counters for this matter.
    conn.execute(
        "UPDATE people SET \
            message_count = 0, \
            as_from_count = 0, \
            as_to_count = 0, \
            as_cc_count = 0, \
            as_bcc_count = 0, \
            self_mail_count = 0, \
            first_seen_at = NULL, \
            last_seen_at = NULL \
         WHERE matter_id = ?1",
        params![matter_id],
    )?;

    // Role counts + first/last.
    conn.execute(
        "UPDATE people SET \
            as_from_count = COALESCE(( \
                SELECT COUNT(*) FROM item_participants ip \
                WHERE ip.person_id = people.id AND ip.role = 'from'), 0), \
            as_to_count = COALESCE(( \
                SELECT COUNT(*) FROM item_participants ip \
                WHERE ip.person_id = people.id AND ip.role = 'to'), 0), \
            as_cc_count = COALESCE(( \
                SELECT COUNT(*) FROM item_participants ip \
                WHERE ip.person_id = people.id AND ip.role = 'cc'), 0), \
            as_bcc_count = COALESCE(( \
                SELECT COUNT(*) FROM item_participants ip \
                WHERE ip.person_id = people.id AND ip.role = 'bcc'), 0), \
            message_count = COALESCE(( \
                SELECT COUNT(DISTINCT ip.item_id) FROM item_participants ip \
                WHERE ip.person_id = people.id), 0), \
            first_seen_at = ( \
                SELECT MIN(ip.item_at) FROM item_participants ip \
                WHERE ip.person_id = people.id AND ip.item_at IS NOT NULL), \
            last_seen_at = ( \
                SELECT MAX(ip.item_at) FROM item_participants ip \
                WHERE ip.person_id = people.id AND ip.item_at IS NOT NULL) \
         WHERE matter_id = ?1",
        params![matter_id],
    )?;

    // Self-mail: from person also appears as to/cc/bcc on same item.
    conn.execute(
        "UPDATE people SET self_mail_count = COALESCE(( \
            SELECT COUNT(DISTINCT ip_from.item_id) \
            FROM item_participants ip_from \
            INNER JOIN item_participants ip_r \
              ON ip_r.item_id = ip_from.item_id \
             AND ip_r.person_id = ip_from.person_id \
             AND ip_r.role IN ('to', 'cc', 'bcc') \
            WHERE ip_from.person_id = people.id AND ip_from.role = 'from' \
         ), 0) \
         WHERE matter_id = ?1",
        params![matter_id],
    )?;

    // Directed edges: from × (to|cc|bcc), exclude self-loops; split counters.
    // Aggregate into a temp structure via INSERT…SELECT with CASE sums.
    let mut edge_stmt = conn.prepare(
        "SELECT f.person_id AS from_pid, r.person_id AS to_pid, r.role, \
                COUNT(*) AS cnt, \
                MIN(COALESCE(f.item_at, r.item_at)) AS first_at, \
                MAX(COALESCE(f.item_at, r.item_at)) AS last_at \
         FROM item_participants f \
         INNER JOIN item_participants r \
           ON r.item_id = f.item_id \
          AND r.matter_id = f.matter_id \
          AND r.role IN ('to', 'cc', 'bcc') \
         WHERE f.matter_id = ?1 \
           AND f.role = 'from' \
           AND f.person_id != r.person_id \
         GROUP BY f.person_id, r.person_id, r.role",
    )?;
    let edge_rows = edge_stmt.query_map(params![matter_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    // Accumulate per (from,to).
    use std::collections::HashMap;
    struct Acc {
        to_count: i64,
        cc_count: i64,
        bcc_count: i64,
        first_at: Option<String>,
        last_at: Option<String>,
    }
    let mut map: HashMap<(String, String), Acc> = HashMap::new();
    for row in edge_rows {
        let (from_pid, to_pid, role, cnt, first_at, last_at) = row?;
        let entry = map.entry((from_pid, to_pid)).or_insert(Acc {
            to_count: 0,
            cc_count: 0,
            bcc_count: 0,
            first_at: None,
            last_at: None,
        });
        match role.as_str() {
            "to" => entry.to_count += cnt,
            "cc" => entry.cc_count += cnt,
            "bcc" => entry.bcc_count += cnt,
            _ => {}
        }
        entry.first_at = min_opt_str(entry.first_at.take(), first_at);
        entry.last_at = max_opt_str(entry.last_at.take(), last_at);
    }
    drop(edge_stmt);

    let mut insert = conn.prepare(
        "INSERT INTO people_edges \
         (id, matter_id, from_person_id, to_person_id, to_count, cc_count, bcc_count, \
          visible_count, first_at, last_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for ((from_pid, to_pid), acc) in map {
        let visible = acc.to_count + acc.cc_count;
        let id = people_edge_id(matter_id, &from_pid, &to_pid);
        insert.execute(params![
            id,
            matter_id,
            from_pid,
            to_pid,
            acc.to_count,
            acc.cc_count,
            acc.bcc_count,
            visible,
            acc.first_at,
            acc.last_at,
        ])?;
    }
    drop(insert);

    // Timeline: one row per distinct item (use any participant's item_at; prefer from).
    // Matter-wide + per-person.
    rebuild_timeline(conn, matter_id, grain)?;

    Ok(())
}

fn rebuild_timeline(conn: &rusqlite::Connection, matter_id: &str, grain: &str) -> Result<()> {
    // Distinct items with a usable date (from participant rows).
    let mut stmt = conn.prepare(
        "SELECT item_id, MIN(item_at) AS item_at \
         FROM item_participants \
         WHERE matter_id = ?1 AND item_at IS NOT NULL AND TRIM(item_at) != '' \
         GROUP BY item_id",
    )?;
    let items = stmt.query_map(params![matter_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    use std::collections::HashMap;
    let mut matter_buckets: HashMap<String, i64> = HashMap::new();
    let mut item_bucket: HashMap<String, String> = HashMap::new();

    for row in items {
        let (item_id, item_at) = row?;
        let Some(bucket) = bucket_start_for(&item_at, grain) else {
            continue;
        };
        *matter_buckets.entry(bucket.clone()).or_insert(0) += 1;
        item_bucket.insert(item_id, bucket);
    }
    drop(stmt);

    let mut ins = conn.prepare(
        "INSERT INTO people_timeline (id, matter_id, bucket_start, grain, person_id, message_count) \
         VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
    )?;
    for (bucket, count) in &matter_buckets {
        let id = people_timeline_id(matter_id, grain, bucket, None);
        ins.execute(params![id, matter_id, bucket, grain, count])?;
    }
    drop(ins);

    // Per-person: distinct items each person appears on, bucketed.
    let mut pstmt =
        conn.prepare("SELECT person_id, item_id FROM item_participants WHERE matter_id = ?1")?;
    let prows = pstmt.query_map(params![matter_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut person_buckets: HashMap<(String, String), i64> = HashMap::new();
    // Dedup (person, item) first.
    let mut seen_pi: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    for row in prows {
        let (person_id, item_id) = row?;
        if !seen_pi.insert((person_id.clone(), item_id.clone())) {
            continue;
        }
        let Some(bucket) = item_bucket.get(&item_id) else {
            continue;
        };
        *person_buckets
            .entry((person_id, bucket.clone()))
            .or_insert(0) += 1;
    }
    drop(pstmt);

    let mut pins = conn.prepare(
        "INSERT INTO people_timeline (id, matter_id, bucket_start, grain, person_id, message_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for ((person_id, bucket), count) in person_buckets {
        let id = people_timeline_id(matter_id, grain, &bucket, Some(&person_id));
        pins.execute(params![id, matter_id, bucket, grain, person_id, count])?;
    }

    Ok(())
}

/// Day: `YYYY-MM-DD`; week: `YYYY-Www` via Monday-based week of year.
fn bucket_start_for(item_at: &str, grain: &str) -> Option<String> {
    let day = extract_ymd(item_at)?;
    if grain == "week" {
        // Parse as NaiveDate-ish from YMD.
        let parts: Vec<_> = day.split('-').collect();
        if parts.len() != 3 {
            return Some(day);
        }
        let y: i32 = parts[0].parse().ok()?;
        let m: u32 = parts[1].parse().ok()?;
        let d: u32 = parts[2].parse().ok()?;
        let date = chrono::NaiveDate::from_ymd_opt(y, m, d)?;
        let iso = date.iso_week();
        Some(format!("{:04}-W{:02}", iso.year(), iso.week()))
    } else {
        Some(day)
    }
}

fn extract_ymd(item_at: &str) -> Option<String> {
    let s = item_at.trim();
    if s.len() >= 10 {
        let ymd = &s[..10];
        if ymd.as_bytes().get(4) == Some(&b'-') && ymd.as_bytes().get(7) == Some(&b'-') {
            return Some(ymd.to_string());
        }
    }
    None
}

fn min_opt_str(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(x), Some(y)) => Some(if x <= y { x } else { y }),
    }
}

fn max_opt_str(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(x), Some(y)) => Some(if x >= y { x } else { y }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn person_id_is_full_64_hex_and_stable() {
        let a = person_id_for("smtp", "bob@example.com");
        let b = person_id_for("smtp", "bob@example.com");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Kind is part of the key — display "bob@example.com" differs.
        let c = person_id_for("display", "bob@example.com");
        assert_ne!(a, c);
    }

    #[test]
    fn bucket_day_and_week() {
        assert_eq!(
            bucket_start_for("2024-06-15T12:00:00Z", "day").as_deref(),
            Some("2024-06-15")
        );
        let w = bucket_start_for("2024-06-15T12:00:00Z", "week").expect("week");
        assert!(w.contains("-W"), "week={w}");
    }
}
