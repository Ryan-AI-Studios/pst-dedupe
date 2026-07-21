//! Multi-user identity, locks, batches, OCC, and sampling QC (track 0058).
//!
//! Solo Desk keeps `multi_user_enabled = 0` and free-string actors. The matter
//! service enables multi-user + strict actor mode and serializes writes.

use std::collections::HashSet;

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use chrono::{Duration, Utc};
use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::audit::{self, AuditEventInput};
use crate::error::{Error, Result};
use crate::matter::{new_id, now_rfc3339, Matter};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default item-lock TTL (hours).
pub const DEFAULT_LOCK_TTL_HOURS: i64 = 4;
/// Default session TTL (hours).
pub const DEFAULT_SESSION_TTL_HOURS: i64 = 12;
/// Role: full admin (users, force-unlock, jobs).
pub const ROLE_ADMIN: &str = "admin";
/// Role: coding / notes / privilege / checkout.
pub const ROLE_REVIEWER: &str = "reviewer";
/// Role: read-only list/body/search.
pub const ROLE_READ_ONLY: &str = "read_only";

/// Batch status: open for checkout.
pub const BATCH_STATUS_OPEN: &str = "open";
/// Batch status: closed.
pub const BATCH_STATUS_CLOSED: &str = "closed";

/// QC sample status.
pub const QC_SAMPLE_STATUS_OPEN: &str = "open";
/// QC outcome: agree with primary coding.
pub const QC_OUTCOME_AGREE: &str = "agree";
/// QC outcome: disagree.
pub const QC_OUTCOME_DISAGREE: &str = "disagree";
/// QC outcome: corrected (disagree + fix).
pub const QC_OUTCOME_CORRECTED: &str = "corrected";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Matter-scoped user (local identity only — no OIDC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatterUser {
    pub id: String,
    pub display_name: String,
    pub role: String,
    pub created_at: String,
    pub disabled_at: Option<String>,
}

/// Issued session (raw token returned **once** at login).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionIssue {
    pub token: String,
    pub user: MatterUser,
    pub expires_at: String,
}

/// Item soft-lock row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemLock {
    pub item_id: String,
    pub user_id: String,
    pub locked_at: String,
    pub expires_at: String,
    pub reason: Option<String>,
}

/// Review batch header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewBatch {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub status: String,
    pub filter_json: Option<String>,
    pub created_at: String,
}

/// Active (or historical) batch checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchCheckout {
    pub batch_id: String,
    pub user_id: String,
    pub checked_out_at: String,
    pub checked_in_at: Option<String>,
}

/// Batch-constrained item row (feed for `GET /v1/batches/{id}/items`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchItemRow {
    pub item_id: String,
    pub review_version: i64,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub sent_at: Option<String>,
}

/// Thin item projection for service list/get (includes OCC version).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemThinRow {
    pub id: String,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub sent_at: Option<String>,
    pub review_version: i64,
    pub status: String,
}

/// Review body payload for multi-user service clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemBodyPayload {
    pub item_id: String,
    pub content_type: String,
    pub text: String,
    pub digest: Option<String>,
    pub review_version: i64,
    pub truncated: bool,
}

/// Sampling QC set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QcSample {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub sample_pct: Option<f64>,
    pub sample_n: Option<i64>,
    pub seed: i64,
    pub created_at: String,
    pub status: String,
}

/// One item in a QC sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QcSampleItem {
    pub sample_id: String,
    pub item_id: String,
    pub primary_coder: Option<String>,
    pub outcome: Option<String>,
    pub notes: Option<String>,
    pub recorded_by: Option<String>,
    pub recorded_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Secret hashing
// ---------------------------------------------------------------------------

/// Hash a password or API-token material with Argon2id (PHC string).
pub fn hash_secret(secret: &str) -> Result<String> {
    if secret.is_empty() {
        return Err(Error::Other("secret must not be empty".into()));
    }
    let salt = SaltString::generate(&mut OsRng);
    let argon = Argon2::default();
    let hash = argon
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|e| Error::Other(format!("argon2 hash failed: {e}")))?;
    Ok(hash.to_string())
}

/// Verify a secret against a stored Argon2id PHC hash.
pub fn verify_secret(secret: &str, secret_hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(secret_hash)
        .map_err(|e| Error::Other(format!("invalid secret_hash: {e}")))?;
    Ok(Argon2::default()
        .verify_password(secret.as_bytes(), &parsed)
        .is_ok())
}

/// SHA-256 hex digest of a bearer token (stored at rest; raw token only at issue).
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let dig = hasher.finalize();
    hex_encode(&dig)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    // URL-safe-ish hex (no padding issues).
    hex_encode(&bytes)
}

fn parse_role(role: &str) -> Result<String> {
    let r = role.trim();
    match r {
        ROLE_ADMIN | ROLE_REVIEWER | ROLE_READ_ONLY => Ok(r.to_string()),
        _ => Err(Error::Other(format!(
            "invalid role '{role}'; expected admin|reviewer|read_only"
        ))),
    }
}

/// Role rank for [`require_role`] (higher = more privileged).
fn role_rank(role: &str) -> i32 {
    match role {
        ROLE_ADMIN => 3,
        ROLE_REVIEWER => 2,
        ROLE_READ_ONLY => 1,
        _ => 0,
    }
}

