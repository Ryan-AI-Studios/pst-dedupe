//! Append-only audit log with integrity hash chain.
//!
//! # Hash chain contract
//!
//! - Rows are append-only (no update/delete via public APIs).
//! - `prev_hash` for `seq = 1` is the fixed genesis sentinel
//!   [`GENESIS_PREV_HASH`].
//! - `entry_hash` = SHA-256 over a **canonical** encoding of
//!   `(seq, ts, actor, action, entity, params, tool_version, prev_hash)`.
//! - [`verify_audit_chain`] walks the chain and fails on break or tamper.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::cas::sha256_hex;
use crate::error::{Error, Result};

/// Fixed genesis sentinel used as `prev_hash` for the first audit event.
pub const GENESIS_PREV_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// One append-only audit event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub seq: i64,
    pub ts: String,
    pub actor: String,
    pub action: String,
    pub entity: String,
    pub params_json: String,
    pub tool_version: String,
    pub prev_hash: String,
    pub entry_hash: String,
}

/// Input fields for appending an audit event (hashes and seq assigned by store).
#[derive(Debug, Clone)]
pub struct AuditEventInput {
    pub actor: String,
    pub action: String,
    pub entity: String,
    pub params_json: String,
    pub tool_version: String,
}

/// Fields hashed into an audit `entry_hash` (excluding the hash itself).
#[derive(Debug, Clone, Copy)]
pub struct AuditHashFields<'a> {
    /// Monotonic sequence number.
    pub seq: i64,
    /// Event timestamp (RFC3339).
    pub ts: &'a str,
    /// Actor (user/system/tool).
    pub actor: &'a str,
    /// Action name.
    pub action: &'a str,
    /// Entity reference.
    pub entity: &'a str,
    /// JSON parameters.
    pub params_json: &'a str,
    /// Tool/version string.
    pub tool_version: &'a str,
    /// Previous entry hash (or genesis).
    pub prev_hash: &'a str,
}

/// Canonical encoding used as the preimage of `entry_hash`.
///
/// Format (LF-separated):
/// ```text
/// seq=<n>
/// ts=<rfc3339>
/// actor=<actor>
/// action=<action>
/// entity=<entity>
/// params=<params_json>
/// tool_version=<tool_version>
/// prev_hash=<prev_hash>
/// ```
pub fn canonical_audit_preimage(fields: &AuditHashFields<'_>) -> String {
    format!(
        "seq={}\nts={}\nactor={}\naction={}\nentity={}\nparams={}\ntool_version={}\nprev_hash={}\n",
        fields.seq,
        fields.ts,
        fields.actor,
        fields.action,
        fields.entity,
        fields.params_json,
        fields.tool_version,
        fields.prev_hash,
    )
}

/// Compute `entry_hash` for the given fields.
pub fn compute_entry_hash(fields: &AuditHashFields<'_>) -> String {
    let preimage = canonical_audit_preimage(fields);
    sha256_hex(preimage.as_bytes())
}

/// Append one audit event. Returns the stored row including hashes.
///
/// Reads the latest link and inserts the next row inside a single
/// `BEGIN IMMEDIATE` transaction when the connection is not already inside one.
/// Nested callers (already in `with_transaction`) recompute under the outer txn
/// without opening a nested BEGIN.
pub(crate) fn append_event(
    conn: &Connection,
    input: &AuditEventInput,
    ts: &str,
) -> Result<AuditEvent> {
    // Own a writer lock only when not already inside an outer transaction.
    let own_txn = conn.is_autocommit();
    if own_txn {
        conn.execute("BEGIN IMMEDIATE", [])?;
    }

    let result = append_event_in_txn(conn, input, ts);

    if own_txn {
        match &result {
            Ok(_) => {
                if let Err(e) = conn.execute("COMMIT", []) {
                    let _ = conn.execute("ROLLBACK", []);
                    return Err(e.into());
                }
            }
            Err(_) => {
                let _ = conn.execute("ROLLBACK", []);
            }
        }
    }

    result
}

/// Compute next seq / prev_hash and insert. Must run under a write transaction
/// (or SQLite autocommit single-statement mode is insufficient for the pair).
fn append_event_in_txn(conn: &Connection, input: &AuditEventInput, ts: &str) -> Result<AuditEvent> {
    // Recompute link inside the transaction so concurrent writers serialize on
    // BEGIN IMMEDIATE and never collide on seq / prev_hash.
    let (next_seq, prev_hash) = latest_link(conn)?;

    let entry_hash = compute_entry_hash(&AuditHashFields {
        seq: next_seq,
        ts,
        actor: &input.actor,
        action: &input.action,
        entity: &input.entity,
        params_json: &input.params_json,
        tool_version: &input.tool_version,
        prev_hash: &prev_hash,
    });

    conn.execute(
        "INSERT INTO audit_events \
         (seq, ts, actor, action, entity, params_json, tool_version, prev_hash, entry_hash) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            next_seq,
            ts,
            input.actor,
            input.action,
            input.entity,
            input.params_json,
            input.tool_version,
            prev_hash,
            entry_hash,
        ],
    )?;

    Ok(AuditEvent {
        seq: next_seq,
        ts: ts.to_string(),
        actor: input.actor.clone(),
        action: input.action.clone(),
        entity: input.entity.clone(),
        params_json: input.params_json.clone(),
        tool_version: input.tool_version.clone(),
        prev_hash,
        entry_hash,
    })
}

fn latest_link(conn: &Connection) -> Result<(i64, String)> {
    let mut stmt =
        conn.prepare("SELECT seq, entry_hash FROM audit_events ORDER BY seq DESC LIMIT 1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let seq: i64 = row.get(0)?;
        let entry_hash: String = row.get(1)?;
        Ok((seq + 1, entry_hash))
    } else {
        Ok((1, GENESIS_PREV_HASH.to_string()))
    }
}

/// Walk the audit chain and verify integrity.
///
/// An empty log is considered valid (nothing to verify).
pub fn verify_audit_chain(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT seq, ts, actor, action, entity, params_json, tool_version, prev_hash, entry_hash \
         FROM audit_events ORDER BY seq ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut expected_seq: i64 = 1;
    let mut expected_prev = GENESIS_PREV_HASH.to_string();

    while let Some(row) = rows.next()? {
        let seq: i64 = row.get(0)?;
        let ts: String = row.get(1)?;
        let actor: String = row.get(2)?;
        let action: String = row.get(3)?;
        let entity: String = row.get(4)?;
        let params_json: String = row.get(5)?;
        let tool_version: String = row.get(6)?;
        let prev_hash: String = row.get(7)?;
        let entry_hash: String = row.get(8)?;

        if seq != expected_seq {
            return Err(Error::AuditChainBroken {
                seq,
                reason: format!("expected seq {expected_seq}, found {seq}"),
            });
        }
        if prev_hash != expected_prev {
            return Err(Error::AuditChainBroken {
                seq,
                reason: format!("prev_hash mismatch: expected {expected_prev}, found {prev_hash}"),
            });
        }

        let computed = compute_entry_hash(&AuditHashFields {
            seq,
            ts: &ts,
            actor: &actor,
            action: &action,
            entity: &entity,
            params_json: &params_json,
            tool_version: &tool_version,
            prev_hash: &prev_hash,
        });
        if computed != entry_hash {
            return Err(Error::AuditChainBroken {
                seq,
                reason: format!("entry_hash mismatch: expected {computed}, stored {entry_hash}"),
            });
        }

        expected_seq += 1;
        expected_prev = entry_hash;
    }

    Ok(())
}