/// Fail closed unless `user.role` meets or exceeds `min_role`.
pub fn require_role(user: &MatterUser, min_role: &str) -> Result<()> {
    if user.disabled_at.is_some() {
        return Err(Error::Forbidden("user is disabled".into()));
    }
    let need = role_rank(min_role);
    if need == 0 {
        return Err(Error::Other(format!("unknown min_role '{min_role}'")));
    }
    if role_rank(&user.role) < need {
        return Err(Error::Forbidden(format!(
            "role '{}' insufficient; requires at least '{min_role}'",
            user.role
        )));
    }
    Ok(())
}

fn map_user_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MatterUser> {
    Ok(MatterUser {
        id: row.get(0)?,
        display_name: row.get(1)?,
        role: row.get(2)?,
        created_at: row.get(3)?,
        disabled_at: row.get(4)?,
    })
}

// ---------------------------------------------------------------------------
// Conn-level helpers (usable inside transactions)
// ---------------------------------------------------------------------------

pub(crate) fn get_review_version_conn(conn: &Connection, item_id: &str) -> Result<i64> {
    conn.query_row(
        "SELECT review_version FROM items WHERE id = ?1",
        params![item_id],
        |row| row.get(0),
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Error::ItemNotFound(item_id.to_string()),
        other => Error::Sqlite(other),
    })
}

/// Check OCC and bump `review_version` in the same txn. Returns new version.
///
/// When `expected` is `None` and `require_expected` is false, bump without check.
/// When `require_expected` is true, `expected` must be `Some`.
pub(crate) fn bump_review_version_conn(
    conn: &Connection,
    item_id: &str,
    expected: Option<i64>,
    require_expected: bool,
) -> Result<i64> {
    if require_expected && expected.is_none() {
        return Err(Error::Other(
            "expected_version is required for this mutate".into(),
        ));
    }
    let actual = get_review_version_conn(conn, item_id)?;
    if let Some(exp) = expected {
        if exp != actual {
            return Err(Error::VersionConflict {
                expected: exp,
                actual,
            });
        }
    }
    conn.execute(
        "UPDATE items SET review_version = review_version + 1 WHERE id = ?1",
        params![item_id],
    )?;
    Ok(actual + 1)
}

/// Active (non-expired) lock for an item, if any. Expired locks are deleted.
pub(crate) fn active_lock_conn(
    conn: &Connection,
    item_id: &str,
    now: &str,
) -> Result<Option<ItemLock>> {
    let row: Option<(String, String, String, String, Option<String>)> = conn
        .query_row(
            "SELECT item_id, user_id, locked_at, expires_at, reason \
             FROM item_locks WHERE item_id = ?1",
            params![item_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((item_id, user_id, locked_at, expires_at, reason)) = row else {
        return Ok(None);
    };
    if expires_at.as_str() <= now {
        conn.execute(
            "DELETE FROM item_locks WHERE item_id = ?1",
            params![item_id],
        )?;
        return Ok(None);
    }
    Ok(Some(ItemLock {
        item_id,
        user_id,
        locked_at,
        expires_at,
        reason,
    }))
}

/// Fail closed if another user holds a non-expired lock, or if the user has an
/// active batch checkout that does not include this item.
///
/// When multi-user is off, this is a no-op (`enforce = false`).
///
/// **Batch-constrained mode (spec §3.4.6):** if `user_id` has one or more
/// active batch checkouts, the item must be a member of at least one of those
/// batches. Users with no active checkout may mutate any unlocked item (global
/// corpus mode).
pub(crate) fn assert_can_mutate_conn(
    conn: &Connection,
    item_id: &str,
    user_id: &str,
    now: &str,
    enforce: bool,
) -> Result<()> {
    if !enforce {
        return Ok(());
    }
    if let Some(lock) = active_lock_conn(conn, item_id, now)? {
        if lock.user_id != user_id {
            return Err(Error::Locked {
                item_id: item_id.to_string(),
                by_user: lock.user_id,
            });
        }
    }
    // Batch-constrained mutate: active checkout(s) narrow allowed items.
    assert_batch_mutate_allowed_conn(conn, item_id, user_id)?;
    Ok(())
}

/// When the user holds any active batch checkout, require membership.
pub(crate) fn assert_batch_mutate_allowed_conn(
    conn: &Connection,
    item_id: &str,
    user_id: &str,
) -> Result<()> {
    let checkout_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM batch_checkouts \
         WHERE user_id = ?1 AND checked_in_at IS NULL",
        params![user_id],
        |row| row.get(0),
    )?;
    if checkout_count == 0 {
        return Ok(());
    }
    let in_checked_out: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM batch_checkouts bc \
         INNER JOIN review_batch_items bi ON bi.batch_id = bc.batch_id \
         WHERE bc.user_id = ?1 AND bc.checked_in_at IS NULL AND bi.item_id = ?2",
        params![user_id, item_id],
        |row| row.get(0),
    )?;
    if !in_checked_out {
        return Err(Error::Forbidden(format!(
            "item {item_id} is outside your checked-out batch(es); check in or mutate batch members only"
        )));
    }
    Ok(())
}

pub(crate) fn is_multi_user_enabled_conn(conn: &Connection) -> Result<bool> {
    let v: i64 = conn
        .query_row(
            "SELECT multi_user_enabled FROM matters LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    Ok(v != 0)
}

pub(crate) fn load_user_conn(conn: &Connection, user_id: &str) -> Result<Option<MatterUser>> {
    conn.query_row(
        "SELECT id, display_name, role, created_at, disabled_at \
         FROM matter_users WHERE id = ?1",
        params![user_id],
        map_user_row,
    )
    .optional()
    .map_err(Error::from)
}

/// Resolve actor for strict mode: must be a non-disabled `matter_users.id`.
pub(crate) fn resolve_strict_actor_conn(conn: &Connection, actor: &str) -> Result<String> {
    let t = actor.trim();
    if t.is_empty() {
        return Err(Error::Unauthorized(
            "strict actor mode requires a valid user id".into(),
        ));
    }
    let user = load_user_conn(conn, t)?.ok_or_else(|| {
        Error::Unauthorized(format!(
            "strict actor mode: actor '{t}' is not a matter user id"
        ))
    })?;
    if user.disabled_at.is_some() {
        return Err(Error::Forbidden("user is disabled".into()));
    }
    Ok(user.id)
}

// ---------------------------------------------------------------------------
// Matter methods
// ---------------------------------------------------------------------------

impl Matter {
    /// Whether multi-user mode is enabled on this matter.
    pub fn is_multi_user_enabled(&self) -> Result<bool> {
        is_multi_user_enabled_conn(self.connection())
    }

    /// Enable multi-user mode (admin bootstrap step). Idempotent.
    pub fn enable_multi_user(&self, actor: &str) -> Result<()> {
        let actor = self.resolve_actor_for_mutate(actor)?;
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            conn.execute(
                "UPDATE matters SET multi_user_enabled = 1 WHERE id = ?1",
                params![self.id()],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "multi_user.enable".into(),
                    entity: format!("matter:{}", self.id()),
                    params_json: "{}".into(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })
    }

    /// Create a matter-scoped user. `secret` is password or API-token material.
    pub fn create_user(
        &self,
        display_name: &str,
        role: &str,
        secret: &str,
        actor: &str,
    ) -> Result<MatterUser> {
        let name = display_name.trim();
        if name.is_empty() {
            return Err(Error::Other("display_name must not be empty".into()));
        }
        let role = parse_role(role)?;
        let secret_hash = hash_secret(secret)?;
        let actor = self.resolve_actor_for_mutate(actor)?;
        let now = now_rfc3339();
        // Spec §3.3.1: UUID text PK for matter_users.
        let id = uuid::Uuid::new_v4().to_string();

        self.with_transaction(|conn| {
            // Case-fold uniqueness via unique index on lower(display_name).
            let clash: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM matter_users WHERE lower(display_name) = lower(?1)",
                params![name],
                |row| row.get(0),
            )?;
            if clash {
                return Err(Error::Conflict {
                    message: format!("display_name '{name}' already exists"),
                });
            }
            conn.execute(
                "INSERT INTO matter_users (id, display_name, role, secret_hash, created_at, disabled_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                params![id, name, role, secret_hash, now],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "user.create".into(),
                    entity: format!("user:{id}"),
                    params_json: serde_json::json!({
                        "display_name": name,
                        "role": role,
                    })
                    .to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;

        self.get_user(&id)?
            .ok_or_else(|| Error::Other(format!("user {id} missing after create")))
    }

    /// List all users (including disabled).
    pub fn list_users(&self) -> Result<Vec<MatterUser>> {
        let mut stmt = self.connection().prepare(
            "SELECT id, display_name, role, created_at, disabled_at \
             FROM matter_users ORDER BY display_name COLLATE NOCASE, id",
        )?;
        let rows = stmt.query_map([], map_user_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Load one user by id.
    pub fn get_user(&self, user_id: &str) -> Result<Option<MatterUser>> {
        load_user_conn(self.connection(), user_id)
    }

    /// Soft-disable a user (sessions remain until expiry; mutate paths reject disabled).
    pub fn disable_user(&self, user_id: &str, actor: &str) -> Result<()> {
        let actor = self.resolve_actor_for_mutate(actor)?;
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            let user = load_user_conn(conn, user_id)?
                .ok_or_else(|| Error::Other(format!("user not found: {user_id}")))?;
            if user.disabled_at.is_some() {
                return Ok(());
            }
            conn.execute(
                "UPDATE matter_users SET disabled_at = ?1 WHERE id = ?2",
                params![now, user_id],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: actor.clone(),
                    action: "user.disable".into(),
                    entity: format!("user:{user_id}"),
                    params_json: "{}".into(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })
    }

    /// Authenticate by display name + password; issue a bearer session token.
    pub fn authenticate(&self, display_name: &str, password: &str) -> Result<SessionIssue> {
        self.authenticate_with_ttl(display_name, password, DEFAULT_SESSION_TTL_HOURS)
    }

    /// Authenticate with custom session TTL (hours).
    pub fn authenticate_with_ttl(
        &self,
        display_name: &str,
        password: &str,
        ttl_hours: i64,
    ) -> Result<SessionIssue> {
        let name = display_name.trim();
        let row: Option<(String, String, String, String, Option<String>, String)> = self
            .connection()
            .query_row(
                "SELECT id, display_name, role, created_at, disabled_at, secret_hash \
                 FROM matter_users WHERE lower(display_name) = lower(?1)",
                params![name],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, display_name, role, created_at, disabled_at, secret_hash)) = row else {
            return Err(Error::Unauthorized("invalid credentials".into()));
        };
        if disabled_at.is_some() {
            return Err(Error::Forbidden("user is disabled".into()));
        }
        if !verify_secret(password, &secret_hash)? {
            return Err(Error::Unauthorized("invalid credentials".into()));
        }
        let user = MatterUser {
            id: id.clone(),
            display_name,
            role,
            created_at,
            disabled_at: None,
        };
        let token = random_token();
        let token_hash = hash_token(&token);
        let now = now_rfc3339();
        let expires_at = (Utc::now() + Duration::hours(ttl_hours.max(1)))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        self.connection().execute(
            "INSERT INTO matter_sessions (token_hash, user_id, expires_at, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            params![token_hash, id, expires_at, now],
        )?;
        Ok(SessionIssue {
            token,
            user,
            expires_at,
        })
    }

    /// Resolve a bearer token to a non-disabled user (expired sessions fail closed).
    pub fn resolve_session(&self, token: &str) -> Result<MatterUser> {
        let token = token.trim();
        if token.is_empty() {
            return Err(Error::Unauthorized("missing token".into()));
        }
        let th = hash_token(token);
        let now = now_rfc3339();
        let row: Option<(String, String)> = self
            .connection()
            .query_row(
                "SELECT user_id, expires_at FROM matter_sessions WHERE token_hash = ?1",
                params![th],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((user_id, expires_at)) = row else {
            return Err(Error::Unauthorized("invalid token".into()));
        };
        if expires_at.as_str() <= now.as_str() {
            let _ = self.connection().execute(
                "DELETE FROM matter_sessions WHERE token_hash = ?1",
                params![th],
            );
            return Err(Error::Unauthorized("session expired".into()));
        }
        let user = load_user_conn(self.connection(), &user_id)?
            .ok_or_else(|| Error::Unauthorized("user not found for session".into()))?;
        if user.disabled_at.is_some() {
            return Err(Error::Forbidden("user is disabled".into()));
        }
        Ok(user)
    }

    /// Current `review_version` for an item.
    pub fn get_review_version(&self, item_id: &str) -> Result<i64> {
        get_review_version_conn(self.connection(), item_id)
    }

    /// Thin item list for multi-user service (keyset on id ASC).
    pub fn list_items_thin(
        &self,
        after_item_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ItemThinRow>> {
        self.list_items_thin_for_user(None, after_item_id, limit)
    }

    /// True when `user_id` holds one or more active batch checkouts.
    pub fn user_has_active_batch_checkout(&self, user_id: &str) -> Result<bool> {
        let n: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM batch_checkouts \
             WHERE user_id = ?1 AND checked_in_at IS NULL",
            params![user_id],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    }

    /// Thin list; when `user_id` has an active batch checkout, results are
    /// constrained to membership of those checked-out batches (spec §3.4.6).
    pub fn list_items_thin_for_user(
        &self,
        user_id: Option<&str>,
        after_item_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ItemThinRow>> {
        let limit = limit.clamp(1, 500);
        let constrain_uid = match user_id {
            Some(uid) if self.user_has_active_batch_checkout(uid)? => Some(uid),
            _ => None,
        };
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<ItemThinRow> {
            Ok(ItemThinRow {
                id: row.get(0)?,
                subject: row.get(1)?,
                from_addr: row.get(2)?,
                sent_at: row.get(3)?,
                review_version: row.get(4)?,
                status: row.get(5)?,
            })
        };
        if let Some(uid) = constrain_uid {
            let mut sql = String::from(
                "SELECT i.id, i.subject, i.from_addr, i.sent_at, i.review_version, i.status \
                 FROM items i \
                 WHERE i.matter_id = ?1 \
                   AND i.id IN ( \
                     SELECT bi.item_id FROM review_batch_items bi \
                     INNER JOIN batch_checkouts bc ON bc.batch_id = bi.batch_id \
                     WHERE bc.user_id = ?2 AND bc.checked_in_at IS NULL \
                   )",
            );
            if after_item_id.is_some() {
                sql.push_str(" AND i.id > ?3");
            }
            sql.push_str(" ORDER BY i.id ASC LIMIT ");
            sql.push_str(&limit.to_string());
            let mut stmt = self.connection().prepare(&sql)?;
            let rows = if let Some(after) = after_item_id {
                stmt.query_map(params![self.id(), uid, after], map)?
            } else {
                stmt.query_map(params![self.id(), uid], map)?
            };
            return rows
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::from);
        }
        let mut sql = String::from(
            "SELECT id, subject, from_addr, sent_at, review_version, status \
             FROM items WHERE matter_id = ?1",
        );
        if after_item_id.is_some() {
            sql.push_str(" AND id > ?2");
        }
        sql.push_str(" ORDER BY id ASC LIMIT ");
        sql.push_str(&limit.to_string());
        let mut stmt = self.connection().prepare(&sql)?;
        let rows = if let Some(after) = after_item_id {
            stmt.query_map(params![self.id(), after], map)?
        } else {
            stmt.query_map(params![self.id()], map)?
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// Fail closed when the user is in batch mode and the item is outside it.
    pub fn assert_user_can_view_item(&self, user_id: &str, item_id: &str) -> Result<()> {
        assert_batch_mutate_allowed_conn(self.connection(), item_id, user_id)
    }

    /// Load review body text for service clients (prefers plain text CAS, then HTML).
    ///
    /// Caps at `max_bytes` (default 2 MiB). Returns empty body when no text CAS.
    pub fn load_item_body_for_service(
        &self,
        item_id: &str,
        max_bytes: u64,
    ) -> Result<ItemBodyPayload> {
        let item = self.get_item(item_id)?;
        let review_version = self.get_review_version(item_id)?;
        let max_bytes = max_bytes.clamp(1, 8 * 1024 * 1024);
        let max_usize = usize::try_from(max_bytes).unwrap_or(usize::MAX);
        let load = |dig: &str, content_type: &str| -> Result<ItemBodyPayload> {
            let len = self.cas_len(dig)?;
            let truncated = len > max_bytes;
            let bytes = if truncated {
                self.read_cas_prefix(dig, max_usize)?
            } else {
                self.get_bytes(dig)?
            };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            Ok(ItemBodyPayload {
                item_id: item_id.to_string(),
                content_type: content_type.into(),
                text,
                digest: Some(dig.to_string()),
                review_version,
                truncated,
            })
        };
        if let Some(ref dig) = item.text_sha256 {
            return load(dig, "text/plain");
        }
        if let Some(ref dig) = item.html_sha256 {
            return load(dig, "text/html");
        }
        Ok(ItemBodyPayload {
            item_id: item_id.to_string(),
            content_type: "text/plain".into(),
            text: String::new(),
            digest: None,
            review_version,
            truncated: false,
        })
    }

    /// Acquire (or refresh) an item lock for `user_id`.
    pub fn lock_item(
        &self,
        item_id: &str,
        user_id: &str,
        reason: Option<&str>,
        ttl_hours: Option<i64>,
    ) -> Result<ItemLock> {
        self.ensure_item_in_matter(item_id)?;
        let user = load_user_conn(self.connection(), user_id)?
            .ok_or_else(|| Error::Other(format!("user not found: {user_id}")))?;
        if user.disabled_at.is_some() {
            return Err(Error::Forbidden("user is disabled".into()));
        }
        let now = now_rfc3339();
        let ttl = ttl_hours.unwrap_or(DEFAULT_LOCK_TTL_HOURS).max(1);
        let expires_at = (Utc::now() + Duration::hours(ttl))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let reason = reason.map(|s| s.to_string());

        self.with_transaction(|conn| {
            if let Some(existing) = active_lock_conn(conn, item_id, &now)? {
                if existing.user_id != user_id {
                    return Err(Error::Locked {
                        item_id: item_id.to_string(),
                        by_user: existing.user_id,
                    });
                }
            }
            conn.execute(
                "INSERT INTO item_locks (item_id, user_id, locked_at, expires_at, reason) \
                 VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(item_id) DO UPDATE SET \
                   user_id = excluded.user_id, \
                   locked_at = excluded.locked_at, \
                   expires_at = excluded.expires_at, \
                   reason = excluded.reason",
                params![item_id, user_id, now, expires_at, reason],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: user_id.to_string(),
                    action: "item.lock".into(),
                    entity: format!("item:{item_id}"),
                    params_json: serde_json::json!({ "expires_at": expires_at }).to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;

        Ok(ItemLock {
            item_id: item_id.to_string(),
            user_id: user_id.to_string(),
            locked_at: now,
            expires_at,
            reason,
        })
    }

    /// Release an item lock held by `user_id` (fail closed if foreign lock).
    pub fn unlock_item(&self, item_id: &str, user_id: &str) -> Result<()> {
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            if let Some(existing) = active_lock_conn(conn, item_id, &now)? {
                if existing.user_id != user_id {
                    return Err(Error::Locked {
                        item_id: item_id.to_string(),
                        by_user: existing.user_id,
                    });
                }
            }
            conn.execute(
                "DELETE FROM item_locks WHERE item_id = ?1",
                params![item_id],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: user_id.to_string(),
                    action: "item.unlock".into(),
                    entity: format!("item:{item_id}"),
                    params_json: "{}".into(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })
    }

    /// Admin force-release of an item lock.
    pub fn force_unlock(&self, item_id: &str, admin_user_id: &str) -> Result<()> {
        let admin = load_user_conn(self.connection(), admin_user_id)?
            .ok_or_else(|| Error::Unauthorized(format!("user not found: {admin_user_id}")))?;
        require_role(&admin, ROLE_ADMIN)?;
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            let prior = active_lock_conn(conn, item_id, &now)?;
            conn.execute(
                "DELETE FROM item_locks WHERE item_id = ?1",
                params![item_id],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: admin_user_id.to_string(),
                    action: "item.force_unlock".into(),
                    entity: format!("item:{item_id}"),
                    params_json: serde_json::json!({
                        "prior_user": prior.as_ref().map(|l| &l.user_id),
                    })
                    .to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })
    }

    /// Assert the user may mutate the item (lock ownership when multi-user on).
    pub fn assert_can_mutate(&self, item_id: &str, user_id: &str) -> Result<()> {
        let enforce = self.is_multi_user_enabled()?;
        let now = now_rfc3339();
        assert_can_mutate_conn(self.connection(), item_id, user_id, &now, enforce)
    }

    /// Create a review batch from an explicit item id list.
    pub fn create_batch(
        &self,
        name: &str,
        item_ids: &[String],
        created_by: &str,
        filter_json: Option<&str>,
    ) -> Result<ReviewBatch> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Other("batch name must not be empty".into()));
        }
        if item_ids.is_empty() {
            return Err(Error::Other("batch requires at least one item_id".into()));
        }
        let created_by = self.resolve_actor_for_mutate(created_by)?;
        let now = now_rfc3339();
        let id = new_id("batch");
        let mut unique: Vec<String> = item_ids.to_vec();
        unique.sort();
        unique.dedup();

        self.with_transaction(|conn| {
            for iid in &unique {
                let ok: bool = conn.query_row(
                    "SELECT COUNT(*) > 0 FROM items WHERE id = ?1 AND matter_id = ?2",
                    params![iid, self.id()],
                    |row| row.get(0),
                )?;
                if !ok {
                    return Err(Error::ItemNotFound(iid.clone()));
                }
            }
            conn.execute(
                "INSERT INTO review_batches (id, name, created_by, status, filter_json, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, name, created_by, BATCH_STATUS_OPEN, filter_json, now],
            )?;
            for iid in &unique {
                conn.execute(
                    "INSERT INTO review_batch_items (batch_id, item_id) VALUES (?1, ?2)",
                    params![id, iid],
                )?;
            }
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: created_by.clone(),
                    action: "batch.create".into(),
                    entity: format!("batch:{id}"),
                    params_json: serde_json::json!({
                        "name": name,
                        "item_count": unique.len(),
                    })
                    .to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;

        Ok(ReviewBatch {
            id,
            name: name.to_string(),
            created_by,
            status: BATCH_STATUS_OPEN.to_string(),
            filter_json: filter_json.map(|s| s.to_string()),
            created_at: now,
        })
    }

    /// Checkout a batch exclusively for `user_id` (one active checkout per batch).
    pub fn checkout_batch(&self, batch_id: &str, user_id: &str) -> Result<BatchCheckout> {
        let user = load_user_conn(self.connection(), user_id)?
            .ok_or_else(|| Error::Other(format!("user not found: {user_id}")))?;
        require_role(&user, ROLE_REVIEWER)?;
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            let status: String = conn
                .query_row(
                    "SELECT status FROM review_batches WHERE id = ?1",
                    params![batch_id],
                    |r| r.get(0),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        Error::Other(format!("batch not found: {batch_id}"))
                    }
                    other => Error::Sqlite(other),
                })?;
            if status != BATCH_STATUS_OPEN {
                return Err(Error::Conflict {
                    message: format!("batch {batch_id} is not open"),
                });
            }
            // Any active checkout by another user → conflict.
            let active: Option<String> = conn
                .query_row(
                    "SELECT user_id FROM batch_checkouts \
                     WHERE batch_id = ?1 AND checked_in_at IS NULL LIMIT 1",
                    params![batch_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(other) = active {
                if other != user_id {
                    return Err(Error::Conflict {
                        message: format!("batch {batch_id} is checked out by {other}"),
                    });
                }
                // Already held by this user — return existing.
                let checked_out_at: String = conn.query_row(
                    "SELECT checked_out_at FROM batch_checkouts \
                     WHERE batch_id = ?1 AND user_id = ?2 AND checked_in_at IS NULL",
                    params![batch_id, user_id],
                    |r| r.get(0),
                )?;
                return Ok(BatchCheckout {
                    batch_id: batch_id.to_string(),
                    user_id: user_id.to_string(),
                    checked_out_at,
                    checked_in_at: None,
                });
            }
            // Fresh checkout or re-checkout after check-in (PK is batch_id+user_id).
            conn.execute(
                "INSERT INTO batch_checkouts (batch_id, user_id, checked_out_at, checked_in_at) \
                 VALUES (?1, ?2, ?3, NULL) \
                 ON CONFLICT(batch_id, user_id) DO UPDATE SET \
                   checked_out_at = excluded.checked_out_at, \
                   checked_in_at = NULL",
                params![batch_id, user_id, now],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: user_id.to_string(),
                    action: "batch.checkout".into(),
                    entity: format!("batch:{batch_id}"),
                    params_json: "{}".into(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(BatchCheckout {
                batch_id: batch_id.to_string(),
                user_id: user_id.to_string(),
                checked_out_at: now.clone(),
                checked_in_at: None,
            })
        })
    }

    /// Check in a batch previously checked out by `user_id`.
    pub fn checkin_batch(&self, batch_id: &str, user_id: &str) -> Result<()> {
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            let n = conn.execute(
                "UPDATE batch_checkouts SET checked_in_at = ?1 \
                 WHERE batch_id = ?2 AND user_id = ?3 AND checked_in_at IS NULL",
                params![now, batch_id, user_id],
            )?;
            if n == 0 {
                return Err(Error::Conflict {
                    message: format!("no active checkout of batch {batch_id} for user {user_id}"),
                });
            }
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: user_id.to_string(),
                    action: "batch.checkin".into(),
                    entity: format!("batch:{batch_id}"),
                    params_json: "{}".into(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })
    }

    /// List items in a batch (keyset on item_id ASC, limit).
    ///
    /// Membership is **authoritative** — callers cannot widen via FilterSpec.
    pub fn list_batch_items(
        &self,
        batch_id: &str,
        after_item_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BatchItemRow>> {
        let limit = limit.clamp(1, 500);
        let exists: bool = self.connection().query_row(
            "SELECT COUNT(*) > 0 FROM review_batches WHERE id = ?1",
            params![batch_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(Error::Other(format!("batch not found: {batch_id}")));
        }
        let mut sql = String::from(
            "SELECT b.item_id, i.review_version, i.subject, i.from_addr, i.sent_at \
             FROM review_batch_items b \
             JOIN items i ON i.id = b.item_id \
             WHERE b.batch_id = ?1",
        );
        if after_item_id.is_some() {
            sql.push_str(" AND b.item_id > ?2");
        }
        sql.push_str(" ORDER BY b.item_id ASC LIMIT ");
        sql.push_str(&limit.to_string());

        let mut stmt = self.connection().prepare(&sql)?;
        let map = |row: &rusqlite::Row<'_>| -> rusqlite::Result<BatchItemRow> {
            Ok(BatchItemRow {
                item_id: row.get(0)?,
                review_version: row.get(1)?,
                subject: row.get(2)?,
                from_addr: row.get(3)?,
                sent_at: row.get(4)?,
            })
        };
        let rows = if let Some(after) = after_item_id {
            stmt.query_map(params![batch_id, after], map)?
        } else {
            stmt.query_map(params![batch_id], map)?
        };
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// True when `item_id` is a member of `batch_id`.
    pub fn item_in_batch(&self, batch_id: &str, item_id: &str) -> Result<bool> {
        let ok: bool = self.connection().query_row(
            "SELECT COUNT(*) > 0 FROM review_batch_items \
             WHERE batch_id = ?1 AND item_id = ?2",
            params![batch_id, item_id],
            |r| r.get(0),
        )?;
        Ok(ok)
    }

    /// Fail closed unless the item is in a batch actively checked out by `user_id`.
    pub fn assert_item_in_checked_out_batch(
        &self,
        batch_id: &str,
        item_id: &str,
        user_id: &str,
    ) -> Result<()> {
        if !self.item_in_batch(batch_id, item_id)? {
            return Err(Error::Forbidden(format!(
                "item {item_id} is not in batch {batch_id}"
            )));
        }
        let active: bool = self.connection().query_row(
            "SELECT COUNT(*) > 0 FROM batch_checkouts \
             WHERE batch_id = ?1 AND user_id = ?2 AND checked_in_at IS NULL",
            params![batch_id, user_id],
            |r| r.get(0),
        )?;
        if !active {
            return Err(Error::Forbidden(format!(
                "batch {batch_id} is not checked out by {user_id}"
            )));
        }
        Ok(())
    }

    /// Create a deterministic QC sample from currently coded items.
    ///
    /// Provide `sample_pct` (0..100] **or** fixed `sample_n` (at least one required).
    pub fn create_qc_sample(
        &self,
        name: &str,
        created_by: &str,
        sample_pct: Option<f64>,
        sample_n: Option<i64>,
        seed: i64,
    ) -> Result<(QcSample, Vec<QcSampleItem>)> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Other("qc sample name must not be empty".into()));
        }
        if sample_pct.is_none() && sample_n.is_none() {
            return Err(Error::Other(
                "create_qc_sample requires sample_pct or sample_n".into(),
            ));
        }
        if let Some(p) = sample_pct {
            if !(p > 0.0 && p <= 100.0) {
                return Err(Error::Other("sample_pct must be in (0, 100]".into()));
            }
        }
        let created_by = self.resolve_actor_for_mutate(created_by)?;
        let now = now_rfc3339();
        let id = new_id("qcs");

        // Collect coded item ids + last setter as primary_coder.
        let mut coded: Vec<(String, Option<String>)> = {
            let mut stmt = self.connection().prepare(
                "SELECT ic.item_id, ic.set_by \
                 FROM item_codes ic \
                 JOIN items i ON i.id = ic.item_id \
                 WHERE i.matter_id = ?1 \
                 ORDER BY ic.item_id, ic.set_at DESC",
            )?;
            let rows = stmt.query_map(params![self.id()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for row in rows {
                let (item_id, set_by) = row?;
                if seen.insert(item_id.clone()) {
                    out.push((item_id, set_by));
                }
            }
            out
        };
        // Deterministic order then seeded shuffle (Fisher–Yates with LCG).
        coded.sort_by(|a, b| a.0.cmp(&b.0));
        seeded_shuffle(&mut coded, seed as u64);

        let n = if let Some(fixed) = sample_n {
            fixed.max(0) as usize
        } else if let Some(pct) = sample_pct {
            let raw = (coded.len() as f64 * pct / 100.0).ceil() as usize;
            raw.max(if coded.is_empty() { 0 } else { 1 })
        } else {
            0
        };
        let n = n.min(coded.len());
        let selected: Vec<(String, Option<String>)> = coded.into_iter().take(n).collect();

        self.with_transaction(|conn| {
            conn.execute(
                "INSERT INTO qc_samples \
                 (id, name, created_by, sample_pct, sample_n, seed, created_at, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    name,
                    created_by,
                    sample_pct,
                    sample_n,
                    seed,
                    now,
                    QC_SAMPLE_STATUS_OPEN
                ],
            )?;
            for (item_id, primary) in &selected {
                conn.execute(
                    "INSERT INTO qc_sample_items \
                     (sample_id, item_id, primary_coder, outcome, notes, recorded_by, recorded_at) \
                     VALUES (?1, ?2, ?3, NULL, NULL, NULL, NULL)",
                    params![id, item_id, primary],
                )?;
            }
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: created_by.clone(),
                    action: "qc_sample.create".into(),
                    entity: format!("qc_sample:{id}"),
                    params_json: serde_json::json!({
                        "name": name,
                        "sample_pct": sample_pct,
                        "sample_n": sample_n,
                        "seed": seed,
                        "selected_count": selected.len(),
                    })
                    .to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;

        let sample = QcSample {
            id: id.clone(),
            name: name.to_string(),
            created_by,
            sample_pct,
            sample_n,
            seed,
            created_at: now,
            status: QC_SAMPLE_STATUS_OPEN.to_string(),
        };
        let items: Vec<QcSampleItem> = selected
            .into_iter()
            .map(|(item_id, primary_coder)| QcSampleItem {
                sample_id: id.clone(),
                item_id,
                primary_coder,
                outcome: None,
                notes: None,
                recorded_by: None,
                recorded_at: None,
            })
            .collect();
        Ok((sample, items))
    }

    /// Record a QC outcome on a sample item.
    pub fn record_qc_outcome(
        &self,
        sample_id: &str,
        item_id: &str,
        outcome: &str,
        notes: Option<&str>,
        recorded_by: &str,
    ) -> Result<QcSampleItem> {
        let outcome = outcome.trim();
        match outcome {
            QC_OUTCOME_AGREE | QC_OUTCOME_DISAGREE | QC_OUTCOME_CORRECTED => {}
            _ => {
                return Err(Error::Other(format!(
                    "invalid qc outcome '{outcome}'; expected agree|disagree|corrected"
                )));
            }
        }
        let recorded_by = self.resolve_actor_for_mutate(recorded_by)?;
        let now = now_rfc3339();
        self.with_transaction(|conn| {
            let exists: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM qc_sample_items \
                 WHERE sample_id = ?1 AND item_id = ?2",
                params![sample_id, item_id],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(Error::Other(format!(
                    "item {item_id} not in qc sample {sample_id}"
                )));
            }
            conn.execute(
                "UPDATE qc_sample_items SET outcome = ?1, notes = ?2, \
                 recorded_by = ?3, recorded_at = ?4 \
                 WHERE sample_id = ?5 AND item_id = ?6",
                params![outcome, notes, recorded_by, now, sample_id, item_id],
            )?;
            audit::append_event(
                conn,
                &AuditEventInput {
                    actor: recorded_by.clone(),
                    action: "qc_sample.record_outcome".into(),
                    entity: format!("qc_sample:{sample_id}"),
                    params_json: serde_json::json!({
                        "item_id": item_id,
                        "outcome": outcome,
                    })
                    .to_string(),
                    tool_version: env!("CARGO_PKG_VERSION").into(),
                },
                &now,
            )?;
            Ok(())
        })?;
        self.get_qc_sample_item(sample_id, item_id)?
            .ok_or_else(|| Error::Other("qc sample item missing after record".into()))
    }

    /// Load one QC sample item.
    pub fn get_qc_sample_item(
        &self,
        sample_id: &str,
        item_id: &str,
    ) -> Result<Option<QcSampleItem>> {
        self.connection()
            .query_row(
                "SELECT sample_id, item_id, primary_coder, outcome, notes, recorded_by, recorded_at \
                 FROM qc_sample_items WHERE sample_id = ?1 AND item_id = ?2",
                params![sample_id, item_id],
                |r| {
                    Ok(QcSampleItem {
                        sample_id: r.get(0)?,
                        item_id: r.get(1)?,
                        primary_coder: r.get(2)?,
                        outcome: r.get(3)?,
                        notes: r.get(4)?,
                        recorded_by: r.get(5)?,
                        recorded_at: r.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Error::from)
    }

    /// Load QC sample header by id.
    pub fn get_qc_sample(&self, sample_id: &str) -> Result<QcSample> {
        self.connection()
            .query_row(
                "SELECT id, name, created_by, sample_pct, sample_n, seed, created_at, status \
                 FROM qc_samples WHERE id = ?1",
                params![sample_id],
                |r| {
                    Ok(QcSample {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        created_by: r.get(2)?,
                        sample_pct: r.get(3)?,
                        sample_n: r.get(4)?,
                        seed: r.get(5)?,
                        created_at: r.get(6)?,
                        status: r.get(7)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    Error::Other(format!("qc sample not found: {sample_id}"))
                }
                other => Error::Sqlite(other),
            })
    }

    /// List items in a QC sample.
    pub fn list_qc_sample_items(&self, sample_id: &str) -> Result<Vec<QcSampleItem>> {
        // Ensure sample exists.
        let _ = self.get_qc_sample(sample_id)?;
        let mut stmt = self.connection().prepare(
            "SELECT sample_id, item_id, primary_coder, outcome, notes, recorded_by, recorded_at \
             FROM qc_sample_items WHERE sample_id = ?1 ORDER BY item_id",
        )?;
        let rows = stmt.query_map(params![sample_id], |r| {
            Ok(QcSampleItem {
                sample_id: r.get(0)?,
                item_id: r.get(1)?,
                primary_coder: r.get(2)?,
                outcome: r.get(3)?,
                notes: r.get(4)?,
                recorded_by: r.get(5)?,
                recorded_at: r.get(6)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::from)
    }

    /// JSON-friendly QC sample summary (header + items) for service report.
    pub fn qc_sample_report(&self, sample_id: &str) -> Result<(QcSample, Vec<QcSampleItem>)> {
        let sample = self.get_qc_sample(sample_id)?;
        let items = self.list_qc_sample_items(sample_id)?;
        Ok((sample, items))
    }

    /// Resolve actor string: free-form when strict off; validated user id when on.
    pub(crate) fn resolve_actor_for_mutate(&self, actor: &str) -> Result<String> {
        if self.strict_actor() {
            resolve_strict_actor_conn(self.connection(), actor)
        } else {
            Ok(crate::matter::normalize_actor(actor))
        }
    }

    /// Whether multi-user / OCC / lock guards apply for this mutate.
    pub(crate) fn multi_user_guards_active(&self, expected_version: Option<i64>) -> Result<bool> {
        if self.strict_actor() {
            return Ok(true);
        }
        if expected_version.is_some() {
            return Ok(true);
        }
        self.is_multi_user_enabled()
    }

    /// Apply lock + OCC checks and bump version for each item id inside a txn.
    ///
    /// Returns new review versions in the same order as `item_ids`.
    pub(crate) fn prepare_review_mutates_conn(
        conn: &Connection,
        item_ids: &[String],
        actor_user_id: &str,
        expected_version: Option<i64>,
        enforce_locks: bool,
        require_expected: bool,
        now: &str,
    ) -> Result<Vec<i64>> {
        let mut versions = Vec::with_capacity(item_ids.len());
        for item_id in item_ids {
            assert_can_mutate_conn(conn, item_id, actor_user_id, now, enforce_locks)?;
            let v = bump_review_version_conn(conn, item_id, expected_version, require_expected)?;
            versions.push(v);
        }
        Ok(versions)
    }
}

/// Deterministic Fisher–Yates using a simple LCG seeded by `seed`.
fn seeded_shuffle<T>(items: &mut [T], seed: u64) {
    if items.len() < 2 {
        return;
    }
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    for i in (1..items.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (state as usize) % (i + 1);
        items.swap(i, j);
    }
}
